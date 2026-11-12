package com.mikerosoft.threadbear.android.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.mikerosoft.threadbear.kit.ApprovalOption
import com.mikerosoft.threadbear.kit.PendingApproval
import com.mikerosoft.threadbear.kit.PhoneMessage
import com.mikerosoft.threadbear.kit.Plans

/** Opens a plan in its sheet (the thread sets it), from its card in the transcript or the card it waits on. */
val LocalOpenPlan = staticCompositionLocalOf<(PhoneMessage) -> Unit> { {} }

/**
 * A Plan first plan in the transcript (backend API 72): its title and first
 * lines, opening the whole plan. The phone gets the plan in full only from
 * this row, as the card it waits on carries one line.
 */
@Composable
fun PlanCard(message: PhoneMessage) {
    val open = LocalOpenPlan.current
    val colors = MaterialTheme.colorScheme
    val inline = rememberInline(message.seq)
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(colors.surfaceContainer)
            .clickable(onClickLabel = "Opens the whole plan") { open(message) }.padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Line.Clipboard, null, Modifier.size(14.dp), tint = colors.onSurfaceVariant)
            Spacer(Modifier.width(6.dp))
            Text("Plan", style = MaterialTheme.typography.labelMedium, color = colors.onSurfaceVariant)
        }
        Text(Plans.title(message.text), style = MaterialTheme.typography.titleMedium)
        val preview = remember(message.text) { preview(Plans.body(message.text)) }
        if (preview.isNotEmpty()) Text(inline.parse(preview), style = MaterialTheme.typography.bodyMedium, color = colors.onSurfaceVariant,
            maxLines = 4, overflow = TextOverflow.Ellipsis)
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Read the plan", style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold, color = LocalSignals.current.accent)
            Spacer(Modifier.width(4.dp))
            Icon(Line.ChevronRight, null, Modifier.size(13.dp), tint = LocalSignals.current.accent)
        }
    }
}

/** The first few lines as plain words: no heading or list marks, which read as noise squeezed into four lines. */
private fun preview(body: String): String =
    body.split('\n').map { it.trim() }
        .filter { it.isNotEmpty() && !it.startsWith("```") && !it.startsWith("|") && it != "---" }
        .take(6)
        .joinToString(" ") { line -> line.trimStart('#').replaceFirst(Regex("""^\s*([-*+]|\d+[.)])\s+"""), "").trim() }

/**
 * A plan waiting for Go ahead, as its card: its title, the whole plan a tap
 * away, and Go ahead. Anything written in the box sends it back with those
 * words, as on the Mac.
 */
@Composable
fun PlanApprovalCard(card: PendingApproval.Approval, plan: PhoneMessage, tapped: String?, respond: (ApprovalOption) -> Unit) {
    val open = LocalOpenPlan.current
    val colors = MaterialTheme.colorScheme
    CardFrame(LocalSignals.current.needsYou) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Line.Clipboard, null, Modifier.size(18.dp), tint = colors.onSurfaceVariant)
            Spacer(Modifier.width(8.dp))
            Text("The plan is ready", style = MaterialTheme.typography.titleMedium)
        }
        Text(Plans.title(plan.text), style = MaterialTheme.typography.bodyMedium, color = colors.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
        val canAllow = ApprovalOption.ALLOW in card.options
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Pill("Read plan", { open(plan) }, Modifier.weight(1f))
            if (canAllow) Pill("Go ahead", { respond(ApprovalOption.ALLOW) }, Modifier.weight(1f), filled = true,
                spinning = tapped == ApprovalOption.ALLOW.wire, enabled = tapped == null)
        }
        Text(
            if (canAllow) "Or write below to ask for changes." else "Read it here, then go ahead on the Mac or write below to ask for changes.",
            style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant,
        )
    }
}

/**
 * The whole plan, to read, copy or share as Markdown, and, while it waits,
 * to approve or send back with what should change. `approvalId`: the card
 * this plan waits on when it's the newest and can be allowed from here.
 * `approving`: that card's answer is on its way.
 */
@Composable
fun PlanSheet(
    plan: PhoneMessage,
    approvalId: String?,
    approving: Boolean,
    canSendBack: Boolean,
    goAhead: (String) -> Unit,
    sendBack: (String, (Boolean) -> Unit) -> Unit,
    close: () -> Unit,
) {
    val context = LocalContext.current
    var change by remember { mutableStateOf("") }
    var sending by remember { mutableStateOf(false) }
    var copied by remember { mutableStateOf(false) }
    var approved by remember { mutableStateOf(false) }
    val typed = change.trim()
    val title = Plans.title(plan.text)
    // Approved: the sheet goes once its card has. A refused one stays.
    LaunchedEffect(approvalId) { if (approved && approvalId == null) close() }
    FullSheet(title, close, actions = {
        IconButton(onClick = {
            (context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager).setPrimaryClip(ClipData.newPlainText(title, plan.text))
            copied = true
        }) { Icon(if (copied) Line.Check else Line.Copy, if (copied) "Copied" else "Copy the plan", Modifier.size(18.dp)) }
        IconButton(onClick = {
            // As Markdown text, to Drive, Gmail, Keep or another app.
            val send = Intent(Intent.ACTION_SEND).setType("text/markdown").putExtra(Intent.EXTRA_TEXT, plan.text)
                .putExtra(Intent.EXTRA_SUBJECT, Plans.fileName(plan.text)).putExtra(Intent.EXTRA_TITLE, Plans.fileName(plan.text))
            context.startActivity(Intent.createChooser(send, "Share the plan").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }) { Icon(Line.Share, "Share the plan", Modifier.size(18.dp)) }
    }) {
        Column(Modifier.fillMaxSize()) {
            Box(Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState()).padding(16.dp)) {
                MarkdownText(plan.text, Modifier.fillMaxWidth(), seq = plan.seq)
            }
            if (canSendBack) {
                val colors = MaterialTheme.colorScheme
                Column(
                    Modifier.fillMaxWidth().background(colors.surfaceContainerLow).navigationBarsPadding().imePadding().padding(16.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    Box(
                        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(colors.surfaceContainerHighest)
                            .heightIn(min = 44.dp).padding(horizontal = 12.dp, vertical = 11.dp),
                    ) {
                        if (change.isEmpty()) Text(if (approvalId == null) "Ask for changes" else "What should change?",
                            style = MaterialTheme.typography.bodyLarge, color = colors.onSurfaceVariant)
                        BasicTextField(
                            change, { change = it }, enabled = !sending && !approving, maxLines = 5,
                            textStyle = MaterialTheme.typography.bodyLarge.copy(color = colors.onSurface),
                            cursorBrush = SolidColor(LocalSignals.current.accent), modifier = Modifier.fillMaxWidth(),
                        )
                    }
                    // Go ahead with nothing typed; Send back once there's something to change.
                    if (approvalId != null && typed.isEmpty()) {
                        Pill("Go ahead", { approved = true; goAhead(approvalId) }, Modifier.fillMaxWidth(), filled = true, spinning = approving)
                    } else {
                        Pill("Send back", {
                            if (typed.isEmpty() || sending) return@Pill
                            sending = true
                            sendBack(typed) { ok ->
                                sending = false
                                if (ok) { change = ""; close() }
                            }
                        }, Modifier.fillMaxWidth(), filled = typed.isNotEmpty(), spinning = sending, enabled = typed.isNotEmpty())
                    }
                }
            }
        }
    }
}
