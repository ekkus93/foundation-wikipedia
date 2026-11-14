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

/**
 * How much of the screen the chat sheet takes (MOB-003). At [PEEK] only its
 * title and status show and the article stays usable; [FULL] covers it.
 */
enum class ChatSheetHeight { PEEK, HALF, FULL }

data class ReaderShellState(
    val offline: Boolean = false,
    val settingsOpen: Boolean = false,
    val actionsOpen: Boolean = false,
    val settingsCategory: ReaderSettingsCategory? = null,
    val chatOpen: Boolean = false,
    val chatHeight: ChatSheetHeight = ChatSheetHeight.HALF,
) {
    fun toggleActions(): ReaderShellState =
        if (settingsOpen || chatOpen) this else copy(actionsOpen = !actionsOpen)

    fun dismissActions(): ReaderShellState = copy(actionsOpen = false)

    /** Open the chat sheet. Opening it sends nothing; each request is reviewed first. */
    fun showChat(): ReaderShellState =
        if (settingsOpen) this else copy(chatOpen = true, actionsOpen = false, chatHeight = ChatSheetHeight.HALF)

    fun closeChat(): ReaderShellState = copy(chatOpen = false, chatHeight = ChatSheetHeight.HALF)

    /** Resize an open chat sheet (drag, handle tap or accessibility action). */
    fun resizeChat(height: ChatSheetHeight): ReaderShellState =
        if (chatOpen) copy(chatHeight = height) else this

    /** The article is hidden from accessibility only when the sheet covers it. */
    val readerCovered: Boolean get() = actionsOpen || (chatShown && chatHeight == ChatSheetHeight.FULL)

    fun withOffline(enabled: Boolean): ReaderShellState =
        copy(offline = enabled, actionsOpen = false)

    /** Settings covers an open chat, which is shown again on return. */
    fun showSettings(): ReaderShellState =
        copy(settingsOpen = true, settingsCategory = null, actionsOpen = false)

    /** The chat sheet is on screen (open and not under Settings). */
    val chatShown: Boolean get() = chatOpen && !settingsOpen

    /** Leave Settings for the reader (and any chat it covered). */
    fun closeSettings(): ReaderShellState = copy(settingsOpen = false, settingsCategory = null)

    fun openSettingsCategory(category: ReaderSettingsCategory): ReaderShellState =
        if (!settingsOpen) this else copy(settingsCategory = category, actionsOpen = false)

    fun back(): ReaderShellState = when {
        settingsCategory != null -> copy(settingsCategory = null)
        settingsOpen -> copy(settingsOpen = false)
        // Back lowers an open chat to its peek height first, then closes it.
        chatOpen && chatHeight != ChatSheetHeight.PEEK -> copy(chatHeight = ChatSheetHeight.PEEK)
        chatOpen -> closeChat()
        actionsOpen -> copy(actionsOpen = false)
        else -> this
    }

    companion object {
        /** Never restore transient chat or expanded actions after configuration recreation. */
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
internal data class ReaderOfflineUpdate(val offline: Boolean, val saved: Boolean)

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

    /**
     * A failed write must never allow a requested online transition. Retain
     * offline mode until the preference can be committed successfully.
     */
    fun update(requestedOffline: Boolean, persist: (String) -> Boolean): ReaderOfflineUpdate {
        val saved = try {
            persist(encode(requestedOffline))
        } catch (_: RuntimeException) {
            false
        }
        return ReaderOfflineUpdate(offline = if (saved) requestedOffline else true, saved = saved)
    }
}

/**
 * Privacy-sensitive Android consent preference. Missing, malformed and future
 * values must never silently opt the user into recording reading history.
 * This does not implement history storage or authorize a network request.
 */
internal data class ReaderHistoryConsentUpdate(val enabled: Boolean, val saved: Boolean)

internal object ReaderHistoryPreference {
    const val KEY = "save_reading_history"
    private const val ENABLED = "v1:enabled"
    private const val DISABLED = "v1:disabled"

    fun encode(enabled: Boolean): String = if (enabled) ENABLED else DISABLED

    fun decode(value: String?): Boolean = value == ENABLED

    /**
     * Commit consent before presenting it as enabled. A failed opt-in never
     * authorizes recording; a failed revocation disables the current session
     * and must be reported because a previous durable choice may remain.
     */
    fun update(requested: Boolean, persist: (String) -> Boolean): ReaderHistoryConsentUpdate {
        val saved = try {
            persist(encode(requested))
        } catch (_: RuntimeException) {
            false
        }
        return ReaderHistoryConsentUpdate(enabled = saved && requested, saved = saved)
    }
}
