package com.mikerosoft.threadbear.android

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import com.mikerosoft.threadbear.android.ui.InboxScreen
import com.mikerosoft.threadbear.android.ui.NewThreadScreen
import com.mikerosoft.threadbear.android.ui.PocketScreen
import com.mikerosoft.threadbear.android.ui.RunnerOrderScreen
import com.mikerosoft.threadbear.android.ui.SettingsScreen
import com.mikerosoft.threadbear.android.ui.LocalPageForward
import com.mikerosoft.threadbear.android.ui.rememberPageForward
import com.mikerosoft.threadbear.android.ui.PairScreen
import com.mikerosoft.threadbear.android.ui.ThreadScreen
import com.mikerosoft.threadbear.android.ui.ThreadbearTheme
import kotlinx.coroutines.flow.MutableStateFlow

val LocalModel = staticCompositionLocalOf<AppModel> { error("No model") }
val LocalSnackbar = staticCompositionLocalOf { SnackbarHostState() }

class MainActivity : ComponentActivity() {
    private val model get() = (application as ThreadbearApplication).model
    /** A thread to open, from a notification or a debug launch. */
    private val openThread = MutableStateFlow<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) handle(intent)
        setContent {
            ThreadbearTheme {
                val snackbar = remember { SnackbarHostState() }
                CompositionLocalProvider(LocalModel provides model, LocalSnackbar provides snackbar) {
                    Root(model, openThread)
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        // Notifications allowed in Settings since the prompt was turned down:
        // register now rather than on the next launch.
        model.registerPush()
        // A newer release on GitHub (at most every six hours, release builds only).
        Updates.check(lifecycleScope)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    private fun handle(intent: Intent?) {
        val data = intent?.dataString
        if (data != null && data.startsWith("threadbear://pair")) {
            if (!model.ask(data)) model.note("That isn't a pairing link Threadbear can use.")
        }
        Push.validThreadId(intent?.getStringExtra("threadId"))?.let { openThread.value = it }
    }
}

@Composable
private fun Root(model: AppModel, openThread: MutableStateFlow<String?>) {
    val pairing by model.pairing.collectAsStateWithLifecycle()
    val pending by model.pendingPair.collectAsStateWithLifecycle()
    val snackbar = LocalSnackbar.current
    LaunchedEffect(Unit) { model.notes.collect { snackbar.showSnackbar(it) } }

    if (pairing == null) {
        PairScreen()
    } else {
        AskForNotifications(model)
        val nav = rememberNavController()
        val open by openThread.collectAsStateWithLifecycle()
        LaunchedEffect(open) {
            open?.let { nav.openThread(it) { launchSingleTop = true }; openThread.value = null }
        }
        // A thread that started soon after it was asked for opens, as the iPhone's
        // sheet used to wait to do, unless you've gone into another meanwhile.
        val started by model.shell.justStarted.collectAsStateWithLifecycle()
        LaunchedEffect(started) {
            val id = started ?: return@LaunchedEffect
            model.shell.justStarted.value = null
            if (nav.currentDestination?.route == "inbox") nav.openThread(id)
        }
        // Forward to the thread last backed out of, for a swipe left on a thread.
        CompositionLocalProvider(LocalPageForward provides rememberPageForward(nav)) {
        NavHost(nav, startDestination = "inbox") {
            composable("inbox") {
                InboxScreen(
                    open = { nav.openThread(it) },
                    newThread = { nav.navigate("new") { launchSingleTop = true } },
                    settings = { nav.navigate("settings") { launchSingleTop = true } },
                    pocket = { nav.navigate("pocket") { launchSingleTop = true } },
                )
            }
            composable("new") {
                // The thread opens by itself through shell.justStarted (above), once the runner's made it.
                NewThreadScreen(close = { nav.popBackStack() })
            }
            composable("settings") { SettingsScreen(back = { nav.popBackStack() }, runnerOrder = { nav.navigate("settings/runners") }) }
            composable("settings/runners") { RunnerOrderScreen(back = { nav.popBackStack() }) }
            composable("pocket") {
                // Only once the Pocket's gone, as the iPhone's cover: the thread
                // or New thread goes on the inbox, not over the Pocket.
                PocketScreen(
                    close = { nav.popBackStack() },
                    openThread = { id -> nav.openThread(id) { popUpTo("inbox") } },
                    add = {
                        model.shell.composeText.value = "Add to my pocket: "
                        nav.navigate("new") { popUpTo("inbox") }
                    },
                )
            }
            composable("thread/{id}") { entry ->
                val context = androidx.compose.ui.platform.LocalContext.current
                LaunchedEffect(entry.arguments?.getString("id")) { entry.arguments?.getString("id")?.let { Push.clear(context, it) } }
                ThreadScreen(threadId = entry.arguments?.getString("id") ?: "", back = { nav.popBackStack() },
                    open = { nav.openThread(it) })
            }
        }
        }
    }

    pending?.let { request ->
        AlertDialog(
            onDismissRequest = { model.cancelPair() },
            title = { Text(breakable(request.title, request.host), style = MaterialTheme.typography.titleLarge) },
            text = { Text(breakable(request.message, request.host)) },
            confirmButton = { TextButton(onClick = { model.acceptPair(request) }) { Text("Pair") } },
            dismissButton = { TextButton(onClick = { model.cancelPair() }) { Text("Cancel") } },
        )
    }
}

/**
 * A Convex host is longer than a dialog line, and Android broke it
 * mid-word with the "?" alone on the last line: let it break only after
 * its dots (a zero-width space), and keep what follows it joined on.
 */
private fun breakable(text: String, host: String): String =
    text.replace(host, host.replace(".", ".\u200B") + "\u2060")

/**
 * Asks for notifications once paired, as the iPhone does (and straight
 * after pairing, not only on a later launch), but not for a backend that
 * sends none, and only once: Android doesn't show the prompt again after
 * a no anyway, and Mike finds notifications annoying, so there's no nagging.
 */
@Composable
private fun AskForNotifications(model: AppModel) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val identity by model.identity.collectAsStateWithLifecycle()
    val ask = androidx.activity.compose.rememberLauncherForActivityResult(
        androidx.activity.result.contract.ActivityResultContracts.RequestPermission(),
    ) { granted -> if (granted) model.registerPush() }
    LaunchedEffect(identity) {
        val id = identity ?: return@LaunchedEffect
        if (!Push.available || !id.sendsNotifications || Push.permitted(context)) return@LaunchedEffect
        val prefs = context.getSharedPreferences("app", android.content.Context.MODE_PRIVATE)
        if (prefs.getBoolean("askedNotifications", false)) return@LaunchedEffect
        prefs.edit().putBoolean("askedNotifications", true).apply()
        if (android.os.Build.VERSION.SDK_INT >= 33) ask.launch(android.Manifest.permission.POST_NOTIFICATIONS)
    }
}

/**
 * Every way into a thread goes through here: an id that isn't a thread id
 * (a "/" from odd backend data, a Pocket card, a link) would make a route
 * that matches nothing, and navigate() throws on that.
 */
private fun androidx.navigation.NavController.openThread(id: String, options: androidx.navigation.NavOptionsBuilder.() -> Unit = {}) {
    val valid = Push.validThreadId(id) ?: return
    navigate("thread/$valid", options)
}
