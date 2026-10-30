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
}
