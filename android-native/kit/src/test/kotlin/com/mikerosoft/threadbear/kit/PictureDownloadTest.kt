package com.mikerosoft.threadbear.kit

import com.sun.net.httpserver.HttpServer
import java.io.IOException
import java.net.InetSocketAddress
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * Daybreak finding 8: a picture's address is any https URL an agent wrote,
 * so its server must not be able to make the phone read without end, or
 * hold it waiting for ever.
 */
class PictureDownloadTest {
    /** A server that sends whatever a test says, a chunk at a time, and notes how much it sent and whether the phone hung up. */
    private class Stub(
        val length: Long = 0,
        val body: ByteArray = ByteArray(0),
        val endless: Boolean = false,
        val holdHeadersMs: Long = 0,
        val dripMs: Long = 0,
        val status: Int = 200,
    ) {
        val sent = AtomicLong()
        val stopped = AtomicBoolean(false)
        private val server = HttpServer.create(InetSocketAddress("127.0.0.1", 0), 0).apply {
            executor = Executors.newCachedThreadPool { r -> Thread(r).apply { isDaemon = true } }
            createContext("/") { ex ->
                try {
                    if (holdHeadersMs > 0) Thread.sleep(holdHeadersMs)
                    // 0 is chunked (no Content-Length), -1 no body.
                    ex.sendResponseHeaders(status, if (length != 0L) length else if (body.isEmpty() && !endless && dripMs == 0L) -1 else 0)
                    val out = ex.responseBody
                    if (body.isNotEmpty()) { out.write(body); sent.addAndGet(body.size.toLong()) }
                    val chunk = ByteArray(4096) { 0x41 }
                    while (endless) {
                        out.write(chunk); out.flush()
                        sent.addAndGet(chunk.size.toLong())
                        // A safety net, so a broken cap can't run the test machine out of memory.
                        if (sent.get() > 64L * 1024 * 1024) break
                    }
                    if (dripMs > 0) repeat(10_000) { out.write(0x41); out.flush(); sent.incrementAndGet(); Thread.sleep(dripMs) }
                    out.close()
                } catch (_: IOException) {
                    stopped.set(true)
                } catch (_: InterruptedException) {
                } finally {
                    ex.close()
                }
            }
            start()
        }
        val url get() = "http://127.0.0.1:${server.address.port}/a.png"
        fun stop() = server.stop(0)
    }

    private val stubs = mutableListOf<Stub>()
    private fun stub(s: Stub) = s.also { stubs += it }

    @AfterTest fun stopServers() = stubs.forEach { it.stop() }

    private val limit = 256 * 1024

    /** The finding's first test, with no Content-Length: a body that never ends is cut off at the cap and the transfer closed, never read whole. */
    @Test fun anEndlessResponseIsCancelledAtTheCap() {
        val s = stub(Stub(endless = true))
        assertFailsWith<PictureDownloadError.TooLarge> { CappedDownload.data(s.url, limit) }
        // Give the hang-up a moment to land on the server's thread.
        repeat(200) { if (!s.stopped.get()) Thread.sleep(10) }
        assertTrue(s.stopped.get())
        val sentAtStop = s.sent.get()
        Thread.sleep(100)
        assertEquals(sentAtStop, s.sent.get(), "still sending after the hang-up")
        assertTrue(sentAtStop < 16L * 1024 * 1024)
    }

    /** And with a Content-Length over the cap: refused before the body. */
    @Test fun aContentLengthOverTheCapIsRefusedUnread() {
        val s = stub(Stub(length = 500L * 1024 * 1024, endless = true))
        assertFailsWith<PictureDownloadError.TooLarge> { CappedDownload.data(s.url, limit) }
        repeat(200) { if (!s.stopped.get()) Thread.sleep(10) }
        assertTrue(s.stopped.get())
        // Only what the sockets' buffers took before the hang-up.
        assertTrue(s.sent.get() < 16L * 1024 * 1024)
    }

    /** The finding's second test: an ordinary picture still comes down whole. */
    @Test fun aNormalPictureStillLoads() {
        val body = ByteArray(100_000) { (it % 251).toByte() }
        val s = stub(Stub(length = body.size.toLong(), body = body))
        assertContentEquals(body, CappedDownload.data(s.url, limit))
        // And without a Content-Length.
        val chunked = stub(Stub(body = body))
        assertContentEquals(body, CappedDownload.data(chunked.url, limit))
    }

    /** Exactly at the cap is fine; one byte over isn't. */
    @Test fun theCapIsExact() {
        val at = stub(Stub(body = ByteArray(limit) { 1 }))
        assertEquals(limit, CappedDownload.data(at.url, limit).size)
        val over = stub(Stub(body = ByteArray(limit + 1) { 1 }))
        assertFailsWith<PictureDownloadError.TooLarge> { CappedDownload.data(over.url, limit) }
    }

    @Test fun aBadStatusSaysSo() {
        val s = stub(Stub(status = 404, body = "no".toByteArray()))
        assertEquals(404, assertFailsWith<PictureDownloadError.BadStatus> { CappedDownload.data(s.url, limit) }.code)
    }

    /** Headers that never come: the deadline covers them too, not only the reads after. */
    @Test fun headersThatNeverComeTimeOutAtTheDeadline() {
        val s = stub(Stub(holdHeadersMs = 20_000, body = "x".toByteArray()))
        val start = System.nanoTime()
        assertFailsWith<PictureDownloadError.TimedOut> { CappedDownload.data(s.url, limit, deadlineMs = 600) }
        val took = (System.nanoTime() - start) / 1_000_000
        assertTrue(took < 5_000, "took $took ms")
    }

    /** A byte at a time beats any read timeout; the whole download's deadline still ends it. */
    @Test fun aDrippingBodyTimesOutAtTheDeadline() {
        val s = stub(Stub(dripMs = 50))
        val start = System.nanoTime()
        assertFailsWith<PictureDownloadError.TimedOut> { CappedDownload.data(s.url, limit, deadlineMs = 800) }
        val took = (System.nanoTime() - start) / 1_000_000
        assertTrue(took < 5_000, "took $took ms")
    }

    /** A small file whose header claims a huge picture is refused before anything decodes it. */
    @Test fun aPixelBombIsRefusedFromItsHeader() {
        assertFalse(PictureSource.allowed(30000, 30000))
        // A big but ordinary picture passes, as does a 48 MP photo.
        assertTrue(PictureSource.allowed(4000, 3000))
        assertTrue(PictureSource.allowed(8064, 6048))
        // Sides that would overflow an Int when multiplied.
        assertFalse(PictureSource.allowed(Int.MAX_VALUE, Int.MAX_VALUE))
        // A header that doesn't say.
        assertFalse(PictureSource.allowed(0, 100))
        assertFalse(PictureSource.allowed(-1, 100))
    }

    @Test fun picturesAreDecodedDownsampled() {
        assertEquals(400 to 300, PictureSource.targetSize(800, 600, 400))
        // Never blown up past its own size.
        assertEquals(100 to 50, PictureSource.targetSize(100, 50, 1400))
        assertEquals(1400 to 1, PictureSource.targetSize(20000, 10, 1400))
        assertEquals(2, PictureSource.sampleSize(800, 600, 400))
        assertEquals(1, PictureSource.sampleSize(800, 600, 1400))
        assertEquals(4, PictureSource.sampleSize(4000, 3000, 1400))
        assertEquals(1, PictureSource.sampleSize(0, 0, 1400))
    }
}
