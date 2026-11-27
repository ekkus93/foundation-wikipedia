package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class AvStreamTest {
    private val store = "https://upload.wikimedia.org/wikipedia/commons"

    @Test
    fun aPlayOnlineLinkNamesItsFileAndKind() {
        assertEquals(
            AvFile("Night_of_the_Living_Dead_(1968).webm", video = true),
            AvStream.fromCommonsPage(
                "https://commons.wikimedia.org/wiki/File:Night_of_the_Living_Dead_(1968).webm",
            ),
        )
        assertEquals(
            AvFile("Tom_&_Jerry_100%.OGG", video = false),
            AvStream.fromCommonsPage("https://commons.wikimedia.org/wiki/File:Tom_&_Jerry_100%25.OGG"),
        )
        assertEquals("Tom & Jerry 100%.OGG", AvFile("Tom_&_Jerry_100%.OGG", false).title)
        for (other in listOf(
            "https://commons.wikimedia.org/wiki/File:Moon.jpg",
            "https://commons.wikimedia.org/wiki/Category:Films.webm",
            "https://commons.wikimedia.org/wiki/File:a%2Fb.webm",
            "https://commons.wikimedia.org/wiki/File:Film.webm?action=raw",
            "https://commons.wikimedia.org:8443/wiki/File:Film.webm",
            "https://user@commons.wikimedia.org/wiki/File:Film.webm",
            "http://commons.wikimedia.org/wiki/File:Film.webm",
            "https://example.org/wiki/File:Film.webm",
            "not a url",
        )) {
            assertNull(other, AvStream.fromCommonsPage(other))
        }
    }

    /** These addresses were checked against Wikimedia on 2026-11-26. */
    @Test
    fun streamsAreSmallRenderingsFirstThenTheFileItself() {
        val film = "Night_of_the_Living_Dead_%281968%29.webm"
        assertEquals(
            listOf(
                "$store/transcoded/2/24/$film/$film.480p.vp9.webm",
                "$store/transcoded/2/24/$film/$film.360p.vp9.webm",
                "$store/transcoded/2/24/$film/$film.240p.vp9.webm",
                "$store/2/24/$film",
            ),
            AvStream.candidates(AvFile("Night_of_the_Living_Dead_(1968).webm", video = true)),
        )
        val speech = "Pronunciation_of_Latin_%28Text%29.ogg"
        assertEquals(
            listOf("$store/transcoded/1/12/$speech/$speech.mp3", "$store/1/12/$speech"),
            AvStream.candidates(AvFile("Pronunciation_of_Latin_(Text).ogg", video = false)),
        )
        assertEquals(1, AvStream.candidates(AvFile("Song.mp3", video = false)).size)
    }

    @Test
    fun everyStreamIsOnWikimediasMediaHostWhateverTheName() {
        for (name in listOf("Ünï cödé.webm", "a b&c=d?e#f.ogg", "x\"y<z>.webm", "..%2F..%2Fetc.ogg")) {
            for (url in AvStream.candidates(AvFile(name, video = name.endsWith("webm")))) {
                val uri = java.net.URI(url)
                assertEquals("https", uri.scheme)
                assertEquals(AvStream.HOST, uri.host)
                assertNull(uri.rawQuery)
                assertNull(uri.rawFragment)
                assertTrue(url, uri.rawPath.startsWith("/wikipedia/commons/"))
                assertTrue(url, !uri.rawPath.contains("..") || name.startsWith(".."))
                assertTrue(url, uri.rawPath.all { it.isLetterOrDigit() || it in "-_.~%/" })
            }
        }
    }
}
