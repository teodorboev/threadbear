package com.mikerosoft.threadbear.android

import android.content.Context
import com.mikerosoft.threadbear.kit.BackendApi
import com.mikerosoft.threadbear.kit.CommandState
import com.mikerosoft.threadbear.kit.CommandStatus
import com.mikerosoft.threadbear.kit.MessageButton
import com.mikerosoft.threadbear.kit.PhoneBackend
import com.mikerosoft.threadbear.kit.PhoneThread
import com.mikerosoft.threadbear.kit.ReplyButtons
import com.mikerosoft.threadbear.kit.SlashCommand
import com.mikerosoft.threadbear.kit.SlashPalette
import java.util.WeakHashMap
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

/**
 * What the thread view keeps across threads, as the iPhone keeps it in its
 * AppModel: reply buttons pressed from here, each thread's slash commands
 * and the ones used lately. Kept beside the AppModel (one per model, see
 * [extras]) rather than in it, as AppModel.kt is shared by other work.
 */
class ThreadExtras(private val model: AppModel, context: Context) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    private val prefs = context.getSharedPreferences("thread", Context.MODE_PRIVATE)
    /** The backend these were for: a new pairing starts them again. */
    private var forBackend: PhoneBackend? = null

    private fun backend(): PhoneBackend? {
        val b = model.backend.value
        if (b !== forBackend) {
            forBackend = b
            presses.value = emptyMap()
            slashCommands.value = emptyMap()
            slashProblem.value = emptyMap()
            slashLoading.clear()
        }
        return b
    }

    // Reply buttons

    /**
     * Reply buttons pressed from this phone, by "<threadId> <buttonId>",
     * until the row says so itself: on the way (a null status) or the
     * command's state. A row outside the live tail may never catch up.
     */
    val presses = MutableStateFlow<Map<String, CommandStatus?>>(emptyMap())

    fun look(button: MessageButton, threadId: String, presses: Map<String, CommandStatus?> = this.presses.value): ReplyButtons.Look {
        val key = "$threadId ${button.id}"
        val press = presses[key]
        return ReplyButtons.look(button, press, asking = key in presses && press == null)
    }

    /**
     * A tap on a reply button: presses it once (a second tap while it's on
     * its way does nothing), or once pressed, opens the thread it started
     * or messaged if this phone has a row for it.
     */
    fun tap(button: MessageButton, threadId: String, open: (String) -> Unit) {
        when (val look = look(button, threadId)) {
            ReplyButtons.Look.Pressing -> return
            is ReplyButtons.Look.Pressed -> {
                val id = look.threadId ?: return
                if (model.knows(id)) open(id) else model.note("That thread isn't on this phone yet.")
                return
            }
            else -> Unit
        }
        val b = backend() ?: return
        val key = "$threadId ${button.id}"
        presses.update { it + (key to null) }
        scope.launch {
            try {
                val command = b.press(threadId, button.id)
                val status = b.command(command).first { it == null || it.state == CommandState.DONE || it.state == CommandState.FAILED }
                presses.update { it + (key to (status ?: CommandStatus(CommandState.DONE))) }
            } catch (e: Exception) {
                presses.update { it + (key to CommandStatus(CommandState.FAILED, e.message ?: "The runner couldn't press it.")) }
            }
        }
    }

    // Slash commands

    /** Each thread's slash commands once its runner has answered. */
    val slashCommands = MutableStateFlow<Map<String, List<SlashCommand>>>(emptyMap())
    /** Why a thread's list didn't come: its runner is away, or it failed. */
    val slashProblem = MutableStateFlow<Map<String, String>>(emptyMap())
    private val slashLoading = HashSet<String>()
    /** The commands picked from the palette lately, newest first. */
    val recentSlash = MutableStateFlow(prefs.getString("recentSlashCommands", null)?.split('\n')?.filter { it.isNotEmpty() } ?: emptyList())

    val canUseSlashCommands get() = model.apiVersion >= BackendApi.SLASH_COMMANDS

    fun used(command: SlashCommand) {
        recentSlash.value = SlashPalette.used(command.name, recentSlash.value)
        prefs.edit().putString("recentSlashCommands", recentSlash.value.joinToString("\n")).apply()
    }

    /**
     * Asks the thread's runner for its commands when the palette opens, once
     * at a time. Kept for the session: a folder's skills change rarely, and
     * the runner keeps its own copy fresh. A runner that's away leaves the
     * command waiting; after a while the palette says so.
     */
    fun loadSlashCommands(threadId: String) {
        val b = backend() ?: return
        if (!canUseSlashCommands || threadId in slashCommands.value || threadId in slashLoading) return
        slashLoading += threadId
        slashProblem.update { it - threadId }
        scope.launch {
            try {
                val command = b.slashCommands(threadId)
                val status = withTimeoutOrNull(20_000) {
                    b.command(command).first { it == null || it.state == CommandState.DONE || it.state == CommandState.FAILED }
                }
                when {
                    status?.state == CommandState.FAILED -> slashProblem.update { it + (threadId to "Couldn't get its commands: ${status.note ?: "the runner couldn't list them."}") }
                    status?.state == CommandState.DONE -> slashCommands.update { it + (threadId to (status.commands ?: emptyList())) }
                    else -> slashProblem.update { it + (threadId to "Its Mac hasn't answered yet. You can still type a command and send it.") }
                }
            } catch (e: Exception) {
                slashProblem.update { it + (threadId to "Couldn't get its commands: ${e.message}") }
            } finally {
                slashLoading -= threadId
            }
        }
    }

    companion object {
        private val all = WeakHashMap<AppModel, ThreadExtras>()

        fun of(model: AppModel, context: Context): ThreadExtras = synchronized(all) {
            all.getOrPut(model) { ThreadExtras(model, context.applicationContext) }
        }
    }
}

/** A thread this phone has a row for: the inbox, or a put-away list it has open. */
fun AppModel.knownThread(id: String): PhoneThread? =
    inbox.value?.firstOrNull { it.threadId == id }
        ?: com.mikerosoft.threadbear.kit.PhoneSection.entries.firstNotNullOfOrNull { s ->
            if (s.view == null) null else putAway(s).value?.firstOrNull { it.threadId == id }
        }

fun AppModel.knows(id: String): Boolean = knownThread(id) != null
