package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class OfficialPackPolicyTest {
    private fun gate(
        disclosure: Boolean = true,
        offline: Boolean = false,
        network: DownloadNetwork = DownloadNetwork.UNMETERED,
        mobile: Boolean = false,
        needed: Long? = null,
        available: Long? = null,
    ) = officialDownloadGate(disclosure, offline, network, mobile, needed, available)

    @Test
    fun theDisclosureComesBeforeAnythingElse() {
        assertEquals(
            DownloadGate.NeedsDisclosure,
            gate(disclosure = false, offline = true, network = DownloadNetwork.NONE),
        )
    }

    @Test
    fun offlineModeAndMissingNetworkBlock() {
        assertEquals(DownloadGate.WorkingOffline, gate(offline = true))
        assertEquals(DownloadGate.NoNetwork, gate(network = DownloadNetwork.NONE))
    }

    @Test
    fun mobileDataNeedsExplicitPermission() {
        assertEquals(DownloadGate.WifiOnly, gate(network = DownloadNetwork.METERED))
        assertEquals(DownloadGate.Allowed, gate(network = DownloadNetwork.METERED, mobile = true))
        assertEquals(DownloadGate.Allowed, gate(network = DownloadNetwork.UNMETERED))
    }

    @Test
    fun freeSpaceIsCheckedAgainstTheEstimate() {
        assertEquals(
            DownloadGate.NotEnoughSpace(2_000, 1_999),
            gate(needed = 2_000, available = 1_999),
        )
        assertEquals(DownloadGate.Allowed, gate(needed = 2_000, available = 2_000))
        val message = downloadGateMessage(gate(needed = 3L shl 30, available = 1L shl 30))!!
        assertTrue(message, message.contains("3.0 GB") && message.contains("1.0 GB"))
    }

    @Test
    fun preferencesDefaultToOff() {
        assertFalse(OfficialPackPreference.disclosureAccepted(null))
        assertFalse(OfficialPackPreference.mobileDataAllowed(null))
        assertTrue(OfficialPackPreference.disclosureAccepted(OfficialPackPreference.encodeDisclosure()))
        assertTrue(
            OfficialPackPreference.mobileDataAllowed(OfficialPackPreference.encodeMobileData(true)),
        )
        assertFalse(
            OfficialPackPreference.mobileDataAllowed(OfficialPackPreference.encodeMobileData(false)),
        )
        assertNull(downloadGateMessage(DownloadGate.Allowed))
    }

    @Test
    fun theDisclosureNamesTheHostAndWhatItSees() {
        assertTrue(OFFICIAL_PACK_DISCLOSURE.contains("github.com"))
        assertTrue(OFFICIAL_PACK_DISCLOSURE.contains("IP address"))
    }
}
