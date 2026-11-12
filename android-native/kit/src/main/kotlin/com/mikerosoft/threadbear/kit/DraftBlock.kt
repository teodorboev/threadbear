package com.mikerosoft.threadbear.kit

import java.net.URI

/**
 * A ```slack, ```email or ```draft block in a reply (DraftBlock.swift): text
 * the agent has written for Mike to send somewhere else. It's shown the way
 * it'll go out, every line where the agent put it (a reply's own Markdown
 * joins lines up, which ran a Slack draft's "•" lines into one paragraph,
 * 6 Nov), in a box with a Copy button. A ```slack one also draws Slack's own
 * formatting: *bold*, _italic_, ~strike~, `code` and <url|label> links.
 *
 * A copy of the Mac's (app/Sources/ThreadbearCore/Markdown/DraftBlock.swift)
 * and the iPhone's; the website (web/src/draft.ts) parses it the same way.
 * Keep them together.
 */
data class DraftBlock(val kind: Kind, val source: String) {
    enum class Kind(val wire: String) { SLACK("slack"), EMAIL("email"), DRAFT("draft") }

    /** A run of a line with one look. `link`: http(s) or mailto only. */
    data class Span(
        val text: String,
        val bold: Boolean = false,
        val italic: Boolean = false,
        val strike: Boolean = false,
        val code: Boolean = false,
        val link: String? = null,
    )

    /** `quote`: a Slack "> " line, drawn with a bar. */
    data class Line(val spans: List<Span>, val quote: Boolean = false)

    /** What the box's header calls it. */
    val label: String
        get() = when (kind) {
            Kind.SLACK -> "Slack draft"
            Kind.EMAIL -> "Email draft"
            Kind.DRAFT -> "Draft"
        }

    /**
     * The draft's lines as they're shown. An email or plain draft is its
     * text as it is (bare addresses are linked when it's drawn); a Slack
     * one has its formatting and <...> tokens worked out.
     */
    val lines: List<Line>
        get() = source.split("\n").map { line ->
            if (kind != Kind.SLACK) return@map Line(listOf(Span(line)))
            if (line.startsWith("> ") || line == ">" || line.startsWith("&gt; ")) {
                val rest = if (line.startsWith("&gt; ")) line.substring(5) else line.substring(minOf(2, line.length))
                return@map Line(slackSpans(rest), quote = true)
            }
            Line(slackSpans(line))
        }

    /**
     * What Copy puts on the clipboard as plain text. A Slack draft keeps its
     * *bold* and _italic_ (Slack's message box formats those as it sends)
     * but its <url|label> links become "label (url)", which pasted as plain
     * text would otherwise stay as typed.
     */
    val plainText: String
        get() {
            if (kind != Kind.SLACK) return source
            return source.split("\n").joinToString("\n") { line ->
                val (marked, tokens) = marked(line)
                if (tokens == null) return@joinToString decode(line)
                // A token in `code` is copied as typed, as it's shown.
                val code = codeSpans(marked)
                val out = StringBuilder()
                for ((offset, c) in marked.withIndex()) {
                    val index = placeholderIndex(c)
                    if (index == null || index >= tokens.size) { out.append(c); continue }
                    if (code.any { offset in it }) { out.append(tokens[index].second); continue }
                    when (val token = tokens[index].first) {
                        is Token.Link -> out.append(if (token.label == token.url || token.label.isEmpty()) token.url else "${token.label} (${token.url})")
                        is Token.Mention -> out.append(token.text)
                    }
                }
                decode(out.toString())
            }
        }

    /**
     * The same as HTML, put on the clipboard beside the plain text, so
     * pasting into Slack or Mail keeps the bold and the links.
     */
    val html: String
        get() {
            val body = lines.map { line ->
                var afterSpace = true
                val inner = line.spans.joinToString("") { span ->
                    val out = html(span, afterSpace)
                    afterSpace = span.text.lastOrNull() == ' '
                    out
                }
                if (line.quote) "<blockquote>$inner</blockquote>" else inner
            }
            // Quotes are blocks of their own, so no <br> beside one.
            val out = StringBuilder()
            for ((index, part) in body.withIndex()) {
                out.append(part)
                if (index < body.size - 1 && !part.startsWith("<blockquote>") && !body[index + 1].startsWith("<blockquote>")) out.append("<br>")
            }
            return "<meta charset=\"utf-8\"><div>$out</div>"
        }

    internal sealed class Token {
        data class Link(val url: String, val label: String) : Token()

        /** <@U123>, <#C123|general>, <!here>: shown as their words. */
        data class Mention(val text: String) : Token()
    }

    internal sealed class Piece {
        data class Text(val text: String) : Piece()

        /** The token, and the <...> it was written as. */
        data class Tok(val token: Token, val raw: String) : Piece()
    }

