package org.foundation.wikipedia

import android.content.Context
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
import uniffi.wiki_ffi.HostedArticleChat

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
 * destination, model, question and the exact passages (and earlier turns)
 * are shown, and nothing leaves the device until the user taps Send.
 * Answers are shown only after Rust has validated every citation against
 * the article revision; tapping a citation shows that passage in the reader.
 */
@Composable
fun ArticleChatSheet(
    target: ArticleChatTarget?,
    conversation: ChatConversation?,
    onConversation: (ChatConversation) -> Unit,
    saveHistory: Boolean,
    onCitation: (CitationRequest) -> Unit,
    onOpenAiSettings: () -> Unit,
    onClose: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val hosted = remember { RustHostedAi(context) }
    var configured by remember { mutableStateOf<List<HostedAiProvider>?>(null) }
    var provider by remember { mutableStateOf<HostedAiProvider?>(null) }
    var model by remember { mutableStateOf("") }
    var question by remember { mutableStateOf("") }
    var preview by remember { mutableStateOf<HostedArticleChat?>(null) }
    var previewQuestion by remember { mutableStateOf("") }
    var busy by remember { mutableStateOf(false) }
    var request by remember { mutableStateOf<Job?>(null) }
    var message by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(Unit) {
        configured = try {
            HostedAiProvider.entries.filter { hosted.isKeyConfigured(it) }
        } catch (error: Exception) {
            message = hostedAiErrorMessage(error)
            emptyList()
        } catch (error: LinkageError) {
            message = chatErrorMessage(error)
            emptyList()
        }
        configured?.firstOrNull()?.let {
            provider = it
            model = HostedAiModelPreference.load(context, it)
        }
    }

    // A preview is consent for one exact request: drop it if anything changes.
    fun discardPreview() {
        preview?.close()
        preview = null
    }

    fun review() {
        val chosen = provider ?: return
        val article = target ?: return
        val text = question.trim()
        if (text.isEmpty() || model.isBlank()) return
        scope.launch {
            busy = true
            message = null
            try {
                val library = OfflineLibraryHolder.get(context)
                val history = if (saveHistory) ChatHistoryHolder.get(context) else null
                discardPreview()
                preview = hosted.prepareArticleChat(
                    library,
                    article.project,
                    article.pageId,
                    text,
                    chosen,
                    model.trim(),
                    history,
                    conversation?.takeIf { saveHistory && it.target == article }?.threadId,
                )
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
        val chosen = provider ?: return
        val asked = previewQuestion
        request = scope.launch {
            busy = true
            message = null
            try {
                val reply = hosted.send(chat)
                HostedAiModelPreference.save(context, chosen, chat.model())
                val current = ChatConversation.forTarget(conversation, article) ?: ChatConversation(article)
                onConversation(current.withReply(reply.answer.toExchange(asked), reply.threadId))
                question = ""
            } catch (error: CancellationException) {
                message = chatErrorMessage(ChatException.Cancelled())
                throw error
            } catch (error: Exception) {
                message = chatErrorMessage(error)
            } finally {
                discardPreview()
                busy = false
                request = null
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
        val providers = configured
        when {
            providers == null -> Text("Checking AI providers…")
            providers.isEmpty() -> {
                Text(
                    "Chat uses a hosted AI provider you choose. No API key is saved yet. " +
                        "Questions and article excerpts you send leave this device.",
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
                            if (saveHistory) "Chats are saved on this device." else
                                "Chats are not saved (History & Saved Content is off), so " +
                                    "earlier questions are not sent with new ones.",
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
            items(exchanges) { exchange -> ExchangeView(exchange, target, onCitation) }
        }

        if (providers != null && providers.size > 1) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                for (option in providers) {
                    Row(
                        modifier = Modifier.clickable(enabled = !busy) {
                            provider = option
                            model = HostedAiModelPreference.load(context, option)
                            discardPreview()
                        },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = provider == option, onClick = null)
                        Text(option.label(), modifier = Modifier.padding(end = 12.dp))
                    }
                }
            }
        }
        OutlinedTextField(
            value = model,
            onValueChange = {
                model = it.take(200)
                discardPreview()
            },
            label = { Text("${provider?.label().orEmpty()} model") },
            singleLine = true,
            enabled = !busy,
            modifier = Modifier.fillMaxWidth(),
        )
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
        if (chat != null) {
            DisclosureCard(chat, previewQuestion)
        }
        if (busy) LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
        message?.let { Text(it, color = MaterialTheme.colorScheme.error) }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            when {
                request != null -> OutlinedButton(onClick = { request?.cancel() }) { Text("Stop") }
                chat != null -> {
                    Button(onClick = { send() }) { Text("Send to ${chat.destination()}") }
                    OutlinedButton(onClick = { discardPreview() }) { Text("Don't send") }
                }
                else -> Button(
                    enabled = !busy && question.isNotBlank() && model.isNotBlank() && provider != null,
                    onClick = { review() },
                ) { Text("Review request") }
            }
        }
    }
}

@Composable
private fun DisclosureCard(chat: HostedArticleChat, question: String) {
    val passages = chat.passages()
    val prior = chat.priorTurns()
    Card(modifier = Modifier.fillMaxWidth().semantics { contentDescription = "Request review" }) {
        Column(modifier = Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(
                "This will be sent to ${chat.destination()} (model ${chat.model()}), " +
                    "outside this device:",
                fontWeight = FontWeight.SemiBold,
            )
            Text("Question: “$question”")
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
