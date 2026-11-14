package org.foundation.wikipedia

/**
 * Where each article in the reader's history was scrolled to, as a fraction
 * of its scrollable length (so it survives rotation and font changes).
 * Saved with the activity's instance state, so reopening an entry after
 * recreation, process death or Back returns to the same place (MOB-004).
 */
data class ReaderScroll(val positions: Map<String, Float> = emptyMap()) {
    fun at(project: String, pageId: Long): Float? = positions[key(project, pageId)]

    /** Record a position; entries not in [keep] are dropped (bounded by history). */
    fun with(project: String, pageId: Long, fraction: Float, keep: Collection<ReaderEntry>): ReaderScroll {
        if (!fraction.isFinite()) return this
        val kept = keep.map { key(it.project, it.pageId) }.toSet()
        val updated = positions.filterKeys { it in kept } + (key(project, pageId) to fraction.coerceIn(0f, 1f))
        return ReaderScroll(updated)
    }

    /** Flat form for saved instance state. */
    fun save(): List<String> = positions.flatMap { (key, fraction) -> listOf(key, fraction.toString()) }

    companion object {
        private fun key(project: String, pageId: Long) = "$project:$pageId"

        /** The fraction of [scrollY] within a page of [contentPx] shown in [viewportPx]. */
        fun fraction(scrollY: Int, contentPx: Int, viewportPx: Int): Float {
            val range = contentPx - viewportPx
            return if (range <= 0) 0f else (scrollY.toFloat() / range).coerceIn(0f, 1f)
        }

        /** The scroll offset for [fraction] of a page of [contentPx] in [viewportPx]. */
        fun offset(fraction: Float, contentPx: Int, viewportPx: Int): Int =
            (fraction.coerceIn(0f, 1f) * (contentPx - viewportPx).coerceAtLeast(0)).toInt()

        /** Restores [save] output; malformed input yields no positions. */
        fun restore(saved: List<*>?): ReaderScroll {
            if (saved == null || saved.size % 2 != 0 || saved.size > 2 * ReaderNavigation.MAX_DEPTH) return ReaderScroll()
            val positions = saved.chunked(2).associate { (key, value) ->
                val fraction = (value as? String)?.toFloatOrNull()
                if (key !is String || fraction == null || !fraction.isFinite() || fraction !in 0f..1f) {
                    return ReaderScroll()
                }
                key to fraction
            }
            return ReaderScroll(positions)
        }
    }
}
