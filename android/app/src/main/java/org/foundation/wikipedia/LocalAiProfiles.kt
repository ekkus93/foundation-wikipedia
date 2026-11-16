package org.foundation.wikipedia

import uniffi.wiki_ffi.LocalAiProtocol

/**
 * AI-004 saved local model profiles: up to [MAX] server/model pairs, at most
 * one active. Removing the active profile leaves none active; another saved
 * profile is never switched on implicitly, so chat cannot silently move to
 * a different server.
 */
data class LocalAiProfiles(
    val profiles: List<LocalAiChoice> = emptyList(),
    val active: LocalAiChoice? = null,
) {
    init {
        require(profiles.size <= MAX) { "at most $MAX profiles" }
        require(profiles.distinct().size == profiles.size) { "duplicate profile" }
        require(active == null || active in profiles) { "active profile must be saved" }
    }

    /** Save [choice] (if new) and make it the active profile. */
    fun saveAndActivate(choice: LocalAiChoice): LocalAiProfiles? {
        val updated = if (choice in profiles) profiles else profiles + choice
        if (updated.size > MAX) return null
        return copy(profiles = updated, active = choice)
    }

    fun activate(choice: LocalAiChoice): LocalAiProfiles =
        if (choice in profiles) copy(active = choice) else this

    /** Local chat off; profiles stay saved. */
    fun deactivate(): LocalAiProfiles = copy(active = null)

    fun remove(choice: LocalAiChoice): LocalAiProfiles = copy(
        profiles = profiles - choice,
        active = active.takeUnless { it == choice },
    )

    fun encode(): String = buildString {
        append(HEADER)
        for (profile in profiles) {
            append('\n')
            append(if (profile == active) '*' else '-')
            append(profile.protocol.name).append('\t')
            append(profile.endpoint).append('\t')
            append(profile.model)
        }
    }

    companion object {
        const val MAX = 8
        private const val HEADER = "local-ai-profiles/v1"
        private const val MAX_FIELD = 200

        private fun validField(value: String) =
            value.isNotBlank() && value.length <= MAX_FIELD &&
                value.none { it == '\t' || it == '\n' || it == '\r' || it.isISOControl() }

        /** Strict decode; anything malformed means no profiles (fail closed). */
        fun decode(text: String?): LocalAiProfiles? {
            if (text == null) return null
            val lines = text.split('\n')
            if (lines.first() != HEADER || lines.size - 1 > MAX) return null
            var active: LocalAiChoice? = null
            val profiles = mutableListOf<LocalAiChoice>()
            for (line in lines.drop(1)) {
                val marker = line.firstOrNull() ?: return null
                val fields = line.drop(1).split('\t')
                if (marker != '*' && marker != '-' || fields.size != 3) return null
                val protocol = LocalAiProtocol.entries.firstOrNull { it.name == fields[0] } ?: return null
                if (!validField(fields[1]) || !validField(fields[2])) return null
                val profile = LocalAiChoice(protocol, fields[1], fields[2])
                if (profile in profiles) return null
                if (marker == '*') {
                    if (active != null) return null
                    active = profile
                }
                profiles += profile
            }
            return LocalAiProfiles(profiles, active)
        }

        fun isStorable(choice: LocalAiChoice) =
            validField(choice.endpoint) && validField(choice.model)
    }
}
