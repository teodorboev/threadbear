package com.mikerosoft.threadbear.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.android.LocalSnackbar
import com.mikerosoft.threadbear.android.NewThreadDraft
import com.mikerosoft.threadbear.android.PhotoDecoding
import com.mikerosoft.threadbear.android.sent
import com.mikerosoft.threadbear.android.startUpload
import com.mikerosoft.threadbear.android.upload
import com.mikerosoft.threadbear.kit.BackendApi
import com.mikerosoft.threadbear.kit.NewThreadChoices
import com.mikerosoft.threadbear.kit.NewThreadSettings
import java.time.Instant
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch

/**
 * A new thread from the phone (the iPhone's NewThreadView). It starts in
 * the runner's default folder (Mike didn't want a folder picker on the
 * phone, 31 Oct); the runner, model and effort can be picked, and the
 * runner picks a login with room for them. The runner starts on the first
 * one online in Mike's order, so the Mac asleep doesn't leave the phone
 * nowhere to start one (6 Nov).
 *
 * It closes as soon as the backend has the request (7 Nov, Mike: waiting
 * on the runner's answer left the spinner up for ages); a row in the inbox
 * stands in for the thread until the runner's made it (ShellModel.starting),
 * and MainActivity opens it if it starts within a minute and the inbox is
 * still showing (shell.justStarted), as the iPhone does, so a thread that
 * lands while Mike's in another, or writing the next, doesn't pull him away.
 *
 * It opens with a failed start's words, photos and picks to try again
 * (shell.retryDraft), or with words to start from (shell.composeText: the
 * Pocket's "Add to my pocket: ").
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun NewThreadScreen(close: () -> Unit) {
    // The keyboard goes down with the screen: popping the route left it up
    // over the inbox, where the Starting row was hidden under it.
    val keyboard = androidx.compose.ui.platform.LocalSoftwareKeyboardController.current
    val closeScreen = close
    @Suppress("NAME_SHADOWING") val close = { keyboard?.hide(); closeScreen() }
    val model = LocalModel.current
    val context = LocalContext.current
    val colors = MaterialTheme.colorScheme
    // A failed start's screen again, as it was.
    val retry = remember { model.shell.retryDraft.value.also { model.shell.retryDraft.value = null } }
    val startText = remember { model.shell.composeText.value.also { model.shell.composeText.value = null } }
    val box = rememberPhotoBox()
    val dictation = rememberDictation()
    val scope = rememberCoroutineScope()
    val runners by model.runners.collectAsStateWithLifecycle()
    val accounts by model.accounts.collectAsStateWithLifecycle()
    val identity by model.identity.collectAsStateWithLifecycle()
    val api = identity?.apiVersion ?: 0
    val now = rememberNow(5000)
    val photosOn = api >= BackendApi.PHOTOS
    /** Whether there's a runner to choose (backend API 56, which lists them in Mike's order and keeps one he sets). */
    val canChooseRunner = runners.size > 1 && api >= BackendApi.RUNNER_ORDER

    /** Null until the runners are in, then the first online in Mike's order. */
    var runnerId by rememberSaveable { mutableStateOf(retry?.runnerId) }
    var modelId by rememberSaveable { mutableStateOf(retry?.modelId) }
    var effort by rememberSaveable { mutableStateOf(retry?.effort) }
    /**
     * From the tap on Start until the command's queued. Without it Start
     * looks dead (and can be tapped again, for a second thread) while the
     * request goes.
     */
    var starting by remember { mutableStateOf<Job?>(null) }
    val focus = remember { FocusRequester() }

    // The models a new thread on this runner can start on; empty hides the
    // pickers, as an older backend would turn an effort down.
    val models = if (api >= BackendApi.MODELS) NewThreadChoices.models(accounts, if (canChooseRunner) runnerId else null) else emptyList()
    val chosenModel = models.firstOrNull { it.id == modelId }
    val chosenRunner = runners.firstOrNull { it.runnerId == runnerId }
    // With a runner picked, whether that one's asleep; otherwise whether they all are.
    val offline = if (canChooseRunner && chosenRunner != null) NewThreadChoices.offlineText(chosenRunner, now)
        else NewThreadChoices.offlineText(runners, now)

    LaunchedEffect(Unit) {
        if (retry != null) {
            box.restore(retry.text, retry.photos)
            retry.photos.forEach(model::startUpload)
        } else if (startText != null && box.text.text.isEmpty()) {
            box.restore(startText, emptyList())
        }
        // Straight to typing, once the field's been laid out (the Scaffold's
        // content comes a frame after this starts).
        withFrameNanos { }
        runCatching { focus.requestFocus() }
    }
    // The first runner online in Mike's order, once the runners are in.
    // Kept once picked, unless it's gone from the list. Down to one runner
    // (one revoked meanwhile, say): back to the backend's choice.
    LaunchedEffect(runners, canChooseRunner) {
        if (!canChooseRunner) runnerId = null
        else if (runnerId == null || runners.none { it.runnerId == runnerId }) runnerId = NewThreadChoices.runner(runners, Instant.now())?.runnerId
    }
    // Changes only, not the first value: a retried draft keeps its picks.
    // Read by the effects below as they are now, not as they were when the effects began.
    val latestAccounts by rememberUpdatedState(accounts)
    val latestApi by rememberUpdatedState(api)
    val latestChoosing by rememberUpdatedState(canChooseRunner)
    fun modelsOn(runner: String?) =
        if (latestApi >= BackendApi.MODELS) NewThreadChoices.models(latestAccounts, if (latestChoosing) runner else null) else emptyList()
    LaunchedEffect(Unit) {
        snapshotFlow { modelId }.drop(1).collect { id ->
            // Codex's Ultra on a Claude model, say: back to the model's own.
            if (!NewThreadChoices.keeps(effort, modelsOn(runnerId).firstOrNull { it.id == id })) effort = null
        }
    }
    LaunchedEffect(Unit) {
        snapshotFlow { runnerId }.drop(1).collect { runner ->
            val offered = modelsOn(runner)
            // A model this runner has no login for: back to its default.
            if (modelId != null && offered.none { it.id == modelId }) modelId = null
            // No pickers on it (its logins aren't in yet): its own effort too.
            if (offered.isEmpty()) effort = null
        }
    }

    fun start() {
        val backend = model.backend.value ?: return
        if (starting != null) return
        dictation.stop()
        val message = box.message
        val typed = box.text.text.toString()
        val photos = box.photos.toList()
        starting = scope.launch {
            try {
                val images = model.upload(photos)
                // Picked now if the screen never got to (the runners or the
                // API version came in as Start was tapped).
                if (canChooseRunner && runnerId == null) runnerId = NewThreadChoices.runner(runners, Instant.now())?.runnerId
                // Taken once: the pickers can still change while the request goes.
                val draft = NewThreadDraft(typed, photos, if (canChooseRunner) runnerId else null, modelId, effort)
                val commandId = backend.create(message, draft.runnerId, null, images, NewThreadSettings(draft.modelId, draft.effort))
                sent(photos)
                model.shell.started(commandId, message, draft.runnerId, photos.size, draft)
                close()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                model.report(e)
            } finally {
                starting = null
            }
        }
    }

    // Not with a photo still on its way, which would be left behind, nor with the mic's last words to come.
    val canStart = (box.message.isNotEmpty() || box.photos.isNotEmpty()) && box.photosUp && !dictation.holdsWords
    val busy = starting != null
    val mic = rememberMicStart(dictation) { dictation.start(box.text) }

    Scaffold(
        snackbarHost = { SnackbarHost(LocalSnackbar.current) },
        topBar = {
            TopAppBar(
                navigationIcon = {
                    IconButton(onClick = { starting?.cancel(); close() }) { Icon(Line.Close, "Cancel", Modifier.size(22.dp)) }
                },
                title = { Text("New thread") },
                actions = {
                    if (busy) {
                        CircularProgressIndicator(Modifier.padding(end = 20.dp).size(20.dp), strokeWidth = 2.dp, color = colors.onSurfaceVariant)
                    } else {
                        TextButton(onClick = ::start, enabled = canStart) { Text("Start", style = MaterialTheme.typography.titleMedium) }
                    }
                },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = colors.background),
            )
        },
    ) { inner ->
        Column(
            Modifier.fillMaxSize().padding(inner).imePadding().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 8.dp),
        ) {
            Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(colors.surfaceContainer).padding(start = 16.dp, end = 8.dp, top = 14.dp, bottom = 6.dp)) {
                // The words are taken as Start is tapped: anything typed or
                // added while it goes would be lost (Mike, 7 Nov). The pickers
                // below stay live, as start() reads them once the photos are up.
                PhotoField(
                    box, "What should it do?", TextFieldLineLimits.MultiLine(4, 12),
                    onPasteImages = if (photosOn) ({ uris -> box.add(uris.map { PhotoDecoding.fromUri(context, it, imagesOnly = true) }, model, scope) }) else null,
                    modifier = Modifier.padding(end = 8.dp),
                    readOnly = busy,
                    fieldModifier = Modifier.focusRequester(focus),
                )
                Spacer(Modifier.height(8.dp))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (photosOn) {
                        PhotoMenu(box) { open -> LabelButton("Add a photo", ComposeLine.Photo, enabled = !busy && box.room > 0, onClick = open) }
                    }
                    Spacer(Modifier.weight(1f))
                    if (dictation.isHearing) ListeningBar(dictation, Modifier.width(200.dp))
                    else LabelButton("Dictate", ComposeLine.Mic, enabled = !busy, onClick = mic)
                }
                dictation.problem?.let { DictationNote(it, Modifier.padding(bottom = 8.dp)) }
                if (photosOn && (box.photos.isNotEmpty() || box.adding > 0)) PhotoStrip(box, Modifier.padding(top = 4.dp, bottom = 10.dp), enabled = !busy)
            }
            if (canChooseRunner || models.isNotEmpty()) {
                Spacer(Modifier.height(16.dp))
                Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).background(colors.surfaceContainer)) {
                    if (canChooseRunner) {
                        // "Automatic" only with no runner placed and no main Mac known: the backend picks.
                        val options = (if (runnerId == null) listOf<Pair<String?, String>>(null to "Automatic") else emptyList()) +
                            runners.map { r -> r.runnerId to (if (r.isOnline(now)) r.name else "${r.name}, offline") }
                        PickerRow("Runner", options, runnerId) { runnerId = it }
                    }
                    if (models.isNotEmpty()) {
                        if (canChooseRunner) HorizontalDivider(Modifier.padding(start = 16.dp), color = colors.outlineVariant)
                        PickerRow("Model", listOf<Pair<String?, String>>(null to "Default") + models.map { it.id to it.title }, modelId) { modelId = it }
                        HorizontalDivider(Modifier.padding(start = 16.dp), color = colors.outlineVariant)
                        val efforts = NewThreadChoices.efforts(chosenModel)
                        PickerRow("Effort", listOf<Pair<String?, String>>(null to "Default") + efforts.map { it to NewThreadChoices.effortTitle(it) }, effort) { effort = it }
                    }
                }
            }
            offline?.let {
                Row(Modifier.padding(start = 4.dp, top = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Icon(Line.Moon, null, Modifier.size(16.dp), tint = colors.onSurfaceVariant)
                    Spacer(Modifier.width(8.dp))
                    Text("$it. It'll start when it's back.", style = MaterialTheme.typography.bodySmall, color = colors.onSurfaceVariant)
                }
            }
            Spacer(Modifier.height(24.dp))
        }
    }
}

