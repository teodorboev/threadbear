package com.mikerosoft.threadbear.android.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import com.mikerosoft.threadbear.kit.DraftBlock
import kotlinx.coroutines.delay

/**
 * A ```slack, ```email or ```draft block in a reply: text the agent wrote
 * for Mike to send somewhere else. A box like a code block's, its label on
 * the left and Copy on the right, then the draft in the body font with
 * every line where the agent put it (the reply's own Markdown would join
 * them up). A Slack draft draws Slack's formatting; the Mac draws the same
 * box.
 */
@Composable
fun DraftBlockView(draft: DraftBlock) {
    val colors = MaterialTheme.colorScheme
    val signals = LocalSignals.current
    val context = LocalContext.current
    val haptics = LocalHapticFeedback.current
    val link = LocalThreadLink.current
    // Which tap the "Copied" belongs to, so an older tap's timer doesn't put "Copy" back early.
    var copies by remember { mutableIntStateOf(0) }
    var copiedFor by remember { mutableIntStateOf(-1) }
    val copied = copiedFor == copies && copies > 0
    LaunchedEffect(copies) {
        if (copies == 0) return@LaunchedEffect
        delay(1500)
        copiedFor = -1
    }
    val shape = RoundedCornerShape(8.dp)
    Column(Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainerHigh).border(1.dp, colors.onSurface.copy(alpha = 0.12f), shape)) {
        Row(Modifier.fillMaxWidth().padding(start = 10.dp, end = 4.dp, top = 2.dp, bottom = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(draft.label, style = MaterialTheme.typography.labelMedium, color = colors.onSurfaceVariant)
            Spacer(Modifier.weight(1f))
            Row(
                Modifier.clip(RoundedCornerShape(8.dp)).clickable {
                    // Plain text and HTML in one clip, so Slack and Gmail keep the
                    // bold and the links, and anywhere else gets the words.
                    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                    clipboard.setPrimaryClip(ClipData.newHtmlText(draft.label, draft.plainText, draft.html))
                    haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                    copies += 1
                    copiedFor = copies
                }.padding(horizontal = 8.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                // Green with a tick for a moment after a tap, as on the Mac.
                val tint = if (copied) signals.done else colors.onSurfaceVariant
                Icon(if (copied) Line.CheckCircle else Line.Copy, null, Modifier.size(14.dp), tint = tint)
                Spacer(Modifier.width(5.dp))
                Text(
                    if (copied) "Copied" else "Copy", style = MaterialTheme.typography.labelMedium, color = tint,
                    fontWeight = if (copied) FontWeight.SemiBold else FontWeight.Normal,
                )
            }
        }
        HorizontalDivider(color = colors.outlineVariant)
        Column(Modifier.fillMaxWidth().padding(10.dp)) {
            for (line in draft.lines) {
                // A blank line keeps its height, so the gaps between paragraphs stay.
                val text = if (line.spans.all { it.text.isEmpty() }) AnnotatedString(" ")
                else attributed(line, draft.kind, signals.accent) { link(it, null) }
                if (line.quote) {
                    Box(Modifier.sideBar(colors.outline, 3.dp).padding(start = 10.dp)) {
                        Text(text, style = MaterialTheme.typography.bodyLarge, color = colors.onSurfaceVariant)
                    }
                } else {
                    Text(text, style = MaterialTheme.typography.bodyLarge)
                }
            }
        }
    }
}

private val bareAddress = Regex("""https?://[^\s<>()]+[^\s<>().,;:!?'"]""")

/** A line's spans with their looks. An email or plain draft is one span as written; its bare web addresses are linked. */
private fun attributed(line: DraftBlock.Line, kind: DraftBlock.Kind, accent: androidx.compose.ui.graphics.Color, open: (String) -> Unit): AnnotatedString =
    buildAnnotatedString {
        for (span in line.spans) {
            val style = SpanStyle(
                fontWeight = if (span.bold) FontWeight.SemiBold else null,
                fontStyle = if (span.italic) FontStyle.Italic else null,
                textDecoration = if (span.strike) TextDecoration.LineThrough else null,
                fontFamily = if (span.code) Mono else null,
            )
            val url = span.link
            if (url != null) {
                withLink(LinkAnnotation.Clickable(url, TextLinkStyles(SpanStyle(color = accent))) { open(url) }) { withStyle(style) { append(span.text) } }
            } else if (kind != DraftBlock.Kind.SLACK && span.text.contains("http")) {
                var last = 0
                withStyle(style) {
                    for (m in bareAddress.findAll(span.text)) {
                        append(span.text.substring(last, m.range.first))
                        val address = m.value
                        withLink(LinkAnnotation.Clickable(address, TextLinkStyles(SpanStyle(color = accent))) { open(address) }) { append(address) }
                        last = m.range.last + 1
                    }
                    append(span.text.substring(last))
                }
            } else {
                withStyle(style) { append(span.text) }
            }
        }
    }
