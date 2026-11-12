package com.mikerosoft.threadbear.kit

import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking

/** Codex's review of Daybreak finding 8: every picture under the per-file cap could still add up, and any number downloaded at once. */
class PictureMemoryTest {
    /** A hundred big pictures never hold more than the budget, the least recently used going first. */
    @Test fun theBudgetHoldsAcrossAllPictures() {
        val cache = CostLRU<Int, ByteArray>(100)
        for (i in 0 until 100) {
            cache.set(ByteArray(1), i, cost = 24)
            assertTrue(cache.totalCost <= 100)
        }
        assertEquals(4, cache.count)
        assertNotNull(cache.value(99))
        assertNull(cache.value(0))
        // Using one keeps it over an older one: 96 is the oldest until it's looked at.
        cache.value(96)
        cache.set(ByteArray(0), 200, cost = 10)
        assertNotNull(cache.value(96))
        assertNull(cache.value(97))
        assertTrue(cache.totalCost <= 100)
        // One over the whole budget isn't kept, and pushes nothing out.
        val before = cache.count
        cache.set(ByteArray(0), 300, cost = 101)
        assertNull(cache.value(300))
        assertEquals(before, cache.count)
    }

    @Test fun downloadsRunAFewAtATime() = runBlocking {
        val limit = ConcurrencyLimit(3)
        val now = AtomicInteger()
        val most = AtomicInteger()
        val jobs = (0 until 20).map {
            launch(Dispatchers.Default) {
                limit.run {
                    most.accumulateAndGet(now.incrementAndGet(), ::maxOf)
                    delay(20)
                    now.decrementAndGet()
                }
            }
        }
        jobs.forEach { it.join() }
        assertEquals(3, most.get())
        assertEquals(0, now.get())
    }
}
