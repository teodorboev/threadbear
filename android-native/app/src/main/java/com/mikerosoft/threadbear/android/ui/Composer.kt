package com.mikerosoft.threadbear.android.ui

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.content.MediaType
import androidx.compose.foundation.content.ReceiveContentListener
import androidx.compose.foundation.content.TransferableContent
import androidx.compose.foundation.content.consume
import androidx.compose.foundation.content.contentReceiver
import androidx.compose.foundation.content.hasMediaType
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.core.content.ContextCompat
import androidx.core.content.FileProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mikerosoft.threadbear.android.AppModel
import com.mikerosoft.threadbear.android.Dictation
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.android.PhotoBox
import com.mikerosoft.threadbear.android.PhotoDecoding
import com.mikerosoft.threadbear.android.PickedPhoto
import com.mikerosoft.threadbear.android.knownThread
import com.mikerosoft.threadbear.android.retryUpload
import com.mikerosoft.threadbear.kit.CardMentions
import com.mikerosoft.threadbear.kit.BackendApi
import com.mikerosoft.threadbear.kit.PhotoUpload
import java.io.File
import kotlinx.coroutines.delay

/** A composer's words and photos, kept with the screen; anything not sent is taken back when it goes. */
@Composable
fun rememberPhotoBox(): PhotoBox {
    val model = LocalModel.current
    val text = rememberTextFieldState()
    val box = remember(text) { PhotoBox(text).apply { pasteRewrite = { model.threadReference(it) } } }
    DisposableEffect(box) { onDispose { box.dropAll() } }
    return box
}

/**
 * A pasted thread id goes in as a `[title](thread:<id>)` link the agent
 * can read the id from and the bubble draws as a chip, for a thread this
 * phone has a row for, as the iPhone's and the Mac's composers do.
 */
fun AppModel.threadReference(pasted: String): String? {
    val id = CardMentions.pastedThreadId(pasted) { knownThread(it) != null } ?: return null
    return CardMentions.threadReference(id, knownThread(id)?.title.orEmpty())
}

/** The mic for one box, stopped when the box goes away. */
@Composable
fun rememberDictation(): Dictation {
    val context = LocalContext.current.applicationContext
    val dictation = remember { Dictation(context) }
    DisposableEffect(dictation) { onDispose { dictation.close() } }
    return dictation
}

/** Whether this backend takes photos, as Compose state (the API version can come after the screen's up). */
@Composable
fun canSendPhotos(model: AppModel): Boolean {
    val identity by model.identity.collectAsStateWithLifecycle()
    return (identity?.apiVersion ?: 0) >= BackendApi.PHOTOS
}

/**
 * The reply box (the iPhone's Composer). Type or dictate, add or paste
 * photos, then send. Send turns into Stop while a turn runs with nothing
 * typed; Stop spins and takes no more taps until the turn has ended. Send
 * waits while a photo's still going up (its tile shows how far) or the mic
 * still has words to give.
 */
