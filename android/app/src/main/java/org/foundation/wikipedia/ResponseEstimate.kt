package org.foundation.wikipedia

import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToLong

/**
 * How fast one model at one destination has answered before: the time it
 * took to read each character of the prompt, how fast it wrote, and how
 * long its answers were. Learned from finished requests, never sent anywhere.
 */
data class ModelTiming(
    val msPerPromptChar: Double,
    val outputCharsPerSecond: Double,
    val outputChars: Double,
    val samples: Int,
) {
    /** Blend a finished request into the running averages (recent weighs more). */
    fun plus(sample: ModelTiming): ModelTiming {
        val weight = if (samples == 0) 1.0 else 0.35
        fun blend(old: Double, new: Double) = old + (new - old) * weight
        return ModelTiming(
            blend(msPerPromptChar, sample.msPerPromptChar),
            blend(outputCharsPerSecond, sample.outputCharsPerSecond),
            blend(outputChars, sample.outputChars),
            min(samples + 1, 1000),
        )
    }

    fun encode(): String = "v1:$msPerPromptChar:$outputCharsPerSecond:$outputChars:$samples"

    companion object {
        /** Unknown or malformed values mean "no history", never a wrong estimate. */
        fun decode(value: String?): ModelTiming? {
            val parts = value?.split(':') ?: return null
            if (parts.size != 5 || parts[0] != "v1") return null
            val numbers = parts.subList(1, 4).map { it.toDoubleOrNull() ?: return null }
            val samples = parts[4].toIntOrNull() ?: return null
            if (numbers.any { !it.isFinite() || it < 0 } || samples <= 0 || numbers[1] <= 0) return null
            return ModelTiming(numbers[0], numbers[1], numbers[2], samples)
        }

        /** The timing of one finished request, or `null` if it is unusable. */
        fun measured(promptChars: Long, firstOutputMs: Long, finishedMs: Long, outputChars: Long): ModelTiming? {
            val writingMs = finishedMs - firstOutputMs
            if (promptChars <= 0 || firstOutputMs < 0 || writingMs <= 0 || outputChars <= 0) return null
            return ModelTiming(
                firstOutputMs.toDouble() / promptChars,
                outputChars * 1000.0 / writingMs,
                outputChars.toDouble(),
                1,
            )
        }
    }
}

/** What a request in flight is doing, for the progress line. */
enum class ResponsePhase { READING, WRITING, CHECKING }

/**
 * A progress estimate: [fraction] in 0..1 and [remainingMs], both `null`
 * when there is nothing to base an estimate on yet (the progress bar is
 * then indeterminate).
 */
data class ResponseEstimate(
    val phase: ResponsePhase,
    val fraction: Float?,
    val remainingMs: Long?,
) {
    companion object {
        /** Live writing speed is trusted only after this much output time. */
        private const val MIN_LIVE_MS = 1500L
        /** Before any history, assume a medium-length answer. */
        private const val DEFAULT_ANSWER_SHARE = 0.4

        fun of(
            timing: ModelTiming?,
            promptChars: Long,
            maxOutputChars: Long,
            elapsedMs: Long,
            firstOutputMs: Long?,
            outputChars: Long,
            finished: Boolean,
        ): ResponseEstimate {
            if (finished) return ResponseEstimate(ResponsePhase.CHECKING, 1f, 0)
            val expectedChars = min(
                maxOutputChars.toDouble(),
                max(timing?.outputChars ?: (maxOutputChars * DEFAULT_ANSWER_SHARE), outputChars * 1.15),
            )
            if (firstOutputMs == null) {
                timing ?: return ResponseEstimate(ResponsePhase.READING, null, null)
                val reading = timing.msPerPromptChar * promptChars
                val writing = expectedChars * 1000.0 / timing.outputCharsPerSecond
                // Past the expected reading time: assume it is nearly done.
                val readingLeft = max(reading - elapsedMs, min(reading, writing) * 0.1)
                return estimate(ResponsePhase.READING, elapsedMs, readingLeft + writing)
            }
            val writingMs = elapsedMs - firstOutputMs
            val rate = when {
                writingMs >= MIN_LIVE_MS && outputChars > 0 -> outputChars * 1000.0 / writingMs
                timing != null -> timing.outputCharsPerSecond
                else -> return ResponseEstimate(ResponsePhase.WRITING, null, null)
            }
            return estimate(ResponsePhase.WRITING, elapsedMs, max(0.0, expectedChars - outputChars) * 1000.0 / rate)
        }

        private fun estimate(phase: ResponsePhase, elapsedMs: Long, remaining: Double): ResponseEstimate {
            val left = remaining.roundToLong().coerceAtLeast(0)
            val total = elapsedMs + left
            val fraction = if (total <= 0) 0f else (elapsedMs.toFloat() / total).coerceIn(0f, 0.99f)
            return ResponseEstimate(phase, fraction, left)
        }

        /** "about 1 min 20 s left", rounded so it does not flicker. */
        fun describe(remainingMs: Long): String {
            val seconds = (remainingMs + 999) / 1000
            return when {
                seconds < 5 -> "a few seconds left"
                seconds < 60 -> "about ${(seconds + 4) / 5 * 5} s left"
                seconds < 600 -> {
                    val rounded = (seconds + 9) / 10 * 10
                    val minutes = rounded / 60
                    val rest = rounded % 60
                    if (rest == 0L) "about $minutes min left" else "about $minutes min $rest s left"
                }
                else -> "about ${(seconds + 59) / 60} min left"
            }
        }
    }
}
