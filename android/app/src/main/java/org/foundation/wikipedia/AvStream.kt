package org.foundation.wikipedia

import java.net.URI
import java.net.URISyntaxException
import java.security.MessageDigest

/** An audio or video file on Wikimedia Commons, by its file name. */
data class AvFile(val name: String, val video: Boolean) {
    /** The name as a reader sees it. */
    val title: String get() = name.replace('_', ' ')
}

/**
 * MOBPACK-003: where Wikimedia serves an audio or video file. Packs hold no
 * audio or video, only the "Play online" link to the file's Commons page;
 * the addresses to stream are worked out from the file name, the way
 * Wikimedia lays out its media store (two directories from the MD5 of the
 * name). Pure, so it is unit-tested without Android.
 */
object AvStream {
    const val HOST = "upload.wikimedia.org"
    private const val STORE = "https://$HOST/wikipedia/commons"
    private const val PAGE_PREFIX = "/wiki/File:"
    private const val MAX_NAME_LENGTH = 255
    private val VIDEO = setOf("webm", "ogv", "mpg", "mpeg", "mp4")
    private val AUDIO = setOf("ogg", "oga", "opus", "mp3", "wav", "flac", "mid")

    /** Smaller renderings Wikimedia makes of a video, best for a phone first. */
    private val VIDEO_RENDERINGS = listOf("480p.vp9.webm", "360p.vp9.webm", "240p.vp9.webm")

    /** The file a "Play online" link names, or `null` for any other link. */
    fun fromCommonsPage(url: String): AvFile? {
        val uri = try {
            URI(url)
        } catch (_: URISyntaxException) {
            return null
        }
        val path = uri.path ?: return null
        if (uri.scheme != "https" || uri.host != "commons.wikimedia.org" ||
            uri.port != -1 || uri.userInfo != null || uri.rawQuery != null ||
            !path.startsWith(PAGE_PREFIX)
        ) {
            return null
        }
        val name = path.removePrefix(PAGE_PREFIX).replace(' ', '_')
        val extension = name.substringAfterLast('.', "").lowercase()
        val valid = name.isNotEmpty() && name.length <= MAX_NAME_LENGTH &&
            name.none { it.isISOControl() || it in "/#|<>\"?" }
        return when {
            !valid -> null
            extension in VIDEO -> AvFile(name, video = true)
            extension in AUDIO -> AvFile(name, video = false)
            else -> null
        }
    }

    private fun encode(name: String): String = buildString {
        for (byte in name.toByteArray(Charsets.UTF_8)) {
            val c = byte.toInt().toChar()
            if (byte >= 0 && (c.isLetterOrDigit() || c in "-_.~")) {
                append(c)
            } else {
                append('%').append("%02X".format(byte.toInt() and 0xFF))
            }
        }
    }

    /**
     * Addresses to try, in order: a rendering every Android phone can play
     * (VP9 video no larger than 480p; MP3 audio), then the file itself.
     * All are on [HOST].
     */
    fun candidates(file: AvFile): List<String> {
        val digest = MessageDigest.getInstance("MD5")
            .digest(file.name.toByteArray(Charsets.UTF_8))
            .joinToString("") { "%02x".format(it) }
        val directory = "${digest[0]}/${digest.substring(0, 2)}"
        val name = encode(file.name)
        val original = "$STORE/$directory/$name"
        val renderings = when {
            file.video -> VIDEO_RENDERINGS
            file.name.endsWith(".mp3", ignoreCase = true) -> emptyList()
            else -> listOf("mp3")
        }
        return renderings.map { "$STORE/transcoded/$directory/$name/$name.$it" } + original
    }
}

/** Whether the one-time notice about streaming from Wikimedia was accepted. */
internal object AvPreference {
    const val DISCLOSURE_KEY = "av_stream_disclosure"
    private const val ACCEPTED = "v1:accepted"

    fun disclosureAccepted(value: String?): Boolean = value == ACCEPTED
    fun encodeDisclosure(): String = ACCEPTED
}

/** Shown once, before the app first contacts Wikimedia's media servers. */
internal const val AV_STREAM_DISCLOSURE =
    "Audio and video are not stored in packs. To play this, the app connects to Wikimedia's " +
        "media servers (upload.wikimedia.org). Wikimedia will see your IP address and which " +
        "file you play, as it would in a browser. Nothing else is sent, and nothing is saved " +
        "on this device. This notice is shown once."
