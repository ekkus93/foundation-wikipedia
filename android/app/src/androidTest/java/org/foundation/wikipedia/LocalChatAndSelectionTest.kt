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
        openTide()
        val device = UiDevice.getInstance(InstrumentationRegistry.getInstrumentation())
        // The WebView exposes the paragraph's text runs to accessibility.
        val words = device.wait(Until.findObject(By.textContains("gravity")), 15_000)
        assertNotNull("article text not shown", words)
        val box = words.visibleBounds
        // Long-press the middle of "'s gravity." to select a word.
        val x = box.left + box.width() * 2 / 3
        device.swipe(x, box.centerY(), x, box.centerY(), 120)
        var ask = device.wait(Until.findObject(By.text("Ask about this")), 5_000)
        if (ask == null) {
            // A crowded selection toolbar moves extra actions into overflow.
            device.findObject(By.desc("More options"))?.click()
            ask = device.wait(Until.findObject(By.text("Ask about this")), 5_000)
        }
        assertNotNull("\"Ask about this\" is missing from the WebView selection menu", ask)
        ask.click()
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
    fun aLocalModelAnswersAboutASelectionWithACitationAndProgress(): Unit = runBlocking {
        val arguments = InstrumentationRegistry.getArguments()
        val endpoint = arguments.getString("ollama")
        val model = arguments.getString("ollamaModel")
        assumeTrue("pass -e ollama <url> -e ollamaModel <model> to run", endpoint != null && model != null)
        val local = RustLocalAi()
        val server = local.discover(LocalAiProtocol.OLLAMA, endpoint!!)
        assertTrue(server.models().toString(), model in server.models())
        val library = OfflineLibraryHolder.get(context)
        val chat = local.prepareArticleChat(
            library, "enwiki", 2, "What do tides follow?", server, model!!,
            selection = "Moon's gravity",
        )
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
        android.util.Log.i("LocalChatTest", "local answer took ${updates.last().elapsedMs} ms: $answer")
    }
}
