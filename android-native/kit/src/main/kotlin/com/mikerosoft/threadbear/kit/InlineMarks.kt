package com.mikerosoft.threadbear.kit

/**
 * Inline marks in a line of a reply: escapes, `code`, **bold**, *italic*,
 * ~~strike~~, [links](url) and bare web addresses, as runs of text with the
 * marks on them. The app turns the runs into styled text; links go to its
 * link gate, never straight to the system.
 *
 * A link's destination may have spaces and one level of brackets, as agents
 * write `[tests pass](work:npm test (unit))`, which a strict parser showed as
 * raw text on the iPhone (Mike, 1 Nov; ThreadLinks.escapeDestinations there).
 *
 * Every pass works on a strictly shorter piece of the text than the one
 * before, and goes at most MAX_DEPTH deep. A link's words were parsed again
 * with links still on, so a bare address was found inside itself forever and
 * one `https://example.com` in a reply overflowed the stack (Daybreak, 11 Nov
 * 2026). Inside a link nothing is a link, as GitHub has it, and a bare address
 * is just its words.
 */
object InlineMarks {
    enum class Mark { CODE, BOLD, ITALIC, STRIKE }

    /** `link` is shared by the runs of one link, so they can be drawn as one. */
    data class Link(val url: String, val index: Int)

    data class Run(val text: String, val marks: Set<Mark> = emptySet(), val link: Link? = null)

    /** Marks inside marks this deep at most; past it the rest is words. */
    const val MAX_DEPTH = 8

    /** `links` false: nothing is a link, its words only (a skill's description, Daybreak 8 Nov). */
    fun parse(text: String, links: Boolean = true): List<Run> {
        val out = Builder()
        out.append(text, emptySet(), if (links) null else NO_LINKS, 0)
        return out.runs()
    }

    /** What's around the text being read: no link yet, or inside one (no more links). */
    private val NO_LINKS = Link("", -1)

    private class Builder {
        // Runs still being written: a run's words in a buffer, so a line of
        // thousands of escapes in a row isn't copied again for each one
        // (cross-lab review, 11 Nov 2026).
        private val open = mutableListOf<Triple<StringBuilder, Set<Mark>, Link?>>()
        var links = 0

        fun runs(): List<Run> = open.map { (text, marks, link) -> Run(text.toString(), marks, link) }

        fun words(text: String, marks: Set<Mark>, link: Link?) {
            if (text.isEmpty()) return
            val shown = link?.takeIf { it !== NO_LINKS }
            val last = open.lastOrNull()
            if (last != null && last.second == marks && last.third == shown) last.first.append(text)
            else open += Triple(StringBuilder(text), marks, shown)
        }

        /** `link` is the link the text is inside, NO_LINKS when none may start, or null. */
        fun append(text: String, marks: Set<Mark>, link: Link?, depth: Int) {
            if (depth >= MAX_DEPTH) return words(text, marks, link)
            var last = 0
            for (m in InlineTokens(text).all()) {
                if (m.start > last) words(text.substring(last, m.start), marks, link)
                val inner = depth + 1
                when (m.kind) {
                    Kind.ESCAPE -> words(m.a, marks, link)
                    Kind.CODE -> words(" ${m.a.trim()} ", marks + Mark.CODE, link)
                    Kind.BOLD -> append(m.a, marks + Mark.BOLD, link, inner)
                    Kind.STRIKE -> append(m.a, marks + Mark.STRIKE, link, inner)
                    Kind.ITALIC -> append(m.a, marks + Mark.ITALIC, link, inner)
                    // Already inside a link, or none allowed: its words, never a second link.
                    Kind.LINK -> append(m.a, marks, link ?: Link(m.b.trim(), links++), inner)
                    // A bare address is never parsed again: it's its own words.
                    Kind.ADDRESS -> words(m.a, marks, link ?: Link(m.a, links++))
                }
                last = m.end
            }
            if (last < text.length) words(text.substring(last), marks, link)
        }
    }
}

internal enum class Kind { ESCAPE, CODE, BOLD, STRIKE, ITALIC, LINK, ADDRESS }

