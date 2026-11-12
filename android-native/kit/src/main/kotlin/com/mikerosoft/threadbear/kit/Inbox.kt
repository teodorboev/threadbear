package com.mikerosoft.threadbear.kit

import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.temporal.ChronoUnit
import java.util.Locale

/** The inbox's headings, top to bottom, as on the Mac sidebar. */
enum class PhoneSection(val title: String, val view: ThreadsView?) {
    STARRED("Starred", null),
    INBOX("Inbox", null),
    RUNNING("Running", null),
    HIBERNATING("Hibernating", ThreadsView.HIBERNATING),
    ARCHIVED("Archived", ThreadsView.DEN),
}

/** Which headings are folded. Everything but Starred and Inbox starts folded. */
data class SectionFolds(private val foldedSet: Set<PhoneSection> = INITIAL) {
    val folded: Set<PhoneSection> = foldedSet - PhoneSection.INBOX

    fun isFolded(section: PhoneSection) = section != PhoneSection.INBOX && section in folded

    fun toggle(section: PhoneSection): SectionFolds {
        if (section == PhoneSection.INBOX) return this
        return SectionFolds(if (section in folded) folded - section else folded + section)
    }

    /** The backend lists to keep a subscription to. */
    fun views(): Set<ThreadsView> =
        setOf(ThreadsView.INBOX) + PhoneSection.entries.filter { !isFolded(it) }.mapNotNull { it.view }

    val saved: List<String> get() = folded.map { it.name.lowercase() }.sorted()

    companion object {
        val INITIAL = setOf(PhoneSection.RUNNING, PhoneSection.HIBERNATING, PhoneSection.ARCHIVED)

        fun fromSaved(saved: List<String>?): SectionFolds =
            SectionFolds(saved?.mapNotNull { s -> PhoneSection.entries.firstOrNull { it.name.lowercase() == s } }?.toSet() ?: INITIAL)
    }
}

/** How much an inbox thread wants Mike, lowest first (InboxUrgency.swift). */
object InboxUrgency {
    fun of(answerWaiting: Boolean, needsYou: Boolean, broken: Boolean): Int = when {
        answerWaiting -> 0
        needsYou -> if (broken) 1 else 2
        else -> if (broken) 3 else 4
    }

    fun counts(urgency: Int) = urgency <= 2
}

