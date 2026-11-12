package com.mikerosoft.threadbear.android.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.size
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.kit.BackendApi
import com.mikerosoft.threadbear.kit.Hibernate
import com.mikerosoft.threadbear.kit.PhoneThread
import com.mikerosoft.threadbear.kit.ThreadChange
import java.time.Instant

/**
 * The Hibernate presets as a dialog, for a swipe or a menu (the iPhone's
 * hibernateDialog): 1 hour, this evening, tomorrow, next Monday, or until
 * the runner restarts. The row goes as soon as one's picked (ShellModel).
 */
@Composable
fun HibernateDialog(thread: PhoneThread, dismiss: () -> Unit) {
    val model = LocalModel.current
    val presets = remember { Hibernate.presets(Instant.now()) }
    fun pick(change: ThreadChange) {
        dismiss()
        model.change(thread.threadId, change)
    }
    AlertDialog(
        onDismissRequest = dismiss,
        title = { Text("Hibernate until") },
        text = {
            Column(Modifier.fillMaxWidth()) {
                for (p in presets) {
                    TextButton(onClick = { pick(ThreadChange.Snooze(p.date)) }, modifier = Modifier.fillMaxWidth()) {
                        Text(p.title, Modifier.fillMaxWidth(), color = MaterialTheme.colorScheme.onSurface)
                    }
                }
                TextButton(onClick = { pick(ThreadChange.SnoozeUntilRestart) }, modifier = Modifier.fillMaxWidth()) {
                    Text("Until the runner restarts", Modifier.fillMaxWidth(), color = MaterialTheme.colorScheme.onSurface)
                }
            }
        },
        confirmButton = {},
        dismissButton = { TextButton(onClick = dismiss) { Text("Cancel") } },
    )
}

/**
 * A row's long-press menu, as the iPhone's contextMenu: Star, Hibernate (or
 * Wake), Archive, Mark as read while it's unread, Copy Thread ID.
 */
@Composable
fun RowMenu(thread: PhoneThread, asleep: Boolean, expanded: Boolean, dismiss: () -> Unit, hibernate: () -> Unit) {
    val model = LocalModel.current
    val context = LocalContext.current
    DropdownMenu(expanded = expanded, onDismissRequest = dismiss) {
        if (model.apiVersion >= BackendApi.STARS) {
            val starred = thread.starredAt != null
            ActionItem(if (starred) "Remove Star" else "Star", if (starred) Line.StarOff else Line.Star) {
                dismiss(); model.change(thread.threadId, ThreadChange.Star(!starred))
            }
        }
        if (asleep) {
            ActionItem("Wake", Line.Sun) { dismiss(); model.change(thread.threadId, ThreadChange.Wake) }
        } else {
            ActionItem("Hibernate…", Line.Moon) { dismiss(); hibernate() }
        }
        ActionItem("Archive", Line.Archive) { dismiss(); model.change(thread.threadId, ThreadChange.Archive) }
        if (thread.unread || thread.needsYou) {
            ActionItem("Mark as read", Line.MailOpen) { dismiss(); model.change(thread.threadId, ThreadChange.MarkSeen) }
        }
        ActionItem("Copy Thread ID", Line.Hash) {
            dismiss(); copyText(context, "Thread ID", thread.threadId)
        }
    }
}

@Composable
fun ActionItem(text: String, icon: ImageVector, onClick: () -> Unit) {
    DropdownMenuItem(text = { Text(text) }, leadingIcon = { Icon(icon, null, Modifier.size(18.dp)) }, onClick = onClick)
}

/** Text on the clipboard, as a copy from a menu. */
fun copyText(context: Context, label: String, text: String) {
    val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
    clipboard.setPrimaryClip(ClipData.newPlainText(label, text))
}