/** One mark: where it is in the text, its words (`a`), and a link's destination (`b`). */
internal data class Token(val start: Int, val end: Int, val kind: Kind, val a: String, val b: String = "")

/**
 * The marks in a text, left to right, each the first that fits where it
 * starts, as this regex finds them:
 *
 *     \\([\\`*_{}\[\]()#+\-.!|~<>])          an escape
 *     (?<!`)(`+)(?!`)(.+?)(?<!`)\2(?!`)       code, closed by a run of as many backticks
 *     \*\*(.+?)\*\*  __(.+?)__  ~~(.+?)~~     bold, bold, strike
 *     (?<![\w*])\*(?!\s)(.+?)(?<!\s)\*(?!\w)  italic
 *     (?<!\w)_(?!\s)(.+?)(?<!\s)_(?!\w)       italic
 *     \[([^\]\n]+)]\(((?:[^()\n]|\([^()\n]*\))+?)\)   a link
 *     (https?://[^\s<>()]+[^\s<>().,;:!?'"])  a bare address
 *
 * By hand rather than with it: a regex looks for each opener's closer again
 * from every opener, so a line of openers with no closers (10,000 backticks,
 * `*a *a *a`, `[[[[...](`) took time in the square of its length. Here each
 * closer is found from tables made in one pass, and a link's destination is
 * read once however many openers share it, so the whole text is a few passes.
 * InlineMarksTest checks it against the regex on random text.
 */
internal class InlineTokens(private val t: String) {
    private val n = t.length

    /** The first index at or after i where `at` holds, n where none does. */
    private fun next(at: (Int) -> Boolean): IntArray {
        val out = IntArray(n + 1) { n }
        for (i in n - 1 downTo 0) out[i] = if (at(i)) i else out[i + 1]
        return out
    }

    private fun lineEnd(c: Char) = c == '\n' || c == '\r' || c == '\u0085' || c == ' ' || c == ' '
    private fun word(c: Char) = c in 'a'..'z' || c in 'A'..'Z' || c in '0'..'9' || c == '_'
    private fun space(c: Char) = c == ' ' || c == '\t' || c == '\n' || c == '\u000B' || c == '\u000C' || c == '\r'
    private fun wordAt(i: Int) = i in 0 until n && word(t[i])
    private fun spaceAt(i: Int) = i in 0 until n && space(t[i])

    // What `.` can't match, and what a link can't have.
    private val lineEnds = next { lineEnd(t[it]) }
    private val newlines = next { t[it] == '\n' }
    private val pairs = mapOf('*' to next { it + 1 < n && t[it] == '*' && t[it + 1] == '*' },
        '_' to next { it + 1 < n && t[it] == '_' && t[it + 1] == '_' },
        '~' to next { it + 1 < n && t[it] == '~' && t[it + 1] == '~' })
    private val closers = mapOf('*' to next { t[it] == '*' && it > 0 && !space(t[it - 1]) && !wordAt(it + 1) },
        '_' to next { t[it] == '_' && it > 0 && !space(t[it - 1]) && !wordAt(it + 1) })
    private val brackets = next { t[it] == ']' }
    private val urlStops = next { space(t[it]) || t[it] == '<' || t[it] == '>' || t[it] == '(' || t[it] == ')' }

    /** Each backtick run's start, and the start of the next run as long after it (or -1). */
    private val runEnd = IntArray(n + 1)
    private val sameRunAfter = HashMap<Int, Int>()

    init {
        val starts = mutableListOf<Int>()
        var i = 0
        while (i < n) {
            if (t[i] != '`') { i++; continue }
            var j = i
            while (j < n && t[j] == '`') j++
            starts += i
            runEnd[i] = j
            i = j
        }
        val lastOfLength = HashMap<Int, Int>()
        for (k in starts.indices.reversed()) {
            val start = starts[k]
            val length = runEnd[start] - start
            sameRunAfter[start] = lastOfLength[length] ?: -1
            lastOfLength[length] = start
        }
    }