/** A quiet grey button with its icon, as the iPhone's Add a photo and Dictate. */
@Composable
private fun LabelButton(text: String, icon: ImageVector, enabled: Boolean, onClick: () -> Unit) {
    TextButton(onClick = onClick, enabled = enabled) {
        Icon(icon, null, Modifier.size(18.dp), tint = if (enabled) MaterialTheme.colorScheme.onSurfaceVariant else LocalSignals.current.faint)
        Spacer(Modifier.width(6.dp))
        Text(text, color = if (enabled) MaterialTheme.colorScheme.onSurfaceVariant else LocalSignals.current.faint)
    }
}

/** One picker: its name, what's chosen, and a menu of the rest. */
@Composable
private fun PickerRow(label: String, options: List<Pair<String?, String>>, selected: String?, pick: (String?) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val title = options.firstOrNull { it.first == selected }?.second ?: options.firstOrNull()?.second ?: ""
    Box {
        Row(
            Modifier.fillMaxWidth().clickable { open = true }.padding(horizontal = 16.dp, vertical = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(label, style = MaterialTheme.typography.bodyLarge)
            Spacer(Modifier.weight(1f))
            Text(title, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
            Spacer(Modifier.width(6.dp))
            Icon(Line.ChevronDown, null, Modifier.size(14.dp), tint = LocalSignals.current.faint)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            for ((value, name) in options) {
                DropdownMenuItem(
                    text = { Text(name) },
                    trailingIcon = { if (value == selected) Icon(Line.Check, null, Modifier.size(16.dp), tint = LocalSignals.current.accent) },
                    onClick = { open = false; pick(value) },
                )
            }
        }
    }
}
