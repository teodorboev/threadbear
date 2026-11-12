package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * TucksTests.swift: hibernating from the phone, the row goes at once, the
 * heading's count goes up at once, and both then wait for the backend
 * without jumping.
 */
class TucksTest {
    private fun t(id: String, snoozed: Boolean = false) =
        PhoneThread(threadId = id, title = id, lastActivityAt = NOW, snoozedUntil = if (snoozed) NOW.plusSeconds(3600) else null)

    @Test fun aTuckHidesTheRowCountsItAndBouncesTheMoonOnce() {
        val tucks = Tucks().tuck("a")
        assertEquals(setOf("a"), tucks.hidden)
        assertEquals(1, tucks.uncounted)
        assertEquals(1, tucks.bounces)
    }

    @Test fun staysHiddenUntilTheListSaysItsAsleepOrGone() {
        var tucks = Tucks().tuck("a").tuck("b").tuck("c")
        // An unrelated update, from before the runner got to them.
        tucks = tucks.inboxArrived(listOf(t("a"), t("b"), t("c")), NOW)
        assertEquals(setOf("a", "b", "c"), tucks.hidden)
        // Gone from the inbox list, or still there but asleep (a star keeps a
        // hibernating thread in the list).
        tucks = tucks.inboxArrived(listOf(t("b", snoozed = true), t("c")), NOW)
        assertEquals(setOf("c"), tucks.hidden)
    }

    @Test fun theCountDoesNotAddOneTwiceWhenTheBackendCatchesUp() {
        val tucks = Tucks().tuck("a").countsArrived(2, 3)
        assertEquals(0, tucks.uncounted)
        assertEquals(setOf("a"), tucks.hidden)
    }

    @Test fun theCountDoesNotDropWhenTheListCatchesUpFirst() {
        var tucks = Tucks().tuck("a").inboxArrived(emptyList(), NOW)
        // The row has gone but phone:counts hasn't said yet: still one more.
        assertEquals(1, tucks.uncounted)
        tucks = tucks.countsArrived(2, 3)
        assertEquals(0, tucks.uncounted)
    }

    @Test fun aFreshCountWithNothingHiddenIsTheWholeTruth() {
        // Something else woke in the same breath, so the count never rose.
        val tucks = Tucks().tuck("a").inboxArrived(emptyList(), NOW).countsArrived(2, 2)
        assertEquals(0, tucks.uncounted)
    }

    @Test fun aRefusedTuckComesBack() {
        var tucks = Tucks().tuck("a").undo("a")
        assertTrue(tucks.hidden.isEmpty())
        assertEquals(0, tucks.uncounted)
        // Undoing one the list had already cleared takes nothing below nought.
        tucks = tucks.undo("a")
        assertEquals(0, tucks.uncounted)
    }
}
