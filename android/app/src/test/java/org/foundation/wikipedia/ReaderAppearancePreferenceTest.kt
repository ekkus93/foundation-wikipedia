package org.foundation.wikipedia

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ReaderAppearancePreferenceTest {
    @Test
    fun allThemesRoundTripWithVersionedEncoding() {
        for (mode in ReaderThemeMode.entries) {
            assertEquals(mode, ReaderAppearancePreference.decode(ReaderAppearancePreference.encode(mode)))
        }
        assertEquals("v1:system", ReaderAppearancePreference.encode(ReaderThemeMode.SYSTEM))
        assertEquals("v1:light", ReaderAppearancePreference.encode(ReaderThemeMode.LIGHT))
        assertEquals("v1:dark", ReaderAppearancePreference.encode(ReaderThemeMode.DARK))
    }

    @Test
    fun themeChangesOnlyTakeEffectAfterDurableWrite() {
        val writes = mutableListOf<String>()
        val success = ReaderAppearancePreference.update(ReaderThemeMode.SYSTEM, ReaderThemeMode.DARK) {
            writes.add(it)
            true
        }
        assertEquals(listOf("v1:dark"), writes)
        assertTrue(success.saved)
        assertEquals(ReaderThemeMode.DARK, success.mode)
        for (requested in ReaderThemeMode.entries) {
            val failed = ReaderAppearancePreference.update(ReaderThemeMode.LIGHT, requested) { false }
            assertFalse(failed.saved)
            assertEquals(ReaderThemeMode.LIGHT, failed.mode)
            val throwing = ReaderAppearancePreference.update(ReaderThemeMode.LIGHT, requested) {
                throw IllegalStateException("storage unavailable")
            }
            assertFalse(throwing.saved)
            assertEquals(ReaderThemeMode.LIGHT, throwing.mode)
        }
    }

    @Test
    fun absentUnknownOrCorruptValuesUseSystemAppearance() {
        for (value in listOf(null, "", "dark", "v2:dark", "v1:DARK", "v1:light\\n")) {
            assertEquals(ReaderThemeMode.SYSTEM, ReaderAppearancePreference.decode(value))
        }
    }
}
