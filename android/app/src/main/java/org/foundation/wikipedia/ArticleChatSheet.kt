package org.foundation.wikipedia

import android.content.Context
import android.os.SystemClock
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import kotlinx.coroutines.delay
import org.foundation.wikipedia.rust.RustLocalAi
import uniffi.wiki_ffi.ChatProgressUpdate
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import org.foundation.wikipedia.rust.HostedAiKeyMissingException
import org.foundation.wikipedia.rust.RustChatHistory
import org.foundation.wikipedia.rust.RustHostedAi
import uniffi.wiki_ffi.ChatAnswer
import uniffi.wiki_ffi.ChatAnswerPart
import uniffi.wiki_ffi.ChatException
import uniffi.wiki_ffi.HostedAiProvider
import uniffi.wiki_ffi.LocalAiException
import uniffi.wiki_ffi.ArticleChat

/** One chat history store per process. */
object ChatHistoryHolder {
    private val lock = Mutex()
    private var history: RustChatHistory? = null

    suspend fun get(context: Context): RustChatHistory = lock.withLock {
        history ?: RustChatHistory.open(context).also { history = it }
    }
}

/** The last model name used per provider; not secret, never a key. */
object HostedAiModelPreference {
    private const val FILE = "foundation_hosted_ai"

    private fun key(provider: HostedAiProvider) = "model_${provider.name}"

    fun load(context: Context, provider: HostedAiProvider): String = try {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).getString(key(provider), null).orEmpty()
    } catch (_: ClassCastException) {
        ""
    }

    fun save(context: Context, provider: HostedAiProvider, model: String) {
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit()
            .putString(key(provider), model.trim()).apply()
    }
}

/** Past response timings per destination and model, for time estimates. */
object ModelTimingPreference {
    private const val FILE = "foundation_ai_timing"

    private fun key(destination: String, model: String) = "$destination|$model".take(400)

    fun load(context: Context, destination: String, model: String): ModelTiming? = try {
        ModelTiming.decode(
            context.getSharedPreferences(FILE, Context.MODE_PRIVATE).getString(key(destination, model), null),
        )
    } catch (_: ClassCastException) {
        null
    }

    fun record(context: Context, destination: String, model: String, sample: ModelTiming) {
        val updated = load(context, destination, model)?.plus(sample) ?: sample
        context.getSharedPreferences(FILE, Context.MODE_PRIVATE).edit()
            .putString(key(destination, model), updated.encode()).apply()
    }
}

/** Where a question can be asked: a hosted provider or the saved local model. */
sealed interface ChatChoice {
    val label: String

    data class Hosted(val provider: HostedAiProvider) : ChatChoice {
        override val label: String get() = provider.label()
    }

    data class Local(val local: LocalAiChoice) : ChatChoice {
        override val label: String get() = "${local.model} (local)"
    }
}

/** User-facing text for a failed chat request. Never includes key material. */
fun chatErrorMessage(error: Throwable): String = when (error) {
    is HostedAiKeyMissingException -> "Save an API key for this provider in Settings › AI Assistant."
    is ChatException.InvalidInput -> "Enter a question and a valid model name."
    is ChatException.InvalidKey -> "The saved API key is not valid. Replace it in Settings › AI Assistant."
    is ChatException.NotInstalled -> "This article is no longer in the active offline pack."
    is ChatException.VerificationFailed -> "The installed article failed verification."
    is ChatException.NoEvidence ->
        "No part of this article matches the question, so nothing was sent. Try rephrasing."
    is ChatException.ConsentRequired -> "Nothing was sent."
    is ChatException.PreviewChanged, is ChatException.AlreadySent ->
        "The article or question changed. Review the request again."
    is ChatException.Cancelled -> "Stopped. No answer was shown."
    is ChatException.NotReady -> "Secure connections are not available on this device yet."
    is ChatException.Rejected ->
        "The provider refused the request. Check the API key and model name."
    is ChatException.Timeout -> "The provider did not respond in time."
    is ChatException.Network -> "Could not connect securely. Check the network connection."
    is ChatException.InvalidResponse -> "The provider sent an unexpected response."
    is ChatException.Unverifiable ->
        "The answer's citations did not match this article, so it is not shown."
    is ChatException.Storage -> "Chat history could not be saved."
    is ChatException.SelectionNotFound ->
        "The selected text was not found within one paragraph of this article. " +
            "Select text inside a single paragraph."
    is LocalAiException -> localAiErrorMessage(error)
    is LinkageError -> "Chat is not available in this build."
    else -> "Something went wrong with the chat request."
}

private fun HostedAiProvider.label() = when (this) {
    HostedAiProvider.ANTHROPIC -> "Anthropic"
    HostedAiProvider.OPEN_AI -> "OpenAI"
}