    companion object {
        /**
         * Past this a fence stays plain code: a draft that size isn't a
         * message, and the box shouldn't be what makes a reply slow.
         */
        const val MAX_SOURCE = 20_000

        /** Past this many characters a line is shown as written, so a huge line can't keep the formatting busy. */
        const val MAX_FORMATTED_LINE = 2_000

        /** Where the placeholders for a line's tokens start (Unicode's private use area). */
        private const val PLACEHOLDER_BASE = 0xE000
        private const val PLACEHOLDER_END = 0xF8FF

        /** The draft for a fence's info string (only the first word counts), or null for any other fence. */
        fun from(language: String?, code: String): DraftBlock? {
            val word = language?.split(' ', '\t', '{')?.firstOrNull { it.isNotEmpty() }?.lowercase() ?: return null
            val kind = Kind.entries.firstOrNull { it.wire == word } ?: return null
            var source = code.replace("\r\n", "\n")
            while (source.endsWith("\n")) source = source.dropLast(1)
            if (source.isBlank() || source.length > MAX_SOURCE) return null
            return DraftBlock(kind, source)
        }

        /** Whether a block of a reply's text could hold one, so only those are checked for a closed fence. */
        fun mayHold(text: String): Boolean {
            if (!text.contains("```") && !text.contains("~~~")) return false
            val lowered = text.lowercase()
            return lowered.contains("slack") || lowered.contains("email") || lowered.contains("draft")
        }

        private fun html(span: Span, afterSpace: Boolean): String {
            var text = keepSpaces(escape(span.text), afterSpace)
            if (span.code) text = "<code>$text</code>"
            if (span.strike) text = "<s>$text</s>"
            if (span.italic) text = "<i>$text</i>"
            if (span.bold) text = "<b>$text</b>"
            if (span.link != null) text = "<a href=\"${escape(span.link)}\">$text</a>"
            return text
        }

        /**
         * HTML folds runs of spaces into one, so a line's indent and any
         * spaces lined up in it would go in a paste: each space at the start
         * of a line or after another is a no-break space.
         */
        private fun keepSpaces(text: String, afterSpace: Boolean): String {
            val out = StringBuilder()
            var previousSpace = afterSpace
            for (c in text) {
                when (c) {
                    ' ' -> { out.append(if (previousSpace) "&nbsp;" else " "); previousSpace = true }
                    '\t' -> { out.append("&nbsp;&nbsp;&nbsp;&nbsp;"); previousSpace = true }
                    else -> { out.append(c); previousSpace = false }
                }
            }
            return out.toString()
        }

        private fun escape(text: String): String =
            text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("\"", "&quot;")

        // Slack's formatting

        /**
         * A <...> token: a link to a web page or a mail address, or a mention.
         * Anything else between angle brackets is left as written.
         */
        internal fun token(inside: String): Token? {
            val parts = inside.split("|", limit = 2)
            val target = parts[0]
            val label = parts.getOrNull(1)
            val first = target.firstOrNull()
            if (first == '@' || first == '#' || first == '!') {
                val name = target.substring(1)
                if (name.isEmpty() || name.contains(" ")) return null
                if (!label.isNullOrEmpty()) return Token.Mention(if (first == '#' && !label.startsWith("#")) "#$label" else label)
                if (first == '!') {
                    // <!subteam^S123> and <!date^...> carry a label or nothing useful.
                    return Token.Mention("@" + name.split("^").first())
                }
                return Token.Mention("$first$name")
            }
            if (target.contains(" ")) return null
            val scheme = runCatching { URI(target).scheme }.getOrNull()?.lowercase() ?: return null
            if (scheme !in setOf("http", "https", "mailto")) return null
            val shown = label?.let(::decode) ?: if (scheme == "mailto") target.substring("mailto:".length) else target
            return Token.Link(target, shown)
        }

        /**
         * Walks a line once, handing over its words and its <...> tokens in
         * order. A "<" that doesn't start a token is a word like any other.
         * The next ">" is looked for once and kept until it's passed, so a
         * line of many "<" (or one with no ">") takes one pass, not one per
         * "<" (cross-lab review, 6 Nov).
         */
        internal fun scanTokens(line: String, visit: (Piece) -> Unit) {
            val plain = StringBuilder()
            var close: Int? = -1
            var i = 0
            while (i < line.length) {
                if (line[i] == '<') {
                    val known = close
                    if (known != null && known <= i) close = line.indexOf('>', i + 1).takeIf { it >= 0 }
                    val end = close
                    // A token never holds a "<", so with another before the ">"
                    // this one isn't a token, and what's between isn't read.
                    if (end != null && !hasBefore(line, '<', i + 1, end)) {
                        val token = token(line.substring(i + 1, end))
                        if (token != null) {
                            if (plain.isNotEmpty()) { visit(Piece.Text(plain.toString())); plain.setLength(0) }
                            visit(Piece.Tok(token, line.substring(i, end + 1)))
                            i = end + 1
                            continue
                        }
                    }
                }
                plain.append(line[i])
                i++
            }
            if (plain.isNotEmpty()) visit(Piece.Text(plain.toString()))
        }

        private fun hasBefore(line: String, c: Char, from: Int, end: Int): Boolean {
            for (k in from until end) if (line[k] == c) return true
            return false
        }

        internal fun decode(text: String): String =
            text.replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")

        /**
         * The line with each <...> token as one placeholder character, and
         * the tokens; no tokens for a line that's too long, or that has
         * placeholder characters of its own, which is shown as written.
         */
        internal fun marked(line: String): Pair<String, List<Pair<Token, String>>?> {
            if (line.length > MAX_FORMATTED_LINE || line.any { it.code in PLACEHOLDER_BASE..PLACEHOLDER_END }) return line to null
            val marked = StringBuilder()
            val tokens = mutableListOf<Pair<Token, String>>()
            scanTokens(line) { piece ->
                when (piece) {
                    is Piece.Text -> marked.append(piece.text)
                    is Piece.Tok -> {
                        marked.append((PLACEHOLDER_BASE + tokens.size).toChar())
                        tokens += piece.token to piece.raw
                    }
                }
            }
            return marked.toString() to tokens
        }

        internal fun placeholderIndex(c: Char): Int? {
            val index = c.code - PLACEHOLDER_BASE
            return if (index in 0..(PLACEHOLDER_END - PLACEHOLDER_BASE)) index else null
        }

        /** Where a line's `code` is, by offset, found the way `emphasis` finds it. */
        internal fun codeSpans(chars: String): List<IntRange> {
            val spans = mutableListOf<IntRange>()
            var i = 0
            while (i < chars.length) {
                val end = if (chars[i] == '`' && opens(chars, i)) closing(chars, i, '`') else null
                if (end != null) {
                    spans += i..end
                    i = end + 1
                } else {
                    i++
                }
            }
            return spans
        }

        /**
         * A Slack line's spans: *bold*, _italic_, ~strike~, `code`, links and
         * mentions. Each <...> token stands in the line as one placeholder
         * character while the emphasis is worked out, so a link can be bold
         * (*see <url|docs>*) and a token inside `code` stays as written.
         */
        fun slackSpans(line: String): List<Span> {
            val (marked, tokens) = marked(line)
            if (tokens == null) return listOf(Span(decode(line)))
            val spans = mutableListOf<Span>()
            for (span in emphasis(marked)) {
                val words = StringBuilder()
                fun flush() {
                    if (words.isEmpty()) return
                    spans += span.copy(text = words.toString())
                    words.setLength(0)
                }
                for (c in span.text) {
                    val index = placeholderIndex(c)
                    if (index == null || index >= tokens.size) { words.append(c); continue }
                    if (span.code) {
                        // Code shows what was typed.
                        words.append(tokens[index].second)
                        continue
                    }
                    flush()
                    spans += when (val token = tokens[index].first) {
                        is Token.Link -> span.copy(text = token.label.ifEmpty { token.url }, link = token.url)
                        is Token.Mention -> span.copy(text = token.text, bold = true)
                    }
                }
                flush()
            }
            return merge(spans)
        }

        /** Joins neighbouring spans that look the same (a "<" that wasn't a token leaves the words around it in pieces). */
        private fun merge(spans: List<Span>): List<Span> {
            val out = mutableListOf<Span>()
            for (span in spans) {
                if (span.text.isEmpty()) continue
                val last = out.lastOrNull()
                if (last != null && last.copy(text = "") == span.copy(text = "")) {
                    out[out.size - 1] = last.copy(text = last.text + span.text)
                } else {
                    out += span
                }
            }
            return out
        }

        /**
         * Slack's emphasis: a marker counts when it opens at the start or
         * after a space or punctuation, is followed by a non-space, and closes
         * on the same line before a space, punctuation or the end. So
         * "2*3*4" and "snake_case_name" stay as written.
         */
        internal fun emphasis(text: String, base: Span = Span("")): List<Span> {
            val spans = mutableListOf<Span>()
            val plain = StringBuilder()
            fun flush() {
                if (plain.isEmpty()) return
                spans += base.copy(text = decode(plain.toString()))
                plain.setLength(0)
            }
            var i = 0
            while (i < text.length) {
                val c = text[i]
                val end = if (c in "*_~`" && opens(text, i)) closing(text, i, c) else null
                if (end != null) {
                    flush()
                    val inner = text.substring(i + 1, end)
                    when (c) {
                        '`' -> spans += base.copy(text = decode(inner), code = true)
                        '*' -> spans += emphasis(inner, base.copy(bold = true))
                        '_' -> spans += emphasis(inner, base.copy(italic = true))
                        else -> spans += emphasis(inner, base.copy(strike = true))
                    }
                    i = end + 1
                    continue
                }
                plain.append(c)
                i++
            }
            flush()
            return spans
        }

        private fun isWordy(c: Char) = c.isLetterOrDigit()

        private fun opens(chars: String, i: Int): Boolean {
            if (i + 1 >= chars.length || chars[i + 1].isWhitespace() || chars[i + 1] == chars[i]) return false
            return i == 0 || !isWordy(chars[i - 1])
        }

        private fun closing(chars: String, start: Int, marker: Char): Int? {
            var j = start + 2
            while (j < chars.length) {
                if (chars[j] == marker && !chars[j - 1].isWhitespace() && (j + 1 == chars.length || !isWordy(chars[j + 1]))) return j
                j++
            }
            return null
        }
    }
}
