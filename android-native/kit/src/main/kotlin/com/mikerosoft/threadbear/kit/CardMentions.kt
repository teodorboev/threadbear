package com.mikerosoft.threadbear.kit

/**
 * Notion cards and other threads (`thread:<id>` links) mentioned in a
 * message, as on the Mac and the iPhone (CardMentions.swift). A message
 * holds a plain Markdown link, so the agent gets an address it can read the
 * card from; Mike's bubble draws it as a chip. Offsets are UTF-16, which is
 * what a Kotlin String indexes.
 */
object CardMentions {
    /** `start` until `end` (exclusive) is the whole link in the text. */
    data class Mention(val start: Int, val end: Int, val title: String, val url: String) {
        /** A thread: link rather than a Notion card. */
        val isThread: Boolean get() = url.startsWith("thread:")
    }

    fun find(text: String): List<Mention> {
        if (!text.contains("notion.") && !text.contains("](thread:")) return emptyList()
        return links(text).map { link ->
            val title = text.substring(link.titleStart, link.titleEnd).replace("\\[", "[").replace("\\]", "]")
            Mention(link.start, link.end, title, text.substring(link.urlStart, link.urlEnd))
        }
    }

    private class Link(val start: Int, val end: Int, val titleStart: Int, val titleEnd: Int, val urlStart: Int, val urlEnd: Int)

    // A forward scan, not a regex: `[` was allowed in a title, so with no
    // `](` after them every `[` started a match that ran to the end of the
    // line, and 32,000 of them took 16 seconds on a paste or a message
    // (Daybreak, 7 Nov 2026). It finds what the regex
    // `(?<!!)\[((?:[^\]\\\n]|\\.)+)\]\((https://(?:[A-Za-z0-9-]+\.)*notion\.(?:so|site|com)/[^\s)]*|thread:[A-Za-z0-9_-]+)\)`
    // found: a title with escaped brackets allowed in it, then an https
    // address on Notion's own domains or a thread: link. Nothing but a
    // link's end or a space ends the address, and the domain has to finish
    // where the path starts, so notion.so.example.com is no card. Each
    // character is looked at a few times at most.
    private fun links(u: String): List<Link> {
        val n = u.length
        val found = mutableListOf<Link>()
        // Where an address from each place would stop: at the next `)` or
        // space. An address can hold `[` and `](`, so the addresses of links
        // that fail overlap, and scanning each to its end again was
        // quadratic too.
        val stop = IntArray(n + 1) { n }
        for (k in n - 1 downTo 0) stop[k] = if (u[k] == ')' || isSpace(u[k])) k else stop[k + 1]
        var i = 0
        while (i < n) {
            if (u[i] != '[') { i++; continue }
            // Where a title from here ends: at the first `]` not escaped, or
            // nowhere if a line ends first. Every `[` before that end gives
            // the same one (a `[` isn't an escape), so they're all settled
            // by this one scan.
            var k = i + 1
            var end: Int? = null
            while (k < n) {
                val c = u[k]
                if (c == ']') { end = k; break }
                if (c == '\n') break
                if (c == '\\') {
                    // An escape takes any one character but a line break.
                    if (k + 1 >= n || u[k + 1] in LINE_BREAKS) break
                    k += 2
                    continue
                }
                k++
            }
            if (end == null) { i = maxOf(k, i + 1); continue }
            // The first of those `[`s not after a `!` (a picture) with a
            // title left between it and the end.
            var start: Int? = null
            var j = i
            while (j < end - 1) {
                if (u[j] == '[' && (j == 0 || u[j - 1] != '!')) { start = j; break }
                j++
            }
            val dest = if (start != null) destination(u, end + 1, stop) else null
            if (start != null && dest != null) {
                found += Link(start, dest.third, start + 1, end, dest.first, dest.second)
                i = dest.third
            } else {
                i = end + 1
            }
        }
        return found
    }

