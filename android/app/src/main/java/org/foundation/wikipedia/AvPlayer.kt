package org.foundation.wikipedia

import android.net.Uri
import android.widget.VideoView
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.delay

internal const val AV_PLAYER_TAG = "av-player"

/** Sent with every stream request, as Wikimedia asks of automated clients. */
private const val STREAM_USER_AGENT =
    "FoundationWikipedia/0.1 (+https://github.com/ekkus93/foundation-wikipedia)"

/** `95000` → `1:35`, `5753000` → `1:35:53`. */
internal fun playbackClock(millis: Int): String {
    val seconds = (millis.coerceAtLeast(0) / 1000)
    val (h, m, s) = Triple(seconds / 3600, seconds / 60 % 60, seconds % 60)
    return if (h > 0) "%d:%02d:%02d".format(h, m, s) else "%d:%02d".format(m, s)
}

/**
 * MOBPACK-003 in-app player: streams one audio or video file from
 * Wikimedia's media servers after the reader tapped "Play online" (and
 * accepted the one-time notice). Nothing is saved. [streams] are tried in
 * order; when none can be played the reader is told, and offered the browser.
 */
@Composable
fun AvPlayer(
    title: String,
    video: Boolean,
    streams: List<String>,
    onOpenInBrowser: () -> Unit,
    onClose: () -> Unit,
) {
    var attempt by remember { mutableIntStateOf(0) }
    var prepared by remember { mutableStateOf(false) }
    var playing by remember { mutableStateOf(false) }
    var failed by remember { mutableStateOf(streams.isEmpty()) }
    var position by remember { mutableIntStateOf(0) }
    var duration by remember { mutableIntStateOf(0) }
    var seeking by remember { mutableStateOf<Float?>(null) }
    var player by remember { mutableStateOf<VideoView?>(null) }
    var sought by remember { mutableFloatStateOf(0f) }

    // Follow the playback position while the player is open.
    LaunchedEffect(prepared) {
        while (prepared) {
            player?.let { view ->
                position = view.currentPosition
                playing = view.isPlaying
            }
            delay(500)
        }
    }
    DisposableEffect(Unit) {
        onDispose { player?.stopPlayback() }
    }

    Dialog(
        onDismissRequest = onClose,
        properties = DialogProperties(usePlatformDefaultWidth = false),
    ) {
        Column(
            modifier = Modifier.fillMaxSize()
                .background(Color.Black)
                .safeDrawingPadding()
                .testTag(AV_PLAYER_TAG)
                .semantics { paneTitle = if (video) "Video player" else "Audio player" },
        ) {
            Row(
                modifier = Modifier.fillMaxWidth().padding(12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(title, color = Color.White, modifier = Modifier.weight(1f))
                FilledTonalButton(onClick = onClose) { Text("Close") }
            }
            Box(modifier = Modifier.fillMaxWidth().weight(1f), contentAlignment = Alignment.Center) {
                if (!failed) {
                    AndroidView(
                        modifier = Modifier.fillMaxWidth(),
                        factory = { context ->
                            VideoView(context).apply {
                                keepScreenOn = true
                                contentDescription = if (video) "Video" else "Audio"
                                setOnPreparedListener { media ->
                                    duration = media.duration.coerceAtLeast(0)
                                    prepared = true
                                    start()
                                    playing = true
                                }
                                setOnCompletionListener { playing = false }
                                // Try the next address; say so when none is left.
                                setOnErrorListener { _, _, _ ->
                                    prepared = false
                                    if (attempt + 1 < streams.size) attempt += 1 else failed = true
                                    true
                                }
                                player = this
                            }
                        },
                        update = { view ->
                            val stream = streams.getOrNull(attempt)
                            if (stream != null && view.tag != stream) {
                                view.tag = stream
                                view.setVideoURI(
                                    Uri.parse(stream),
                                    mapOf("User-Agent" to STREAM_USER_AGENT),
                                )
                            }
                        },
                    )
                }
                when {
                    failed -> Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        modifier = Modifier.padding(24.dp),
                    ) {
                        Text("This could not be played here.", color = Color.White)
                        TextButton(onClick = onOpenInBrowser) { Text("Open in browser") }
                    }
                    !prepared -> Column(horizontalAlignment = Alignment.CenterHorizontally) {
                        CircularProgressIndicator(
                            modifier = Modifier.semantics { contentDescription = "Loading" },
                        )
                        Text(
                            "Connecting to Wikimedia…",
                            color = Color.White,
                            modifier = Modifier.padding(top = 12.dp),
                        )
                    }
                }
            }
            if (prepared && !failed) {
                Row(
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    FilledTonalButton(onClick = {
                        val view = player ?: return@FilledTonalButton
                        if (view.isPlaying) view.pause() else view.start()
                        playing = view.isPlaying
                    }) { Text(if (playing) "Pause" else "Play") }
                    val shown = seeking?.let { (it * duration).toInt() } ?: position
                    Slider(
                        value = seeking ?: if (duration > 0) position.toFloat() / duration else 0f,
                        onValueChange = {
                            seeking = it
                            sought = it
                        },
                        onValueChangeFinished = {
                            player?.seekTo((sought * duration).toInt())
                            position = (sought * duration).toInt()
                            seeking = null
                        },
                        enabled = duration > 0,
                        modifier = Modifier.weight(1f)
                            .semantics { contentDescription = "Position" },
                    )
                    Text(
                        "${playbackClock(shown)} / ${playbackClock(duration)}",
                        color = Color.White,
                    )
                }
            }
        }
    }
}
