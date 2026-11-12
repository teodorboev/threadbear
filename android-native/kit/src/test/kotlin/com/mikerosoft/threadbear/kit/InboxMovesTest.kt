package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertTrue

/**
 * InboxMovesTests.swift and InboxZeroTests.swift: a thread changing heading
 * is news, so its row moves. A title or a working line changing isn't, or a
 * busy inbox would never sit still under his thumb.
 */
class InboxMovesTest {
    private fun t(id: String, status: ThreadStatus = ThreadStatus.IDLE, working: String? = null, starred: Boolean = false,
                  title: String? = null, ago: Long = 60, needsYou: Boolean = false) =
        PhoneThread(threadId = id, title = title ?: id, status = status, needsYou = needsYou, lastActivityAt = NOW.minusSeconds(ago),
            starredAt = if (starred) NOW else null, working = working)

    private fun sections(vararg threads: PhoneThread) = InboxSections.build(threads.toList(), NOW)

    @Test fun aFinishedTurnMovesFromRunningToInbox() {
        assertTrue(sections(t("a", ThreadStatus.RUNNING, "Running npm test"), t("b")).changedHeadings(sections(t("a"), t("b"))))
    }

    @Test fun aStarElsewhereMoves() {
        assertTrue(sections(t("a")).changedHeadings(sections(t("a", starred = true))))
    }

    @Test fun aThreadTurningUpOrLeavingCounts() {
        assertTrue(sections(t("a")).changedHeadings(sections(t("a"), t("b"))))
        assertTrue(sections(t("a"), t("b")).changedHeadings(sections(t("a"))))
    }

    @Test fun aFieldChangingStaysStill() {
        val before = sections(t("a", ThreadStatus.RUNNING, "Running npm test"), t("b"))
        val after = sections(t("a", ThreadStatus.RUNNING, "Reading a file", title = "A new title"), t("b", ago = 5))
        assertFalse(before.changedHeadings(after))
    }

    @Test fun aReorderInsideAHeadingStaysStill() {
        val before = sections(t("a", ago = 10), t("b", ago = 20))
        val after = sections(t("a", ago = 30), t("b", ago = 20))
        assertNotEquals(before.inbox.map { it.threadId }, after.inbox.map { it.threadId })
        assertFalse(before.changedHeadings(after))
    }

    @Test fun theRowsThatLandedAreTheOnesInANewHeading() {
        val before = sections(t("a", ThreadStatus.RUNNING, "Busy"), t("b"), t("c", starred = true))
        val after = sections(t("a"), t("b"), t("c", starred = true), t("d"))
        assertEquals(setOf("a", "d"), before.landed(after))
        assertTrue(after.landed(after).isEmpty())
    }

    @Test fun theRowsThatTravelAreTheOnesUnderANewHeading() {
        val before = sections(t("a", ThreadStatus.RUNNING, "Busy"), t("b"), t("c", starred = true), t("e"))
        val after = sections(t("a"), t("b"), t("c"), t("d"), t("e", starred = true))
        assertEquals(mapOf(
            "a" to InboxMove(PhoneSection.RUNNING, PhoneSection.INBOX),
            "c" to InboxMove(PhoneSection.STARRED, PhoneSection.INBOX),
            "e" to InboxMove(PhoneSection.INBOX, PhoneSection.STARRED),
        ), before.moves(after))
        assertTrue(after.moves(after).isEmpty())
    }

    @Test fun aThreadTurningUpOrLeavingDoesntTravel() {
        val before = sections(t("a"), t("gone", ThreadStatus.RUNNING, "Busy"))
        val after = sections(t("a"), t("new"))
        assertTrue(before.moves(after).isEmpty())
        assertEquals(setOf("new"), before.landed(after))
    }

    @Test fun aScheduledRowTravelsToHibernating() {
        val wake = ScheduleWake("s", "a", NOW.plusSeconds(3600))
        val before = InboxSections.build(listOf(t("a", ThreadStatus.RUNNING, "Busy")), NOW)
        val after = InboxSections.build(listOf(t("a")), NOW, mapOf("a" to wake))
        assertEquals(mapOf("a" to InboxMove(PhoneSection.RUNNING, PhoneSection.HIBERNATING)), before.moves(after))
    }

    // InboxZeroTests

    @Test fun runningOrQuietStarredThreadsDontStopInboxZero() {
        assertTrue(sections().isInboxZero)
        assertTrue(sections(t("a", ThreadStatus.RUNNING, "Busy")).isInboxZero)
        assertTrue(sections(t("a", starred = true)).isInboxZero)
        assertFalse(sections(t("a")).isInboxZero)
        // A star doesn't hide a thread that needs him.
        assertFalse(sections(t("a", needsYou = true, starred = true)).isInboxZero)
    }

    @Test fun clearingTheLastRowCelebratesOnce() {
        val cleared = InboxCleared()
        cleared.note(sections(t("a")), sections(), NOW)
        assertTrue(cleared.isPending(NOW))
        assertTrue(cleared.take(NOW))
        assertFalse(cleared.isPending(NOW))
        assertFalse(cleared.take(NOW))
    }

    @Test fun anInboxAlreadyEmptyNeverCelebrates() {
        val cleared = InboxCleared()
        assertFalse(cleared.take(NOW))
        cleared.note(sections(), sections(), NOW)
        assertFalse(cleared.take(NOW))
    }

    @Test fun aRowLeftBehindDoesntCelebrate() {
        val cleared = InboxCleared()
        cleared.note(sections(t("a"), t("b")), sections(t("b")), NOW)
        assertFalse(cleared.take(NOW))
    }

    @Test fun tooLateIsNotACelebration() {
        val cleared = InboxCleared()
        cleared.note(sections(t("a")), sections(), NOW)
        assertFalse(cleared.isPending(NOW.plusSeconds(3)))
        assertFalse(cleared.take(NOW.plusSeconds(3)))
    }
}
