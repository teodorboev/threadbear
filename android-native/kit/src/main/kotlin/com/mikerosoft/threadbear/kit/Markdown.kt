package com.mikerosoft.threadbear.kit

/**
 * Assistant text split into the blocks the phone draws (MarkdownBlocks.swift):
 * paragraphs, headings, list items (with their depth), code, drafts, quotes,
 * rules, tables and pictures.
 *
 * The Mac renders the same text with react-markdown and remark-gfm, so this
 * follows GitHub's rules for what agents actually write; the phone's own
 * first parser knew only paragraphs, headings, flat bullets and code, and
 * showed tables and quotes as raw pipes and arrows (Mike, 31 Oct).
 * Inline markdown (bold, code, links) is left in the text for the view.
 */
sealed class MarkdownBlock {
    data class Paragraph(val text: String) : MarkdownBlock()
    data class Heading(val level: Int, val text: String) : MarkdownBlock()

    /** `marker` is "•" or "3."; `task` is null, or whether its box is ticked. */
    data class ListItem(val text: String, val marker: String, val depth: Int, val task: Boolean?) : MarkdownBlock()
    data class Code(val text: String) : MarkdownBlock()

    /**
     * A ```mermaid or ```html block, drawn as the diagram or page it
     * describes, with a switch back to its source.
     */
    data class Rich(val kind: RichKind, val source: String) : MarkdownBlock()

    /**
     * A ```slack, ```email or ```draft block: text for Mike to send
     * elsewhere, shown line for line in a box with a Copy button.
     */
    data class Draft(val draft: DraftBlock) : MarkdownBlock()
    data class Quote(val blocks: List<MarkdownBlock>) : MarkdownBlock()
    data object Rule : MarkdownBlock()
    data class Table(val header: List<String>, val alignments: List<Alignment>, val rows: List<List<String>>) : MarkdownBlock()

    /**
     * `![alt](src)`, with src as the agent wrote it: mostly a path on the
     * Mac, which ThreadPictures swaps for what the runner uploaded.
     */
    data class Image(val alt: String, val src: String) : MarkdownBlock()

    enum class Alignment { LEADING, CENTER, TRAILING }

    enum class RichKind(val wire: String) {
        MERMAID("mermaid"), HTML("html");

        companion object {
            /** The fence's info string: its first word, in any case. */
            fun of(info: String): RichKind? {
                val word = info.split(' ', '\t', '{').firstOrNull { it.isNotEmpty() } ?: return null
                val lower = word.lowercase()
                return entries.firstOrNull { it.wire == lower }
            }
        }
    }

    companion object {
        /** Past this a fence stays code (Pele refuses more itself). */
        const val MAX_RICH_SOURCE = 50_000

        /**
         * Quotes nest this deep at most; a `>` past it is words. Each level
         * was parsed again from its text, and drawn inside the one above, so
         * a line of 8,000 `>`s took seconds and could run out of stack
         * (Daybreak, 7 Nov 2026). GitHub's own replies never go past a few.
         */
        const val MAX_QUOTE_DEPTH = 8

        /**
         * The cells one reply's tables may come to; rows past it are words.
         * Short rows are padded to the header's width, and every cell is a
         * view, so 9 KB of text could ask for 1.6 million (cross-lab review,
         * 7 Nov 2026). A real table is tens of rows of a few columns.
         */
        const val MAX_TABLE_CELLS = 2_000

        fun parse(text: String): List<MarkdownBlock> = parse(text, 0, Cells(MAX_TABLE_CELLS))

        /** `depth` is how many quotes this text is inside; `cells` is shared with the quotes around it. */
        internal fun parse(text: String, depth: Int, cells: Cells): List<MarkdownBlock> =
            MarkdownParser(text.replace("\r\n", "\n").split("\n").toMutableList(), depth, cells).run()
    }
}

/** Table cells a reply can still have (MarkdownBlock.MAX_TABLE_CELLS), shared by its quotes. */
internal class Cells(var left: Int)

/** Swift's `.whitespaces`: spaces and tabs (and other horizontal spaces), not line breaks. */
internal fun isHorizontalSpace(c: Char): Boolean =
    c == ' ' || c == '\t' || Character.getType(c) == Character.SPACE_SEPARATOR.toInt()

internal fun String.trimSpaces(): String = trim(::isHorizontalSpace)

/**
 * Finds `![alt](src)` and `![alt](<src> "title")`, as the runner does
 * (MARKDOWN_IMAGE in runner/src/share-images.ts), so src is the same string
 * on both sides. Not inside a code span, where it's only being talked about.
 */
internal object InlineImages {
    sealed class Piece {
        data class Words(val text: String) : Piece()
        data class Image(val alt: String, val src: String) : Piece()
    }

