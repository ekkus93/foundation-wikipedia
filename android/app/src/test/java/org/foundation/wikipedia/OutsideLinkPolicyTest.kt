package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OutsideLinkPolicyTest {
    private val film = "https://commons.wikimedia.org/wiki/File:Night_of_the_Living_Dead_(1968).webm"
    private val page = "https://www.openstreetmap.org/?mlat=48.85&mlon=2.29"

    @Test
    fun onlyCommonsAudioAndVideoPagesAreMedia() {
        assertTrue(isCommonsAudioOrVideo(film))
        assertTrue(isCommonsAudioOrVideo("https://commons.wikimedia.org/wiki/File:Speech.OGG"))
        for (other in listOf(
            page,
            "https://commons.wikimedia.org/wiki/File:Moon.jpg",
            "https://commons.wikimedia.org/wiki/Category:Films.webm",
            "http://commons.wikimedia.org/wiki/File:Film.webm",
            "https://commons.wikimedia.org.example/wiki/File:Film.webm",
            "https://example.org/wiki/File:Film.webm",
            "not a url",
        )) {
            assertFalse(other, isCommonsAudioOrVideo(other))
        }
    }

    @Test
    fun nothingIsOfferedInOfflineModeOrWithoutANetwork() {
        for (url in listOf(film, page)) {
            for (network in DownloadNetwork.entries) {
                assertTrue(
                    outsideLinkGate(url, workingOffline = true, network, allowMobileData = true)
                        is OutsideLink.Refuse,
                )
            }
            assertTrue(
                outsideLinkGate(url, false, DownloadNetwork.NONE, allowMobileData = true)
                    is OutsideLink.Refuse,
            )
        }
        assertEquals(
            OutsideLink.Refuse("Playing this needs an Internet connection."),
            outsideLinkGate(film, false, DownloadNetwork.NONE, false),
        )
        assertEquals(
            OutsideLink.Refuse("Offline mode is on. Go online to play this."),
            outsideLinkGate(film, true, DownloadNetwork.UNMETERED, false),
        )
    }

    @Test
    fun audioAndVideoFollowTheWifiOnlySettingButPagesDoNot() {
        assertTrue(
            outsideLinkGate(film, false, DownloadNetwork.METERED, allowMobileData = false)
                is OutsideLink.Refuse,
        )
        assertEquals(
            OutsideLink.Offer("This plays from Wikimedia.", film),
            outsideLinkGate(film, false, DownloadNetwork.METERED, allowMobileData = true),
        )
        assertEquals(
            OutsideLink.Offer("This plays from Wikimedia.", film),
            outsideLinkGate(film, false, DownloadNetwork.UNMETERED, allowMobileData = false),
        )
        assertEquals(
            OutsideLink.Offer("This link leads outside the offline reader.", page),
            outsideLinkGate(page, false, DownloadNetwork.METERED, allowMobileData = false),
        )
    }
}
