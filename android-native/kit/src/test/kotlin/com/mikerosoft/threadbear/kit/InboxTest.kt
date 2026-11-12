package com.mikerosoft.threadbear.kit

import java.io.File
import java.time.ZoneOffset
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.serialization.json.JsonObject

class InboxTest {
    @Test fun ordersByUrgencyThenNewest() {
        val s = InboxSections.build(listOf(
            thread("done", 1),
            thread("unread", 5, needsYou = true),
            thread("asking", 30, status = ThreadStatus.WAITING, approval = true),
            thread("failed", 2, status = ThreadStatus.ERROR, needsYou = true),
            thread("busy", 0, status = ThreadStatus.RUNNING, working = "Bear with me"),
            thread("background", 3, working = "1 sub-agent"),
            thread("snoozed", 0, snoozed = true),
        ), NOW)
        assertEquals(listOf("asking", "failed", "unread", "done"), s.inbox.map { it.threadId })
        assertEquals(listOf("busy", "background"), s.running.map { it.threadId })
        assertEquals(3, s.needsYouCount)
    }

    @Test fun starredThreadsStayAtTheTop() {
        fun starred(t: PhoneThread, minutesAgo: Long) = t.copy(starredAt = NOW.minusSeconds(minutesAgo * 60))
        val archived = starred(thread("archived", 0), 40).copy(archivedAt = NOW)
        val s = InboxSections.build(listOf(
            starred(thread("second", 0), 10),
            starred(thread("asleep", 0, needsYou = true, snoozed = true), 20),
            starred(thread("first", 9, status = ThreadStatus.RUNNING), 30),
            starred(thread("asks", 0, needsYou = true), 5),
            archived,
            thread("plain", 0),
        ), NOW)
        assertEquals(listOf("first", "asleep", "second", "asks"), s.starred.map { it.threadId })
        assertEquals(listOf("plain"), s.inbox.map { it.threadId })
        assertTrue(s.running.isEmpty())
        assertEquals(1, s.needsYouCount)
    }

    @Test fun bookedThreadsWait() {
        val said = thread("said-done-unread", 1).copy(needs = "nothing", unread = true)
        val asked = thread("asked-read", 2).copy(needs = "you")
        val review = thread("review", 10).copy(pendingReviews = 1.0)
        val noRoom = thread("no-room", 11).copy(waitingForRoom = PhoneWaitingForRoom("2026-10-30T12:00:00.000Z"))
        val threads = listOf(
            thread("read", 5), said, asked, thread("unread", 3, needsYou = true),
            thread("question", 4, status = ThreadStatus.WAITING, approval = true),
            thread("failed", 6, status = ThreadStatus.ERROR), thread("running", 7, status = ThreadStatus.RUNNING),
            thread("no-wake", 8), review, noRoom,
        )
        fun w(id: String, t: String?, secs: Long) = ScheduleWake(id, t, NOW.plusSeconds(secs))
        val wakes = InboxSections.wakes(listOf(
            w("1", "read", 600), w("2", "said-done-unread", 120), w("3", "said-done-unread", 9000),
            w("4", "asked-read", 60), w("5", "unread", 60), w("6", "question", 60), w("7", "failed", 60),
            w("8", "running", 60), w("12", "review", 60), w("13", "no-room", 60), w("11", null, 30),
        ))
        assertEquals("2", wakes["said-done-unread"]?.scheduleId)
        val s = InboxSections.build(threads, NOW, wakes)
        assertEquals(listOf("said-done-unread", "read"), s.scheduled.map { it.threadId })
        val snoozed = thread("snoozed", 12).copy(snoozedUntil = NOW.plusSeconds(300))
        val restart = thread("restart", 13).copy(snoozedUntilRestart = true)
        val rows = InboxSections.hibernating(s.scheduled, listOf(restart, snoozed, s.scheduled[1]), wakes)
        assertEquals(listOf("said-done-unread", "snoozed", "read", "restart"), rows.map { it.threadId })
        assertEquals(setOf("asked-read", "unread", "question", "failed", "no-wake", "review", "no-room"), s.inbox.map { it.threadId }.toSet())
        assertEquals(listOf("running"), s.running.map { it.threadId })
    }

    @Test fun anOpenAskStaysInTheInboxWhileBackgroundWorkRuns() {
        fun asking(id: String, ticked: Boolean = false) =
            thread(id, 1, working = "1 job").copy(needs = "you", asks = listOf(PhoneAsk("Put the card back", ticked = ticked)))
        val said = thread("said-nothing", 1, working = "1 job").copy(needs = "nothing")
        val underway = asking("underway").copy(status = ThreadStatus.RUNNING)
        val s = InboxSections.build(listOf(asking("open"), asking("ticked", true), said, underway), NOW)
        assertEquals(listOf("open"), s.inbox.map { it.threadId })
        assertEquals(setOf("ticked", "said-nothing", "underway"), s.running.map { it.threadId }.toSet())
    }