    /**
     * `(https://…notion.so/…)` or `(thread:<id>)` from `at`: the address's
     * start and end, and where the link ends, just past its `)`.
     */
    private fun destination(u: String, at: Int, stop: IntArray): Triple<Int, Int, Int>? {
        val n = u.length
        if (at >= n || u[at] != '(') return null
        val begin = at + 1
        var k: Int
        if (u.startsWith("thread:", begin)) {
            k = begin + "thread:".length
            val idStart = k
            while (k < n && isIdCharacter(u[k])) k++
            if (k <= idStart) return null
        } else if (u.startsWith("https://", begin)) {
            val host = begin + "https://".length
            k = host
            while (k < n && isHostCharacter(u[k])) k++
            if (k >= n || u[k] != '/' || !isNotionHost(u.substring(host, k))) return null
            k = stop[k]
        } else {
            return null
        }
        if (k >= n || u[k] != ')') return null
        return Triple(begin, k, k + 1)
    }

    /** `(?:[A-Za-z0-9-]+\.)*notion\.(?:so|site|com)`, exactly. */
    private fun isNotionHost(host: String): Boolean {
        for (top in NOTION_HOSTS) {
            if (!host.endsWith(top)) continue
            val labels = host.dropLast(top.length)
            if (labels.isEmpty()) return true
            if (labels.last() != '.') continue
            // Each label before it has a character at least, and no dots.
            var length = 0
            var ok = true
            for (c in labels) {
                if (c == '.') {
                    if (length == 0) { ok = false; break }
                    length = 0
                } else {
                    length++
                }
            }
            if (ok) return true
        }
        return false
    }

    private fun isAlphanumeric(c: Char) = c in '0'..'9' || c in 'A'..'Z' || c in 'a'..'z'
    private fun isIdCharacter(c: Char) = isAlphanumeric(c) || c == '_' || c == '-'
    private fun isHostCharacter(c: Char) = isAlphanumeric(c) || c == '-' || c == '.'

    /** What the regex's `\s` matched (ICU's White_Space), so an address ends where it did. */
    private fun isSpace(c: Char): Boolean {
        val v = c.code
        return v in 0x09..0x0D || v == 0x20 || v == 0x85 || v == 0xA0 || v == 0x1680 ||
            v in 0x2000..0x200A || v == 0x2028 || v == 0x2029 || v == 0x202F || v == 0x205F || v == 0x3000
    }

    /** What `.` doesn't match after a backslash. */
    private val LINE_BREAKS = setOf('\n', '\u000B', '\u000C', '\r', '\u0085', ' ', ' ')
    private val NOTION_HOSTS = listOf("notion.so", "notion.site", "notion.com")

    /**
     * How a pasted thread id goes into a message: a link the agent can read
     * the id from and the bubble draws as a chip with the thread's title.
     * The same as the Mac's.
     */
    fun threadReference(id: String, title: String): String {
        val words = title.split('\n', '\r', '\u000B', '\u000C', '\u0085', ' ', ' ')
            .filter { it.isNotEmpty() }.joinToString(" ").trimSpaces()
        val name = words.ifEmpty { "Thread" }.replace("[", "\\[").replace("]", "\\]")
        return "[$name](thread:$id)"
    }

    /**
     * The thread a paste names on its own: a bare id ("Copy thread ID" on
     * either app), `thread:<id>` or `threadbear://thread/<id>`. Only ids of
     * known threads count, so any other UUID pasted stays words. The same as
     * the Mac's.
     */
    fun pastedThreadId(pasted: String, known: (String) -> Boolean): String? {
        var text = pasted.trim()
        for (prefix in listOf("threadbear://thread/", "thread:")) {
            if (text.lowercase().startsWith(prefix)) text = text.substring(prefix.length)
        }
        if (text.isEmpty() || text.length > 64 || !text.all { it.code < 128 && (it.isLetterOrDigit() || it == '-' || it == '_') }) return null
        if (known(text)) return text
        val lower = text.lowercase()
        return if (known(lower)) lower else null
    }
}
