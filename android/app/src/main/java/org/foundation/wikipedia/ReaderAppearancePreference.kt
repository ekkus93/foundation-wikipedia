package org.foundation.wikipedia

/** Local presentation choice; it never authorizes article network access. */
enum class ReaderThemeMode(val title: String) {
    SYSTEM("System"),
    LIGHT("Light"),
    DARK("Dark"),
}

/**
 * Versioned, non-secret appearance preference. Unknown future values fall back
 * to the system setting instead of silently imposing an outdated color choice.
 */
internal object ReaderAppearancePreference {
    const val FILE = "foundation_reader_settings"
    const val KEY = "theme_mode"

    fun encode(mode: ReaderThemeMode): String = when (mode) {
        ReaderThemeMode.SYSTEM -> "v1:system"
        ReaderThemeMode.LIGHT -> "v1:light"
        ReaderThemeMode.DARK -> "v1:dark"
    }

    fun decode(value: String?): ReaderThemeMode = when (value) {
        "v1:light" -> ReaderThemeMode.LIGHT
        "v1:dark" -> ReaderThemeMode.DARK
        else -> ReaderThemeMode.SYSTEM
    }
}