    /**
     * Where a link's destination ends (after its `)`), read on from each
     * index after at least one character of it, or -1: worked out for every
     * index at once, right to left, so openers whose destinations run on
     * into each other don't each read the rest of the text.
     */
    private val parens = next { t[it] == '(' || t[it] == ')' || t[it] == '\n' }
    private val destinationFrom = IntArray(n + 1).also { f ->
        f[n] = -1
        for (i in n - 1 downTo 0) f[i] = when (t[i]) {
            ')' -> i + 1
            '\n' -> -1
            '(' -> group(i)?.let { f[it] } ?: -1
            else -> f[i + 1]
        }
    }

    /** `(...)` from i, with no paren or line break inside: the index after it, or null. */
    private fun group(i: Int): Int? {
        val j = parens[i + 1]
        return if (j < n && t[j] == ')') j + 1 else null
    }

    /** A destination from d: its first character can't be `)` or a line break. */
    private fun destination(d: Int): Int = when {
        d >= n || t[d] == ')' || t[d] == '\n' -> -1
        t[d] == '(' -> group(d)?.let { destinationFrom[it] } ?: -1
        else -> destinationFrom[d + 1]
    }

    /** `x...x` with a pair of `c`s each side: where its closing pair starts, or -1. */
    private fun paired(p: Int, c: Char): Int {
        if (p + 1 >= n || t[p] != c || t[p + 1] != c || p + 3 > n) return -1
        val q = pairs.getValue(c)[p + 3]
        return if (q < n && lineEnds[p + 2] >= q) q else -1
    }

    private fun italic(p: Int, c: Char): Int {
        if (t[p] != c || p + 2 > n) return -1
        if (p > 0 && (word(t[p - 1]) || (c == '*' && t[p - 1] == '*'))) return -1
        if (p + 1 >= n || space(t[p + 1])) return -1
        val q = closers.getValue(c)[p + 2]
        return if (q < n && lineEnds[p + 1] >= q) q else -1
    }

    private fun at(p: Int): Token? {
        val c = t[p]
        if (c == '\\' && p + 1 < n && t[p + 1] in ESCAPABLE) return Token(p, p + 2, Kind.ESCAPE, t[p + 1].toString())
        if (c == '`' && (p == 0 || t[p - 1] != '`')) {
            val e = runEnd[p]
            val q = sameRunAfter[p] ?: -1
            if (e < n && q > e && lineEnds[e] >= q) return Token(p, q + (e - p), Kind.CODE, t.substring(e, q))
        }
        paired(p, '*').let { if (it >= 0) return Token(p, it + 2, Kind.BOLD, t.substring(p + 2, it)) }
        paired(p, '_').let { if (it >= 0) return Token(p, it + 2, Kind.BOLD, t.substring(p + 2, it)) }
        paired(p, '~').let { if (it >= 0) return Token(p, it + 2, Kind.STRIKE, t.substring(p + 2, it)) }
        italic(p, '*').let { if (it >= 0) return Token(p, it + 1, Kind.ITALIC, t.substring(p + 1, it)) }
        if (c == '_' && !wordAt(p - 1)) italic(p, '_').let { if (it >= 0) return Token(p, it + 1, Kind.ITALIC, t.substring(p + 1, it)) }
        if (c == '[' && p + 1 < n) {
            val r = brackets[p + 1]
            if (r < n && r > p + 1 && newlines[p + 1] > r && r + 1 < n && t[r + 1] == '(') {
                val end = destination(r + 2)
                if (end > 0) return Token(p, end, Kind.LINK, t.substring(p + 1, r), t.substring(r + 2, end - 1))
            }
        }
        if (c == 'h' && (t.startsWith("http://", p) || t.startsWith("https://", p))) {
            val s = p + if (t.startsWith("https://", p)) 8 else 7
            val e = urlStops[s]
            var j = e - 1
            while (j >= s + 1 && t[j] in TRAILING) j--
            if (j >= s + 1) return Token(p, j + 1, Kind.ADDRESS, t.substring(p, j + 1))
        }
        return null
    }

    fun all(): List<Token> {
        val out = mutableListOf<Token>()
        var p = 0
        while (p < n) {
            val token = at(p)
            if (token == null) { p++; continue }
            out += token
            p = token.end
        }
        return out
    }

    private companion object {
        const val ESCAPABLE = "\\`*_{}[]()#+-.!|~<>"
        const val TRAILING = ".,;:!?'\""
    }
}