internal fun ChatAnswer.toExchange(question: String): ChatExchange = when (this) {
    is ChatAnswer.InsufficientEvidence -> ChatExchange.insufficient(question)
    is ChatAnswer.Supported -> ChatExchange.supported(
        question,
        parts.map { part ->
            when (part) {
                is ChatAnswerPart.Text -> RawAnswerPart.Text(part.text)
                is ChatAnswerPart.Citation -> RawAnswerPart.Cite(
                    part.passage.blockId,
                    part.passage.headingPath,
                    part.passage.excerpt,
                )
            }
        },
    )
}

/**
 * MOB-003 article chat (sheet content). Every request is reviewed first: the
 * destination, model, question, any reader selection and the exact passages
 * (and earlier turns) are shown, and nothing leaves the device until the
 * user taps Send. While the model works, a progress bar and an estimate of
 * the time left are shown (learned from earlier replies of the same model).
 * Answers are shown only after Rust has validated every citation against the
 * article revision; tapping a citation shows that passage in the reader.
 */
@Composable
fun ArticleChatSheet(
    target: ArticleChatTarget?,
    conversation: ChatConversation?,
    onConversation: (ChatConversation) -> Unit,
    saveHistory: Boolean,
    selection: String?,
    onClearSelection: () -> Unit,
    onCitation: (CitationRequest) -> Unit,
    onOpenAiSettings: () -> Unit,
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val hosted = remember { RustHostedAi(context) }
    val localAi = remember { RustLocalAi() }
    var choices by remember { mutableStateOf<List<ChatChoice>?>(null) }
    var choice by remember { mutableStateOf<ChatChoice?>(null) }
    var model by remember { mutableStateOf("") }
    var question by remember { mutableStateOf(if (selection != null) "What does this mean?" else "") }
    var preview by remember { mutableStateOf<ArticleChat?>(null) }
    var previewQuestion by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var request by remember { mutableStateOf<Job?>(null) }
    var message by remember { mutableStateOf<String?>(null) }
    // Progress of the request in flight (written from the sending thread).
    var progress by remember { mutableStateOf<ChatProgressUpdate?>(null) }
    var startedAt by remember { mutableLongStateOf(0L) }
    var now by remember { mutableLongStateOf(0L) }
    var timing by remember { mutableStateOf<ModelTiming?>(null) }

    LaunchedEffect(Unit) {
        val hostedChoices = try {
            HostedAiProvider.entries.filter { hosted.isKeyConfigured(it) }.map { ChatChoice.Hosted(it) }
        } catch (error: Exception) {
            message = hostedAiErrorMessage(error)
            emptyList()
        } catch (error: LinkageError) {
            message = chatErrorMessage(error)
            emptyList()
        }
        val localChoice = LocalAiPreference.load(context)?.let { ChatChoice.Local(it) }
        val all = listOfNotNull(localChoice) + hostedChoices
        choices = all
        all.firstOrNull()?.let {
            choice = it
            if (it is ChatChoice.Hosted) model = HostedAiModelPreference.load(context, it.provider)
        }
    }

    LaunchedEffect(selection) {
        if (selection != null && question.isBlank()) question = "What does this mean?"
    }

    // Re-estimate twice a second while waiting, even when no output arrives.
    LaunchedEffect(request) {
        while (request != null) {
            now = SystemClock.elapsedRealtime()
            delay(500)
        }
    }

    // A preview is consent for one exact request: drop it if anything changes.
    fun discardPreview() {
        preview?.close()
        preview = null
    }

    fun review() {
        val chosen = choice ?: return
        val article = target ?: return
        val text = question.trim()
        if (text.isEmpty() || (chosen is ChatChoice.Hosted && model.isBlank())) return
        scope.launch {
            busy = true
            message = null
            try {
                val library = OfflineLibraryHolder.get(context)
                val history = if (saveHistory) ChatHistoryHolder.get(context) else null
                val thread = conversation?.takeIf { saveHistory && it.target == article }?.threadId
                discardPreview()
                preview = when (chosen) {
                    is ChatChoice.Hosted -> hosted.prepareArticleChat(
                        library, article.project, article.pageId, text, chosen.provider,
                        model.trim(), history, thread, selection,
                    )
                    is ChatChoice.Local -> {
                        // Reads the server's model list only; nothing is sent yet.
                        val server = localAi.discover(chosen.local.protocol, chosen.local.endpoint)
                        try {
                            localAi.prepareArticleChat(
                                library, article.project, article.pageId, text, server,
                                chosen.local.model, history, thread, selection,
                            )
                        } finally {
                            server.close()
                        }
                    }
                }
                previewQuestion = text
            } catch (error: Exception) {
                message = chatErrorMessage(error)
            } catch (error: LinkageError) {
                message = chatErrorMessage(error)
            } finally {
                busy = false
            }
        }
    }

    fun send() {
        val chat = preview ?: return
        val article = target ?: return
        val chosen = choice ?: return
        val asked = previewQuestion
        val destination = chat.destination()
        val chatModel = chat.model()
        val promptChars = chat.promptChars().toLong()
        timing = ModelTimingPreference.load(context, destination, chatModel)
        progress = null
        startedAt = SystemClock.elapsedRealtime()
        now = startedAt
        request = scope.launch {
            busy = true
            message = null
            try {
                val onProgress: (ChatProgressUpdate) -> Unit = { progress = it }
                val reply = when (chosen) {
                    is ChatChoice.Hosted -> hosted.send(chat, onProgress)
                    is ChatChoice.Local -> localAi.send(chat, onProgress)
                }
                if (chosen is ChatChoice.Hosted) HostedAiModelPreference.save(context, chosen.provider, chatModel)
                progress?.takeIf { it.finished }?.let { done ->
                    val first = done.firstOutputMs?.toLong()
                    val sample = first?.let {
                        ModelTiming.measured(promptChars, it, done.elapsedMs.toLong(), done.outputChars.toLong())
                    }
                    if (sample != null) ModelTimingPreference.record(context, destination, chatModel, sample)
                }
                val current = ChatConversation.forTarget(conversation, article) ?: ChatConversation(article)
                onConversation(current.withReply(reply.answer.toExchange(asked), reply.threadId))
                question = ""
                if (selection != null) onClearSelection()
            } catch (error: CancellationException) {
                message = chatErrorMessage(ChatException.Cancelled())
                throw error
            } catch (error: Exception) {
                message = chatErrorMessage(error)
            } finally {
                discardPreview()
                busy = false
                request = null
                progress = null
            }
        }
    }

    Column(
        modifier = Modifier.fillMaxWidth().padding(horizontal = 20.dp).padding(bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                if (target != null) "Ask about “${target.title}”" else "Article chat",
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = onClose) { Text("Close") }
        }
        if (target == null) {
            Text("Open an article from your offline pack first, then ask about it here.")
            return@Column
        }
        if (selection != null) {
            Card(modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Selected text" }) {
                Row(modifier = Modifier.padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        "Asking about: “${selection.take(160)}${if (selection.length > 160) "…" else ""}”",
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(enabled = !busy, onClick = {
                        discardPreview()
                        onClearSelection()
                    }) { Text("Remove") }
                }
            }
        }
        val available = choices
        when {
            available == null -> Text("Checking AI models…")
            available.isEmpty() -> {
                Text(
                    "Chat needs a model: a local or LAN model server (such as Ollama), or a " +
                        "hosted AI provider with an API key. Questions and article excerpts " +
                        "you send go to the model you choose.",
                )
                OutlinedButton(onClick = onOpenAiSettings) { Text("Set up in Settings › AI Assistant") }
                return@Column
            }
        }

        LazyColumn(
            modifier = Modifier.fillMaxWidth().weight(1f, fill = false),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            val exchanges = conversation?.takeIf { it.target == target }?.exchanges.orEmpty()
            if (exchanges.isEmpty()) {
                item {
                    Text(
                        "Answers use only passages from this article and cite them. " +
                            "To ask about specific text, select it in the article and choose " +
                            "“Ask about this”. " +
                            if (saveHistory) "Chats are saved on this device." else
                                "Chats are not saved (History & Saved Content is off), so " +
                                    "earlier questions are not sent with new ones.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
            items(exchanges) { exchange -> ExchangeView(exchange, target, onCitation) }
        }

        if (available != null && available.size > 1) {
            for (option in available) {
                Row(
                    modifier = Modifier.fillMaxWidth().clickable(enabled = !busy) {
                        choice = option
                        if (option is ChatChoice.Hosted) model = HostedAiModelPreference.load(context, option.provider)
                        discardPreview()
                    },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    RadioButton(selected = choice == option, onClick = null)
                    Text(option.label)
                }
            }
        }
        when (val chosen = choice) {
            is ChatChoice.Hosted -> OutlinedTextField(
                value = model,
                onValueChange = {
                    model = it.take(200)
                    discardPreview()
                },
                label = { Text("${chosen.provider.label()} model") },
                singleLine = true,
                enabled = !busy,
                modifier = Modifier.fillMaxWidth(),
            )
            is ChatChoice.Local -> Text(
                "Model ${chosen.local.model} at ${chosen.local.endpoint}",
                style = MaterialTheme.typography.bodySmall,
            )
            null -> {}
        }
        OutlinedTextField(
            value = question,
            onValueChange = {
                question = it.take(2000)
                discardPreview()
            },
            label = { Text("Question") },
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        )

        val chat = preview
        if (chat != null && request == null) {
            DisclosureCard(chat, previewQuestion)
        }
        if (request != null) {
            val update = progress
            val elapsed = (now - startedAt).coerceAtLeast(update?.elapsedMs?.toLong() ?: 0)
            val estimate = ResponseEstimate.of(
                timing,
                chat?.promptChars()?.toLong() ?: 0,
                (chat?.maxOutputTokens()?.toLong() ?: 1024) * 4,
                elapsed,
                update?.firstOutputMs?.toLong(),
                update?.outputChars?.toLong() ?: 0,
                update?.finished ?: false,
            )
            ResponseProgress(estimate, elapsed)
        } else if (busy) {
            LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
        }
        message?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            when {
                request != null -> OutlinedButton(onClick = { request?.cancel() }) { Text("Stop") }
                chat != null -> {
                    Button(onClick = { send() }) { Text("Send") }
                    OutlinedButton(onClick = { discardPreview() }) { Text("Don't send") }
                }
                else -> Button(
                    enabled = !busy && question.isNotBlank() && choice != null &&
                        (choice !is ChatChoice.Hosted || model.isNotBlank()),
                    onClick = { review() },
                ) { Text("Review request") }
            }
        }
        if (request != null) {
            Text("Closing the chat stops the request.", style = MaterialTheme.typography.bodySmall)
        }
    }
}

/** The progress bar and the time-left line of a request in flight. */
@Composable
private fun ResponseProgress(estimate: ResponseEstimate, elapsedMs: Long) {
    val fraction = estimate.fraction
    if (fraction == null) {
        LinearProgressIndicator(modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Working" })
    } else {
        LinearProgressIndicator(
            progress = { fraction },
            modifier = Modifier.fillMaxWidth().semantics {
                contentDescription = "About ${(fraction * 100).toInt()} percent done"
            },
        )
    }
    val phase = when (estimate.phase) {
        ResponsePhase.READING -> "Reading the passages"
        ResponsePhase.WRITING -> "Writing the answer"
        ResponsePhase.CHECKING -> "Checking citations"
    }
    val left = when {
        estimate.phase == ResponsePhase.CHECKING -> ""
        estimate.remainingMs != null -> " · " + ResponseEstimate.describe(estimate.remainingMs)
        else -> " · estimating time…"
    }
    val seconds = elapsedMs / 1000
    Text(
        "$phase$left (${seconds / 60}:${"%02d".format(seconds % 60)} so far)",
        style = MaterialTheme.typography.bodySmall,
        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
    )
}

@Composable
private fun DisclosureCard(chat: ArticleChat, question: String) {
    val passages = chat.passages()
    val prior = chat.priorTurns()
    val selected = chat.selectedPassage()
    Card(modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Request review" }) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(
                "This will be sent to ${chat.destination()}, ${chat.locality().describe()} " +
                    "(model ${chat.model()}):",
                fontWeight = FontWeight.SemiBold,
            )
            Text("Question: “$question”")
            selected?.let {
                val place = it.headingPath.joinToString(" › ").ifEmpty { "Introduction" }
                Text("Your selection is in the passage from $place, which is always included.")
            }
            Text(
                "${passages.size} passage${if (passages.size == 1) "" else "s"} from " +
                    "“${chat.articleTitle()}”, revision ${chat.revisionId()}:",
            )
            for (passage in passages) {
                val place = passage.headingPath.joinToString(" › ").ifEmpty { "Introduction" }
                Text("• $place: ${passage.excerpt.take(280)}", style = MaterialTheme.typography.bodySmall)
            }
            if (prior.isNotEmpty()) {
                Text(
                    "Also ${prior.size} earlier question${if (prior.size == 1) "" else "s"} " +
                        "and answer${if (prior.size == 1) "" else "s"} from this chat.",
                )
            }
        }
    }
}

@Composable
private fun ExchangeView(
    exchange: ChatExchange,
    target: ArticleChatTarget,
    onCitation: (CitationRequest) -> Unit,
) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(exchange.question, fontWeight = FontWeight.SemiBold)
        val segments = exchange.segments
        if (segments == null) {
            Text("The article passages do not answer this question.")
            return@Column
        }
        Text(
            segments.joinToString("") { segment ->
                when (segment) {
                    is ChatSegment.Text -> segment.text
                    is ChatSegment.Cite -> " [${segment.citation.number}]"
                }
            }.trim(),
        )
        for (citation in exchange.citations) {
            TextButton(
                onClick = {
                    onCitation(CitationRequest(target, citation.blockId, System.nanoTime()))
                },
                modifier = Modifier.semantics {
                    contentDescription = "Show cited passage ${citation.number}, ${citation.place}, in the article"
                },
            ) {
                Text("[${citation.number}] ${citation.place}", style = MaterialTheme.typography.labelLarge)
            }
        }
        HorizontalDivider()
    }
}
