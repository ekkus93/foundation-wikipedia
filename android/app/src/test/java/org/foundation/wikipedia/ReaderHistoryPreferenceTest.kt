package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReaderHistoryPreferenceTest {
    @Test
    fun historyConsentDefaultsOffAndOnlyExplicitVersionedOptInEnablesIt() {
        assertFalse(ReaderHistoryPreference.decode(null))
        assertEquals("v1:enabled", ReaderHistoryPreference.encode(true))
        assertEquals("v1:disabled", ReaderHistoryPreference.encode(false))
        assertTrue(ReaderHistoryPreference.decode(ReaderHistoryPreference.encode(true)))
        assertFalse(ReaderHistoryPreference.decode(ReaderHistoryPreference.encode(false)))
        for (value in listOf("", "true", "1", "v2:enabled", "v1:ENABLED", "v1:enabled ", "v1:enabled\\n")) {
            assertFalse("unexpected history consent for $value", ReaderHistoryPreference.decode(value))
        }
    }

    @Test
    fun consentIsEnabledOnlyAfterSuccessfulSynchronousPersistence() {
        val written = mutableListOf<String>()
        val enabled = ReaderHistoryPreference.update(true) {
            written.add(it)
            true
        }
        assertEquals(listOf("v1:enabled"), written)
        assertTrue(enabled.saved)
        assertTrue(enabled.enabled)

        val disabled = ReaderHistoryPreference.update(false) {
            written.add(it)
            true
        }
        assertEquals(listOf("v1:enabled", "v1:disabled"), written)
        assertTrue(disabled.saved)
        assertFalse(disabled.enabled)
    }

    @Test
    fun failedAndThrowingWritesFailClosedForOptInAndRevocation() {
        for (requested in listOf(true, false)) {
            val rejected = ReaderHistoryPreference.update(requested) { false }
            assertFalse(rejected.saved)
            assertFalse(rejected.enabled)

            val throwing = ReaderHistoryPreference.update(requested) {
                throw IllegalStateException("simulated storage failure")
            }
            assertFalse(throwing.saved)
            assertFalse(throwing.enabled)
        }
    }
}
