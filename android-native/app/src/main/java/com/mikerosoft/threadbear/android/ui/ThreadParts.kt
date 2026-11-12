package com.mikerosoft.threadbear.android.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.InlineTextContent
import androidx.compose.foundation.text.appendInlineContent
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.PlainTooltip
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TooltipBox
import androidx.compose.material3.TooltipDefaults
import androidx.compose.material3.rememberTooltipState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.text.Placeholder
import androidx.compose.ui.text.PlaceholderVerticalAlign
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.mikerosoft.threadbear.kit.AgentHeader
import com.mikerosoft.threadbear.kit.CardMentions
import com.mikerosoft.threadbear.kit.InlinePictures
import com.mikerosoft.threadbear.kit.MessageImage
import com.mikerosoft.threadbear.kit.PhoneMessage
import com.mikerosoft.threadbear.kit.ReviewAnswer
import com.mikerosoft.threadbear.kit.Role
import com.mikerosoft.threadbear.kit.ThreadItem
import com.mikerosoft.threadbear.kit.TurnGap
import com.mikerosoft.threadbear.kit.WaitingSend
import com.mikerosoft.threadbear.kit.WaitingSends
import com.mikerosoft.threadbear.kit.WorkedTurn
import com.mikerosoft.threadbear.kit.agentHeader
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

// The transcript's rows, as ThreadView.swift and AsksCard.swift draw them.

/** One row of a thread, a turn's working, or a work: link's sheet. */
@Composable
fun ItemView(item: ThreadItem, pinned: Map<Int, List<MessageImage>> = emptyMap(), nested: Boolean = false) {
    when (item) {
        is ThreadItem.Message -> when {
            item.message.plan -> PlanCard(item.message)
            nested && item.message.role == Role.ASSISTANT -> StepLine(item.message.seq) {
                MarkdownText(item.message.text, Modifier.fillMaxWidth(), MaterialTheme.typography.bodyMedium.copy(color = MaterialTheme.colorScheme.onSurfaceVariant),
                    seq = item.message.seq)
            }
            else -> MessageView(item.message, pinned[item.message.seq].orEmpty())
        }
        is ThreadItem.Tools -> ToolRunView(item.items)
        is ThreadItem.Worked -> WorkedView(item.worked)
        is ThreadItem.Gap -> GapView(item.gap)
    }
}

@Composable
fun MessageView(m: PhoneMessage, pinned: List<MessageImage> = emptyList()) {
    when (m.role) {
        Role.USER -> {
            val header = m.agentHeader
            if (header != null) AgentMessageCard(header, m.seq) else UserBubble(m)
        }
        Role.ASSISTANT -> Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            MarkdownText(m.text, Modifier.fillMaxWidth(), seq = m.seq, buttons = m.buttons)
            for (image in pinned) PinnedPicture(image)
        }
        Role.TOOL -> ToolRunView(listOf(m))
    }
}

/**
 * Mike's message: the photos he sent over his words as the Mac shows them
 * (Mike, 8 Nov: a bare "[2 pictures]" looked like they'd been lost), an
 * answer to a review as what he picked, [Picture N] markers and Notion cards
 * and threads as chips, and under it where it came from.
 */
@Composable
private fun UserBubble(m: PhoneMessage) {
    val colors = MaterialTheme.colorScheme
    val pictures = LocalThreadPictures.current
    val shown = remember(m.text, pictures) { pictures.words(m.text, m.seq) }
    Column(Modifier.fillMaxWidth().padding(start = 40.dp), horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        for (image in pictures.pasted(m.seq)) ReplyPicture(image.url, "", image.width, image.height, tallest = 220.dp, alignEnd = true)
        if (shown.isNotEmpty()) {
            SelectionContainer {
                Box(Modifier.clip(RoundedCornerShape(16.dp)).background(colors.surfaceContainerHigh).padding(horizontal = 14.dp, vertical = 10.dp)) {
                    val review = remember(m.text) { ReviewAnswer.parse(m.text) }
                    if (review != null) ReviewLines(review) else ChipText(shown)
                }
            }
        }
        when (m.origin) {
            "phone" -> PhoneMark()
            "schedule" -> Text("Scheduled", style = MaterialTheme.typography.labelSmall, color = LocalSignals.current.faint)
            "runner" -> Text("From the runner", style = MaterialTheme.typography.labelSmall, color = LocalSignals.current.faint)
        }
    }
}

