package org.foundation.wikipedia

import android.content.ComponentName
import android.content.Intent
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import org.foundation.wikipedia.rust.RustLocalAi
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.wiki_ffi.ChatAnswer
import uniffi.wiki_ffi.ChatLocality
import uniffi.wiki_ffi.ChatAnswerPart
import uniffi.wiki_ffi.ChatProgressUpdate
import uniffi.wiki_ffi.LibraryException
import uniffi.wiki_ffi.LocalAiProtocol

/**
 * MOB-003 on device/emulator: long-pressing article text offers "Ask about
 * this", which opens the chat with the selected text (and refuses text from
 * other apps), and (when an Ollama
 * address is passed with `-e ollama http://10.0.2.2:11434 -e ollamaModel
 * qwen2.5:3b`) a real local model answers about a selection with a
 * validated citation while reporting progress.
 */
@RunWith(AndroidJUnit4::class)
class LocalChatAndSelectionTest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val context get() = InstrumentationRegistry.getInstrumentation().targetContext

    @Before
    fun installFixturePack(): Unit = runBlocking {
        val pack = File(context.cacheDir, "selection-sample-v2.wpack")
        InstrumentationRegistry.getInstrumentation().context.assets
            .open("android-sample-v2.wpack").use { input ->
                pack.outputStream().use { input.copyTo(it) }
            }
        val library = OfflineLibraryHolder.get(context)
        try {
            library.installPack(pack, acceptCustomPack = true)
        } catch (_: LibraryException.AlreadyInstalled) {
            // A previous test in this process installed it.
        }
        library.activate("android-sample")
        pack.delete()
    }

    private fun openTide() {
        compose.onNode(hasSetTextAction()).performTextInput("tide")
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) {
            compose.onAllNodesWithText("Tide").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onAllNodesWithText("Tide").onFirst().performClick()
        compose.waitUntil(timeoutMillis = 30_000) { readerTitle() == "Tide" }
    }

    private fun webViewOrigin(): Pair<Int, Int> {
        val location = IntArray(2)
        compose.runOnUiThread {
            fun search(view: View) {
                if (view is WebView) view.getLocationOnScreen(location)
                if (view is ViewGroup) for (i in 0 until view.childCount) search(view.getChildAt(i))
            }
            search(compose.activity.window.decorView)
        }
        return location[0] to location[1]
    }

    private fun readerTitle(): String? {
        var title: String? = null
        compose.runOnUiThread {
            fun search(view: View) {
                if (view is WebView) title = view.title
                if (view is ViewGroup) for (i in 0 until view.childCount) search(view.getChildAt(i))
            }
            search(compose.activity.window.decorView)
        }
        return title
    }

    @Test
    fun longPressingArticleTextOffersAskAboutThisWhichOpensTheChat() {
        // Connect UiAutomation first so accessibility is on before the
        // WebView builds its page, then the page exposes its text runs.
        val device = UiDevice.getInstance(InstrumentationRegistry.getInstrumentation())
        openTide()
        val words = device.wait(Until.findObject(By.textContains("gravity")), 30_000)
        // On the slow CI emulator a swipe-made press was sometimes not taken
        // as a long-press at all (no selection toolbar): each attempt uses a
        // different gesture, and logs which one it was.
        fun longPress(attempt: Int) {
            val target = words ?: device.findObject(By.textContains("gravity"))
            if (target != null) {
                val box = target.visibleBounds
                // Long-press inside "'s gravity." to select a word.
                val x = box.left + box.width() * 2 / 3
                when (attempt) {
                    1 -> device.swipe(x, box.centerY(), x, box.centerY(), 120) // ~0.6 s
                    2 -> target.longClick()
                    else -> device.swipe(x, box.centerY(), x, box.centerY(), 400) // ~2 s
                }
            } else {
                // Slow emulators may not expose WebView text in time: press the
                // lead paragraph ("Tides follow…"), about 130 dp below the top
                // of the page and 60 dp in from its left edge.
                val (left, top) = webViewOrigin()
                val density = context.resources.displayMetrics.density
                val x = left + (60 * density).toInt()
                val y = top + (130 * density).toInt()
                device.swipe(x, y, x, y, 120)
            }
        }
        fun findAsk(): androidx.test.uiautomator.UiObject2? {
            device.wait(Until.findObject(By.text("Ask about this")), 8_000)?.let { return it }
            // A crowded selection toolbar moves extra actions into overflow.
            device.findObject(By.desc("More options"))?.click() ?: return null
            return device.wait(Until.findObject(By.text("Ask about this")), 5_000)
        }
        // A slow emulator can miss the first long-press (no selection, or a
        // toolbar that never appears): clear it and try again.
        var ask: androidx.test.uiautomator.UiObject2? = null
        for (attempt in 1..3) {
            longPress(attempt)
            ask = findAsk()
            if (ask != null) {
                android.util.Log.i("SelectionTest", "attempt $attempt (gesture $attempt) opened the selection menu")
                break
            }
            android.util.Log.w("SelectionTest", "attempt $attempt (gesture $attempt): no \"Ask about this\" (text found: ${words != null})")
            // Tap the empty page below Tide's short text to clear any selection.
            val (left, top) = webViewOrigin()
            val density = context.resources.displayMetrics.density
            device.click(left + (40 * density).toInt(), top + (420 * density).toInt())
            device.waitForIdle()
        }
        if (ask == null) {
            val dump = java.io.ByteArrayOutputStream()
            device.dumpWindowHierarchy(dump)
            dump.toString().chunked(3_000).forEach { android.util.Log.w("SelectionTest", it) }
        }
        assertNotNull("\"Ask about this\" is missing from the WebView selection menu", ask)
        ask!!.click()
        compose.waitUntil(timeoutMillis = 30_000) {
            compose.onAllNodesWithText("Ask about “Tide”").fetchSemanticsNodes().isNotEmpty() &&
                compose.onAllNodesWithText("Asking about: “", substring = true).fetchSemanticsNodes().isNotEmpty()
        }
        assertEquals(null, ReaderSelection.selections.value)
    }

    @Test
    fun aSelectionFromAnotherAppIsRefusedUnread() {
        assertTrue(AskSelectionActivity.acceptsCaller(context.packageName, context.packageName))
        assertFalse(AskSelectionActivity.acceptsCaller(null, context.packageName))
        assertFalse(AskSelectionActivity.acceptsCaller("com.example.other", context.packageName))
        openTide()
        // startActivity (not ForResult) leaves no known caller, as when
        // another app's selection menu starts it.
        compose.activity.startActivity(
            Intent(Intent.ACTION_PROCESS_TEXT)
                .setComponent(ComponentName(context, AskSelectionActivity::class.java))
                .setType("text/plain")
                .putExtra(Intent.EXTRA_PROCESS_TEXT, "Moon's gravity")
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
        compose.waitForIdle()
        Thread.sleep(2_000)
        compose.waitForIdle()
        assertEquals(null, ReaderSelection.selections.value)
        assertTrue(compose.onAllNodesWithText("Asking about: “", substring = true).fetchSemanticsNodes().isEmpty())
    }

    @Test
    fun aLocalModelAnswersAboutASelectionWithACitationAndProgress(): Unit =
        groundedLocalAnswer(LocalAiProtocol.OLLAMA)

    /** AI-002: the OpenAI-compatible adapter (llama-server, Ollama's /v1). */
    @Test
    fun anOpenAiCompatibleLocalServerAlsoGivesAGroundedAnswer(): Unit =
        groundedLocalAnswer(LocalAiProtocol.OPEN_AI_COMPATIBLE)

    private fun groundedLocalAnswer(protocol: LocalAiProtocol): Unit = runBlocking {
        val arguments = InstrumentationRegistry.getArguments()
        val endpoint = arguments.getString("ollama")
        val model = arguments.getString("ollamaModel")
        assumeTrue("pass -e ollama <url> -e ollamaModel <model> to run", endpoint != null && model != null)
        val local = RustLocalAi()
        val server = local.discover(protocol, endpoint!!)
        assertTrue(server.models().toString(), model in server.models())
        // The emulator reaches the host at 10.0.2.2, a private address: it
        // must be labelled as the local network, never this device or cloud.
        assertEquals(ChatLocality.LOCAL_NETWORK, server.locality())
        val library = OfflineLibraryHolder.get(context)
        val chat = local.prepareArticleChat(
            library, "enwiki", 2, "What do tides follow?", server, model!!,
            selection = "Moon's gravity",
        )
        assertEquals(ChatLocality.LOCAL_NETWORK, chat.locality())
        assertEquals("wkb:enwiki:2:7002:b0", chat.selectedPassage()?.blockId)
        val updates = CopyOnWriteArrayList<ChatProgressUpdate>()
        val reply = local.send(chat) { updates += it }
        val answer = reply.answer
        assertTrue("$answer", answer is ChatAnswer.Supported)
        assertTrue(
            "$answer",
            (answer as ChatAnswer.Supported).parts.any {
                it is ChatAnswerPart.Citation && it.passage.blockId == "wkb:enwiki:2:7002:b0"
            },
        )
        assertTrue("$updates", updates.last().finished && updates.last().firstOutputMs != null)
        android.util.Log.i("LocalChatTest", "$protocol answer took ${updates.last().elapsedMs} ms: $answer")
    }
}