    private val codeSpan = Regex("`+[^`]*`+")

    // No `[` in the alt or a bare src, nor `<` in a bracketed one: with
    // them, every `![` was tried to the end of the text again, 4 s on an
    // 8,000-character reply (the sweep after Daybreak's findings, 7 Nov
    // 2026). A picture's path never has one.
    private val image = Regex("""!\[([^\[\]]*)]\(\s*(?:<([^<>\n]+)>|([^)\s\[]+))(?:\s+"[^"]*")?\s*\)""")

    fun split(text: String): List<Piece> {
        if (!text.contains("![")) return listOf(Piece.Words(text))
        val pieces = mutableListOf<Piece>()
        var from = 0
        val spans = CodeSpans(codeSpan.findAll(text).map { it.range.first until it.range.last + 1 }.toList())
        for (match in image.findAll(text)) {
            val range = match.range.first until match.range.last + 1
            if (spans.overlap(range)) continue
            val src = match.groups[2]?.value ?: match.groups[3]?.value ?: continue
            add(text.substring(from, range.first), pieces)
            pieces += Piece.Image(match.groupValues[1], src)
            from = range.last + 1
        }
        add(text.substring(from), pieces)
        return pieces
    }

    private fun add(words: String, pieces: MutableList<Piece>) {
        val trimmed = words.trim()
        if (trimmed.isNotEmpty()) pieces += Piece.Words(trimmed)
    }
}

/**
 * A text's code spans, asked about matches in the order they come: each
 * span is passed once, rather than every span looked at for every match,
 * which was quadratic on a reply of many spans and links (cross-lab
 * review, 7 Nov 2026).
 */
internal class CodeSpans(private val spans: List<IntRange>) {
    private var next = 0

    /** Whether `range` overlaps a span. Ranges asked about must come in order. */
    fun overlap(range: IntRange): Boolean {
        while (next < spans.size && spans[next].last + 1 <= range.first) next++
        return next < spans.size && spans[next].first < range.last + 1
    }
}