/** The inbox's sections, the same rules as the Mac's ThreadSections.build. */
data class InboxSections(
    val starred: List<PhoneThread> = emptyList(),
    val inbox: List<PhoneThread> = emptyList(),
    val running: List<PhoneThread> = emptyList(),
    val scheduled: List<PhoneThread> = emptyList(),
) {
    /** The badge: threads waiting on an answer or wanting him. */
    val needsYouCount: Int
        get() {
            val awake = starred.filter { it.snoozedUntil == null && it.snoozedUntilRestart != true }
            return (awake + inbox).count { InboxUrgency.counts(urgency(it)) }
        }

    val isEmpty get() = starred.isEmpty() && inbox.isEmpty() && running.isEmpty() && scheduled.isEmpty()

    companion object {
        fun build(threads: Iterable<PhoneThread>, now: Instant, wakes: Map<String, ScheduleWake> = emptyMap()): InboxSections {
            val starred = mutableListOf<PhoneThread>()
            val inbox = mutableListOf<PhoneThread>()
            val running = mutableListOf<PhoneThread>()
            val scheduled = mutableListOf<PhoneThread>()
            for (t in threads) {
                if (t.archivedAt != null) continue
                if (t.starredAt != null) { starred += t; continue }
                if (isSnoozed(t, now)) continue
                if (t.reviewWaiting || needsAnswer(t) || t.waitingOnMike || (t.needsYou && t.working == null)) {
                    inbox += t
                } else if (t.isBusy || t.working != null) {
                    running += t
                } else if (t.status == ThreadStatus.IDLE && t.needs != "you" && t.waitingForRoom == null && wakes[t.threadId] != null) {
                    scheduled += t
                } else {
                    inbox += t
                }
            }
            starred.sortWith(compareBy<PhoneThread> { it.starredAt ?: Instant.MAX }.thenBy { it.threadId })
            inbox.sortWith(compareBy<PhoneThread> { urgency(it) }.then(NEWEST_FIRST))
            running.sortWith(NEWEST_FIRST)
            scheduled.sortWith(compareBy<PhoneThread> { wakes[it.threadId]?.nextRunAt ?: Instant.MAX }.then(NEWEST_FIRST))
            return InboxSections(starred, inbox, running, scheduled)
        }

        /** The Hibernating heading: scheduled and snoozed together, soonest to wake first. */
        fun hibernating(scheduled: List<PhoneThread>, snoozed: List<PhoneThread>, wakes: Map<String, ScheduleWake>): List<PhoneThread> {
            val seen = HashSet<String>()
            val all = (scheduled + snoozed).filter { seen.add(it.threadId) }
            val wakesAt = { t: PhoneThread -> t.snoozedUntil ?: wakes[t.threadId]?.nextRunAt ?: Instant.MAX }
            return all.sortedWith(compareBy(wakesAt).then(NEWEST_FIRST))
        }

        /** For each thread a schedule will message, the one due soonest. */
        fun wakes(schedules: Iterable<ScheduleWake>): Map<String, ScheduleWake> {
            val soonest = HashMap<String, ScheduleWake>()
            for (s in schedules) {
                val id = s.threadId ?: continue
                val next = s.nextRunAt ?: continue
                val current = soonest[id]?.nextRunAt
                if (current != null && current <= next) continue
                soonest[id] = s
            }
            return soonest
        }

        fun urgency(t: PhoneThread): Int = InboxUrgency.of(
            answerWaiting = t.reviewWaiting || needsAnswer(t),
            needsYou = t.needsYou,
            broken = t.status == ThreadStatus.ERROR || t.status == ThreadStatus.STOPPED,
        )

        fun needsAnswer(t: PhoneThread) = t.pendingApproval != null || t.status == ThreadStatus.WAITING

        fun isSnoozed(t: PhoneThread, now: Instant): Boolean {
            if (t.snoozedUntilRestart == true) return true
            val until = t.snoozedUntil ?: return false
            return until > now
        }

        /** "Until \"Convex I/O\" finishes" while hibernating on more than a time. */
        fun wakeLine(t: PhoneThread, now: Instant): String? {
            val note = t.wakeNote?.takeIf { it.isNotEmpty() } ?: return null
            return if (isSnoozed(t, now)) "Until $note" else null
        }

        val NEWEST_FIRST: Comparator<PhoneThread> =
            compareByDescending<PhoneThread> { it.lastActivityAt }.thenBy { it.threadId }
    }
}

/** What a row says at its right: the status while it means something, else the time. */
object RowStatus {
    enum class Kind { NEEDS_YOU, WORKING, FAILED, WAITING_FOR_CONNECTION, STOPPED, TIME }

    fun kind(t: PhoneThread): Kind {
        if (t.reviewWaiting || t.pendingApproval != null || t.status == ThreadStatus.WAITING) return Kind.NEEDS_YOU
        return when (t.status) {
            ThreadStatus.RUNNING, ThreadStatus.STARTING -> Kind.WORKING
            ThreadStatus.ERROR -> if (t.waitingForNetwork != null) Kind.WAITING_FOR_CONNECTION else Kind.FAILED
            ThreadStatus.STOPPED -> Kind.STOPPED
            else -> Kind.TIME
        }
    }

    fun text(t: PhoneThread, now: Instant, zone: ZoneId = ZoneId.systemDefault()): String = when (kind(t)) {
        Kind.NEEDS_YOU -> "Needs you"
        Kind.WORKING -> "Working"
        Kind.FAILED -> "Failed"
        Kind.WAITING_FOR_CONNECTION -> "Waiting for connection"
        Kind.STOPPED -> "Stopped"
        Kind.TIME -> if (t.woke != null) "Woke up" else RelativeTime.short(t.lastActivityAt, now, zone)
    }