@Composable
fun Composer(
    box: PhotoBox,
    placeholder: String,
    running: Boolean,
    canStop: Boolean,
    stopping: Boolean,
    onSend: (message: String, photos: List<PickedPhoto>) -> Unit,
    onStop: () -> Unit,
) {
    val model = LocalModel.current
    val colors = MaterialTheme.colorScheme
    val signals = LocalSignals.current
    val dictation = rememberDictation()
    val photosOn = canSendPhotos(model)
    val hearing = dictation.isHearing
    val message = box.message
    val mic = rememberMicStart(dictation) { dictation.start(box.text) }
    Surface(color = colors.background) {
        Column(Modifier.fillMaxWidth().navigationBarsPadding().imePadding().padding(start = 8.dp, end = 8.dp, top = 6.dp, bottom = 8.dp)) {
            if (box.photos.isNotEmpty() || box.adding > 0) PhotoStrip(box, Modifier.padding(start = 4.dp, bottom = 8.dp))
            dictation.problem?.let { DictationNote(it, Modifier.padding(start = 8.dp, bottom = 6.dp)) }
            // What's typed (or dictated as it goes) stays in sight above the bar.
            if (hearing && box.text.text.isNotEmpty()) {
                FieldBox(box, placeholder, photosOn, Modifier.fillMaxWidth().padding(bottom = 6.dp))
            }
            Row(verticalAlignment = Alignment.Bottom) {
                if (hearing) {
                    // While it's hearing, the whole box is the waveform and its
                    // stop, as squeezed beside the text it was too cramped to read
                    // (Mike, 7 Nov, on the iPhone).
                    ListeningBar(dictation, Modifier.weight(1f))
                } else {
                    if (photosOn) {
                        PhotoMenu(box) { open ->
                            IconButton(onClick = open, enabled = box.room > 0, modifier = Modifier.size(44.dp)) {
                                Icon(ComposeLine.PlusCircle, "Add a photo", Modifier.size(24.dp), tint = colors.onSurfaceVariant)
                            }
                        }
                    } else {
                        Spacer(Modifier.width(4.dp))
                    }
                    FieldBox(box, placeholder, photosOn, Modifier.weight(1f))
                    // Grey like Add a photo, as only Send has the accent.
                    IconButton(onClick = mic, modifier = Modifier.size(44.dp)) {
                        Icon(ComposeLine.Mic, "Dictate", Modifier.size(22.dp), tint = colors.onSurfaceVariant)
                    }
                }
                Spacer(Modifier.width(4.dp))
                val hasSomething = message.isNotEmpty() || box.photos.isNotEmpty()
                // Not with a photo still on its way, which would be left behind,
                // nor with the mic's last words still to come.
                val canSend = hasSomething && box.photosUp && !dictation.holdsWords
                val showStop = running && canStop && !hasSomething && box.adding == 0 && !hearing
                Box(Modifier.size(44.dp), contentAlignment = Alignment.Center) {
                    when {
                        showStop && stopping -> CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp, color = colors.onSurfaceVariant)
                        showStop -> IconButton(onClick = onStop, modifier = Modifier.size(40.dp).clip(CircleShape).border(1.dp, colors.outline, CircleShape)) {
                            Icon(Line.Stop, "Stop", Modifier.size(16.dp), tint = colors.onSurface)
                        }
                        canSend -> IconButton(
                            onClick = {
                                // Still listening, it would type into the emptied box.
                                dictation.stop()
                                onSend(message, box.photos.toList())
                            },
                            modifier = Modifier.size(40.dp).clip(CircleShape).background(signals.accent),
                        ) {
                            Icon(Line.ArrowUp, "Send", Modifier.size(20.dp), tint = Color.White)
                        }
                        else -> Icon(Line.ArrowUp, null, Modifier.size(20.dp), tint = signals.faint)
                    }
                }
            }
        }
    }
}

/** The reply box's text field in its rounded grey box. */
@Composable
private fun FieldBox(box: PhotoBox, placeholder: String, photosOn: Boolean, modifier: Modifier) {
    val model = LocalModel.current
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    PhotoField(
        box, placeholder, TextFieldLineLimits.MultiLine(1, 6),
        onPasteImages = if (photosOn) ({ uris -> box.add(uris.map { PhotoDecoding.fromUri(context, it, imagesOnly = true) }, model, scope) }) else null,
        modifier = modifier.clip(RoundedCornerShape(22.dp)).background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .heightIn(min = 44.dp).padding(horizontal = 16.dp, vertical = 11.dp),
    )
}

/**
 * A text box that also takes pasted photos and shows each photo as a
 * "Picture N" chip where it was put (the iPhone's PastableTextField). A
 * chip deleted takes its photo with it. Pasting from the clipboard or the
 * keyboard's image picker comes through the content receiver; Compose's
 * state-based text field is the one that has it.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun PhotoField(
    box: PhotoBox,
    placeholder: String,
    lines: TextFieldLineLimits,
    onPasteImages: ((List<Uri>) -> Unit)?,
    modifier: Modifier = Modifier,
    readOnly: Boolean = false,
    fieldModifier: Modifier = Modifier,
) {
    val colors = MaterialTheme.colorScheme
    val signals = LocalSignals.current
    val context = LocalContext.current
    val chipStyle = SpanStyle(background = colors.surfaceContainerHighest, color = colors.onSurfaceVariant, fontSize = 0.87.em)
    val chips = remember(box, chipStyle) { box.chips(chipStyle) }
    val paste = remember(onPasteImages) { onPasteImages?.let { receiver(it) } }
    Box(modifier, contentAlignment = Alignment.TopStart) {
        if (box.text.text.isEmpty()) {
            Text(placeholder, style = MaterialTheme.typography.bodyLarge, color = colors.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        BasicTextField(
            state = box.text,
            modifier = fieldModifier.fillMaxWidth().then(if (paste != null) Modifier.contentReceiver(paste) else Modifier),
            readOnly = readOnly,
            inputTransformation = box.keepMarks,
            outputTransformation = chips,
            textStyle = MaterialTheme.typography.bodyLarge.copy(color = if (readOnly) colors.onSurface.copy(alpha = 0.5f) else colors.onSurface),
            cursorBrush = SolidColor(signals.accent),
            lineLimits = lines,
            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
        )
    }
}

/**
 * Takes the pictures out of what's pasted and leaves the rest (text) for
 * the field. Decided from the clip's own description, with no call to the
 * app that owns the uris: that would be on the main thread, and a stalled
 * provider would freeze the box (Codex's review). Each is checked as a
 * picture when it's read, off the main thread (PhotoDecoding.fromUri).
 */