/** An answer to a review as the Mac shows it: the review's title small, then what he picked and his notes (Mike, 29 Oct). */
@Composable
private fun ReviewLines(review: ReviewAnswer) {
    val faint = MaterialTheme.colorScheme.onSurfaceVariant
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        review.lines.forEachIndexed { i, line ->
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                // The same mark as the Mac's review card. A reply under its pick has no label of its own.
                if (i == 0 || line.label.isNotEmpty()) Row(verticalAlignment = Alignment.CenterVertically) {
                    if (i == 0) {
                        Icon(Line.Layers, null, Modifier.size(13.dp), tint = faint)
                        Spacer(Modifier.width(5.dp))
                    }
                    Text(line.label, style = MaterialTheme.typography.labelMedium, color = faint)
                }
                Text(line.text, style = MaterialTheme.typography.bodyLarge)
            }
        }
    }
}

/**
 * A message's words with its [Picture N] markers and Notion cards drawn as
 * chips, as the Mac draws them; a thread's chip opens it.
 */
@Composable
private fun ChipText(text: String) {
    val pieces = remember(text) { InlinePictures.pieces(text) }
    val mentions = remember(text) { CardMentions.find(text) }
    val hasChip = pieces.any { it is InlinePictures.Piece.Picture } || mentions.isNotEmpty()
    val style = MaterialTheme.typography.bodyLarge
    if (!hasChip) {
        Text(text, style = style)
        return
    }
    val push = LocalPushThread.current
    val chips = HashMap<String, InlineTextContent>()
    val annotated = buildAnnotatedString {
        for (piece in pieces) when (piece) {
            is InlinePictures.Piece.Words -> {
                var last = 0
                for (mention in CardMentions.find(piece.text)) {
                    append(piece.text.substring(last, mention.start))
                    val id = "c${chips.size}"
                    val words = mention.title.take(28)
                    chips[id] = InlineTextContent(Placeholder((words.length * 0.55 + 2.4).em, 1.45.em, PlaceholderVerticalAlign.TextCenter)) {
                        val target = mention.url.removePrefix("thread:")
                        Chip(mention.title, if (mention.isThread) Line.Message else Line.FileText,
                            if (mention.isThread) ({ push(target) }) else null)
                    }
                    appendInlineContent(id, mention.title)
                    last = mention.end
                }
                append(piece.text.substring(last))
            }
            is InlinePictures.Piece.Picture -> {
                val id = "c${chips.size}"
                val words = "Picture ${piece.number}"
                chips[id] = InlineTextContent(Placeholder((words.length * 0.55 + 2.4).em, 1.45.em, PlaceholderVerticalAlign.TextCenter)) {
                    Chip(words, Line.Photo, null)
                }
                appendInlineContent(id, InlinePictures.marker(piece.number))
            }
        }
    }
    Text(annotated, style = style, inlineContent = chips)
}

