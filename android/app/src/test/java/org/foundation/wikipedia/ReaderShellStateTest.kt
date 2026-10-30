package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReaderShellStateTest {
    @Test
    fun settingsCategoriesNavigateBackBeforeReturningToReader() {
        val reader = ReaderShellState(offline = true)
        val root = reader.showSettings()
        assertTrue(root.settingsOpen)
        assertEquals(null, root.settingsCategory)
        assertEquals(reader, root.openSettingsCategory(ReaderSettingsCategory.STORAGE).back().back())
        val detail = root.openSettingsCategory(ReaderSettingsCategory.WIKIPEDIA_OFFLINE_DATA)
        assertEquals(ReaderSettingsCategory.WIKIPEDIA_OFFLINE_DATA, detail.settingsCategory)
        assertEquals(root, detail.back())
        assertEquals(reader, detail.back().back())
        assertEquals(reader, reader.openSettingsCategory(ReaderSettingsCategory.ADVANCED))
    }

    @Test
    fun settingsSubpageRecreationUsesStableNamesAndRejectsUnknownCategories() {
        val before = ReaderShellState(offline = true).showSettings()
            .openSettingsCategory(ReaderSettingsCategory.PRIVACY_NETWORK)
        val restored = ReaderShellState.restore(
            before.offline, before.settingsOpen, before.settingsCategory?.name,
        )
        assertEquals(before, restored)
        assertEquals(null, ReaderShellState.restore(true, true, "REMOVED").settingsCategory)
        assertEquals(null, ReaderShellState.restore(true, false, "STORAGE").settingsCategory)
        assertFalse(restored.actionsOpen)
    }

    @Test
    fun persistedOfflinePreferenceRoundTripsAndDefaultsOnlineFirst() {
        assertFalse(ReaderOfflinePreference.decode(null))
        assertEquals("v1:offline", ReaderOfflinePreference.encode(true))
        assertEquals("v1:online", ReaderOfflinePreference.encode(false))
        assertTrue(ReaderOfflinePreference.decode(ReaderOfflinePreference.encode(true)))
        assertFalse(ReaderOfflinePreference.decode(ReaderOfflinePreference.encode(false)))
    }

    @Test
    fun offlineModeOnlyGoesOnlineAfterSuccessfulPersistence() {
        val recorded = mutableListOf<String>()
        val online = ReaderOfflinePreference.update(false) {
            recorded.add(it)
            true
        }
        assertEquals(listOf("v1:online"), recorded)
        assertTrue(online.saved)
        assertFalse(online.offline)

        val offline = ReaderOfflinePreference.update(true) {
            recorded.add(it)
            true
        }
        assertEquals(listOf("v1:online", "v1:offline"), recorded)
        assertTrue(offline.saved)
        assertTrue(offline.offline)
    }

    @Test
    fun failedOfflinePreferenceWritesNeverEnableNetworkMode() {
        for (requested in listOf(true, false)) {
            val failed = ReaderOfflinePreference.update(requested) { false }
            assertFalse(failed.saved)
            assertTrue(failed.offline)

            val throwing = ReaderOfflinePreference.update(requested) {
                throw IllegalStateException("simulated storage failure")
            }
            assertFalse(throwing.saved)
            assertTrue(throwing.offline)
        }
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
    fun outsideTapDismissesActionsWithoutChangingReaderOrSettings() {
        val reader = ReaderShellState(offline = true)
        val expanded = reader.toggleActions()
        assertTrue(expanded.actionsOpen)
        assertEquals(reader, expanded.dismissActions())
        assertEquals(reader, reader.dismissActions())
        val settings = reader.showSettings()
        assertEquals(settings, settings.dismissActions())
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
    @Test
    fun chatActionOpensDismissibleSheetWithoutChangingOfflineState() {
        val reader = ReaderShellState(offline = true)
        val chat = reader.toggleActions().showChat()
        assertTrue(chat.chatOpen)
        assertFalse(chat.actionsOpen)
        assertTrue(chat.offline)
        assertEquals(reader, chat.closeChat())
        assertEquals(reader, chat.back())
        assertEquals(chat, chat.toggleActions())
    }

    @Test
    fun settingsAndChatAreMutuallyExclusiveAndRecreationClosesChat() {
        val reader = ReaderShellState()
        val chat = reader.showChat()
        assertFalse(chat.showSettings().chatOpen)
        val settings = reader.showSettings()
        assertEquals(settings, settings.showChat())
        val restored = ReaderShellState.restore(
            chat.offline, chat.settingsOpen, chat.settingsCategory?.name,
        )
        assertFalse(restored.chatOpen)
        assertEquals(reader, restored)
    }

}