package com.mikerosoft.threadbear.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.android.ThreadExtras
import com.mikerosoft.threadbear.kit.SlashCommand
import com.mikerosoft.threadbear.kit.SlashPalette

/**
 * The Mac's slash command palette, as the phone has room for it: over the
 * reply box while what's typed starts with "/", the categories as chips
 * across the top, and each command with its description. A tap puts the
 * command in the box, with a space after it when it takes details, ready
 * to send. The list and its matching are the Mac's (the kit's
 * SlashCommands), asked of the thread's runner through the backend
 * (phone:slashCommands, API 62).
 */
@Composable
fun SlashPaletteList(threadId: String, provider: String, query: String, onPick: (SlashCommand) -> Unit) {
    val model = LocalModel.current
    val extras = ThreadExtras.of(model, LocalContext.current)
    val lists by extras.slashCommands.collectAsStateWithLifecycle()
    val problems by extras.slashProblem.collectAsStateWithLifecycle()
    val recent by extras.recentSlash.collectAsStateWithLifecycle()
    var category by remember { mutableStateOf(SlashPalette.Category.ALL) }
    LaunchedEffect(threadId) { extras.loadSlashCommands(threadId) }
    val colors = MaterialTheme.colorScheme
    val all = lists[threadId]
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp).clip(RoundedCornerShape(16.dp)).background(colors.surfaceContainer).padding(10.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        when {
            all != null -> {
                Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    for ((item, count) in SlashPalette.counts(all, query, recent)) {
                        val on = item == category
                        Text(
                            "${item.title(provider)} $count",
                            style = MaterialTheme.typography.labelMedium,
                            modifier = Modifier.clip(CircleShape)
                                .background(if (on) LocalSignals.current.accent.copy(alpha = 0.25f) else colors.surfaceContainerHighest)
                                .clickable { category = item }.padding(horizontal = 10.dp, vertical = 5.dp),
                        )
                    }
                }
                val matches = remember(all, query, category, recent) { SlashPalette.matches(all, query, category, recent) }
                if (matches.isEmpty()) {
                    Note("No command matches /$query. Send it and it goes to the agent as words.")
                } else {
                    LazyColumn(Modifier.heightIn(max = 260.dp)) {
                        itemsIndexed(matches, key = { _, c -> c.name }) { i, command ->
                            CommandRow(command) { extras.used(command); onPick(command) }
                            if (i < matches.size - 1) HorizontalDivider(color = colors.outlineVariant)
                        }
                    }
                }
            }
            problems[threadId] != null -> Note(problems.getValue(threadId))
            else -> Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(vertical = 6.dp)) {
                CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 1.5.dp, color = colors.onSurfaceVariant)
                Spacer(Modifier.width(8.dp))
                Text("Getting this thread's commands", style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant)
            }
        }
    }
}

@Composable
private fun CommandRow(command: SlashCommand, pick: () -> Unit) {
    val colors = MaterialTheme.colorScheme
    val colorsForCode = MaterialTheme.colorScheme.surfaceContainerHighest
    val accent = LocalSignals.current.accent
    val inline = remember(accent, colorsForCode) { InlineStyle(accent, colorsForCode, onLink = null) }
    Column(Modifier.fillMaxWidth().clickable(onClickLabel = "Puts /${command.name} in the reply box", onClick = pick).padding(vertical = 8.dp)) {
        // The name to tap for, its details' hint quieter after it.
        val hint = command.usage.drop(command.name.length + 1)
        Text(
            buildAnnotatedString {
                withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append("/${command.name}") }
                withStyle(SpanStyle(color = colors.onSurfaceVariant)) { append(hint) }
            },
            style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis,
        )
        val text = command.description
        if (!text.isNullOrEmpty()) {
            // Its inline marks drawn, its links only words: a description comes
            // from whoever wrote the skill (Daybreak, 8 Nov).
            Text(inline.parse(SlashPalette.description(text)), style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant,
                maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun Note(text: String) {
    Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(vertical = 6.dp))
}