private class MarkdownParser(
    /** Not a val: a fence opening on a list item's line is put back as the fence alone (run()). */
    val lines: MutableList<String>,
    /** How many quotes these lines are inside. */
    val depth: Int,
    val cells: Cells,
) {
    var i = 0
    val blocks = mutableListOf<MarkdownBlock>()
    /** Raw lines, so `flush` can see a hard break's two trailing spaces. */
    val paragraph = mutableListOf<String>()
    /** Indents of the open list levels, so a deeper indent nests one level whatever its width. */
    val listIndents = mutableListOf<Int>()
    /** A wrapped line belongs to the list item above it, until a blank line. */
    var lastWasListItem = false

    fun run(): List<MarkdownBlock> {
        while (i < lines.size) {
            val raw = lines[i]
            val line = raw.trimSpaces()
            val indent = indent(raw)

            val fence = fence(line)
            if (fence != null) {
                flush()
                val info = line.substring(fence.length).trimSpaces()
                val (body, closed) = code(fence, indent)
                // Drawn only once its fence is closed (a reply still
                // streaming in stays code) and not too big to draw, as on
                // the Mac (RichBlock.from). A draft is the same: closed only.
                val draft = if (closed) DraftBlock.from(info, body) else null
                val kind = MarkdownBlock.RichKind.of(info)
                blocks += when {
                    draft != null -> MarkdownBlock.Draft(draft)
                    closed && body.length <= MarkdownBlock.MAX_RICH_SOURCE && body.isNotBlank() && kind != null ->
                        MarkdownBlock.Rich(kind, body)
                    else -> MarkdownBlock.Code(body)
                }
                continue
            }
            if (line.isEmpty()) {
                flush()
                lastWasListItem = false
                i++
                continue
            }
            if (isRule(line)) {
                flush()
                blocks += MarkdownBlock.Rule
                i++
                continue
            }
            // Past the deepest quote, the line is words like any other.
            if (line.startsWith(">") && depth < MarkdownBlock.MAX_QUOTE_DEPTH) {
                flush()
                val inner = quoteLines().joinToString("\n")
                blocks += MarkdownBlock.Quote(MarkdownBlock.parse(inner, depth + 1, cells))
                continue
            }
            // Each regex only for a line it could match: a reply of thousands
            // of short lines paid for all of them on each.
            if (line.first() == '#') {
                val match = HEADING.find(line)
                if (match != null) {
                    flush()
                    blocks += MarkdownBlock.Heading(match.groupValues[1].length, withoutClosingHashes(match.groupValues[2]))
                    i++
                    continue
                }
            }
            if (i + 1 < lines.size && line.contains("|")) {
                val alignments = delimiterRow(lines[i + 1])
                if (alignments != null) {
                    val header = cells(line)
                    // With no room for the header and a row, it's words.
                    if (header.size == alignments.size && header.size * 2 <= cells.left) {
                        flush()
                        blocks += table(header, alignments)
                        continue
                    }
                }
            }
            val first = line.first()
            val item = if (first == '-' || first == '*' || first == '+' || first.isDigit()) LIST_ITEM.find(line) else null
            if (item != null) {
                flush()
                val words = item.groupValues[2]
                // "- ```slack": the item is the fenced block. Its line goes
                // back as the fence alone, indented as the item's words are,
                // and is read again as a fence (no bullet beside the box).
                val content = indent + line.length - words.length
                if (fence(words) != null) {
                    // Still an item of the list, so the ones after it nest
                    // as they would after any other.
                    depth(indent)
                    lines[i] = " ".repeat(content) + words
                    lastWasListItem = false
                    continue
                }
                var text = words
                var task: Boolean? = null
                val box = TASK_BOX.find(text)
                if (box != null) {
                    task = box.groupValues[1] != " "
                    text = text.removeRange(box.range)
                }
                val marker = item.groupValues[1].let { if (it.first().isDigit()) it.dropLast(1) + "." else "•" }
                // A picture in a list item goes straight under it, as there's
                // no drawing one among a line's words.
                val (shown, images) = wordsAndImages(text)
                val level = depth(indent)
                if (shown.isNotEmpty() || images.isEmpty()) blocks += MarkdownBlock.ListItem(shown, marker, level, task)
                blocks += images
                lastWasListItem = true
                i++
                continue
            }
            // A wrapped line of a list item, or an indented paragraph under
            // one after a blank line: it stays with the item.
            val last = blocks.lastOrNull()
            if (last is MarkdownBlock.ListItem && paragraph.isEmpty() && (lastWasListItem || indent >= 2)) {
                val joiner = if (lastWasListItem) " " else "\n\n"
                val (words, images) = wordsAndImages(line)
                if (words.isNotEmpty()) blocks[blocks.size - 1] = last.copy(text = last.text + joiner + words)
                blocks += images
                lastWasListItem = true
                i++
                continue
            }
            if (indent == 0) listIndents.clear()
            paragraph += raw
            i++
        }
        flush()
        return blocks
    }

    /**
     * Lines in a paragraph are joined with spaces, as GitHub shows a soft
     * line break; two trailing spaces or a backslash keep the break.
     */
    fun flush() {
        if (paragraph.isEmpty()) return
        val text = StringBuilder()
        for ((n, raw) in paragraph.withIndex()) {
            val line = raw.trimSpaces()
            text.append(line)
            if (n == paragraph.size - 1) break
            if (raw.endsWith("  ") || line.endsWith("\\")) {
                if (line.endsWith("\\")) text.setLength(text.length - 1)
                text.append('\n')
            } else {
                text.append(' ')
            }
        }
        for (piece in InlineImages.split(text.toString())) {
            blocks += when (piece) {
                is InlineImages.Piece.Words -> MarkdownBlock.Paragraph(piece.text)
                is InlineImages.Piece.Image -> MarkdownBlock.Image(piece.alt, piece.src)
            }
        }
        paragraph.clear()
        lastWasListItem = false
    }

    fun wordsAndImages(text: String): Pair<String, List<MarkdownBlock>> {
        val pieces = InlineImages.split(text)
        val words = pieces.filterIsInstance<InlineImages.Piece.Words>().joinToString(" ") { it.text }
        val images = pieces.filterIsInstance<InlineImages.Piece.Image>().map { MarkdownBlock.Image(it.alt, it.src) }
        return words to images
    }

    fun depth(indent: Int): Int {
        while (listIndents.isNotEmpty() && listIndents.last() > indent) listIndents.removeAt(listIndents.size - 1)
        if (listIndents.lastOrNull() != indent) listIndents += indent
        return listIndents.size - 1
    }

    /** The fenced block's lines, and whether its closing fence came. */
    fun code(fence: String, indent: Int): Pair<String, Boolean> {
        i++
        val body = mutableListOf<String>()
        var closed = false
        while (i < lines.size) {
            val raw = lines[i]
            i++
            val line = raw.trimSpaces()
            // A fence on its own, of the same character and at least as
            // wide, closes it at any indent, as on the Mac. Measuring the
            // indent from a list item's words swallowed ordinary replies
            // when the list state was wrong (cross-lab review, 6 Nov); a
            // fence-like line indented four spaces inside a block ending
            // it early is the price.
            if (line.startsWith(fence) && line.all { it == fence[0] }) { closed = true; break }
            // Code fenced inside a list item is indented with it; take that off.
            var drop = minOf(indent, indent(raw))
            var start = 0
            while (drop > 0 && start < raw.length && (raw[start] == ' ' || raw[start] == '\t')) { start++; drop-- }
            body += raw.substring(start)
        }
        return body.joinToString("\n") to closed
    }

    fun quoteLines(): List<String> {
        val out = mutableListOf<String>()
        while (i < lines.size) {
            val line = lines[i].trimSpaces()
            if (!line.startsWith(">")) break
            var rest = line.substring(1)
            if (rest.startsWith(" ")) rest = rest.substring(1)
            out += rest
            i++
        }
        return out
    }

    fun table(header: List<String>, alignments: List<MarkdownBlock.Alignment>): MarkdownBlock {
        i += 2
        cells.left -= header.size
        val rows = mutableListOf<List<String>>()
        while (i < lines.size && cells.left >= header.size) {
            val line = lines[i].trimSpaces()
            if (line.isEmpty() || !line.contains("|")) break
            var row = cells(line)
            // GitHub pads short rows and drops extra cells.
            if (row.size < header.size) row = row + List(header.size - row.size) { "" }
            rows += row.take(header.size)
            cells.left -= header.size
            i++
        }
        return MarkdownBlock.Table(header, alignments, rows)
    }

    companion object {
        val HEADING = Regex("""^(#{1,6})\s+(.*)$""")
        val LIST_ITEM = Regex("""^([-*+]|\d{1,9}[.)])\s+(.*)$""")
        val TASK_BOX = Regex("""^\[([ xX])]\s+""")
        val FENCE = Regex("""^(`{3,}|~{3,})""")
        val RULE = Regex("""(?:-[ \t]*){3,}|(?:\*[ \t]*){3,}|(?:_[ \t]*){3,}""")
        val DELIMITER_CELL = Regex(""":?-+:?""")

        /**
         * "Title ##" is "Title": the closing #s go with the space before them,
         * as `(.*?)(?:\s+#+)?$` had it. By hand, as that regex tried every
         * place in a long line of spaces with all that came after: 2 s on an
         * 8,000-character reply.
         */
        fun withoutClosingHashes(text: String): String {
            val hashes = text.reversed().takeWhile { it == '#' }.length
            if (hashes == 0) return text
            val rest = text.dropLast(hashes)
            val last = rest.lastOrNull() ?: return text
            if (!last.isWhitespace()) return text
            return rest.trimEnd { it.isWhitespace() }
        }

        fun indent(line: String): Int {
            var n = 0
            for (c in line) {
                when (c) {
                    ' ' -> n += 1
                    '\t' -> n += 4
                    else -> return n
                }
            }
            return n
        }

        fun fence(line: String): String? {
            val first = line.firstOrNull() ?: return null
            if (first != '`' && first != '~') return null
            val match = FENCE.find(line) ?: return null
            val fence = match.groupValues[1]
            // A backtick fence's info string has no backtick (CommonMark), so
            // "```literal``` and more" is a code span in words, not a fence.
            if (fence[0] == '`' && line.substring(fence.length).contains('`')) return null
            return fence
        }

        fun isRule(line: String): Boolean {
            val first = line.firstOrNull() ?: return false
            if (first != '-' && first != '*' && first != '_') return false
            return RULE.matches(line)
        }

        /** `| --- | :-: | --: |`, with or without the outer pipes. */
        fun delimiterRow(raw: String): List<MarkdownBlock.Alignment>? {
            val line = raw.trimSpaces()
            if (!line.contains("-") || !(line.contains("|") || line.startsWith(":") || line.startsWith("-"))) return null
            val alignments = mutableListOf<MarkdownBlock.Alignment>()
            for (cell in cells(line)) {
                if (!DELIMITER_CELL.matches(cell)) return null
                alignments += when {
                    cell.startsWith(":") && cell.endsWith(":") -> MarkdownBlock.Alignment.CENTER
                    cell.endsWith(":") -> MarkdownBlock.Alignment.TRAILING
                    else -> MarkdownBlock.Alignment.LEADING
                }
            }
            return alignments.ifEmpty { null }
        }

        /** A row's cells: split on pipes that aren't escaped, outer pipes optional. */
        fun cells(line: String): List<String> {
            val cells = mutableListOf<String>()
            val cell = StringBuilder()
            var escaped = false
            for (c in line) {
                if (escaped) {
                    if (c == '|') cell.append('|') else cell.append('\\').append(c)
                    escaped = false
                    continue
                }
                if (c == '\\') { escaped = true; continue }
                if (c == '|') { cells += cell.toString(); cell.setLength(0); continue }
                cell.append(c)
            }
            if (escaped) cell.append('\\')
            cells += cell.toString()
            if (line.startsWith("|")) cells.removeAt(0)
            if (line.endsWith("|") && !line.endsWith("\\|")) cells.removeAt(cells.size - 1)
            return cells.map { it.trimSpaces() }
        }
    }
}
