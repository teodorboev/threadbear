package com.mikerosoft.threadbear.android.ui

import android.content.ContentValues
import android.content.Context
import android.graphics.Bitmap
import android.graphics.ImageDecoder
import android.graphics.drawable.AnimatedImageDrawable
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.os.Environment
import android.provider.MediaStore
import android.widget.ImageView
import android.widget.MediaController
import android.widget.VideoView
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.rememberTransformableState
import androidx.compose.foundation.gestures.transformable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import com.mikerosoft.threadbear.android.BuildConfig
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.kit.CappedDownload
import com.mikerosoft.threadbear.kit.ConcurrencyLimit
import com.mikerosoft.threadbear.kit.CostLRU
import com.mikerosoft.threadbear.kit.FSize
import com.mikerosoft.threadbear.kit.PictureDownloadError
import com.mikerosoft.threadbear.kit.PictureFraming
import com.mikerosoft.threadbear.kit.PictureLimits
import com.mikerosoft.threadbear.kit.PictureSource
import java.io.File
import java.nio.ByteBuffer
import java.security.MessageDigest
import kotlin.coroutines.cancellation.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * What a picture's address turned out to be: a still (or a moving
 * picture's first frame), a moving picture played by Android's
 * AnimatedImageDrawable, which decodes one frame at a time at the size it
 * was asked for (never the file's own), or a video, played full screen
 * from its file.
 */
class LoadedPicture(
    val image: ImageBitmap,
    val width: Int,
    val height: Int,
    val moving: AnimatedImageDrawable? = null,
    val video: File? = null,
    /** A moving picture's file, which it plays from. */
    val file: File? = null,
)

/**
 * Pictures from file storage or the web (the iPhone's PictureLoader). The
 * address is whatever an agent wrote, so the server is anyone's: read with
 * a byte cap and one deadline over the whole download, headers included
 * (CappedDownload), its size checked from its header before anything is
 * decoded (50 million source pixels, PictureLimits), and decoded down to
 * the size it's shown at. The decoded pictures share one budget, least
 * recently used out first, and a row only holds its picture while it's
 * composed, which in the transcript's lazy list is while it's near the
 * screen. Four downloads at once at most.
 */
object PictureLoader {
    private val decoded = CostLRU<String, LoadedPicture>(96 * 1024 * 1024)
    private val downloads = ConcurrencyLimit(4)

    /** Only https (http too for a local backend in a debug build). */
    fun allowed(url: String): Boolean =
        url.startsWith("https://", ignoreCase = true) || (BuildConfig.ALLOW_LOCAL && url.startsWith("http://", ignoreCase = true))

    /** Null `maxSide`: full screen, still held to PictureLimits.MAX_FULL_SCREEN_SIDE. */
    suspend fun picture(context: Context, url: String, maxSide: Int?): LoadedPicture {
        val key = "${maxSide ?: 0} $url"
        // A moving picture whose file the disk budget let go is fetched again.
        decoded.value(key)?.takeIf { p -> listOfNotNull(p.video, p.file).all { it.exists() } }?.let { return it }
        if (!allowed(url)) throw PictureDownloadError.BadStatus(0)
        val data = downloads.run { withContext(Dispatchers.IO) { CappedDownload.data(url) } }
        val picture = withContext(Dispatchers.Default) { decode(context, data, url, maxSide ?: PictureLimits.MAX_FULL_SCREEN_SIDE) }
        // A moving picture's frames come and go one at a time; count two.
        val frame = picture.width.toLong() * picture.height * 4
        decoded.set(picture, key, (if (picture.moving != null) frame * 2 else frame).coerceAtMost(Int.MAX_VALUE.toLong()).toInt())
        return picture
    }

    private fun decode(context: Context, data: ByteArray, url: String, maxSide: Int): LoadedPicture {
        // An MP4 isn't a picture: its first frame, and the file to play.
        if (data.size > 12 && String(data, 4, 4, Charsets.ISO_8859_1) == "ftyp") return decodeVideo(context, data, url, maxSide)
        var animated = false
        val listener = ImageDecoder.OnHeaderDecodedListener { decoder, info, _ ->
            val w = info.size.width
            val h = info.size.height
            // Refused from its header, before anything is decoded: a small
            // file can say it's 30000 pixels square.
            if (!PictureSource.allowed(w, h)) throw PictureDownloadError.TooManyPixels()
            val (tw, th) = PictureSource.targetSize(w, h, maxSide)
            decoder.setTargetSize(tw, th)
            decoder.allocator = ImageDecoder.ALLOCATOR_SOFTWARE
            animated = info.isAnimated
        }
        // A moving picture's first frame, or the still.
        val bitmap = ImageDecoder.decodeBitmap(ImageDecoder.createSource(ByteBuffer.wrap(data)), listener)
        if (animated) {
            // Played from a file, so the bytes go once this returns: a row
            // kept every GIF's whole file in memory on the iPhone (Codex
            // review, Daybreak finding 8).
            val file = runCatching { saved(context, data, url, "anim") }.getOrNull()
            val drawable = file?.let { runCatching { ImageDecoder.decodeDrawable(ImageDecoder.createSource(it), listener) }.getOrNull() }
            if (drawable is AnimatedImageDrawable) return LoadedPicture(bitmap.asImageBitmap(), bitmap.width, bitmap.height, moving = drawable, file = file)
        }
        return LoadedPicture(bitmap.asImageBitmap(), bitmap.width, bitmap.height)
    }

    /**
     * The whole file comes down first (the runner never sends one over 10 MB,
     * and CappedDownload stops anything over PictureLimits.MAX_BYTES), which
     * also means it plays with no signal.
     */
    private fun decodeVideo(context: Context, data: ByteArray, url: String, maxSide: Int): LoadedPicture {
        val quickTime = String(data, 8, 4, Charsets.ISO_8859_1) == "qt  "
        val file = saved(context, data, url, if (quickTime) "mov" else "mp4")
        val retriever = MediaMetadataRetriever()
        try {
            retriever.setDataSource(file.path)
            val w = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH)?.toIntOrNull() ?: maxSide
            val h = retriever.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT)?.toIntOrNull() ?: maxSide
            val (tw, th) = PictureSource.targetSize(w.coerceAtLeast(1), h.coerceAtLeast(1), maxSide)
            val poster = retriever.getScaledFrameAtTime(0, MediaMetadataRetriever.OPTION_CLOSEST_SYNC, tw, th)
                ?: throw PictureDownloadError.BadStatus(0)
            return LoadedPicture(poster.asImageBitmap(), poster.width, poster.height, video = file)
        } finally {
            retriever.release()
        }
    }

    /**
     * One at a time, trim, write and rename together: two saves trimming
     * before either wrote went over the budget (Codex review, 11 Nov).
     */
    @Synchronized
    private fun saved(context: Context, data: ByteArray, url: String, ext: String): File {
        val folder = File(context.cacheDir, "moving-pictures").apply { mkdirs() }
        val name = MessageDigest.getInstance("SHA-256").digest(url.toByteArray()).joinToString("") { "%02x".format(it) }
        val file = File(folder, "$name.$ext")
        if (!file.exists()) {
            trim(folder, room = data.size.toLong())
            val tmp = File(folder, "$name.$ext.part")
            tmp.writeBytes(data)
            tmp.renameTo(file)
        }
        file.setLastModified(System.currentTimeMillis())
        return file
    }

    /** The files moving pictures play from, held to a disk budget (Codex review, 11 Nov: they piled up). */
    private const val DISK_BUDGET = 150L * 1024 * 1024
    private const val DISK_FILES = 200
    private var swept = false

    /**
     * Makes room for `room` more bytes, the least recently used files going
     * first. The first time in a run it starts afresh: nothing on screen yet
     * plays from the files of an earlier run. A file going while a row still
     * plays it is fine (an open file outlives its name), and the cache hands
     * out a new one next time.
     */
    @Synchronized
    private fun trim(folder: File, room: Long) {
        val files = folder.listFiles()?.filter { it.isFile } ?: return
        if (!swept) {
            swept = true
            files.forEach { it.delete() }
            return
        }
        var total = files.sumOf { it.length() } + room
        var count = files.size + 1
        for (f in files.sortedBy { it.lastModified() }) {
            if (total <= DISK_BUDGET && count <= DISK_FILES) break
            total -= f.length()
            count--
            f.delete()
        }
    }
}

