package org.foundation.wikipedia

import java.net.URI
import java.net.URISyntaxException

/** What tapping a link that leads outside the offline reader does. */
sealed interface OutsideLink {
    /** Offer to open it in the browser. */
    data class Offer(val message: String, val url: String) : OutsideLink

    /** Say why it cannot be opened now; nothing is requested. */
    data class Refuse(val message: String) : OutsideLink
}

private val AV_EXTENSIONS = setOf(
    "webm", "ogv", "ogg", "oga", "opus", "mp3", "wav", "flac", "mid", "mpg", "mpeg", "mp4",
)

/**
 * Whether [url] is the Wikimedia Commons page of an audio or video file:
 * the "Play online" link the reader shows in place of a player (MOBPACK-003).
 */
fun isCommonsAudioOrVideo(url: String): Boolean = try {
    val uri = URI(url)
    val path = uri.path.orEmpty()
    uri.scheme == "https" && uri.host == "commons.wikimedia.org" &&
        path.startsWith("/wiki/File:") &&
        path.substringAfterLast('.', "").lowercase() in AV_EXTENSIONS
} catch (_: URISyntaxException) {
    false
}

/**
 * MOBPACK-003: an outside link is offered only when it could work. In
 * Offline mode or with no network the reader says a connection is needed;
 * audio and video also follow the Wi-Fi-only setting, since playing them
 * streams far more than a page.
 */
fun outsideLinkGate(
    url: String,
    workingOffline: Boolean,
    network: DownloadNetwork,
    allowMobileData: Boolean,
): OutsideLink {
    val media = isCommonsAudioOrVideo(url)
    return when {
        workingOffline -> OutsideLink.Refuse(
            if (media) "Offline mode is on. Go online to play this."
            else "Offline mode is on. Go online to open this link.",
        )
        network == DownloadNetwork.NONE -> OutsideLink.Refuse(
            if (media) "Playing this needs an Internet connection."
            else "This link needs an Internet connection.",
        )
        media && network == DownloadNetwork.METERED && !allowMobileData -> OutsideLink.Refuse(
            "Wi-Fi only is set. Connect to Wi-Fi, or allow mobile data in " +
                "Settings › Privacy & Network, to play this.",
        )
        media -> OutsideLink.Offer("This plays from Wikimedia.", url)
        else -> OutsideLink.Offer("This link leads outside the offline reader.", url)
    }
}
