package com.mikerosoft.threadbear.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.mikerosoft.threadbear.kit.InlineMarks
import com.mikerosoft.threadbear.kit.MarkdownBlock
import com.mikerosoft.threadbear.kit.MessageButton
import com.mikerosoft.threadbear.kit.ReplyButtons
import com.mikerosoft.threadbear.kit.ThreadPictures

/**
 * Assistant text, block by block from the kit's MarkdownBlock, so
 * paragraphs, lists, tables, quotes and code keep their shape as on the
 * Mac; inline markdown inside each. `seq` says which upload a picture's
 * address means; `buttons` are the reply buttons the message links
 * (backend API 59), null: its button links are just their titles.
 */
@Composable
fun MarkdownText(
    text: String,
    modifier: Modifier = Modifier,
    style: TextStyle = MaterialTheme.typography.bodyLarge,
    seq: Int? = null,
    buttons: List<MessageButton>? = null,
    selectable: Boolean = true,
) {
    val blocks = remember(text) { MarkdownBlock.parse(text) }
    if (selectable) {
        SelectionContainer(modifier) { MarkdownBlocks(blocks, style = style, seq = seq, buttons = buttons) }
    } else {
        MarkdownBlocks(blocks, modifier, style, seq, buttons)
    }
}

@Composable
fun MarkdownBlocks(
    blocks: List<MarkdownBlock>,
    modifier: Modifier = Modifier,
    style: TextStyle = MaterialTheme.typography.bodyLarge,
    seq: Int? = null,
    buttons: List<MessageButton>? = null,
) {
    val inline = rememberInline(seq)
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        for (block in blocks) Block(block, inline, style, seq, buttons)
    }
}

/** The inline marks, with links going to the open thread's handler. */
@Composable
fun rememberInline(seq: Int?): InlineStyle {
    val accent = LocalSignals.current.accent
    val codeBg = MaterialTheme.colorScheme.surfaceContainerHigh
    val link = LocalThreadLink.current
    return remember(accent, codeBg, link, seq) { InlineStyle(accent, codeBg) { link(it, seq) } }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Block(block: MarkdownBlock, inline: InlineStyle, style: TextStyle, seq: Int?, buttons: List<MessageButton>?) {
    val colors = MaterialTheme.colorScheme
    when (block) {
        is MarkdownBlock.Paragraph -> TextLine(block.text, inline, style, buttons)
        is MarkdownBlock.Heading -> Text(
            inline.parse(block.text),
            style = style.copy(fontWeight = FontWeight.SemiBold, fontSize = if (block.level <= 2) 17.sp else style.fontSize),
        )
        is MarkdownBlock.ListItem -> Row(Modifier.padding(start = (block.depth * 18).dp)) {
            val task = block.task
            if (task != null) {
                Icon(if (task) Line.CheckSquare else Line.Square, if (task) "Done" else "To do",
                    Modifier.padding(top = 3.dp).size(16.dp), tint = colors.onSurfaceVariant)
            } else {
                Text(block.marker, style = style, color = colors.onSurfaceVariant, modifier = Modifier.widthIn(min = 14.dp))
            }
            Spacer(Modifier.width(6.dp))
            TextLine(block.text, inline, style, buttons)
        }
        is MarkdownBlock.Code -> Box(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(8.dp)).background(colors.surfaceContainerHigh)
                .horizontalScroll(rememberScrollState()).padding(10.dp),
        ) {
            Text(block.text, style = style.copy(fontFamily = Mono, fontSize = 13.sp, lineHeight = 18.sp))
        }
        is MarkdownBlock.Rich -> RichBlockView(block.kind, block.source)
        is MarkdownBlock.Draft -> DraftBlockView(block.draft)
        // The bar drawn behind, not a sibling sized by IntrinsicSize: a quote can
        // hold a picture, whose layout can't be asked its intrinsic height
        // (it threw: Codex review, 11 Nov).
        is MarkdownBlock.Quote -> Box(Modifier.sideBar(colors.outline, 3.dp).padding(start = 12.dp)) {
            MarkdownBlocks(block.blocks, style = style.copy(color = colors.onSurfaceVariant), seq = seq, buttons = buttons)
        }
        MarkdownBlock.Rule -> HorizontalDivider(Modifier.padding(vertical = 4.dp), color = colors.outlineVariant)
        is MarkdownBlock.Table -> MarkdownTable(block, inline, style)
        is MarkdownBlock.Image -> {
            val pictures = LocalThreadPictures.current
            val image = pictures.image(block.src, seq)
            val web = ThreadPictures.webUrl(block.src)
            when {
                image != null -> ReplyPicture(image.url, block.alt, image.width, image.height)
                web != null -> ReplyPicture(web, block.alt)
                else -> PictureOnMac(block.alt)
            }
        }
    }
}