/** The longest side decoded for the column: a phone's column doesn't need all of a big PNG. */
private const val COLUMN_PIXELS = 1400

/**
 * A picture in a reply: as wide as the column at most, no taller than a
 * good part of the screen and never blown up past its own size. Tap for
 * full screen. Its place is kept from the size the runner sent, so the
 * thread doesn't jump as it loads. Pixels count as 2x, so a small diagram
 * stays small and sharp while a big one fills the column; under 120 dp
 * wide a picture is too small to make out or to tap.
 */
@Composable
fun ReplyPicture(
    url: String,
    alt: String,
    width: Int? = null,
    height: Int? = null,
    tallest: Dp = 360.dp,
    alignEnd: Boolean = false,
) {
    val context = LocalContext.current
    var loaded by remember(url) { mutableStateOf<LoadedPicture?>(null) }
    var failed by remember(url) { mutableStateOf(false) }
    var fullScreen by remember { mutableStateOf(false) }
    LaunchedEffect(url) {
        try {
            loaded = PictureLoader.picture(context, url, COLUMN_PIXELS)
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            failed = true
        }
    }
    if (failed) {
        PictureOnMac(alt, "Couldn't load this picture")
        return
    }
    val colors = MaterialTheme.colorScheme
    val pw = (loaded?.width ?: width?.takeIf { it > 0 } ?: 2000).toFloat()
    val ph = (loaded?.height ?: height?.takeIf { it > 0 } ?: 1500).toFloat()
    // A plain Layout rather than BoxWithConstraints, which is a
    // SubcomposeLayout and throws when asked for its intrinsic size, as a
    // quote's bar once did (Codex review, 11 Nov). This answers any measure.
    Layout(content = {
        val shape = RoundedCornerShape(10.dp)
        Box(
            Modifier.clip(shape).background(colors.surfaceContainerHigh)
                .border(0.5.dp, colors.outlineVariant, shape)
                .clickable(enabled = loaded != null, onClickLabel = if (loaded?.video != null) "Play it full screen" else "Show it full screen") { fullScreen = true },
            contentAlignment = Alignment.Center,
        ) {
            val picture = loaded
            when {
                picture == null -> CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 1.5.dp, color = colors.onSurfaceVariant)
                picture.moving != null -> MovingImage(picture.moving, Modifier.fillMaxSize())
                else -> {
                    Image(picture.image, alt.ifEmpty { "Picture" }, Modifier.fillMaxSize(), contentScale = ContentScale.FillBounds)
                    if (picture.video != null) PlayBadge()
                }
            }
        }
    }, modifier = Modifier.fillMaxWidth()) { measurables, constraints ->
        var nw = pw / 2
        var nh = ph / 2
        if (nw < 120) { nh = nh * 120 / nw; nw = 120f }
        val room = if (constraints.hasBoundedWidth) constraints.maxWidth.toDp().value else nw
        val fit = minOf(1f, room / nw, tallest.value / nh)
        val w = (nw * fit).dp.roundToPx().coerceIn(1, maxOf(1, if (constraints.hasBoundedWidth) constraints.maxWidth else 4096))
        val h = (nh * fit).dp.roundToPx().coerceIn(1, tallest.roundToPx().coerceAtLeast(1))
        val placeable = measurables.first().measure(androidx.compose.ui.unit.Constraints.fixed(w, h))
        val outer = if (constraints.hasBoundedWidth) constraints.maxWidth else w
        layout(outer, h) { placeable.place(if (alignEnd) outer - w else 0, 0) }
    }
    if (fullScreen) loaded?.let { PictureViewer(url, alt, it) { fullScreen = false } }
}

