package com.mikerosoft.threadbear.kit

import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit

/**
 * Getting a photo ready to send (PhotoPrep.swift), the parts that aren't
 * pixels: the limits and the sizes. The app decodes and encodes with
 * Android's ImageDecoder (app/PhotoDecoding.kt): a JPEG no longer than
 * [MAX_PIXELS] on its long edge, the right way up. A 12 MP HEIC is far
 * bigger than a model looks at and would be slow on mobile data. Drawing it
 * afresh also leaves its metadata behind, so a photo's location never goes
 * up to Convex.
 */
object PhotoPrep {
    const val MAX_PIXELS = 2048

    /** How many photos one message takes (phone.ts, MAX_COMMAND_IMAGES). */
    const val MAX_PHOTOS = 6

    /**
     * The most a photo can be before it's prepared: past these it's refused
     * before anything decodes it. A pasted picture can be a small file that
     * unpacks to a huge one (a 16,000 px square PNG is 750 KB and 768 MB of
     * pixels), and decoding that froze the phone or got it killed for its
     * memory (Daybreak, 7 Nov 2026). The camera's 48 MP photos and its
     * panoramas fit.
     */
    const val MAX_SOURCE_BYTES = 100L * 1024 * 1024
    const val MAX_SOURCE_PIXELS = 100_000_000L

    /** The long edge of a composer's thumbnail: its 64 dp tile at 3x for a 3:2 photo's short side, and plenty for a chip. */
    const val THUMBNAIL_PIXELS = 320

    const val JPEG_QUALITY = 85

    /** Whether a picture is small enough to prepare, from its file's size (if known) and the width and height its header gives. */
    fun fits(width: Int, height: Int, bytes: Long? = null): Boolean {
        if (bytes != null && bytes > MAX_SOURCE_BYTES) return false
        return width >= 1 && height >= 1 && width.toLong() * height.toLong() <= MAX_SOURCE_PIXELS
    }

    /** `width` by `height` shrunk to `longest` on its long edge, never blown up, at least 1 px each way. */
    fun fitted(width: Int, height: Int, longest: Int = MAX_PIXELS): Pair<Int, Int> {
        val long = maxOf(width, height)
        if (long <= longest) return width to height
        val scale = longest.toDouble() / long
        return maxOf(1, Math.round(width * scale).toInt()) to maxOf(1, Math.round(height * scale).toInt())
    }

    private val gate = Semaphore(2)

    /**
     * `work`, two at a time at most across the app, so six big photos added
     * together don't all hold their pixels at once.
     */
    suspend fun <T> inTurn(work: suspend () -> T): T = gate.withPermit { work() }

    /**
     * A turn taken now, for work that can outlive whoever waits on it: a
     * decode left running after its waiter gave up (a stalled provider)
     * still holds its turn until it really ends, so the two-at-a-time cap
     * holds. The function it gives back hands the turn back, once only.
     */
    suspend fun takeTurn(): () -> Unit {
        gate.acquire()
        val given = AtomicBoolean(false)
        return { if (given.compareAndSet(false, true)) gate.release() }
    }

    /** How many turns are free now, for tests. */
    internal val freeTurns: Int get() = gate.availablePermits
}
