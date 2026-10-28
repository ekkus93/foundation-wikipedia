package org.foundation.wikipedia

/**
 * Presentation-only reader shell state. This does not authorize network access,
 * claim installed offline media, or change the native content trust boundary.
 */
data class ReaderShellState(
    val offline: Boolean = false,
    val settingsOpen: Boolean = false,
    val actionsOpen: Boolean = false,
) {
    fun toggleActions(): ReaderShellState =
        if (settingsOpen) this else copy(actionsOpen = !actionsOpen)

    fun withOffline(enabled: Boolean): ReaderShellState =
        copy(offline = enabled, actionsOpen = false)

    fun showSettings(): ReaderShellState =
        copy(settingsOpen = true, actionsOpen = false)

    fun back(): ReaderShellState = when {
        actionsOpen -> copy(actionsOpen = false)
        settingsOpen -> copy(settingsOpen = false)
        else -> this
    }

    companion object {
        /** Never restore an expanded speed dial after configuration recreation. */
        fun restore(offline: Boolean, settingsOpen: Boolean): ReaderShellState =
            ReaderShellState(offline = offline, settingsOpen = settingsOpen)
    }
}