/** A moving picture, playing while it's composed. */
@Composable
private fun MovingImage(drawable: AnimatedImageDrawable, modifier: Modifier) {
    AndroidView(
        factory = { ImageView(it).apply { scaleType = ImageView.ScaleType.FIT_CENTER } },
        update = { view ->
            if (view.drawable !== drawable) view.setImageDrawable(drawable)
            drawable.start()
        },
        onRelease = { view -> drawable.stop(); view.setImageDrawable(null) },
        modifier = modifier,
    )
}

/** On a video's first frame in the thread, which plays once it's tapped. */
@Composable
private fun PlayBadge() {
    Box(Modifier.size(56.dp).clip(CircleShape).background(Color.Black.copy(alpha = 0.55f)), contentAlignment = Alignment.Center) {
        Icon(Line.Play, "Video", Modifier.size(24.dp), tint = Color.White)
    }
}

/**
 * In place of a picture or video the phone can't have: it hasn't gone up
 * yet, it's too big, or it's from a thread too old to have been looked through.
 */
@Composable
fun PictureOnMac(alt: String, note: String = "On your Mac") {
    val colors = MaterialTheme.colorScheme
    Row(
        Modifier.clip(RoundedCornerShape(10.dp)).background(colors.surfaceContainerHigh).padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Line.Photo, null, Modifier.size(18.dp), tint = colors.onSurfaceVariant)
        Spacer(Modifier.width(10.dp))
        Column {
            if (alt.isNotEmpty()) Text(alt, style = MaterialTheme.typography.bodyMedium)
            Text(note, style = MaterialTheme.typography.labelSmall, color = colors.onSurfaceVariant)
        }
    }
}

