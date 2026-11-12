package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class MessageHistoryTest {
    private fun m(seq: Int, text: String = "m$seq", role: Role = if (seq % 2 == 0) Role.ASSISTANT else Role.USER) = PhoneMessage(seq, role, text, at = T0)

    private fun page(seqs: IntRange, hasMore: Boolean, images: List<Int> = emptyList()) =
        MessagePage(seqs.map { m(it) }, hasMore, images.map { MessageImage(it, url = "https://x/$it.png") })

    /**
     * The live page never grows: "Load earlier" reads a chunk before it,
     * once, rather than asking the live query for more rows to read again on
     * every new message (37.6 MB of Convex I/O on 8 Nov).
     */
    @Test fun loadEarlierAsksForAChunkNotABiggerLivePage() {
        val h = MessageHistory()
        assertNull(h.nextBefore)
        h.takeLive(page(921..1000, hasMore = true))
        assertEquals(921, h.nextBefore)
        val all = MessageHistory()
        all.takeLive(page(1..80, hasMore = false))
        assertNull(all.nextBefore)
    }

    /** Going back is never cut off by the cap: what you load is room on top of it. */
    @Test fun chunksGoAboveTheLivePageUntilTheStart() {
        val h = MessageHistory()
        h.takeLive(page(7421..7500, hasMore = true))
        assertTrue(h.takeOlder(page(7221..7420, hasMore = true, images = listOf(7310)), before = 7421))
        assertEquals((7221..7500).toList(), h.messages.map { it.seq })
        assertEquals(7221, h.nextBefore)
        assertTrue(h.takeOlder(page(1..7220, hasMore = false), before = 7221))
        assertEquals(7500, h.messages.size)
        assertFalse(h.hasMore)
        assertNull(h.nextBefore)
        assertEquals(listOf(7310), h.images.map { it.seq })
    }

    /** After loading, rows sliding off the live page still only get the cap on top of what was loaded. */
    @Test fun aStreamAfterLoadingEarlierKeepsWithinWhatWasLoadedPlusTheCap() {
        val h = MessageHistory()
        h.takeLive(page(3001..3080, hasMore = true))
        h.takeOlder(page(2001..3000, hasMore = true), before = 3001)
        for (i in 1..400) h.takeLive(page((3001 + i * 10)..(3080 + i * 10), hasMore = true))
        assertEquals(1000 + MessageHistory.OLDER_MAX_ROWS, h.older.size)
        assertEquals((h.live?.messages?.first()?.seq ?: 0) - 1, h.older.last().seq)
        assertTrue(h.hasMore)
        assertEquals(h.older.first().seq, h.nextBefore)
    }

    /**
     * Codex's second review: rows the cap let go were read again and earned
     * more room each time. They earn nothing now, and a chunk that brings
     * nothing that stays ends loading.
     */
    @Test fun rowsLetGoAndReadAgainEarnNoRoomAndLoadingStops() {
        val h = MessageHistory()
        h.takeLive(page(1..80, hasMore = true))
        for (i in 1..300) h.takeLive(page((1 + i * 10)..(80 + i * 10), hasMore = true))
        assertEquals(MessageHistory.OLDER_MAX_ROWS, h.older.size)
        val before = h.nextBefore ?: 0
        h.takeOlder(page((before - 22)..(before - 1), hasMore = true), before = before)
        assertEquals(MessageHistory.OLDER_MAX_ROWS, h.older.size)
        assertFalse(h.hasMore)
        assertNull(h.nextBefore)
    }

    /** However far back you go, what loading adds stops at LOADED_MAX times the caps. */
    @Test fun loadingEarlierAddsRoomOnlyUpToItsCeiling() {
        val top = 100_000
        val h = MessageHistory()
        h.takeLive(page((top - 79)..top, hasMore = true))
        repeat(60) {
            val before = h.nextBefore ?: return@repeat
            h.takeOlder(page((before - 1000)..(before - 1), hasMore = true), before = before)
        }
        assertTrue(h.older.size <= MessageHistory.OLDER_MAX_ROWS * (MessageHistory.LOADED_MAX + 1))
        assertTrue(h.older.size >= MessageHistory.OLDER_MAX_ROWS * MessageHistory.LOADED_MAX)
        assertFalse(h.hasMore)
    }

    /**
     * Daybreak, 10 Nov: another runner writing its own thread a few rows at a
     * time, with nobody pressing Load earlier. `older` stays within its caps,
     * nearest the live page, and each update is cheap.
     */
    @Test fun aPacedStreamOfBigRowsKeepsOlderWithinItsByteCap() {
        val text = "x".repeat(8192)
        fun big(seqs: IntRange) = MessagePage(seqs.map { PhoneMessage(it, Role.ASSISTANT, text, at = T0) }, true)
        val h = MessageHistory()
        val pages = (0..1250).map { big((1 + it * 8)..(80 + it * 8)) }
        val start = System.nanoTime()
        for (p in pages) h.takeLive(p)
        val tookMs = (System.nanoTime() - start) / 1_000_000
        val bytes = h.older.sumOf { MessageHistory.bytes(it) }
        assertTrue(bytes <= MessageHistory.OLDER_MAX_BYTES)
        assertTrue(h.older.size < 1100)
        assertEquals((h.live?.messages?.first()?.seq ?: 0) - 1, h.older.last().seq)
        // What went can be loaded again.
        assertEquals(h.older.first().seq, h.nextBefore)
        assertTrue(tookMs < 2000, "took $tookMs ms")
    }

    @Test fun aLongStreamOfSmallRowsKeepsOlderWithinItsRowCap() {
        val h = MessageHistory()
        for (i in 0 until 400) h.takeLive(page((1 + i * 10)..(80 + i * 10), hasMore = true))
        assertEquals(MessageHistory.OLDER_MAX_ROWS, h.older.size)
        assertEquals((h.live?.messages?.first()?.seq ?: 0) - 1, h.older.last().seq)
    }

    /** Pictures between rows count with the row before them, so a stream of pictures can't grow `older` past its bytes either. */
    @Test fun picturesBetweenRowsCountAgainstTheCap() {
        val url = "https://x/" + "p".repeat(100_000)
        val h = MessageHistory()
        for (i in 0 until 300) {
            val a = 1 + i * 1000
            h.takeLive(MessagePage(listOf(PhoneMessage(a, Role.USER, "a", at = T0), PhoneMessage(a + 1000, Role.USER, "b", at = T0)),
                true, listOf(MessageImage(a + 500, url = "$url?$i"))))
        }
        val bytes = h.older.sumOf { MessageHistory.bytes(it) } + h.olderImages.sumOf { MessageHistory.bytes(it) }
        assertTrue(bytes <= MessageHistory.OLDER_MAX_BYTES)
        assertTrue(h.olderImages.size < 100)
        assertTrue(h.olderImages.all { it.seq >= (h.older.firstOrNull()?.seq ?: Int.MAX_VALUE) })
    }

    /** Codex's review: a picture at a seq below the only row, with no older rows, was added again on every live update. */
    @Test fun aPictureBeforeEveryRowIsKeptOnceHoweverManyUpdatesCome() {
        val one = MessagePage(listOf(PhoneMessage(10, Role.USER, "a", at = T0)), false, listOf(MessageImage(1, url = "https://x/1.png")))
        val h = MessageHistory()
        repeat(1000) { h.takeLive(one) }
        assertEquals(1, h.olderImages.size)
        assertEquals(listOf(1), h.images.map { it.seq })
    }

    @Test fun picturesWithNoOlderRowsAreCappedToo() {
        val h = MessageHistory()
        for (i in 0 until 3000) {
            h.takeLive(MessagePage(listOf(PhoneMessage(1_000_000, Role.USER, "a", at = T0)), false, listOf(MessageImage(i, url = "https://x/$i.png"))))
        }
        assertTrue(h.older.isEmpty())
        assertTrue(h.olderImages.size <= MessageHistory.OLDER_MAX_PICTURES)
        assertEquals(2998, h.olderImages.maxOf { it.seq })
    }

    /** New messages slide the oldest off the live page. Those rows stay, or there'd be a hole above the live ones. */
    @Test fun rowsThatSlideOffTheLivePageStay() {
        val h = MessageHistory()
        h.takeLive(page(921..1000, hasMore = true, images = listOf(921)))
        h.takeLive(page(922..1001, hasMore = true))
        assertEquals((921..1001).toList(), h.messages.map { it.seq })
        // Two more while a chunk was on its way.
        h.takeLive(page(924..1003, hasMore = true))
        assertEquals(921, h.firstSeq)
        assertTrue(h.takeOlder(page(721..920, hasMore = true), before = 921))
        assertEquals((721..1003).toList(), h.messages.map { it.seq })
        assertEquals(listOf(921), h.images.map { it.seq })
        // And again, after the chunk.
        h.takeLive(page(925..1004, hasMore = true))
        assertEquals((721..1004).toList(), h.messages.map { it.seq })
    }

    /** A chunk asked for before rows that are no longer the first is dropped. */
    @Test fun aLateChunkIsDropped() {
        val h = MessageHistory()
        h.takeLive(page(921..1000, hasMore = true))
        assertTrue(h.takeOlder(page(721..920, hasMore = true), before = 921))
        assertFalse(h.takeOlder(page(721..920, hasMore = true), before = 921))
        assertEquals(280, h.messages.size)
    }

    /** Away long enough for a whole page of new messages: it starts again from the newest rather than show a hole. */
    @Test fun noOverlapWithThePageBeforeStartsAgain() {
        val h = MessageHistory()
        h.takeLive(page(921..1000, hasMore = true))
        assertTrue(h.takeOlder(page(721..920, hasMore = true), before = 921))
        h.takeLive(page(1101..1180, hasMore = true))
        assertEquals((1101..1180).toList(), h.messages.map { it.seq })
        assertEquals(1101, h.nextBefore)
    }

    /** The live page's top moving back up (big rows that got smaller) takes its rows back from the chunks: nothing shows twice. */
    @Test fun aLivePageReachingFurtherBackTakesItsRowsBack() {
        val h = MessageHistory()
        h.takeLive(page(921..1000, hasMore = true, images = listOf(925)))
        h.takeLive(page(931..1010, hasMore = true))
        h.takeLive(page(910..1010, hasMore = true, images = listOf(925)))
        assertEquals((910..1010).toList(), h.messages.map { it.seq })
        assertEquals(listOf(925), h.images.map { it.seq })
    }

    /** A picture that finishes uploading after its row slid off the live page arrives only from phone:images. */
    @Test fun latePicturesOfRowsReadOnceShow() {
        val h = MessageHistory()
        assertNull(h.lateImagesFrom)
        h.takeLive(page(921..1000, hasMore = true))
        h.takeLive(page(925..1004, hasMore = true))
        assertEquals(921, h.lateImagesFrom)
        h.takeLateImages(listOf(MessageImage(922, url = "https://x/922.png"), MessageImage(900, url = "https://x/900.png"), MessageImage(1001, url = "https://x/1001.png")))
        assertEquals(listOf(922, 1001), h.images.map { it.seq }.sorted())
    }

    /** A copy is a snapshot: what's taken after doesn't change it. */
    @Test fun aCopyStaysAsItWas() {
        val h = MessageHistory()
        h.takeLive(page(921..1000, hasMore = true))
        val snapshot = h.copy()
        h.takeLive(page(925..1004, hasMore = true))
        assertEquals((921..1000).toList(), snapshot.messages.map { it.seq })
        assertEquals((921..1004).toList(), h.messages.map { it.seq })
    }
}
