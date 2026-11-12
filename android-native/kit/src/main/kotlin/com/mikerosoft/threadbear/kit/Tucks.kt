package com.mikerosoft.threadbear.kit

import java.time.Instant

/**
 * Threads hibernated from this phone, on their way to the Hibernating
 * heading (AppModel.hibernate, Tucks.swift). The row goes as soon as it's
 * picked and the heading's count goes up with it, but the runner may be a
 * while, and the inbox list and phone:counts then catch up separately, in
 * either order. Immutable, so it can sit in a StateFlow.
 */
data class Tucks(
    /** Kept out of the inbox until the list says they're asleep. */
    val hidden: Set<String> = emptySet(),
    /** How many phone:counts hasn't counted yet, to add to its number. */
    val uncounted: Int = 0,
    /** Goes up on each tuck, for the moon's one bounce. */
    val bounces: Int = 0,
) {
    fun tuck(threadId: String) = Tucks(hidden + threadId, uncounted + 1, bounces + 1)

    /** The runner refused it: back in the inbox, and off the count. */
    fun undo(threadId: String): Tucks {
        if (threadId !in hidden) return this
        return copy(hidden = hidden - threadId, uncounted = maxOf(0, uncounted - 1))
    }

    /**
     * Hidden until a list has it asleep or doesn't have it. Unlike an
     * archived thread, a hibernating one can stay in this list (a starred
     * one does), so "gone from the list" alone could hide it for good.
     */
    fun inboxArrived(list: List<PhoneThread>, now: Instant): Tucks {
        val awake = list.filter { !InboxSections.isSnoozed(it, now) }.map { it.threadId }.toSet()
        return copy(hidden = hidden intersect awake)
    }

    /**
     * phone:counts has moved: what it's gained is no longer ours to add.
     * With nothing still hidden it knows about them all, even if it didn't
     * rise because something else woke at the same moment.
     */
    fun countsArrived(old: Int?, new: Int?): Tucks {
        var n = uncounted
        if (old != null && new != null) n = maxOf(0, n - maxOf(0, new - old))
        if (hidden.isEmpty()) n = 0
        return copy(uncounted = n)
    }
}