    @Test fun foldedSectionsAreNotLoaded() {
        var folds = SectionFolds.fromSaved(null)
        assertFalse(folds.isFolded(PhoneSection.INBOX))
        assertFalse(folds.isFolded(PhoneSection.STARRED))
        assertEquals(setOf(ThreadsView.INBOX), folds.views())
        folds = folds.toggle(PhoneSection.ARCHIVED).toggle(PhoneSection.INBOX)
        assertEquals(setOf(ThreadsView.INBOX, ThreadsView.DEN), folds.views())
        assertEquals(folds, SectionFolds.fromSaved(folds.saved + "someday"))
    }

    /** The same cases the Mac, the iPhone and the website run (app/Tests/Fixtures). */
    @Test fun urgencyMatchesTheSharedCases() {
        val file = File("../../app/Tests/Fixtures/inbox-urgency-cases.json")
        val cases = (parseJson(file.readText()) as JsonObject).arr("cases")!!
        assertTrue(cases.size > 5)
        for (c in cases) {
            val o = c as JsonObject
            val t = o.obj("thread")!!
            val row = thread("x", 1).copy(
                status = ThreadStatus.of(t.str("status") ?: "idle"),
                needsYou = t.bool("needsYou") ?: false,
                unread = t.bool("unread") ?: false,
                pendingReviews = t.num("pendingReviews"),
            )
            val urgency = InboxSections.urgency(row)
            assertEquals(o.num("urgency")!!.toInt(), urgency, o.str("name"))
            assertEquals(o.bool("counted"), InboxUrgency.counts(urgency), o.str("name"))
        }
    }

    @Test fun rowStatusText() {
        assertEquals("Needs you", RowStatus.text(thread("a", 1, status = ThreadStatus.WAITING, approval = true), NOW))
        assertEquals("Working", RowStatus.text(thread("b", 1, status = ThreadStatus.RUNNING), NOW))
        assertEquals("5m", RowStatus.text(thread("c", 5), NOW))
        assertEquals("Failed", RowStatus.text(thread("d", 1, status = ThreadStatus.ERROR), NOW))
        val offline = thread("e", 1, status = ThreadStatus.ERROR).copy(waitingForNetwork = PhoneWaitingForNetwork("2026-11-03T03:59:55.000Z"))
        assertEquals("Waiting for connection", RowStatus.text(offline, NOW))
        assertEquals("53s", RowStatus.workingLabel(NOW.minusSeconds(53), NOW))
        assertEquals("1h 2m", RowStatus.workingLabel(NOW.minusSeconds(62 * 60), NOW))
    }

    @Test fun relativeTimesAreShort() {
        val utc = ZoneOffset.UTC
        assertEquals("now", RelativeTime.short(NOW.minusSeconds(30), NOW, utc))
        assertEquals("3h", RelativeTime.short(NOW.minusSeconds(3 * 3600 + 5), NOW, utc))
        assertEquals("Sat", RelativeTime.short(NOW.minusSeconds(3 * 86_400), NOW, utc))
        assertEquals("12 Oct", RelativeTime.short(java.time.Instant.parse("2026-10-12T10:00:00Z"), NOW, utc))
        assertEquals("12 Oct 2025", RelativeTime.short(java.time.Instant.parse("2025-10-12T10:00:00Z"), NOW, utc))
    }

    @Test fun previewIsPlain() {
        assertEquals("See the PR and Fixed it", PreviewText.plain("See [the PR](https://x) and **Fixed** `it`"))
        assertEquals("Title then text", PreviewText.plain("## Title\n\nthen   text"))
        assertEquals("[not a link", PreviewText.plain("[not a link"))
    }

    @Test fun aHibernatingRowSaysWhatItWaitsFor() {
        val t = thread("h", 1).copy(snoozedUntil = NOW.plusSeconds(60), wakeNote = "\"Convex I/O\" finishes")
        assertEquals("Until \"Convex I/O\" finishes", InboxSections.wakeLine(t, NOW))
        assertNull(InboxSections.wakeLine(t.copy(snoozedUntil = null), NOW))
    }

    @Test fun hibernatePresets() {
        val utc = ZoneOffset.UTC
        // Tuesday 3 Nov 2026, 04:00 UTC.
        val p = Hibernate.presets(NOW, utc)
        assertEquals(listOf("hour", "evening", "tomorrow", "monday"), p.map { it.id })
        assertEquals(java.time.Instant.parse("2026-11-09T09:00:00Z"), p.last().date)
    }
}