@OptIn(ExperimentalFoundationApi::class)
private fun receiver(onImages: (List<Uri>) -> Unit) = object : ReceiveContentListener {
    override fun onReceive(transferableContent: TransferableContent): TransferableContent? {
        if (!transferableContent.hasMediaType(MediaType.Image)) return transferableContent
        val uris = mutableListOf<Uri>()
        // An item with words goes in as words; one that's only a uri, in a clip that has pictures, is a picture.
        val rest = transferableContent.consume { item ->
            val uri = item.uri
            if (uri == null || item.text != null) return@consume false
            uris += uri
            true
        }
        if (uris.isNotEmpty()) onImages(uris)
        return rest
    }
}

/**
 * The menu that adds a photo: take one or choose some. Android's photo
 * picker runs outside the app, so it needs no permission; the camera is
 * the system's own, given a file to write.
 */
@Composable
fun PhotoMenu(box: PhotoBox, button: @Composable (open: () -> Unit) -> Unit) {
    val model = LocalModel.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var open by remember { mutableStateOf(false) }
    val room = box.room
    fun add(uris: List<Uri>) = box.add(uris.map { PhotoDecoding.fromUri(context, it) }, model, scope)
    val one = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri -> uri?.let { add(listOf(it)) } }
    // As many as there's room for; the picker's own count needs at least two.
    val manyContract = remember(room) { ActivityResultContracts.PickMultipleVisualMedia(maxOf(2, room)) }
    val many = rememberLauncherForActivityResult(manyContract) { add(it) }
    // A path, not a File, so it outlives the activity being recreated while the camera's up.
    var shot by rememberSaveable { mutableStateOf<String?>(null) }
    val camera = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { ok ->
        val file = shot?.let(::File)
        shot = null
        if (ok && file != null) box.add(listOf(PhotoDecoding.fromFile(file)), model, scope) else file?.delete()
    }
    val hasCamera = remember { context.packageManager.hasSystemFeature(PackageManager.FEATURE_CAMERA_ANY) }
    Box {
        button { if (box.room > 0) open = true }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            if (hasCamera) {
                PhotoMenuItem("Take photo", ComposeLine.Camera) {
                    open = false
                    val file = File(context.cacheDir, "camera/shot-${System.currentTimeMillis()}.jpg").apply { parentFile?.mkdirs() }
                    try {
                        shot = file.path
                        camera.launch(FileProvider.getUriForFile(context, "${context.packageName}.photos", file))
                    } catch (_: Exception) {
                        shot = null
                        file.delete()
                        model.note("Couldn't open the camera.")
                    }
                }
            }
            PhotoMenuItem("Choose photo", ComposeLine.Photo) {
                open = false
                val request = PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)
                if (box.room > 1) many.launch(request) else one.launch(request)
            }
        }
    }
}

@Composable
private fun PhotoMenuItem(text: String, icon: ImageVector, onClick: () -> Unit) {
    DropdownMenuItem(text = { Text(text) }, leadingIcon = { Icon(icon, null, Modifier.size(18.dp)) }, onClick = onClick)
}

/** The photos waiting to go, each with a button to take it out, and how far its upload's got. */
@Composable
fun PhotoStrip(box: PhotoBox, modifier: Modifier = Modifier, enabled: Boolean = true) {
    Row(modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        for (photo in box.photos) key(photo.id) { PhotoTile(photo, enabled) { box.remove(photo) } }
        repeat(box.adding) {
            Box(
                Modifier.size(64.dp).clip(RoundedCornerShape(10.dp)).background(MaterialTheme.colorScheme.surfaceContainerHighest),
                contentAlignment = Alignment.Center,
            ) { CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = MaterialTheme.colorScheme.onSurfaceVariant) }
        }
    }
}

