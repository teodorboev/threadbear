package com.mikerosoft.threadbear.kit

/**
 * The transcript across the recognizer's restarts (Dictation.swift's Carry).
 * Each result is meant to be the whole transcript so far, but after a pause
 * a recognizer can begin again from the new words alone, and on Android
 * each listening session ends at a pause and the app starts the next. When
 * a result is shorter than the last and doesn't begin the same way, the
 * last is kept and the new words go after it; [commit] keeps what a session
 * ended on, so the next one's words always go after it.
 */
class Carry {
    private var kept = ""
    private var last = ""

    fun add(text: String): String {
        if (last.isNotEmpty() && text.length < last.length && firstWord(text) != firstWord(last)) {
            kept = join(kept, last)
        }
        last = text
        return join(kept, text)
    }

    /** A session ended on `text`: it stays, whatever the next one says. */
    fun commit(text: String): String {
        kept = join(kept, text)
        last = ""
        return kept
    }

    val text: String get() = join(kept, last)

    private fun firstWord(text: String) =
        text.lowercase().split(Regex("[^\\p{L}\\p{N}]+")).firstOrNull { it.isNotEmpty() } ?: ""

    private fun join(a: String, b: String) = if (a.isEmpty()) b else if (b.isEmpty()) a else "$a $b"
}

/**
 * Where dictated words go: in at the cursor as it was when the mic started,
 * a space either side where the words beside them need one, each new
 * transcript replacing the last. Edited around meanwhile so the words
 * aren't where they were left, they carry on at the end instead, rather
 * than overwriting what was typed.
 */
class DictationSpot private constructor(private var anchor: Int, private var before: String, private var after: String) {
    private var shown = ""

    data class Placed(val text: String, val cursor: Int)

    fun place(text: String, transcript: String): Placed {
        val stillThere = anchor + shown.length <= text.length && text.regionMatches(anchor, shown, 0, shown.length)
        if (!stillThere) {
            anchor = text.length
            before = if (text.isNotEmpty() && !text.last().isWhitespace()) " " else ""
            after = ""
            shown = ""
        }
        val piece = if (transcript.isEmpty()) "" else before + transcript + after
        val out = text.substring(0, anchor) + piece + text.substring(anchor + shown.length)
        shown = piece
        return Placed(out, anchor + if (piece.isEmpty()) 0 else piece.length - after.length)
    }

    companion object {
        fun at(text: String, cursor: Int): DictationSpot {
            val at = cursor.coerceIn(0, text.length)
            val before = if (at > 0 && !text[at - 1].isWhitespace()) " " else ""
            val after = if (at < text.length && !text[at].isWhitespace()) " " else ""
            return DictationSpot(at, before, after)
        }
    }
}

/**
 * The mic's loudness over the last moments (Dictation.swift's MicLevel),
 * for the waveform, so it's clear it's hearing (Mike, 6 Nov).
 */
class MicLevels(val count: Int = 24) {
    private val levels = ArrayDeque(List(count) { 0f })

    /**
     * Android's onRmsChanged, in dB: about -2 in silence to 10 when loud,
     * as 0 to 1. [shown] then stretches them to the last couple of seconds.
     */
    fun push(rmsDb: Float) {
        val value = ((rmsDb + 2f) / 12f).coerceIn(0f, 1f)
        levels.removeFirst()
        levels.addLast(if (value.isNaN()) 0f else value)
    }

    fun reset() {
        levels.clear()
        repeat(count) { levels.addLast(0f) }
    }

    val history: List<Float> get() = levels.toList()

    /**
     * For the bars: the history stretched between its quietest and loudest,
     * so whatever the mic's level, talking fills them. At least 0.12 apart,
     * so near silence stays near flat rather than blowing up room noise.
     */
    val shown: List<Float>
        get() {
            val values = history
            val low = values.minOrNull() ?: 0f
            val span = maxOf((values.maxOrNull() ?: 0f) - low, 0.12f)
            return values.map { (it - low) / span }
        }
}