@Composable
private fun Chip(title: String, icon: androidx.compose.ui.graphics.vector.ImageVector, open: (() -> Unit)?) {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier.fillMaxSize().padding(horizontal = 1.dp).clip(RoundedCornerShape(6.dp)).background(colors.onSurface.copy(alpha = 0.10f))
            .border(0.5.dp, colors.outline, RoundedCornerShape(6.dp))
            .then(if (open != null) Modifier.clickable(onClickLabel = "Open the thread", onClick = open) else Modifier)
            .padding(horizontal = 5.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, Modifier.size(12.dp), tint = colors.onSurfaceVariant)
        Spacer(Modifier.width(4.dp))
        Text(title, style = MaterialTheme.typography.bodySmall.copy(fontSize = 13.sp), color = colors.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** Under a message sent from a phone: a small phone in place of a line of text, as on the Mac, that says what it means when held. */
@OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun PhoneMark() {
    val state = rememberTooltipState()
    val scope = rememberCoroutineScope()
    TooltipBox(TooltipDefaults.rememberPlainTooltipPositionProvider(), { PlainTooltip { Text("Sent from the phone app") } }, state) {
        Box(Modifier.clip(RoundedCornerShape(6.dp)).clickable(onClickLabel = "Sent from the phone app") { scope.launch { state.show() } }.padding(4.dp)) {
            Icon(Line.Phone, "Sent from the phone app", Modifier.size(12.dp), tint = LocalSignals.current.faint)
        }
    }
}

/**
 * A message an agent wrote, not Mike: another thread's (its send_message or
 * create_thread) or this thread's own before a move. Full width in a quiet
 * card with who sent it on top, as the Mac draws it, so it never reads as
 * his bubble.
 */
@Composable
private fun AgentMessageCard(header: AgentHeader, seq: Int) {
    val colors = MaterialTheme.colorScheme
    val shape = RoundedCornerShape(16.dp)
    Column(
        Modifier.fillMaxWidth().clip(shape).background(colors.surfaceContainer.copy(alpha = 0.5f)).border(1.dp, colors.surfaceContainerHigh, shape).padding(12.dp),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(if (header.movedTo == null) Line.CornerDownRight else Line.Folder, null, Modifier.size(14.dp), tint = colors.onSurfaceVariant)
            Spacer(Modifier.width(6.dp))
            val from = header.movedTo?.let { "Left by this thread's agent when it moved to ${abbreviate(it)}" }
                ?: "From the agent in ${header.title.ifEmpty { header.threadId }}"
            Text(from, style = MaterialTheme.typography.labelMedium, color = colors.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        val body = header.body.trim()
        if (body.isNotEmpty()) MarkdownText(body, Modifier.fillMaxWidth(), seq = seq)
    }
}

/** The home folder as ~, as the Mac writes a path. */
private fun abbreviate(path: String): String = path.replace(Regex("^/Users/[^/]+"), "~")

/** A picture the agent pinned rather than wrote into its reply. */
@Composable
fun PinnedPicture(image: MessageImage) = ReplyPicture(image.url, "", image.width, image.height)

/**
 * Tool calls between two messages: up to three as small lines, more folded
 * into "12 tool calls" that opens to them. What they printed stays on the Mac.
 */
@Composable
fun ToolRunView(lines: List<PhoneMessage>) {
    val lit = LocalLitStep.current
    var open by remember { mutableStateOf(false) }
    LaunchedEffect(lit) { if (lit != null && lines.any { it.seq == lit }) open = true }
    val faint = MaterialTheme.colorScheme.onSurfaceVariant
    @Composable
    fun Lines() = Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        for (m in lines) StepLine(m.seq) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Line.tool(m.toolKind), null, Modifier.size(13.dp), tint = faint)
                Spacer(Modifier.width(7.dp))
                Text(middle(m.text), style = MaterialTheme.typography.bodySmall, color = faint, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }
    if (lines.size <= 3) {
        Lines()
        return
    }
    Column(Modifier.fillMaxWidth()) {
        Disclosure("${lines.size} tool calls", open) { open = !open }
        AnimatedVisibility(open, enter = expandVertically() + fadeIn(), exit = shrinkVertically() + fadeOut()) {
            Box(Modifier.padding(top = 4.dp)) { Lines() }
        }
    }
}

/** A long line cut in the middle, as the iPhone truncates it, so its end shows. */
private fun middle(text: String, limit: Int = 90): String {
    val line = text.lineSequence().firstOrNull().orEmpty()
    if (line.length <= limit) return line
    return line.take(limit / 2) + "…" + line.takeLast(limit / 2 - 1)
}

@Composable
private fun Disclosure(title: String, open: Boolean, toggle: () -> Unit) {
    val turn by animateFloatAsState(if (open) 90f else 0f, label = "chevron")
    val faint = MaterialTheme.colorScheme.onSurfaceVariant
    Row(
        Modifier.clip(RoundedCornerShape(8.dp)).clickable(onClickLabel = if (open) "Hide" else "Show", onClick = toggle).padding(vertical = 4.dp, horizontal = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = MaterialTheme.typography.labelLarge, color = faint)
        Spacer(Modifier.width(4.dp))
        Icon(Line.ChevronRight, null, Modifier.size(14.dp).rotate(turn), tint = faint)
    }
}

/** A finished turn's work, folded into one line that opens to it, as the Mac shows it. */
@Composable
fun WorkedView(worked: WorkedTurn) {
    var open by remember { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth()) {
        Disclosure(worked.title, open) { open = !open }
        AnimatedVisibility(open, enter = expandVertically() + fadeIn(), exit = shrinkVertically() + fadeOut()) {
            // The line drawn behind: a turn's working can hold pictures and rich
            // blocks, which can't be measured intrinsically (sideBar).
            Box(Modifier.padding(top = 6.dp).sideBar(MaterialTheme.colorScheme.outlineVariant, 1.dp).padding(start = 13.dp)) {
                WorkedItems(worked)
            }
        }
    }
}

/** What a turn's working is made of, in its fold or a work: link's sheet. */
@Composable
fun WorkedItems(worked: WorkedTurn) {
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        for (item in worked.items) ItemView(item, nested = true)
    }
}

/**
 * Marks a line of a turn's working as step `seq`, for a work: link to
 * scroll to and light up: the accent behind it, past the words' edges.
 */
@Composable
private fun StepLine(seq: Int, content: @Composable () -> Unit) {
    val lit = LocalLitStep.current == seq
    val at = LocalStepAt.current
    val color by animateColorAsState(if (lit) LocalSignals.current.accent.copy(alpha = 0.25f) else Color.Transparent, label = "lit")
    Box(
        Modifier.then(if (lit && at != null) Modifier.onGloballyPositioned { at.value = it.positionInRoot().y } else Modifier)
            .clip(RoundedCornerShape(6.dp)).background(color).padding(horizontal = 6.dp, vertical = 3.dp),
    ) { content() }
}

/**
 * "6 min later · Mine the repo finished": a thin rule with the wait in the
 * middle, before a turn the agent started by itself. Quiet, like a chat
 * app's day divider, as on the Mac.
 */
@Composable
fun GapView(gap: TurnGap) {
    Row(Modifier.fillMaxWidth().padding(vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
        HorizontalDivider(Modifier.weight(1f).widthIn(min = 24.dp), color = MaterialTheme.colorScheme.outlineVariant)
        Text(gap.label, style = MaterialTheme.typography.labelSmall, color = LocalSignals.current.faint, maxLines = 1,
            overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(horizontal = 10.dp))
        HorizontalDivider(Modifier.weight(1f).widthIn(min = 24.dp), color = MaterialTheme.colorScheme.outlineVariant)
    }
}

/**
 * "Bear with me · 12s" while a turn runs, or what it's doing. The seconds
 * count from when the phone first saw it working.
 */
@Composable
fun WorkingLine(text: String?) {
    val started = remember { System.currentTimeMillis() }
    val now = rememberNow(1000)
    val seconds = maxOf(0L, (now.toEpochMilli() - started) / 1000)
    val elapsed = if (seconds < 60) "${seconds}s" else "${seconds / 60}m ${seconds % 60}s"
    val words = if (text.isNullOrEmpty() || text == "Bear with me") "Bear with me · $elapsed" else "$text · $elapsed"
    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(vertical = 4.dp)) {
        WorkingDot(7.dp)
        Spacer(Modifier.width(10.dp))
        Text(words, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** At the top of a thread: older messages, from Convex (API 2) or from the Mac on request (before). */
@Composable
fun EarlierButton(loading: Boolean, waitingForMac: Boolean, load: () -> Unit) {
    Box(Modifier.fillMaxWidth().heightIn(min = 36.dp), contentAlignment = Alignment.Center) {
        TextButton(onClick = load, enabled = !loading) {
            if (loading) {
                CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 1.5.dp, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Spacer(Modifier.width(8.dp))
            }
            Text(
                if (!loading) "Load earlier messages" else if (waitingForMac) "Waiting for the Mac to wake up" else "Loading…",
                style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/**
 * A message the runner hasn't taken yet (or one on its way from here): the
 * bubble dimmed, and under it why it's waiting and a Cancel while it can
 * still be stopped. Why goes under the last one only, not repeated down
 * the column.
 */
@Composable
fun WaitingMessageView(text: String, send: WaitingSend?, offlineRunner: String?, says: Boolean, cancelling: Boolean, cancel: (() -> Unit)?) {
    val colors = MaterialTheme.colorScheme
    val faint = LocalSignals.current.faint
    Column(Modifier.fillMaxWidth().padding(start = 40.dp), horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(
            text, style = MaterialTheme.typography.bodyLarge, color = colors.onSurface.copy(alpha = 0.55f),
            modifier = Modifier.clip(RoundedCornerShape(16.dp)).background(colors.surfaceContainerHigh.copy(alpha = 0.55f)).padding(horizontal = 14.dp, vertical = 10.dp),
        )
        val canCancel = send?.canCancel == true && cancel != null
        Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.heightIn(min = 28.dp)) {
            if (says || !canCancel) {
                Icon(if (offlineRunner != null && canCancel) Line.Clock else Line.ArrowUpCircle, null, Modifier.size(12.dp), tint = faint)
                Spacer(Modifier.width(5.dp))
                Text(if (send == null) "Sending…" else WaitingSends.note(send, offlineRunner), style = MaterialTheme.typography.labelSmall, color = faint)
                if (canCancel) Text(" · ", style = MaterialTheme.typography.labelSmall, color = faint)
            }
            if (canCancel) {
                if (cancelling) {
                    CircularProgressIndicator(Modifier.size(12.dp), strokeWidth = 1.5.dp, color = faint)
                } else {
                    // The one thing on the line to tap.
                    Text(
                        "Cancel", style = MaterialTheme.typography.labelMedium, color = LocalSignals.current.accent,
                        modifier = Modifier.clip(RoundedCornerShape(6.dp)).clickable(onClickLabel = "Takes this message back before it's sent", onClick = cancel!!)
                            .padding(horizontal = 4.dp, vertical = 4.dp),
                    )
                }
            }
        }
    }
}

/**
 * A turn's working, from a work: link: scrolled to the step, which lights
 * up for a moment. The Mac opens it in place; on this small screen a sheet
 * leaves the place in the reply alone.
 */
@Composable
fun WorkingSheet(hit: com.mikerosoft.threadbear.kit.ThreadLinks.Hit, close: () -> Unit) {
    var lit by remember { mutableStateOf<Int?>(null) }
    val scroll = androidx.compose.foundation.rememberScrollState()
    // Where the lit step and the top of the list are on screen, once drawn.
    val stepAt = remember { mutableStateOf<Float?>(null) }
    var top by remember { mutableStateOf(0f) }
    LaunchedEffect(hit) {
        val step = hit.step ?: return@LaunchedEffect
        lit = step
        // Once a folded run of tool calls has opened for it.
        delay(150)
        stepAt.value?.let { scroll.animateScrollTo((scroll.value + it - top - 240f).toInt().coerceAtLeast(0)) }
        delay(2000)
        lit = null
    }
    FullSheet(hit.turn.title, close) {
        androidx.compose.runtime.CompositionLocalProvider(LocalLitStep provides lit, LocalStepAt provides stepAt) {
            Column(
                Modifier.fillMaxSize().onGloballyPositioned { top = it.positionInRoot().y }
                    .verticalScroll(scroll).padding(16.dp),
            ) { WorkedItems(hit.turn) }
        }
    }
}

/** Where the lit step was drawn, for its sheet to scroll to. */
private val LocalStepAt = androidx.compose.runtime.compositionLocalOf<androidx.compose.runtime.MutableState<Float?>?> { null }

/** A full-screen sheet with a title and Done, as the iPhone's sheets have. */
@Composable
fun FullSheet(title: String, close: () -> Unit, actions: @Composable () -> Unit = {}, content: @Composable () -> Unit) {
    androidx.compose.ui.window.Dialog(onDismissRequest = close, properties = androidx.compose.ui.window.DialogProperties(usePlatformDefaultWidth = false)) {
        Column(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
            Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 8.dp, top = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = close, contentPadding = PaddingValues(horizontal = 12.dp)) { Text("Done") }
                Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                actions()
            }
            Box(Modifier.weight(1f)) { content() }
        }
    }
}
