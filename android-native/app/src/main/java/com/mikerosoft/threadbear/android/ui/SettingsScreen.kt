package com.mikerosoft.threadbear.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.zIndex
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.android.LocalSnackbar
import com.mikerosoft.threadbear.kit.NotifyMode
import com.mikerosoft.threadbear.kit.PhoneIdentity
import com.mikerosoft.threadbear.kit.PhoneRunner
import com.mikerosoft.threadbear.kit.RunnerOrder
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.util.Locale

/**
 * Settings (the iPhone's SettingsView): what this phone is paired with,
 * the runners and their order, when it's notified, and Unpair.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SettingsScreen(back: () -> Unit, runnerOrder: () -> Unit) {
    val model = LocalModel.current
    val shell = model.shell
    val pairing by model.pairing.collectAsStateWithLifecycle()
    val known by model.identity.collectAsStateWithLifecycle()
    val runners by shell.runners.collectAsStateWithLifecycle()
    val mode by shell.notifyMode.collectAsStateWithLifecycle()
    val token by shell.pushToken.collectAsStateWithLifecycle()
    val now = rememberNow(15_000)
    var confirmUnpair by remember { mutableStateOf(false) }
    // Asked afresh as it opens, as the iPhone does: the expiry may have moved.
    val identity by produceState<PhoneIdentity?>(known, model.backend.value) {
        value = runCatching { model.backend.value?.me() }.getOrNull() ?: known
    }
    val chooseRunner = shell.canChooseRunner(runners)

    Scaffold(
        snackbarHost = { SnackbarHost(LocalSnackbar.current) },
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = back) { Icon(Line.Back, "Back", Modifier.size(22.dp)) } },
                title = { Text("Settings") },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
    ) { inner ->
        Column(Modifier.padding(inner).fillMaxSize().verticalScroll(rememberScrollState()).padding(bottom = 32.dp)) {
            Group("Paired with") {
                // Breaks after its dots, not mid-word, when it needs two lines.
                Field("Server", pairing?.serverName?.replace(".", ".\u200B") ?: "None")
                identity?.let { me ->
                    Field("Phone", me.name)
                    me.expiresAt?.let { Field("Token expires", day.format(it.atZone(ZoneId.systemDefault()))) }
                }
                if (!chooseRunner) runners.forEach { Field(it.name, RunnerOrder.status(it, now)) }
            }
            if (chooseRunner) {
                Group("Runners", footer = "New threads start on the first one that's online.") {
                    runners.forEach { Field(it.name, RunnerOrder.status(it, now)) }
                    Row(
                        Modifier.fillMaxWidth().clickable(onClick = runnerOrder).heightIn(min = 48.dp).padding(horizontal = 16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("Change the order", style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                        Icon(Line.ChevronRight, null, Modifier.size(16.dp), tint = LocalSignals.current.faint)
                    }
                }
            }
            if (identity?.pushOff == true) {
                // A self-hosted backend: the push relay only reaches Convex cloud,
                // so say why nothing comes rather than offer a picker that does
                // nothing (docs/self-hosting.md).
                Group("Notifications") {
                    Row(Modifier.padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.Top) {
                        Icon(Line.BellOff, null, Modifier.size(18.dp).padding(top = 2.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        Spacer(Modifier.width(12.dp))
                        Text(
                            "Your Threadbear backend is self-hosted, so it can't send notifications. Open the app to see what needs you.",
                            style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            } else if (shell.canChooseNotifications(token)) {
                Group("Notify me") {
                    NotifyMode.entries.forEach { m ->
                        Row(
                            Modifier.fillMaxWidth().selectable(selected = m == mode, role = Role.RadioButton) { shell.setNotifyMode(m) }
                                .heightIn(min = 48.dp).padding(horizontal = 16.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(m.label, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                            RadioButton(
                                selected = m == mode, onClick = null,
                                colors = RadioButtonDefaults.colors(selectedColor = LocalSignals.current.accent),
                            )
                        }
                    }
                }
            }
            // The app's own version, and a newer release when there is one (Updates).
            val update by com.mikerosoft.threadbear.android.Updates.available.collectAsStateWithLifecycle()
            val context = androidx.compose.ui.platform.LocalContext.current
            Group("App") {
                Field("Version", com.mikerosoft.threadbear.android.Updates.installed?.let { com.mikerosoft.threadbear.android.Updates.label(it) } ?: "Development build")
                update?.let { u ->
                    Row(
                        Modifier.fillMaxWidth().clickable { com.mikerosoft.threadbear.android.Updates.open(context, u) }.heightIn(min = 48.dp).padding(horizontal = 16.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text("Update to ${com.mikerosoft.threadbear.android.Updates.label(u.date)}", style = MaterialTheme.typography.bodyLarge, color = LocalSignals.current.accent)
                    }
                }
            }
            Group(null, footer = "Forgets the token on this phone. Revoke it on the Mac too if the phone's lost.") {
                Row(
                    Modifier.fillMaxWidth().clickable { confirmUnpair = true }.heightIn(min = 48.dp).padding(horizontal = 16.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Unpair this phone", style = MaterialTheme.typography.bodyLarge, color = LocalSignals.current.failed)
                }
            }
        }
    }

    if (confirmUnpair) {
        AlertDialog(
            onDismissRequest = { confirmUnpair = false },
            title = { Text("Unpair this phone?") },
            confirmButton = {
                TextButton(onClick = { confirmUnpair = false; model.unpair() }) { Text("Unpair", color = LocalSignals.current.failed) }
            },
            dismissButton = { TextButton(onClick = { confirmUnpair = false }) { Text("Cancel") } },
        )
    }
}

private val day = DateTimeFormatter.ofPattern("d MMM yyyy", Locale.UK)

/** A Form-like group: a heading, rows on a raised card, and a note under it. */
@Composable
private fun Group(title: String?, footer: String? = null, content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        if (title != null) {
            Text(title, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(start = 16.dp, top = 22.dp, bottom = 8.dp))
        } else {
            Spacer(Modifier.height(22.dp))
        }
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(MaterialTheme.colorScheme.surfaceContainer), content = content)
        if (footer != null) {
            Text(footer, style = MaterialTheme.typography.bodySmall, color = LocalSignals.current.faint,
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 6.dp))
        }
    }
}

