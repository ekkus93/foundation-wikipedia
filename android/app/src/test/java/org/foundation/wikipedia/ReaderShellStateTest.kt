package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReaderShellStateTest {
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
