package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ReaderLinksTest {
    private val current = "https://foundation.invalid/wiki/Moon"

    @Test
    fun articleLinksResolveToTitlesWithFragments() {
        assertEquals(
            ReaderLink.Article("Isaac Newton", null),
            ReaderLinks.classify("https://foundation.invalid/wiki/Isaac_Newton", current),
        )
        assertEquals(
            ReaderLink.Article("General relativity", "History"),
            ReaderLinks.classify("https://foundation.invalid/wiki/General_relativity#History", current),
        )
        assertEquals(
            ReaderLink.Article("AC/DC", null),
            ReaderLinks.classify("https://foundation.invalid/wiki/AC/DC", current),
        )
        assertEquals(
            ReaderLink.Article("Éclair", null),
            ReaderLinks.classify("https://foundation.invalid/wiki/%C3%89clair", current),
        )
    }

    @Test
    fun anchorsInTheCurrentDocumentStayInPage() {
        assertEquals(
            ReaderLink.InPage("cite_note-1"),
            ReaderLinks.classify("https://foundation.invalid/wiki/Moon#cite_note-1", current),
        )
        assertEquals(ReaderLink.Blocked, ReaderLinks.classify(current, current))
    }

    @Test
    fun outsideLinksAreExternalAndEverythingElseIsBlocked() {
        assertEquals(
            ReaderLink.External("https://example.org/a?b=c"),
            ReaderLinks.classify("https://example.org/a?b=c", current),
        )
        for (blocked in listOf(
            "javascript:alert(1)",
            "file:///data/data/org.foundation.wikipedia/x",
            "content://x/y",
            "intent://x#Intent;end",
            "http://foundation.invalid/wiki/Moon",
            "https://foundation.invalid:8443/wiki/Moon",
            "https://foundation.invalid/w/index.php?title=Moon",
            "https://foundation.invalid/wiki/Moon?action=raw",
            "https://foundation.invalid/wiki/",
            "https://user@example.org/",
            "not a url",
        )) {
            assertEquals(blocked, ReaderLink.Blocked, ReaderLinks.classify(blocked, current))
        }
    }

    @Test
    fun onlyTheCurrentDocumentIsServed() {
        val doc = "https://foundation.invalid/wiki/%C3%89clair"
        assertTrue(ReaderLinks.isDocumentRequest(doc, true, doc))
        assertTrue(ReaderLinks.isDocumentRequest("https://foundation.invalid/wiki/Éclair", true, doc))
        assertFalse(ReaderLinks.isDocumentRequest(doc, false, doc))
        assertFalse(ReaderLinks.isDocumentRequest(doc, true, null))
        for (other in listOf(
            "https://foundation.invalid/wiki/Moon",
            "https://foundation.invalid/wiki/%C3%89clair?x=1",
            "http://foundation.invalid/wiki/%C3%89clair",
            "https://foundation.invalid:444/wiki/%C3%89clair",
            "https://example.org/wiki/%C3%89clair",
            "https://foundation.invalid/favicon.ico",
        )) {
            assertFalse(other, ReaderLinks.isDocumentRequest(other, true, doc))
        }
    }

    @Test
    fun theBackStackIsBoundedDeduplicatedAndRestorable() {
        val moon = ReaderEntry("enwiki", 1, "Moon")
        val tide = ReaderEntry("enwiki", 2, "Tide")
        var nav = ReaderNavigation().open(moon).open(moon).open(tide)
        assertEquals(listOf(moon, tide), nav.entries)
        assertTrue(nav.canGoBack)
        nav = nav.back()
        assertEquals(moon, nav.current)
        assertFalse(nav.canGoBack)
        assertEquals(nav, nav.back())

        var long = ReaderNavigation()
        for (id in 1L..80L) long = long.open(ReaderEntry("enwiki", id, "A$id"))
        assertEquals(ReaderNavigation.MAX_DEPTH, long.entries.size)
        assertEquals(80L, long.current?.pageId)

        val restored = ReaderNavigation.restore(ReaderNavigation().open(moon).open(tide).save())
        assertEquals(listOf(moon, tide), restored.entries)
        assertNull(ReaderNavigation.restore(listOf("enwiki", "x", "Moon")).current)
        assertNull(ReaderNavigation.restore(listOf("enwiki", "1")).current)
        assertNull(ReaderNavigation.restore(listOf("enwiki", "-1", "Moon")).current)
        assertNull(ReaderNavigation.restore(null).current)
    }
}
