package com.mikerosoft.threadbear.kit

/**
 * Links from a message back into the thread (docs/protocol.md, "Links into
 * the thread"; ThreadLinks.swift), as the Mac's WorkLinks reads them:
 * `work:<words>` quotes a step in the turn's working, `thread:<id>` is
 * another thread and `review:<id>` a design review, which only the Mac can
 * open. `button:<id>` is a reply button (ReplyButtons.kt).
 */
object ThreadLinks {
    sealed class Target {
        data class Step(val quote: String) : Target()
        data class Thread(val id: String) : Target()
        data object Review : Target()
        data class Button(val id: String) : Target()
    }

    /**
     * What a work: link opens: its turn's working, and the step if it was
     * found. Often it isn't where the Mac would find it, as a command with a
     * description comes to the phone as only the description, so the working
     * opens anyway.
     */
    data class Hit(val turn: WorkedTurn, val step: Int?)

    private val codeSpan = Regex("`+[^`\\n]*`+")
    // One level of brackets inside, for `work:npm test (unit)`.
    private val destination = Regex("""\]\((work|thread):((?:[^()\n]|\([^()\n]*\))*)\)""")

    /**
     * The markdown with work: and thread: destinations escaped, as agents
     * write `[tests pass](work:npm test)` and a destination with a space isn't
     * a link to the parser: the phone showed the whole thing as raw text
     * (Mike, 1 Nov). Code spans stay as written, as they're being talked about.
     */
    fun escapeDestinations(markdown: String): String {
        if (!markdown.contains("](work:") && !markdown.contains("](thread:")) return markdown
        val spans = codeSpan.findAll(markdown).map { it.range }.toList()
        var next = 0
        val out = StringBuilder()
        var from = 0
        for (match in destination.findAll(markdown)) {
            // Spans and matches both come in order: each span is passed once.
            while (next < spans.size && spans[next].last < match.range.first) next++
            if (next < spans.size && spans[next].first <= match.range.last) continue
            val group = match.groups[2]!!
            out.append(markdown, from, group.range.first)
            val raw = group.value
            out.append(percentEncode(percentDecode(raw) ?: raw))
            from = group.range.last + 1
        }
        out.append(markdown, from, markdown.length)
        return out.toString()
    }

    /** What CharacterSet.urlPathAllowed keeps as it is, less `%`, `(` and `)`. */
    private const val PATH_ALLOWED = "!$&'*+,-./:=@_~"

    private fun percentEncode(text: String): String {
        val out = StringBuilder()
        for (b in text.toByteArray(Charsets.UTF_8)) {
            val c = (b.toInt() and 0xFF).toChar()
            if (c < '\u0080' && (c.isLetterOrDigit() || c in PATH_ALLOWED)) out.append(c)
            else out.append('%').append("%02X".format(b.toInt() and 0xFF))
        }
        return out.toString()
    }

    /** The text with its %XX escapes decoded as UTF-8, or null if they aren't valid (removingPercentEncoding). */
    fun percentDecode(text: String): String? {
        if ('%' !in text) return text
        val bytes = java.io.ByteArrayOutputStream()
        var i = 0
        while (i < text.length) {
            val c = text[i]
            if (c == '%') {
                if (i + 2 >= text.length) return null
                val hex = text.substring(i + 1, i + 3).toIntOrNull(16) ?: return null
                bytes.write(hex)
                i += 3
            } else {
                val cp = text.codePointAt(i)
                bytes.write(String(Character.toChars(cp)).toByteArray(Charsets.UTF_8))
                i += Character.charCount(cp)
            }
        }
        val decoder = Charsets.UTF_8.newDecoder()
        return try {
            decoder.decode(java.nio.ByteBuffer.wrap(bytes.toByteArray())).toString()
        } catch (_: java.nio.charset.CharacterCodingException) {
            null
        }
    }

    private val scheme = Regex("^([A-Za-z][A-Za-z0-9+.-]*):")

    /** What a link means, or null for anywhere else (a web page), which the system opens. */
    fun target(url: String): Target? {
        val name = scheme.find(url)?.groupValues?.get(1)?.lowercase() ?: return null
        val raw = url.substring(name.length + 1)
        val words = (percentDecode(raw) ?: raw).trim { it == ' ' || it == '\t' }
        return when (name) {
            "work" -> if (words.isEmpty()) null else Target.Step(words)
            "thread" -> if (words.isEmpty()) null else Target.Thread(words)
            "review" -> Target.Review
            "button" -> if (words.isEmpty()) null else Target.Button(words)
            else -> null
        }
    }

    /**
     * Where a work: link in message `from` (null: the latest turn, for the
     * card of asks) goes: the latest step of that message's turn with the
     * words in it, as the Mac picks it. Null when the turn has no working to show.
     */
    fun find(quote: String, from: Int?, items: List<ThreadItem>): Hit? {
        val q = normal(quote)
        if (q.isEmpty() || items.isEmpty()) return null
        val last = from?.let { seq -> items.indexOfLast { it.holds(seq) }.takeIf { it >= 0 } } ?: (items.size - 1)
        var first = last
        while (first > 0 && !items[first - 1].isFromMike) first--
        val turn = items.subList(first, last + 1).filter { item ->
            if (item is ThreadItem.Message) item.message.seq != from && item.message.role != Role.USER else true
        }
        if (turn.isEmpty()) return null
        val folds = turn.mapNotNull { (it as? ThreadItem.Worked)?.worked }
        // A turn still going isn't folded, so its lines are the working.
        val latest = folds.lastOrNull()
        if (latest == null) {
            val working = WorkedTurn(turn, null)
            return Hit(working, step(ThreadItem.Worked(working), q))
        }
        for (fold in folds.asReversed()) {
            step(ThreadItem.Worked(fold), q)?.let { return Hit(fold, it) }
        }
        return Hit(latest, null)
    }

    private fun step(item: ThreadItem, q: String): Int? = when (item) {
        is ThreadItem.Message -> if (matches(item.message, q)) item.message.seq else null
        is ThreadItem.Tools -> item.items.lastOrNull { matches(it, q) }?.seq
        is ThreadItem.Worked -> item.worked.items.asReversed().firstNotNullOfOrNull { step(it, q) }
        is ThreadItem.Gap -> null
    }

    private val ownLink = Regex("""\]\(work:[^)\n]*(\))?""")

    private fun matches(m: PhoneMessage, q: String): Boolean = when (m.role) {
        Role.TOOL -> normal(m.text).contains(q)
        // A line that links to a step isn't the step, nor is the summary.
        // One with no `)` uses up its line and stays: trying every `](work:`
        // after it again was quadratic.
        Role.ASSISTANT -> !m.summary && normal(ownLink.replace(m.text) { if (it.groups[1] == null) it.value else "]" }).contains(q)
        Role.USER -> false
    }

    private val home = Regex("""/Users/[^/\s]+""")
    private val space = Regex("""\s+""")

    /**
     * Lowercase, one space, and home as the runner's step lines write it, so
     * `/Users/mike/a.md` finds "Edited ~/a.md".
     */
    private fun normal(text: String): String =
        home.replace(text, "~").lowercase().split(space).filter { it.isNotEmpty() }.joinToString(" ")

    private val ThreadItem.isFromMike get() = this is ThreadItem.Message && message.role == Role.USER

    private fun ThreadItem.holds(seq: Int): Boolean = when (this) {
        is ThreadItem.Message -> message.seq == seq
        is ThreadItem.Tools -> items.any { it.seq == seq }
        is ThreadItem.Worked -> worked.items.any { it.holds(seq) }
        is ThreadItem.Gap -> false
    }
}
