package org.foundation.wikipedia

import android.provider.Settings
import androidx.compose.animation.core.animateDpAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.spring
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.ime
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

/** The chat sheet's height at each state, in pixels of the space it opens in. */
data class ChatSheetAnchors(val peek: Float, val half: Float, val full: Float) {
    fun of(height: ChatSheetHeight): Float = when (height) {
        ChatSheetHeight.PEEK -> peek
        ChatSheetHeight.HALF -> half
        ChatSheetHeight.FULL -> full
    }

    /**
     * Where a drag released at [heightPx] settles: a fling (upward velocity
     * is positive here) moves one step that way, otherwise the nearest state.
     */
    fun settle(heightPx: Float, velocityPx: Float, flingPx: Float): ChatSheetHeight {
        val ordered = ChatSheetHeight.entries.sortedBy { of(it) }
        if (velocityPx > flingPx) return ordered.firstOrNull { of(it) > heightPx } ?: ordered.last()
        if (velocityPx < -flingPx) return ordered.lastOrNull { of(it) < heightPx } ?: ordered.first()
        return ordered.minBy { kotlin.math.abs(of(it) - heightPx) }
    }

    companion object {
        val PEEK_HEIGHT: Dp = 152.dp

        fun forSpace(spacePx: Float, peekPx: Float): ChatSheetAnchors {
            val peek = peekPx.coerceAtMost(spacePx)
            return ChatSheetAnchors(peek, (spacePx / 2).coerceAtLeast(peek), spacePx.coerceAtLeast(peek))
        }
    }
}

private fun ChatSheetHeight.label() = when (this) {
    ChatSheetHeight.PEEK -> "collapsed"
    ChatSheetHeight.HALF -> "half height"
    ChatSheetHeight.FULL -> "full height"
}

/**
 * A non-modal bottom sheet with peek, half and full heights over the reader
 * (MOB-003). Its content stays composed at every height, so a request in
 * flight continues while the sheet is lowered. Drag the handle to resize, or
 * tap it to step up; TalkBack offers Expand/Collapse actions on it. Motion is
 * skipped when the system turns animations off.
 */
@Composable
fun ChatSheetFrame(
    height: ChatSheetHeight,
    onHeight: (ChatSheetHeight) -> Unit,
    modifier: Modifier = Modifier,
    content: @Composable (compact: Boolean) -> Unit,
) {
    val context = LocalContext.current
    val density = LocalDensity.current
    val reduceMotion = remember {
        Settings.Global.getFloat(context.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f
    }
    BoxWithConstraints(modifier = modifier.fillMaxSize().imePadding(), contentAlignment = Alignment.BottomCenter) {
        val space = constraints.maxHeight.toFloat()
        val anchors = ChatSheetAnchors.forSpace(space, with(density) { ChatSheetAnchors.PEEK_HEIGHT.toPx() })
        var dragging by remember { mutableStateOf(false) }
        var dragged by remember { mutableFloatStateOf(0f) }
        // Typing at half height would leave too little room above the
        // keyboard, so the sheet uses all of it while the keyboard is open.
        val typing = WindowInsets.ime.getBottom(density) > 0
        val shown = if (typing && height != ChatSheetHeight.PEEK) ChatSheetHeight.FULL else height
        val target = if (dragging) dragged else anchors.of(shown)
        val animated by animateDpAsState(
            targetValue = with(density) { target.toDp() },
            animationSpec = if (dragging || reduceMotion) snap() else spring(dampingRatio = 0.85f, stiffness = 500f),
            label = "chat sheet height",
        )
        val drag = rememberDraggableState { delta ->
            dragged = (dragged - delta).coerceIn(anchors.peek, anchors.full)
        }
        val flingPx = with(density) { 600.dp.toPx() }
        val steps = ChatSheetHeight.entries
        Surface(
            modifier = Modifier.fillMaxWidth().height(animated),
            shape = RoundedCornerShape(topStart = 28.dp, topEnd = 28.dp),
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            tonalElevation = 3.dp,
            shadowElevation = 8.dp,
        ) {
            Column {
                Box(
                    modifier = Modifier
                        .fillMaxWidth()
                        // 48 dp: the minimum touch target.
                        .height(48.dp)
                        .draggable(
                            state = drag,
                            orientation = Orientation.Vertical,
                            onDragStarted = {
                                dragged = anchors.of(shown)
                                dragging = true
                            },
                            onDragStopped = { velocity ->
                                dragging = false
                                // Screen velocity is positive downward.
                                onHeight(anchors.settle(dragged, -velocity, flingPx))
                            },
                        )
                        .clickable(role = Role.Button) {
                            onHeight(if (height == ChatSheetHeight.FULL) ChatSheetHeight.HALF else steps[height.ordinal + 1])
                        }
                        .semantics {
                            contentDescription = "Chat sheet size"
                            stateDescription = height.label()
                            customActions = buildList {
                                if (height != ChatSheetHeight.FULL) {
                                    add(CustomAccessibilityAction("Expand") { onHeight(steps[height.ordinal + 1]); true })
                                }
                                if (height != ChatSheetHeight.PEEK) {
                                    add(CustomAccessibilityAction("Collapse") { onHeight(steps[height.ordinal - 1]); true })
                                }
                            }
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Box(
                        Modifier
                            .padding(vertical = 12.dp)
                            .size(width = 32.dp, height = 4.dp)
                            .background(MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f), RoundedCornerShape(2.dp)),
                    )
                }
                content(height == ChatSheetHeight.PEEK && !dragging)
            }
        }
    }
}