/**
 * A line's words, then the buttons it ends with as real buttons. A button
 * link whose id the row doesn't carry is its title, as text.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun TextLine(written: String, inline: InlineStyle, style: TextStyle, buttons: List<MessageButton>?) {
    val text = remember(written, buttons) { ReplyButtons.unlinking(written, buttons.orEmpty().map { it.id }.toSet()) }
    val split = remember(text) { ReplyButtons.trailing(text) }
    if (split.links.isEmpty()) {
        Text(inline.parse(text), style = style)
        return
    }
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        if (split.text.isNotEmpty()) Text(inline.parse(split.text), style = style)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            for (link in split.links) {
                buttons?.firstOrNull { it.id == link.id }?.let { ReplyButtonPill(it) }
            }
        }
    }
}

/**
 * A markdown table: a bold header row, lines between rows, and sideways
 * scrolling when it's wider than the phone. Cells wrap at 220 dp rather
 * than run off forever; past that the row scrolls. Columns line up across
 * rows, each as wide as its widest cell.
 */
@Composable
private fun MarkdownTable(table: MarkdownBlock.Table, inline: InlineStyle, style: TextStyle) {
    val colors = MaterialTheme.colorScheme
    val small = style.copy(fontSize = 14.sp, lineHeight = 19.sp)
    // Bounded whatever the reply says: the parser allows 1,000 columns within
    // its cell budget, and 1,000 columns of 220 dp is wider than Compose can
    // lay out (Constraints throws past its limits; Codex review, 11 Nov). So
    // at most MAX_COLUMNS columns, MAX_ROWS rows and MAX_CELL_LINES lines a
    // cell, which keeps the table well inside them; what's cut says so.
    val columns = minOf(table.header.size, MAX_COLUMNS)
    val rows = listOf(table.header) + table.rows.take(MAX_ROWS)
    val hidden = buildList {
        if (table.header.size > MAX_COLUMNS) add("${table.header.size - MAX_COLUMNS} more columns")
        if (table.rows.size > MAX_ROWS) add("${table.rows.size - MAX_ROWS} more rows")
    }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
    Box(
        Modifier.clip(RoundedCornerShape(8.dp)).background(colors.surfaceContainerHigh)
            .horizontalScroll(rememberScrollState()).padding(10.dp),
    ) {
        Layout(content = {
            rows.forEachIndexed { r, row ->
                for (c in 0 until columns) {
                    Text(
                        inline.parse(row.getOrElse(c) { "" }),
                        style = if (r == 0) small.copy(fontWeight = FontWeight.SemiBold) else small,
                        maxLines = MAX_CELL_LINES,
                        overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis,
                        textAlign = when (table.alignments.getOrNull(c)) {
                            MarkdownBlock.Alignment.CENTER -> androidx.compose.ui.text.style.TextAlign.Center
                            MarkdownBlock.Alignment.TRAILING -> androidx.compose.ui.text.style.TextAlign.End
                            else -> androidx.compose.ui.text.style.TextAlign.Start
                        },
                    )
                }
            }
            // A line under each row but the last.
            repeat(rows.size - 1) { HorizontalDivider(color = colors.outlineVariant) }
        }) { measurables, _ ->
            val gap = 14.dp.roundToPx()
            val rowGap = 6.dp.roundToPx()
            val cellMax = 220.dp.roundToPx()
            val cells = measurables.take(rows.size * columns).map { it.measure(Constraints(maxWidth = cellMax)) }
            val widths = IntArray(columns) { c -> rows.indices.maxOf { r -> cells[r * columns + c].width } }
            val heights = IntArray(rows.size) { r -> (0 until columns).maxOf { c -> cells[r * columns + c].height } }
            val total = widths.sum() + gap * (columns - 1).coerceAtLeast(0)
            val lines = measurables.drop(rows.size * columns).map { it.measure(Constraints.fixedWidth(total.coerceAtLeast(1))) }
            val lineHeight = lines.firstOrNull()?.height ?: 0
            val height = heights.sum() + (rows.size - 1) * (rowGap * 2 + lineHeight)
            layout(total, height) {
                var y = 0
                for (r in rows.indices) {
                    var x = 0
                    for (c in 0 until columns) {
                        val p = cells[r * columns + c]
                        val dx = when (table.alignments.getOrNull(c)) {
                            MarkdownBlock.Alignment.CENTER -> (widths[c] - p.width) / 2
                            MarkdownBlock.Alignment.TRAILING -> widths[c] - p.width
                            else -> 0
                        }
                        p.place(x + dx, y)
                        x += widths[c] + gap
                    }
                    y += heights[r]
                    if (r < rows.size - 1) {
                        y += rowGap
                        lines[r].place(0, y)
                        y += lineHeight + rowGap
                    }
                }
            }
        }
    }
    if (hidden.isNotEmpty()) Text(hidden.joinToString(" and ") + " on the Mac", style = MaterialTheme.typography.labelSmall, color = colors.onSurfaceVariant)
    }
}

