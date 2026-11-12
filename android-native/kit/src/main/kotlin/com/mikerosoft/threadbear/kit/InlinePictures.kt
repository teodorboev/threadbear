package com.mikerosoft.threadbear.kit

/**
 * Pictures among a message's words, as on the Mac (docs/protocol.md,
 * "Pictures in a message"; InlinePictures.swift on the iPhone). In the
 * composer each photo is a mark in the text, U+FFFC, the k-th mark standing
 * for the k-th photo. A message carries `[Picture N]` markers instead, and
 * the runner puts picture N straight after its marker when it goes to the
 * agent. Indexes are UTF-16, as Kotlin strings and the text field count.
 */
object InlinePictures {
    const val MARK = '￼'
    const val MARK_STRING = "￼"

    fun marker(number: Int) = "[Picture $number]"

    /** The same marker the runner reads (runner/src/pictures.ts). */
    private val markerPattern = Regex("""\[Picture (\d{1,3})]""")

    fun markCount(text: String): Int = text.count { it == MARK }

    /** Words put right for `count` photos: one mark each. Spare marks go; a photo with no mark gets one at the end. */
    fun normalized(text: String, count: Int): String {
        val marks = markCount(text)
        if (marks == count) return text
        if (marks > count) {
            var kept = 0
            val out = StringBuilder()
            for (c in text) {
                if (c == MARK) {
                    kept++
                    if (kept > count) continue
                }
                out.append(c)
            }
            return out.toString()
        }
        var out = text
        repeat(count - marks) { out = appending(MARK_STRING, out) }
        return out
    }

    internal fun appending(piece: String, text: String): String {
        val last = text.lastOrNull()
        if (last == null || last.isWhitespace()) return text + piece
        return "$text $piece"
    }

    /** The words as they're sent: the k-th mark as `[Picture first+k]`, or "" when there's nothing but photos, which go as they always did. */
    fun sentText(text: String, first: Int = 1): String {
        if (!text.contains(MARK)) return text
        if (text.all { it == MARK || it.isWhitespace() }) return ""
        return messageText(text, first)
    }

    /** The words with the k-th mark as `[Picture first+k]`. */
    fun messageText(text: String, first: Int = 1): String {
        if (!text.contains(MARK)) return text
        val out = StringBuilder()
        var number = first
        for (c in text) {
            if (c == MARK) {
                out.append(marker(number))
                number++
            } else {
                out.append(c)
            }
        }
        return out.toString()
    }

    /** What `inserting` gives: the new words, which photo the first new mark is, and where the cursor goes. */
    data class Inserted(val text: String, val firstIndex: Int, val cursor: Int)

    /**
     * `count` marks in place of `start`..`start + length` (UTF-16, clamped),
     * with a space either side, as the Mac's composer puts them. Also says
     * which photo the first one is (how many marks come before it) and where
     * the cursor goes after them.
     */
    fun inserting(count: Int, text: String, start: Int, length: Int): Inserted {
        val location = start.coerceIn(0, text.length)
        val replace = length.coerceIn(0, text.length - location)
        fun isSpace(index: Int) = index >= 0 && index < text.length && text[index].isWhitespace()
        val piece = StringBuilder()
        if (location > 0 && !isSpace(location - 1)) piece.append(' ')
        piece.append(List(count) { MARK_STRING }.joinToString(" "))
        if (!isSpace(location + replace)) piece.append(' ')
        val before = text.substring(0, location)
        val out = text.replaceRange(location, location + replace, piece)
        return Inserted(out, markCount(before), location + piece.length)
    }

    /** The words without the `index`-th mark and one space beside it. */
    fun removingMark(index: Int, text: String): String {
        var seen = 0
        var at = -1
        for ((i, c) in text.withIndex()) {
            if (c != MARK) continue
            if (seen == index) { at = i; break }
            seen++
        }
        if (at < 0) return text
        val chars = StringBuilder(text)
        chars.deleteCharAt(at)
        if (at < chars.length && chars[at] == ' ') {
            chars.deleteCharAt(at)
        } else if (at > 0 && chars[at - 1] == ' ') {
            chars.deleteCharAt(at - 1)
        }
        return chars.toString()
    }

    /** The words without their photos' marks. */
    fun words(text: String): String = text.filter { it != MARK }

    /** How many of a message's `count` pictures no marker places, as the runner counts them for its "[2 pictures]" line (sync.ts). */
    fun unplaced(text: String, count: Int): Int {
        val placed = pieces(text).mapNotNull { (it as? Piece.Picture)?.number?.takeIf { n -> n <= count } }.toSet()
        return count - placed.size
    }

    sealed class Piece {
        data class Words(val text: String) : Piece()
        data class Picture(val number: Int) : Piece()
    }

    /**
     * A message as words and chips, for drawing. The phone gets a message's
     * words but not its pictures, so any `[Picture N]` from 1 to 99 is a
     * chip, the first naming each number, as the runner places them.
     */
    fun pieces(text: String): List<Piece> {
        if (!text.contains("[Picture ")) return listOf(Piece.Words(text))
        val pieces = mutableListOf<Piece>()
        val seen = HashSet<Int>()
        var last = 0
        for (match in markerPattern.findAll(text)) {
            val number = match.groupValues[1].toIntOrNull() ?: continue
            if (number !in 1..99 || !seen.add(number)) continue
            if (match.range.first > last) pieces += Piece.Words(text.substring(last, match.range.first))
            pieces += Piece.Picture(number)
            last = match.range.last + 1
        }
        if (last < text.length) pieces += Piece.Words(text.substring(last))
        return pieces
    }

    /**
     * Which photos an edit took out: the places, among `original`'s marks,
     * of those inside the ranges it replaced (`start until end` each, in
     * `original`'s offsets). A chip deleted or cut in the composer takes its
     * photo with it.
     */
    fun removedMarks(original: CharSequence, replaced: List<IntRange>): List<Int> {
        if (replaced.isEmpty()) return emptyList()
        val out = mutableListOf<Int>()
        var place = 0
        for (i in original.indices) {
            if (original[i] != MARK) continue
            if (replaced.any { i in it }) out += place
            place++
        }
        return out
    }
}
