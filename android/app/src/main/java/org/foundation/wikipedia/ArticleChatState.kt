package org.foundation.wikipedia

/**
 * MOB-003 chat state, kept free of native types so it is unit-testable on
 * the JVM. A conversation is bound to one exact article revision: answers
 * and citations never carry over to another article or revision.
 */
data class ArticleChatTarget(
    val project: String,
    val pageId: Long,
    val revisionId: Long,
    val title: String,
)

/** One cited passage, numbered in order of first appearance in an answer. */
data class ChatCitation(
    val number: Int,
    val blockId: String,
    val headings: List<String>,
    val excerpt: String,
) {
    /** Where the passage is, for the citation button. */
    val place: String get() = if (headings.isEmpty()) "Introduction" else headings.joinToString(" › ")
}

sealed interface ChatSegment {
    data class Text(val text: String) : ChatSegment
    data class Cite(val citation: ChatCitation) : ChatSegment
}

/** A raw answer part from Rust, before numbering. */
sealed interface RawAnswerPart {
    data class Text(val text: String) : RawAnswerPart
    data class Cite(val blockId: String, val headings: List<String>, val excerpt: String) : RawAnswerPart
}

data class ChatExchange(
    val question: String,
    /** `null` when the model found the passages do not answer the question. */
    val segments: List<ChatSegment>?,
) {
    val citations: List<ChatCitation>
        get() = segments.orEmpty().filterIsInstance<ChatSegment.Cite>()
            .map { it.citation }.distinctBy { it.number }

    companion object {
        /** Number citations by first appearance; repeats reuse their number. */
        fun supported(question: String, parts: List<RawAnswerPart>): ChatExchange {
            val numbers = LinkedHashMap<String, Int>()
            val segments = parts.map { part ->
                when (part) {
                    is RawAnswerPart.Text -> ChatSegment.Text(part.text)
                    is RawAnswerPart.Cite -> {
                        val number = numbers.getOrPut(part.blockId) { numbers.size + 1 }
                        ChatSegment.Cite(ChatCitation(number, part.blockId, part.headings, part.excerpt))
                    }
                }
            }
            return ChatExchange(question, segments)
        }

        fun insufficient(question: String) = ChatExchange(question, null)
    }
}

data class ChatConversation(
    val target: ArticleChatTarget,
    val exchanges: List<ChatExchange> = emptyList(),
    /** The saved history thread to continue, when chat history is on. */
    val threadId: Long? = null,
) {
    fun withReply(exchange: ChatExchange, thread: Long?): ChatConversation =
        copy(exchanges = (exchanges + exchange).takeLast(MAX_EXCHANGES), threadId = thread ?: threadId)

    companion object {
        const val MAX_EXCHANGES = 50

        /** Keep [current] only for the same article revision; otherwise start over. */
        fun forTarget(current: ChatConversation?, target: ArticleChatTarget?): ChatConversation? = when {
            target == null -> current
            current?.target == target -> current
            else -> ChatConversation(target)
        }
    }
}

/**
 * A request to show [blockId] of [target] in the reader. [nonce] makes a
 * repeated tap on the same citation scroll to it again.
 */
data class CitationRequest(val target: ArticleChatTarget, val blockId: String, val nonce: Long)
