package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ReaderScrollTest {
    private val tide = ReaderEntry("enwiki", 2, "Tide")
    private val moon = ReaderEntry("enwiki", 3, "Moon")

    @Test
    fun positionsAreFractionsOfTheScrollableLength() {
        assertEquals(0.5f, ReaderScroll.fraction(scrollY = 500, contentPx = 2000, viewportPx = 1000), 1e-6f)
        assertEquals(0f, ReaderScroll.fraction(0, 800, 1000))
        assertEquals(1f, ReaderScroll.fraction(5000, 2000, 1000))
        assertEquals(1500, ReaderScroll.offset(0.5f, contentPx = 4000, viewportPx = 1000))
        assertEquals(0, ReaderScroll.offset(0.7f, 800, 1000))
    }

    @Test
    fun positionsAreKeptPerHistoryEntryAndSurviveSaving() {
        val scroll = ReaderScroll()
            .with("enwiki", 2, 0.25f, listOf(tide))
            .with("enwiki", 3, 0.75f, listOf(tide, moon))
        assertEquals(0.25f, scroll.at("enwiki", 2))
        assertEquals(0.75f, scroll.at("enwiki", 3))
        assertEquals(scroll, ReaderScroll.restore(scroll.save()))
        // Entries that left the history are dropped.
        val trimmed = scroll.with("enwiki", 3, 0.5f, listOf(moon))
        assertNull(trimmed.at("enwiki", 2))
        assertEquals(1.2f.coerceAtMost(1f), scroll.with("enwiki", 2, 1.2f, listOf(tide)).at("enwiki", 2))
    }

    @Test
    fun malformedSavedStateRestoresNothing() {
        for (bad in listOf(null, listOf("enwiki:2"), listOf("enwiki:2", "x"), listOf("enwiki:2", "NaN"), listOf("enwiki:2", "2.0"), listOf(7, "0.5"))) {
            assertEquals("$bad", ReaderScroll(), ReaderScroll.restore(bad))
        }
    }
}