@Composable
private fun PhotoTile(photo: PickedPhoto, enabled: Boolean, remove: () -> Unit) {
    val model = LocalModel.current
    val shape = RoundedCornerShape(10.dp)
    Box(Modifier.size(64.dp).clip(shape)) {
        Image(photo.thumbnail, "Photo to send", Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
        when (photo.state) {
            PhotoUpload.State.GOING -> Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.35f)), contentAlignment = Alignment.Center) {
                val progress = photo.progress
                if (progress <= 0f) CircularProgressIndicator(Modifier.size(22.dp), strokeWidth = 2.dp, color = Color.White)
                else CircularProgressIndicator(progress = { progress }, modifier = Modifier.size(22.dp), strokeWidth = 2.dp, color = Color.White, trackColor = Color.White.copy(alpha = 0.25f))
            }
            // Its upload failed: tap to try it again.
            PhotoUpload.State.FAILED -> Box(
                Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.5f)).clickable(enabled = enabled) { model.retryUpload(photo) },
                contentAlignment = Alignment.Center,
            ) { Icon(ComposeLine.Retry, "Upload failed, try again", Modifier.size(22.dp), tint = Color.White) }
            PhotoUpload.State.UP -> {}
        }
        if (enabled) {
            Box(
                Modifier.align(Alignment.TopEnd).padding(3.dp).size(20.dp).clip(CircleShape).background(Color.Black.copy(alpha = 0.6f)).clickable(onClick = remove),
                contentAlignment = Alignment.Center,
            ) { Icon(Line.Close, "Remove photo", Modifier.size(12.dp), tint = Color.White) }
        }
    }
}

/**
 * The mic's tap: listening stops it; otherwise it starts, asking for the
 * microphone the first time, with a plain line if that's refused.
 */
@Composable
fun rememberMicStart(dictation: Dictation, start: () -> Unit): () -> Unit {
    val context = LocalContext.current
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) start() else dictation.problem = Dictation.MIC_REFUSED
    }
    return {
        when {
            dictation.isListening -> dictation.finish()
            dictation.isFinishing -> {}
            ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED -> start()
            else -> ask.launch(Manifest.permission.RECORD_AUDIO)
        }
    }
}

/** Why the mic won't start. */
@Composable
fun DictationNote(text: String, modifier: Modifier = Modifier) {
    Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = modifier)
}

/**
 * The box while dictating: the waveform across it, the time so far and a
 * stop, so it's plain the phone's hearing (Mike, 6 Nov). Once tapped off,
 * a spinner while the last words come.
 */
@Composable
fun ListeningBar(dictation: Dictation, modifier: Modifier = Modifier) {
    val red = LocalSignals.current.failed
    Row(
        modifier.clip(RoundedCornerShape(22.dp)).background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .clickable(enabled = dictation.isListening) { dictation.finish() }
            .heightIn(min = 44.dp).padding(start = 16.dp, end = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Waveform(dictation.levels, red, Modifier.weight(1f))
        Spacer(Modifier.width(12.dp))
        Text(elapsed(dictation.startedAt), style = MaterialTheme.typography.labelLarge, color = red)
        Spacer(Modifier.width(10.dp))
        if (dictation.isFinishing) CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp, color = red)
        else Box(Modifier.size(24.dp).clip(CircleShape).background(red), contentAlignment = Alignment.Center) {
            Icon(Line.Stop, "Stop dictating", Modifier.size(12.dp), tint = Color.White)
        }
    }
}

/** "0:08" since the mic started, ticking. */
@Composable
private fun elapsed(startedAt: Long?): String {
    var now by remember { mutableLongStateOf(android.os.SystemClock.elapsedRealtime()) }
    LaunchedEffect(startedAt) {
        while (startedAt != null) { now = android.os.SystemClock.elapsedRealtime(); delay(250) }
    }
    val seconds = ((now - (startedAt ?: now)) / 1000).coerceAtLeast(0)
    return "%d:%02d".format(seconds / 60, seconds % 60)
}

/** The mic's last moments as bars spread across the width they're given. */
@Composable
private fun Waveform(levels: List<Float>, color: Color, modifier: Modifier = Modifier) {
    Row(modifier.height(24.dp), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
        for (value in levels) Box(Modifier.width(3.dp).height((3 + value * 20).dp).clip(CircleShape).background(color))
    }
}
