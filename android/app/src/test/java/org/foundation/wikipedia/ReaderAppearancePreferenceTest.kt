package org.foundation.wikipedia

import org.junit.Assert.assertEquals
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
    fun absentUnknownOrCorruptValuesUseSystemAppearance() {
        for (value in listOf(null, "", "dark", "v2:dark", "v1:DARK", "v1:light\\n")) {
            assertEquals(ReaderThemeMode.SYSTEM, ReaderAppearancePreference.decode(value))
        }
    }
}
