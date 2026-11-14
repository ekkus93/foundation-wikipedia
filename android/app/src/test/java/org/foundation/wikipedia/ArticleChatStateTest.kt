package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotSame
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Test

class ArticleChatStateTest {
    private val tide = ArticleChatTarget("enwiki", 2, 7002, "Tide")

    @Test
    fun citationsAreNumberedByFirstAppearanceAndRepeatsReuseTheirNumber() {
        val exchange = ChatExchange.supported(
            "Why tides?",
            listOf(
                RawAnswerPart.Text("Tides follow the Moon"),
                RawAnswerPart.Cite("wkb:a", listOf("Causes"), "Tides follow the Moon."),
                RawAnswerPart.Text(" and the Sun"),
                RawAnswerPart.Cite("wkb:b", emptyList(), "The Sun also matters."),
                RawAnswerPart.Cite("wkb:a", listOf("Causes"), "Tides follow the Moon."),
            ),
        )
        val numbers = exchange.segments!!.filterIsInstance<ChatSegment.Cite>().map { it.citation.number }
        assertEquals(listOf(1, 2, 1), numbers)
        assertEquals(listOf("wkb:a", "wkb:b"), exchange.citations.map { it.blockId })
        assertEquals(listOf("Causes", "Introduction"), exchange.citations.map { it.place })
    }

    @Test
    fun anAbstentionHasNoCitations() {
        val exchange = ChatExchange.insufficient("Who won?")
        assertNull(exchange.segments)
        assertEquals(emptyList<ChatCitation>(), exchange.citations)
    }

    @Test
    fun aConversationIsKeptOnlyForTheSameArticleRevision() {
        val chat = ChatConversation(tide).withReply(ChatExchange.insufficient("q"), 5)
        assertSame(chat, ChatConversation.forTarget(chat, tide))
        // Navigating away leaves the conversation until another article is shown.
        assertSame(chat, ChatConversation.forTarget(chat, null))
        val newer = ChatConversation.forTarget(chat, tide.copy(revisionId = 7003))!!
        assertNotSame(chat, newer)
        assertEquals(emptyList<ChatExchange>(), newer.exchanges)
        assertNull(newer.threadId)
        assertEquals(emptyList<ChatExchange>(), ChatConversation.forTarget(chat, tide.copy(pageId = 1))!!.exchanges)
    }

    @Test
    fun repliesKeepTheirThreadAndAreBounded() {
        var chat = ChatConversation(tide)
        chat = chat.withReply(ChatExchange.insufficient("first"), 9)
        // An unsaved reply never forgets the thread being continued.
        chat = chat.withReply(ChatExchange.insufficient("second"), null)
        assertEquals(9L, chat.threadId)
        repeat(ChatConversation.MAX_EXCHANGES + 5) { chat = chat.withReply(ChatExchange.insufficient("q$it"), null) }
        assertEquals(ChatConversation.MAX_EXCHANGES, chat.exchanges.size)
        assertEquals("q${ChatConversation.MAX_EXCHANGES + 4}", chat.exchanges.last().question)
    }
}
