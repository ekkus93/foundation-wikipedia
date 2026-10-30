package org.foundation.wikipedia

/**
 * Presentation-only reader shell state. This does not authorize network access,
 * claim installed offline media, or change the native content trust boundary.
 */
enum class ReaderSettingsCategory(val title: String) {
    GENERAL("General"),
    APPEARANCE_READING("Appearance & Reading"),
    AI_ASSISTANT("AI Assistant"),
    WIKIPEDIA_OFFLINE_DATA("Wikipedia & Offline Data"),
    STORAGE("Storage"),
    HISTORY_SAVED_CONTENT("History & Saved Content"),
    PRIVACY_NETWORK("Privacy & Network"),
    ADVANCED("Advanced"),
    ABOUT("About"),
}

data class ReaderShellState(
    val offline: Boolean = false,
    val settingsOpen: Boolean = false,
    val actionsOpen: Boolean = false,
    val settingsCategory: ReaderSettingsCategory? = null,
) {
    fun toggleActions(): ReaderShellState =
        if (settingsOpen) this else copy(actionsOpen = !actionsOpen)

    fun dismissActions(): ReaderShellState = copy(actionsOpen = false)

    fun withOffline(enabled: Boolean): ReaderShellState =
        copy(offline = enabled, actionsOpen = false)

    fun showSettings(): ReaderShellState =
        copy(settingsOpen = true, settingsCategory = null, actionsOpen = false)

    fun openSettingsCategory(category: ReaderSettingsCategory): ReaderShellState =
        if (!settingsOpen) this else copy(settingsCategory = category, actionsOpen = false)

    fun back(): ReaderShellState = when {
        actionsOpen -> copy(actionsOpen = false)
        settingsCategory != null -> copy(settingsCategory = null)
        settingsOpen -> copy(settingsOpen = false)
        else -> this
    }

    companion object {
        /** Never restore an expanded speed dial after configuration recreation. */
        fun restore(
            offline: Boolean,
            settingsOpen: Boolean,
            categoryName: String? = null,
        ): ReaderShellState = ReaderShellState(
            offline = offline,
            settingsOpen = settingsOpen,
            settingsCategory = if (settingsOpen) {
                ReaderSettingsCategory.entries.firstOrNull { it.name == categoryName }
            } else {
                null
            },
        )
    }
}

/**
 * Versioned, non-secret Android setting. Unknown/corrupt values are treated as
 * offline rather than unexpectedly permitting future network-capable readers
 * to fetch content. A missing setting defaults to online-first.
 */
internal object ReaderOfflinePreference {
    const val FILE = "foundation_reader_settings"
    const val KEY = "work_offline"
    private const val ONLINE = "v1:online"
    private const val OFFLINE = "v1:offline"

    fun encode(offline: Boolean): String = if (offline) OFFLINE else ONLINE

    fun decode(value: String?): Boolean = when (value) {
        null, ONLINE -> false
        OFFLINE -> true
        else -> true
    }
}
