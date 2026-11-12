package com.mikerosoft.threadbear.kit

import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.HttpURLConnection
import java.net.SocketTimeoutException
import java.net.URL
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.ceil
import kotlin.math.max
import kotlin.math.min

/**
 * How much of a picture the phone will take (PictureDownload.swift). A
 * picture's address can be any https URL an agent wrote in its Markdown, so
 * whoever runs that server picks what comes back: a response with no end, or
 * a small file that says it's 30000 pixels square, would otherwise be read
 * and decoded whole and run the phone out of memory (Daybreak finding 8, Nov
 * 2026).
 */
object PictureLimits {
    /** Bytes read for one picture or video. The runner never sends a file over 10 MB, so this leaves room for pictures from the web. */
    const val MAX_BYTES = 25 * 1024 * 1024
    /** Pixels (width times height) in the picture's header before anything is decoded: a 48 MP phone photo fits, a decompression bomb doesn't. */
    const val MAX_SOURCE_PIXELS = 50_000_000L
    /** The longest side decoded for full screen, rather than the whole picture. */
    const val MAX_FULL_SCREEN_SIDE = 4096
    /**
     * The whole download, headers and all. A server can hold a connection
     * open by sending a byte at a time, which beats any read timeout on
     * every read, or never send its headers at all.
     */
    const val DEADLINE_MS = 30_000L
}

sealed class PictureDownloadError(message: String) : Exception(message) {
    /** Over the byte cap, by its Content-Length or as it came in. */
    class TooLarge : PictureDownloadError("That picture is too big for the phone.")
    /** Over PictureLimits.MAX_SOURCE_PIXELS, by its own header. */
    class TooManyPixels : PictureDownloadError("That picture has too many pixels for the phone.")
    class BadStatus(val code: Int) : PictureDownloadError("The picture's server said $code.")
    /** Past the whole download's deadline. */
    class TimedOut : PictureDownloadError("The picture took too long to come.")
}

object CappedDownload {
    /**
     * Cuts a download off at its deadline from outside: a server dripping its
     * headers keeps `responseCode` waiting with no read to check the clock
     * between, and disconnecting from another thread ends it.
     */
    private val watchdog = Executors.newSingleThreadScheduledExecutor { r -> Thread(r, "picture-download-watchdog").apply { isDaemon = true } }

    /**
     * The body of `url`, refused as soon as it's known to be over `limit`
     * bytes: from a Content-Length over it before any of the body is read,
     * or once more than that many bytes have come, when the connection is
     * closed. Nothing much over the limit is ever held. Blocking: call it
     * off the main thread.
     */
    fun data(url: String, limit: Int = PictureLimits.MAX_BYTES, deadlineMs: Long = PictureLimits.DEADLINE_MS): ByteArray {
        val start = System.nanoTime()
        fun late() = (System.nanoTime() - start) / 1_000_000 >= deadlineMs
        val conn = URL(url).openConnection() as HttpURLConnection
        val timeout = min(10_000L, max(1L, deadlineMs)).toInt()
        conn.connectTimeout = timeout
        conn.readTimeout = timeout
        val cut = AtomicBoolean(false)
        val cutOff = watchdog.schedule({ cut.set(true); conn.disconnect() }, deadlineMs, TimeUnit.MILLISECONDS)
        try {
            val code = conn.responseCode
            if (code !in 200..299) throw PictureDownloadError.BadStatus(code)
            // -1 when the server didn't say.
            if (conn.contentLengthLong > limit) throw PictureDownloadError.TooLarge()
            val out = ByteArrayOutputStream(conn.contentLengthLong.takeIf { it > 0 }?.toInt() ?: (64 * 1024))
            conn.inputStream.use { input ->
                val buf = ByteArray(16 * 1024)
                while (true) {
                    // A server trickling a byte at a time beats the read
                    // timeout on every read; this is the whole download's.
                    if (late()) throw PictureDownloadError.TimedOut()
                    // Never read more than one byte past the cap.
                    val n = input.read(buf, 0, min(buf.size, limit + 1 - out.size()))
                    if (n < 0) break
                    out.write(buf, 0, n)
                    if (out.size() > limit) throw PictureDownloadError.TooLarge()
                }
            }
            return out.toByteArray()
        } catch (e: PictureDownloadError) {
            throw e
        } catch (e: SocketTimeoutException) {
            throw PictureDownloadError.TimedOut()
        } catch (e: IOException) {
            if (cut.get() || late()) throw PictureDownloadError.TimedOut()
            throw e
        } finally {
            cutOff.cancel(false)
            // Stops the transfer now, rather than whenever the stream goes away.
            conn.disconnect()
        }
    }
}

/** What a picture's header allows, before anything is decoded. The app reads the size (ImageDecoder's header, or BitmapFactory's bounds) and asks here. */
object PictureSource {
    /** Whether a picture of this size may be decoded: both sides known, and its pixels within `maxPixels`, worked out so huge sides can't overflow. */
    fun allowed(width: Int, height: Int, maxPixels: Long = PictureLimits.MAX_SOURCE_PIXELS): Boolean {
        if (width <= 0 || height <= 0) return false
        return width.toDouble() * height.toDouble() <= maxPixels.toDouble()
    }

    /** The power of two to sample by so the longer side comes to `maxSide` or under (BitmapFactory's inSampleSize). */
    fun sampleSize(width: Int, height: Int, maxSide: Int): Int {
        val longest = max(width, height).toLong()
        if (longest <= 0 || maxSide <= 0) return 1
        var sample = 1
        while (longest / sample > maxSide && sample < (1 shl 30)) sample *= 2
        return sample
    }

    /** The size to decode at: the longer side `maxSide` at most, never bigger than the picture, its shape kept (as ImageIO's thumbnail rounds, up). */
    fun targetSize(width: Int, height: Int, maxSide: Int): Pair<Int, Int> {
        if (width <= 0 || height <= 0) return 0 to 0
        val scale = min(1.0, maxSide.toDouble() / max(width, height))
        val w = ceil(width * scale - 1e-9).toInt().coerceAtLeast(1)
        val h = ceil(height * scale - 1e-9).toInt().coerceAtLeast(1)
        return w to h
    }
}
