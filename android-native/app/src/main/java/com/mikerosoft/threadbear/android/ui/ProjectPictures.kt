package com.mikerosoft.threadbear.android.ui

import android.graphics.BitmapFactory
import android.os.SystemClock
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import com.mikerosoft.threadbear.android.BuildConfig
import java.io.ByteArrayOutputStream
import java.net.HttpURLConnection
import java.net.URL
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Project pictures (the iPhone's ProjectPictures): fetched once per URL. A
 * changed picture is a new file in storage, so a new URL, and nothing here
 * goes stale. Reading [image] from a composable redraws it when the picture
 * arrives, as the map is Compose state. Only touched on the main thread;
 * the download runs on IO.
 *
 * The address comes from the mirror, so a download is held to what an icon
 * needs (a cross-lab review found the first cut wasn't): 5 MB, 50 million
 * source pixels (as PictureLimits on the iPhone, as decoding a huge
 * progressive JPEG takes memory whatever the sample size), 20 seconds in
 * all, two at once, and the newest few dozen kept.
 */
@OptIn(ExperimentalCoroutinesApi::class)
object ProjectPictures {
    private val images = mutableStateMapOf<String, ImageBitmap>()
    /** Kept order, oldest first, so the cache can let the oldest go. */
    private val order = ArrayDeque<String>()
    private val loading = HashSet<String>()
    /** When a URL last failed: tried again after [RETRY_MS], not never. */
    private val failedAt = HashMap<String, Long>()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main)
    /** Two downloads at once, so a slow server can't hold up everything else on IO. */
    private val io = Dispatchers.IO.limitedParallelism(2)

    private const val LIMIT = 5 * 1024 * 1024
    private const val MAX_SOURCE_PIXELS = 50_000_000L
    /** An icon is never drawn bigger than a tile. */
    private const val MAX_SIDE = 512
    private const val DEADLINE_MS = 20_000L
    private const val RETRY_MS = 60_000L
    /** At most about 1 MB each (512 x 512 x 4). */
    private const val KEEP = 48
    /**
     * Cuts a download off at its deadline from outside: a server dripping
     * its headers keeps `responseCode` waiting with no read to check the
     * clock between, and disconnecting from another thread ends it.
     */
    private val watchdog = Executors.newSingleThreadScheduledExecutor { r -> Thread(r, "project-pictures-watchdog").apply { isDaemon = true } }

    fun image(url: String): ImageBitmap? {
        images[url]?.let { return it }
        if (url in loading) return null
        failedAt[url]?.let { if (SystemClock.elapsedRealtime() - it < RETRY_MS) return null }
        // Only https from the backend's storage (http for a local backend in debug).
        if (!url.startsWith("https://") && !(BuildConfig.ALLOW_LOCAL && url.startsWith("http://"))) {
            failedAt[url] = Long.MAX_VALUE / 2 // never: it won't become https
            return null
        }
        loading += url
        scope.launch {
            val bitmap = withContext(io) { runCatching { fetch(url) }.getOrNull() }
            loading -= url
            if (bitmap == null) {
                failedAt[url] = SystemClock.elapsedRealtime()
            } else {
                failedAt -= url
                images[url] = bitmap
                order.remove(url)
                order.addLast(url)
                while (order.size > KEEP) images.remove(order.removeFirst())
            }
        }
        return null
    }

    private fun fetch(url: String): ImageBitmap? {
        val deadline = SystemClock.elapsedRealtime() + DEADLINE_MS
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.connectTimeout = 10_000
        conn.readTimeout = 10_000
        val cutOff = watchdog.schedule({ conn.disconnect() }, DEADLINE_MS, TimeUnit.MILLISECONDS)
        try {
            if (conn.responseCode != 200) return null
            if (conn.contentLengthLong > LIMIT) return null
            val out = ByteArrayOutputStream()
            conn.inputStream.use { input ->
                val buf = ByteArray(16 * 1024)
                while (true) {
                    // A server trickling a byte at a time beats the read
                    // timeout on every read; this is the whole download's.
                    if (SystemClock.elapsedRealtime() > deadline) return null
                    val n = input.read(buf)
                    if (n < 0) break
                    out.write(buf, 0, n)
                    if (out.size() > LIMIT) return null
                }
            }
            val bytes = out.toByteArray()
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size, bounds)
            val w = bounds.outWidth.toLong()
            val h = bounds.outHeight.toLong()
            if (w <= 0 || h <= 0 || w * h > MAX_SOURCE_PIXELS) return null
            // Halve until the longer side is MAX_SIDE or under.
            var sample = 1
            while (maxOf(w, h) / sample > MAX_SIDE) sample *= 2
            val opts = BitmapFactory.Options().apply { inSampleSize = sample }
            return BitmapFactory.decodeByteArray(bytes, 0, bytes.size, opts)?.asImageBitmap()
        } finally {
            cutOff.cancel(false)
            conn.disconnect()
        }
    }
}
