package org.foundation.wikipedia

import android.content.ComponentName
import android.content.Intent
import android.content.pm.PackageManager
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onAllNodesWithText
import androidx.compose.ui.test.onFirst
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.io.File
import java.util.concurrent.CopyOnWriteArrayList
import kotlinx.coroutines.runBlocking
import org.foundation.wikipedia.rust.RustLocalAi
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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
 * MOB-003 on device/emulator: "Ask about this" is a private text-selection
 * action that opens the chat with the selected text, and (when an Ollama
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

    @Test
    fun askAboutThisIsOfferedToThisAppsWebViewButNotToOtherApps() {
        val query = Intent(Intent.ACTION_PROCESS_TEXT).setType("text/plain")
        val ours = context.packageManager.queryIntentActivities(query, PackageManager.MATCH_DEFAULT_ONLY)
            .map { it.activityInfo }
            .filter { it.packageName == context.packageName }
        assertEquals(listOf(AskSelectionActivity::class.java.name), ours.map { it.name })
        assertFalse("must not appear in other apps' selection menus", ours.single().exported)
    }

    @Test
    fun askAboutThisOpensTheChatWithTheSelectedText() {
        compose.onNode(hasSetTextAction()).performTextInput("tide")
        compose.onNodeWithText("Search").performClick()
        compose.waitUntil(timeoutMillis = 15_000) {
            compose.onAllNodesWithText("Tide").fetchSemanticsNodes().isNotEmpty()
        }
        compose.onAllNodesWithText("Tide").onFirst().performClick()
        // What the WebView's selection menu does for "Ask about this".
        compose.activity.startActivity(
            Intent(Intent.ACTION_PROCESS_TEXT)
                .setComponent(ComponentName(context, AskSelectionActivity::class.java))
                .setType("text/plain")
                .putExtra(Intent.EXTRA_PROCESS_TEXT, "Moon's gravity")
                .putExtra(Intent.EXTRA_PROCESS_TEXT_READONLY, true),
        )
        // The chat opens about the article being read, with the selection.
        compose.waitUntil(timeoutMillis = 30_000) {
            compose.onAllNodesWithText("Ask about “Tide”").fetchSemanticsNodes().isNotEmpty() &&
                compose.onAllNodesWithText("Asking about: “Moon's gravity”").fetchSemanticsNodes().isNotEmpty()
        }
        assertEquals(null, ReaderSelection.selections.value)
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