/**
 * A rounded bar down the left of whatever this is on, as tall as it is.
 * Drawn behind rather than laid out beside, so nothing is ever measured
 * intrinsically: what's inside can be a picture or a web view, which can't
 * answer an intrinsic measure.
 */
fun Modifier.sideBar(color: Color, width: androidx.compose.ui.unit.Dp): Modifier = drawBehind {
    val w = width.toPx()
    drawRoundRect(color, size = androidx.compose.ui.geometry.Size(w, size.height), cornerRadius = androidx.compose.ui.geometry.CornerRadius(w / 2, w / 2))
}

private const val MAX_COLUMNS = 24
private const val MAX_ROWS = 100
private const val MAX_CELL_LINES = 8

/**
 * Inline marks drawn: the kit's InlineMarks runs as styled text. Every link
 * goes to `onLink`, never straight to the system. The runs come from one
 * pass that can't loop (a bare address once parsed itself forever, Daybreak
 * 11 Nov 2026), so this only styles them.
 */
class InlineStyle(
    private val accent: Color,
    private val codeBg: Color,
    /** Null: nothing in it is a link, its words only (a skill's description, Daybreak 8 Nov). */
    private val onLink: ((String) -> Unit)? = {},
) {
    fun parse(text: String): AnnotatedString = buildAnnotatedString {
        val runs = InlineMarks.parse(text, links = onLink != null)
        var i = 0
        while (i < runs.size) {
            val link = runs[i].link
            // A link's runs together, as one link with its words styled inside.
            var j = i + 1
            while (link != null && j < runs.size && runs[j].link == link) j++
            val handler = onLink
            if (link != null && handler != null) {
                val url = link.url
                withLink(LinkAnnotation.Clickable(url, TextLinkStyles(SpanStyle(color = accent))) { handler(url) }) {
                    for (k in i until j) draw(this, runs[k])
                }
            } else {
                for (k in i until j) draw(this, runs[k])
            }
            i = j
        }
    }

    private fun draw(b: AnnotatedString.Builder, run: InlineMarks.Run) {
        if (run.marks.isEmpty()) return b.append(run.text)
        val style = SpanStyle(
            fontFamily = if (InlineMarks.Mark.CODE in run.marks) Mono else null,
            fontSize = if (InlineMarks.Mark.CODE in run.marks) 0.9.em() else androidx.compose.ui.unit.TextUnit.Unspecified,
            background = if (InlineMarks.Mark.CODE in run.marks) codeBg else Color.Unspecified,
            fontWeight = if (InlineMarks.Mark.BOLD in run.marks) FontWeight.SemiBold else null,
            fontStyle = if (InlineMarks.Mark.ITALIC in run.marks) FontStyle.Italic else null,
            textDecoration = if (InlineMarks.Mark.STRIKE in run.marks) TextDecoration.LineThrough else null,
        )
        b.withStyle(style) { append(run.text) }
    }

    private fun Double.em() = androidx.compose.ui.unit.TextUnit(this.toFloat(), androidx.compose.ui.unit.TextUnitType.Em)
}
