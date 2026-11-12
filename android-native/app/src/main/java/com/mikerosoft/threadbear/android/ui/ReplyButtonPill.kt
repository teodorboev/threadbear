package com.mikerosoft.threadbear.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.android.ThreadExtras
import com.mikerosoft.threadbear.kit.MessageButton
import com.mikerosoft.threadbear.kit.ReplyButtons

/** The blue a start button is, and the purple of one that sends a message, as on the Mac. */
private val StartBlue = Color(0xFF0A84FF)
private val SendPurple = Color(0xFFBF5AF2)
private val PressedGreen = Color(0xFF30D158)

/**
 * A reply button (docs/protocol.md, "Reply buttons"): blue starts a thread,
 * purple sends a message, green with a tick once pressed, which then opens
 * the thread it started or messaged. A spinner while a press is on its way,
 * and the error under it if the press failed. The kit's ReplyButtons says
 * which.
 */
@Composable
fun ReplyButtonPill(button: MessageButton) {
    val model = LocalModel.current
    val extras = ThreadExtras.of(model, LocalContext.current)
    val threadId = LocalButtonThread.current
    val push = LocalPushThread.current
    val presses by extras.presses.collectAsStateWithLifecycle()
    val look = threadId?.let { extras.look(button, it, presses) } ?: ReplyButtons.Look.Ready
    val pressed = look is ReplyButtons.Look.Pressed
    val failed = look as? ReplyButtons.Look.Failed
    // Nothing to open: the thread it went to isn't known.
    val enabled = threadId != null && look != ReplyButtons.Look.Pressing && look != ReplyButtons.Look.Pressed(null)
    val hint = when (look) {
        is ReplyButtons.Look.Pressed -> "Pressed. Opens the thread it went to."
        ReplyButtons.Look.Pressing -> "On its way to the Mac."
        is ReplyButtons.Look.Failed -> "Didn't work: ${look.note}. Tap to try again."
        ReplyButtons.Look.Ready -> ReplyButtons.hint(button)
    }
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(
            Modifier.clip(CircleShape)
                .background(if (pressed) PressedGreen.copy(alpha = 0.14f) else if (button.kind == "start") StartBlue else SendPurple)
                .then(if (pressed) Modifier.border(1.dp, PressedGreen.copy(alpha = 0.45f), CircleShape) else Modifier)
                .then(if (failed != null) Modifier.border(1.5.dp, LocalSignals.current.failed.copy(alpha = 0.6f), CircleShape) else Modifier)
                .clickable(enabled = enabled) { threadId?.let { extras.tap(button, it, push) } }
                .semantics { contentDescription = "${button.title}. $hint" }
                .alpha(if (look == ReplyButtons.Look.Pressing) 0.75f else 1f)
                .padding(horizontal = 12.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            when (look) {
                ReplyButtons.Look.Pressing -> {
                    CircularProgressIndicator(Modifier.size(12.dp), strokeWidth = 1.5.dp, color = Color.White)
                    Spacer(Modifier.width(5.dp))
                }
                is ReplyButtons.Look.Pressed -> {
                    Icon(Line.Check, null, Modifier.size(13.dp), tint = PressedGreen)
                    Spacer(Modifier.width(5.dp))
                }
                else -> Unit
            }
            Text(
                button.title, maxLines = 2, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium,
                color = if (pressed) PressedGreen else Color.White,
            )
        }
        if (failed != null) Text(failed.note, style = MaterialTheme.typography.labelSmall, color = LocalSignals.current.failed,
            modifier = Modifier.padding(start = 4.dp))
    }
}
