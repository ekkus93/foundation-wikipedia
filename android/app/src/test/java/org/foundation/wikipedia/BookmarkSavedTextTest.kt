package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class BookmarkSavedTextTest {
    @Test
    fun aSavedBookmarkShowsItsDateFromTheFormatterInMilliseconds() {
        var seen = 0L
        val text = bookmarkSavedText(1_795_651_200L) { millis ->
            seen = millis
            "26 Nov 2026"
        }
        assertEquals("Saved 26 Nov 2026", text)
        assertEquals(1_795_651_200_000L, seen)
    }

    @Test
    fun aBookmarkSavedBeforeDatesWereKeptShowsNothing() {
        assertNull(bookmarkSavedText(null) { error("must not format") })
        assertNull(bookmarkSavedText(0L) { error("must not format") })
    }
}