    /** How long a running turn has been going: "53s", "4m", "1h 2m". */
    fun workingLabel(since: Instant, now: Instant): String {
        val seconds = maxOf(0L, ChronoUnit.SECONDS.between(since, now))
        if (seconds < 60) return "${seconds}s"
        val minutes = seconds / 60
        if (minutes < 60) return "${minutes}m"
        val hours = minutes / 60
        return if (minutes % 60 == 0L) "${hours}h" else "${hours}h ${minutes % 60}m"
    }
}

/** Short relative times, as on the Mac sidebar: "now", "5m", "3h", "Mon", "12 Oct". */
object RelativeTime {
    // Both dates held to FlexibleDate's range first, wherever they came from:
    // past it toEpochMilli() overflows and atZone() throws, in the middle of
    // drawing the inbox (Daybreak, 11 Nov 2026).
    fun short(date: Instant, now: Instant, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.UK): String =
        heldShort(FlexibleDate.held(date), FlexibleDate.held(now), zone, locale)

    private fun heldShort(date: Instant, now: Instant, zone: ZoneId, locale: Locale): String {
        val seconds = (now.toEpochMilli() - date.toEpochMilli()) / 1000.0
        if (seconds < 60) return "now"
        if (seconds < 3600) return "${(seconds / 60).saturatingInt()}m"
        if (seconds < 86_400) return "${(seconds / 3600).saturatingInt()}h"
        val d = date.atZone(zone)
        if (seconds < 6 * 86_400) return DateTimeFormatter.ofPattern("EEE", locale).format(d)
        val sameYear = d.year == now.atZone(zone).year
        return DateTimeFormatter.ofPattern(if (sameYear) "d MMM" else "d MMM yyyy", locale).format(d)
    }

    /** "10:42" today, "Mon 10:42" otherwise. */
    fun clock(date: Instant, now: Instant, zone: ZoneId = ZoneId.systemDefault(), locale: Locale = Locale.UK): String {
        val d = FlexibleDate.held(date).atZone(zone)
        val sameDay = d.toLocalDate() == FlexibleDate.held(now).atZone(zone).toLocalDate()
        return DateTimeFormatter.ofPattern(if (sameDay) "HH:mm" else "EEE HH:mm", locale).format(d)
    }
}

/** The preview is the start of markdown; rows show plain words (PreviewText). */
object PreviewText {
    private val link = Regex("""!?\[([^\]]*)(?:\]\([^)]*(\))?)?""")
    private val heading = Regex("""(^|\s)(#{1,6}|>)\s+""")
    private val spaces = Regex("""\s+""")

    fun plain(text: String): String {
        var line = link.replace(text) { m -> if (m.groups[2] == null) m.value else m.groupValues[1] }
        line = heading.replace(line, "$1")
        for (marker in listOf("**", "__", "~~", "`")) line = line.replace(marker, "")
        line = spaces.replace(line, " ")
        return line.trim()
    }
}

/** The Hibernate menu: 1 hour, this evening 6 pm, tomorrow 9 am, next Monday 9 am. */
object Hibernate {
    data class Preset(val id: String, val title: String, val date: Instant)

    fun presets(now: Instant, zone: ZoneId = ZoneId.systemDefault()): List<Preset> {
        val out = mutableListOf(Preset("hour", "1 hour", now.plusSeconds(3600)))
        val today = now.atZone(zone).toLocalDate()
        val evening = today.atTime(18, 0).atZone(zone).toInstant()
        if (evening.epochSecond - now.epochSecond > 3600) out += Preset("evening", "This evening, 6 pm", evening)
        out += Preset("tomorrow", "Tomorrow, 9 am", today.plusDays(1).atTime(9, 0).atZone(zone).toInstant())
        for (offset in 1L..7L) {
            val day = today.plusDays(offset)
            if (day.dayOfWeek == java.time.DayOfWeek.MONDAY) {
                out += Preset("monday", "Next Monday, 9 am", day.atTime(9, 0).atZone(zone).toInstant())
                break
            }
        }
        return out
    }
}
