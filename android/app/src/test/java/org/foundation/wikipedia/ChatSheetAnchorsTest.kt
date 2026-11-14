package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Test

class ChatSheetAnchorsTest {
    private val anchors = ChatSheetAnchors.forSpace(spacePx = 2000f, peekPx = 350f)
    private val fling = 1500f

    @Test
    fun anchorsArePeekHalfAndFullOfTheSpace() {
        assertEquals(ChatSheetAnchors(350f, 1000f, 2000f), anchors)
        // A short window never puts half or full below peek.
        assertEquals(ChatSheetAnchors(300f, 300f, 300f), ChatSheetAnchors.forSpace(300f, 350f))
        assertEquals(ChatSheetAnchors(350f, 350f, 500f), ChatSheetAnchors.forSpace(500f, 350f))
    }

    @Test
    fun aSlowReleaseSettlesAtTheNearestHeight() {
        assertEquals(ChatSheetHeight.PEEK, anchors.settle(600f, 0f, fling))
        assertEquals(ChatSheetHeight.HALF, anchors.settle(700f, 100f, fling))
        assertEquals(ChatSheetHeight.HALF, anchors.settle(1400f, -100f, fling))
        assertEquals(ChatSheetHeight.FULL, anchors.settle(1600f, 0f, fling))
    }

    @Test
    fun aFlingMovesOneStepInItsDirection() {
        assertEquals(ChatSheetHeight.HALF, anchors.settle(400f, 3000f, fling))
        assertEquals(ChatSheetHeight.FULL, anchors.settle(1100f, 3000f, fling))
        assertEquals(ChatSheetHeight.FULL, anchors.settle(2000f, 3000f, fling))
        assertEquals(ChatSheetHeight.HALF, anchors.settle(1900f, -3000f, fling))
        assertEquals(ChatSheetHeight.PEEK, anchors.settle(900f, -3000f, fling))
        assertEquals(ChatSheetHeight.PEEK, anchors.settle(350f, -3000f, fling))
    }
}
