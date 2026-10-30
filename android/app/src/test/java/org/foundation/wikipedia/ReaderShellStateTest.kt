package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReaderShellStateTest {
    @Test
    fun persistedOfflinePreferenceRoundTripsAndDefaultsOnlineFirst() {
        assertFalse(ReaderOfflinePreference.decode(null))
        assertEquals("v1:offline", ReaderOfflinePreference.encode(true))
        assertEquals("v1:online", ReaderOfflinePreference.encode(false))
        assertTrue(ReaderOfflinePreference.decode(ReaderOfflinePreference.encode(true)))
        assertFalse(ReaderOfflinePreference.decode(ReaderOfflinePreference.encode(false)))
    }

    @Test
    fun corruptOrFuturePreferenceFailsClosedToOffline() {
        for (value in listOf("", "true", "1", "v2:online", "v1:OFFLINE", "v1:online\\n")) {
            assertTrue("unexpected online fallback for $value", ReaderOfflinePreference.decode(value))
        }
    }

    @Test
    fun offlineChoiceIsSharedAcrossActionsAndSettings() {
        val start = ReaderShellState()
        val offline = start.toggleActions().withOffline(true)
        assertTrue(offline.offline)
        assertFalse(offline.actionsOpen)
        val settings = offline.showSettings()
        assertTrue(settings.settingsOpen)
        assertTrue(settings.offline)
        assertEquals(settings, settings.toggleActions())
        assertEquals(offline, settings.back())
        assertFalse(settings.withOffline(false).offline)
    }

    @Test
    fun backDismissesActionsBeforeLeavingReaderAndSettings() {
        val expanded = ReaderShellState().toggleActions()
        assertTrue(expanded.actionsOpen)
        assertEquals(ReaderShellState(), expanded.back())
        assertEquals(ReaderShellState(), ReaderShellState().back())
        val settings = expanded.showSettings()
        assertFalse(settings.actionsOpen)
        assertTrue(settings.settingsOpen)
        assertEquals(ReaderShellState(), settings.back())
    }

    @Test
    fun recreationRestoresOfflineAndSettingsButNotExpandedActions() {
        val before = ReaderShellState(offline = true, settingsOpen = true, actionsOpen = true)
        val restored = ReaderShellState.restore(before.offline, before.settingsOpen)
        assertTrue(restored.offline)
        assertTrue(restored.settingsOpen)
        assertFalse(restored.actionsOpen)
        assertFalse(restored.back().settingsOpen)
    }
}
