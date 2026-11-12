package com.mikerosoft.threadbear.android.ui

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext
import com.mikerosoft.threadbear.kit.LinkPolicy

/**
 * The thread view's one way to hand a link to Android (SECURITY.md, "Links
 * and files"), as LinkGate is the iPhone's and OpenGate the Mac's. A web
 * page opens in the browser; any other scheme starts another app with what
 * the link says, so it waits for a yes that names it; a file is the Mac's.
 * A thread's own link handling ends here for anything that isn't a link
 * into the thread (Daybreak, 7 Nov).
 */
class LinkGate(private val context: Context) {
    data class Ask(val title: String, val detail: String, val yes: String?, val answer: () -> Unit)

    /** The question on screen, if any. */
    var asking by mutableStateOf<Ask?>(null)
        private set

    fun decide(url: String) {
        when (val kind = LinkPolicy.kind(url)) {
            is LinkPolicy.Kind.Web -> systemOpen(kind.url)
            is LinkPolicy.Kind.File -> say("That file opens on the Mac.")
            is LinkPolicy.Kind.Ask -> {
                val scheme = kind.url.substringBefore(':')
                asking = Ask(
                    "Open this $scheme link?",
                    "It isn't a web page: it opens another app, which acts on what the link says. It's from a thread, so Threadbear asks first.\n\n${LinkPolicy.shown(kind.url)}",
                    "Open",
                ) { systemOpen(kind.url) }
            }
            LinkPolicy.Kind.Refuse -> Unit
        }
    }

    /** A word with only OK, for what can't be opened here. */
    fun say(title: String) {
        asking = Ask(title, "", null) {}
    }

    fun dismiss() {
        asking = null
    }

    private fun systemOpen(url: String) {
        try {
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)).addCategory(Intent.CATEGORY_BROWSABLE).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        } catch (_: ActivityNotFoundException) {
            say("Nothing on this phone opens that link.")
        }
    }
}

@Composable
fun rememberLinkGate(): LinkGate {
    val context = LocalContext.current
    return remember(context) { LinkGate(context) }
}

/** The gate's question, when it has one. */
@Composable
fun LinkGateDialog(gate: LinkGate) {
    val ask = gate.asking ?: return
    AlertDialog(
        onDismissRequest = gate::dismiss,
        title = { Text(ask.title) },
        text = if (ask.detail.isEmpty()) null else ({ Text(ask.detail) }),
        confirmButton = {
            TextButton(onClick = { gate.dismiss(); ask.answer() }) { Text(ask.yes ?: "OK") }
        },
        dismissButton = if (ask.yes == null) null else ({ TextButton(onClick = gate::dismiss) { Text("Cancel") } }),
    )
}