/**
 * Full screen on black: pinch or double tap to zoom, drag about, save from
 * the top bar. It opens clear of the status bar and the top bar
 * (PictureFraming), so all of a tall picture shows. A video plays instead.
 */
@Composable
fun PictureViewer(url: String, alt: String, preview: LoadedPicture, close: () -> Unit) {
    val context = LocalContext.current
    val model = LocalModel.current
    val scope = rememberCoroutineScope()
    var full by remember { mutableStateOf<LoadedPicture?>(null) }
    var saving by remember { mutableStateOf(false) }
    // A fetch that came to nothing: said so, with Try again, never a spinner
    // left turning for good (offline, say, when the file had to come again).
    var failed by remember { mutableStateOf(false) }
    var attempt by remember { mutableStateOf(0) }
    LaunchedEffect(url, attempt) {
        // A video's file is the whole of it already, while it's still on disk:
        // the disk budget may have let it go since the row loaded it, and the
        // loader fetches it again then.
        val video = preview.video
        if (video == null || !video.exists()) {
            failed = false
            full = runCatching { PictureLoader.picture(context, url, null) }.getOrNull()
            failed = full == null && preview.video != null
        }
    }
    // Never the preview's video once its file has gone: the player would get a dead path.
    val picture = full ?: preview.takeIf { p -> p.video?.exists() != false }
    Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Box(Modifier.fillMaxSize().background(Color.Black)) {
            val video = picture?.video
            if (picture == null && failed) {
                Column(Modifier.align(Alignment.Center), horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("Couldn't load the video", color = Color.White.copy(alpha = 0.8f), style = MaterialTheme.typography.bodyMedium)
                    TextButton(onClick = { attempt++ }) { Text("Try again", color = Color.White) }
                }
            } else if (picture == null) {
                CircularProgressIndicator(Modifier.align(Alignment.Center).size(24.dp), strokeWidth = 2.dp, color = Color.White)
            } else if (video != null) {
                AndroidView(
                    factory = { ctx ->
                        VideoView(ctx).apply {
                            setMediaController(MediaController(ctx).also { it.setAnchorView(this) })
                            setVideoPath(video.path)
                            setOnPreparedListener { start() }
                        }
                    },
                    onRelease = { it.stopPlayback() },
                    modifier = Modifier.fillMaxSize().padding(top = 52.dp).statusBarsPadding(),
                )
            } else {
                Zoomable(picture)
            }
            Row(Modifier.fillMaxWidth().statusBarsPadding().padding(horizontal = 12.dp, vertical = 4.dp), horizontalArrangement = Arrangement.SpaceBetween) {
                BarIcon(Line.Close, "Close", onClick = close)
                if (picture != null && video == null) {
                    if (saving) {
                        Box(Modifier.size(44.dp), contentAlignment = Alignment.Center) {
                            CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 1.5.dp, color = Color.White)
                        }
                    } else {
                        BarIcon(Line.Download, "Save to Pictures") {
                            saving = true
                            scope.launch {
                                val ok = withContext(Dispatchers.IO) { runCatching { save(context, url, alt) }.isSuccess }
                                saving = false
                                model.note(if (ok) "Saved to Pictures" else "Couldn't save it")
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Pinch or double tap to zoom (up to 8x), drag a zoomed one about. */
@Composable
private fun Zoomable(picture: LoadedPicture) {
    val density = LocalDensity.current
    val direction = LocalLayoutDirection.current
    val insets = WindowInsets.safeDrawing.asPaddingValues()
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    val state = rememberTransformableState { zoom, pan, _ ->
        scale = (scale * zoom).coerceIn(1f, 8f)
        offset = if (scale == 1f) Offset.Zero else offset + pan
    }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        // Clear of the status bar and the top bar's buttons, and the gesture bar.
        val framing = with(density) {
            PictureFraming(
                top = (insets.calculateTopPadding() + 52.dp).toPx(),
                left = insets.calculateLeftPadding(direction).toPx(),
                bottom = insets.calculateBottomPadding().toPx(),
                right = insets.calculateRightPadding(direction).toPx(),
            )
        }
        val screen = with(density) { FSize(maxWidth.toPx(), maxHeight.toPx()) }
        val size = framing.fitted(FSize(picture.width.toFloat(), picture.height.toFloat()), screen)
        val at = framing.insets(size, screen)
        val drawn = Modifier
            .padding(start = with(density) { at.left.toDp() }, top = with(density) { at.top.toDp() })
            .size(with(density) { size.width.toDp() }, with(density) { size.height.toDp() })
            .graphicsLayer(scaleX = scale, scaleY = scale, translationX = offset.x, translationY = offset.y)
        Box(
            Modifier.fillMaxSize().transformable(state).pointerInput(Unit) {
                detectTapGestures(onDoubleTap = { if (scale > 1f) { scale = 1f; offset = Offset.Zero } else scale = 3f })
            },
        ) {
            val moving = picture.moving
            if (moving != null) MovingImage(moving, drawn) else Image(picture.image, null, drawn, contentScale = ContentScale.Fit)
        }
    }
}

@Composable
private fun BarIcon(icon: androidx.compose.ui.graphics.vector.ImageVector, label: String, onClick: () -> Unit) {
    Box(
        Modifier.size(44.dp).clip(CircleShape).background(Color.Black.copy(alpha = 0.45f)).clickable(onClickLabel = label, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) { Icon(icon, label, Modifier.size(18.dp), tint = Color.White) }
}

/**
 * The picture as it came, into Pictures/Threadbear (MediaStore, so no
 * permission is needed). The bytes are read again through the same caps,
 * so a moving picture is saved moving.
 */
private fun save(context: Context, url: String, alt: String) {
    val data = CappedDownload.data(url)
    val (mime, ext) = when {
        data.size > 4 && data[0] == 'G'.code.toByte() && data[1] == 'I'.code.toByte() -> "image/gif" to "gif"
        data.size > 4 && data[0] == 0x89.toByte() && data[1] == 'P'.code.toByte() -> "image/png" to "png"
        data.size > 12 && String(data, 8, 4, Charsets.ISO_8859_1) == "WEBP" -> "image/webp" to "webp"
        else -> "image/jpeg" to "jpg"
    }
    val name = alt.replace(Regex("[^A-Za-z0-9 _-]"), "").trim().take(60).ifEmpty { "Picture" }
    val values = ContentValues().apply {
        put(MediaStore.Images.Media.DISPLAY_NAME, "$name.$ext")
        put(MediaStore.Images.Media.MIME_TYPE, mime)
        put(MediaStore.Images.Media.RELATIVE_PATH, Environment.DIRECTORY_PICTURES + "/Threadbear")
        put(MediaStore.Images.Media.IS_PENDING, 1)
    }
    val resolver = context.contentResolver
    val uri: Uri = resolver.insert(MediaStore.Images.Media.EXTERNAL_CONTENT_URI, values) ?: error("No place to save")
    try {
        resolver.openOutputStream(uri)?.use { it.write(data) } ?: error("Couldn't write")
        values.clear()
        values.put(MediaStore.Images.Media.IS_PENDING, 0)
        resolver.update(uri, values, null, null)
    } catch (e: Exception) {
        resolver.delete(uri, null, null)
        throw e
    }
}