@Composable
private fun Field(label: String, value: String) {
    Row(Modifier.fillMaxWidth().heightIn(min = 48.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        // The label keeps its width and the value takes the rest, right-aligned
        // (as a LabeledContent): with both weighted, a Convex host pushed the
        // label off the row, and short values floated mid-row.
        Text(label, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
        Spacer(Modifier.width(16.dp))
        Text(value, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2,
            overflow = TextOverflow.Ellipsis, textAlign = androidx.compose.ui.text.style.TextAlign.End, modifier = Modifier.weight(1f).padding(vertical = 8.dp))
    }
}

/**
 * The runners in Mike's order, to drag about by their handles: new threads
 * start on the first one online (phone:setRunnerOrder). Its own screen, as
 * on the iPhone.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun RunnerOrderScreen(back: () -> Unit) {
    val model = LocalModel.current
    val shell = model.shell
    val runners by shell.runners.collectAsStateWithLifecycle()
    val latest by rememberUpdatedState(runners)
    val now = rememberNow(15_000)
    var order by remember { mutableStateOf(runners) }
    var dragging by remember { mutableStateOf<String?>(null) }
    var offset by remember { mutableFloatStateOf(0f) }
    // The backend's list, except mid-drag, when the finger's order holds.
    LaunchedEffect(runners) { if (dragging == null) order = runners }
    val rowHeight = 56.dp
    val rowPx = with(LocalDensity.current) { rowHeight.toPx() }

    fun moved(id: String) {
        var i = order.indexOfFirst { it.runnerId == id }
        if (i < 0) return
        // Past half a row, the row swaps with its neighbour and the finger's
        // offset carries on from its new place.
        while (offset > rowPx / 2 && i < order.lastIndex) {
            order = order.toMutableList().apply { add(i + 1, removeAt(i)) }; i++; offset -= rowPx
        }
        while (offset < -rowPx / 2 && i > 0) {
            order = order.toMutableList().apply { add(i - 1, removeAt(i)) }; i--; offset += rowPx
        }
    }

    fun dropped() {
        dragging = null
        offset = 0f
        if (order.map { it.runnerId } != latest.map { it.runnerId }) shell.setRunnerOrder(order)
    }

    Scaffold(
        snackbarHost = { SnackbarHost(LocalSnackbar.current) },
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = back) { Icon(Line.Back, "Back", Modifier.size(22.dp)) } },
                title = { Text("Runner order") },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
    ) { inner ->
        Column(Modifier.padding(inner).fillMaxSize().verticalScroll(rememberScrollState())) {
            Group(null, footer = "New threads start on the first one that's online. Drag to change the order.") {
                order.forEachIndexed { index, runner ->
                    key(runner.runnerId) {
                        val lifted = dragging == runner.runnerId
                        if (index > 0) HorizontalDivider(Modifier.padding(start = 16.dp), color = MaterialTheme.colorScheme.outlineVariant)
                        RunnerRow(
                            runner, runner.isOnline(now),
                            Modifier.height(rowHeight).zIndex(if (lifted) 1f else 0f)
                                .graphicsLayer {
                                    translationY = if (lifted) offset else 0f
                                    shadowElevation = if (lifted) 12f else 0f
                                }
                                .background(if (lifted) MaterialTheme.colorScheme.surfaceContainerHighest else MaterialTheme.colorScheme.surfaceContainer),
                            handle = Modifier.pointerInput(runner.runnerId) {
                                detectVerticalDragGestures(
                                    onDragStart = { dragging = runner.runnerId; offset = 0f },
                                    onVerticalDrag = { change, dy -> change.consume(); offset += dy; moved(runner.runnerId) },
                                    onDragEnd = { dropped() },
                                    onDragCancel = { dropped() },
                                )
                            },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun RunnerRow(runner: PhoneRunner, online: Boolean, modifier: Modifier, handle: Modifier) {
    Row(modifier.fillMaxWidth().padding(start = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.Start) {
        Text(runner.name, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(if (online) "Online" else "Offline", style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Icon(Line.Grip, "Drag to reorder", handle.size(48.dp).padding(14.dp), tint = LocalSignals.current.faint)
    }
}
