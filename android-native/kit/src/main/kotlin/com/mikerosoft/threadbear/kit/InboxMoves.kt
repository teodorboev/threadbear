package com.mikerosoft.threadbear.kit

import java.time.Duration
import java.time.Instant

// InboxMoves.swift: what changed heading between two looks at the inbox.

/** Each thread's heading, Starred, Inbox, Running or (waiting on a schedule) Hibernating. */
private val InboxSections.placements: Map<String, PhoneSection>
    get() {
        val all = HashMap<String, PhoneSection>()
        starred.forEach { all[it.threadId] = PhoneSection.STARRED }
        inbox.forEach { all[it.threadId] = PhoneSection.INBOX }
        running.forEach { all[it.threadId] = PhoneSection.RUNNING }
        scheduled.forEach { all[it.threadId] = PhoneSection.HIBERNATING }
        return all
    }

/**
 * Whether any thread is now under a different heading, or has turned up
 * or gone. The inbox animates only for this: a list update that changes a
 * title or a working line leaves it still, as they can come in bursts.
 */
fun InboxSections.changedHeadings(to: InboxSections): Boolean = placements != to.placements

/**
 * The threads under a heading they weren't under before: the rows to give
 * a done tick or unread dot to as they land, not with them.
 */
fun InboxSections.landed(into: InboxSections): Set<String> {
    val old = placements
    return into.placements.filter { old[it.key] != it.value }.keys
}

/**
 * The threads under a heading both before and after, but a different one:
 * the rows that travel from one to the other. One that's turned up or gone
 * has nowhere to travel from or to, so it isn't here.
 */
fun InboxSections.moves(to: InboxSections): Map<String, InboxMove> {
    val old = placements
    val out = HashMap<String, InboxMove>()
    for ((id, section) in to.placements) {
        val from = old[id] ?: continue
        if (from != section) out[id] = InboxMove(from, section)
    }
    return out
}

/**
 * Nothing for Mike to do: what the empty view says. A thread still running
 * or waiting on a schedule, or a star on one that doesn't need him, doesn't
 * stop it, as they each have a heading of their own under it.
 */
val InboxSections.isInboxZero: Boolean get() = inbox.isEmpty() && needsYouCount == 0

/** A thread's row going from one heading to another (`moves`). */
data class InboxMove(val from: PhoneSection, val to: PhoneSection)

/**
 * He's just cleared the inbox (AppModel, the moment a row he took away was
 * the last), for the empty view to greet it. Never on launch.
 */
class InboxCleared {
    private var at: Instant? = null

    fun note(before: InboxSections, after: InboxSections, now: Instant) {
        if (!before.isInboxZero && after.isInboxZero) at = now
    }

    fun isPending(now: Instant): Boolean {
        val at = at ?: return false
        return Duration.between(at, now) < LASTS
    }

    /** Whether to celebrate, once. */
    fun take(now: Instant): Boolean = isPending(now).also { at = null }

    companion object {
        /**
         * The row's collapse comes first, and this is a moment or nothing:
         * cleared behind a search, the view that shows later shouldn't wave.
         */
        val LASTS: Duration = Duration.ofSeconds(2)
    }
}
