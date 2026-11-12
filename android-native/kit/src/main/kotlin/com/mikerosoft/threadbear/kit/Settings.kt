package com.mikerosoft.threadbear.kit

import java.time.Instant

/**
 * When this phone is notified that a thread needs Mike (backend API 3).
 * `AWAY` (the default) only while he isn't at the Mac: the Mac app is
 * closed, or the keyboard and mouse have been idle a couple of minutes.
 */
enum class NotifyMode(val wire: String, val label: String) {
    AWAY("away", "When I'm away from the Mac"),
    ALWAYS("always", "Always"),
    OFF("off", "Never");

    companion object {
        fun of(raw: String?): NotifyMode = entries.firstOrNull { it.wire == raw } ?: AWAY
    }
}

/**
 * The runners in Mike's order (backend API 56): new threads start on the
 * first one online (phone:setRunnerOrder). AppModel.setRunnerOrder and
 * receiveRunners on the iPhone.
 */
object RunnerOrder {
    /** The runners as dragged, here at once: each one now placed. */
    fun placed(order: List<PhoneRunner>): List<PhoneRunner> = order.map { it.copy(placed = true) }

    /**
     * The backend's runners, kept in a dragged order it hasn't caught up with:
     * a heartbeat's update in between would otherwise snap the list back, and
     * the next drag would move the wrong row. `caughtUp` once its own runners
     * come back in that order; the pending order can then be let go. One that's
     * first heartbeated meanwhile is only added at the end.
     */
    fun receive(list: List<PhoneRunner>, pending: List<String>?): Received {
        if (pending == null) return Received(list, caughtUp = true)
        val ids = list.map { it.runnerId }
        if (ids.filter { it in pending } == pending.filter { it in ids }) return Received(list, caughtUp = true)
        val rank = { r: PhoneRunner -> pending.indexOf(r.runnerId).let { if (it < 0) pending.size else it } }
        val runners = list.withIndex()
            .sortedWith(compareBy<IndexedValue<PhoneRunner>> { rank(it.value) }.thenBy { it.index })
            .map { if (it.value.runnerId in pending) it.value.copy(placed = true) else it.value }
        return Received(runners, caughtUp = false)
    }

    data class Received(val runners: List<PhoneRunner>, val caughtUp: Boolean)

    /** "Mac offline since 10:42" once no runner has checked in for three minutes. */
    fun offlineText(runners: List<PhoneRunner>, now: Instant): String? {
        if (runners.isEmpty() || runners.any { it.isOnline(now) }) return null
        val newest = runners.maxBy { it.lastSeenAt }
        val name = if (runners.size == 1) "Mac" else newest.name
        return "$name offline since ${RelativeTime.clock(newest.lastSeenAt, now)}"
    }

    /** Settings' line for a runner: "Online", or when it was last seen. */
    fun status(runner: PhoneRunner, now: Instant): String =
        if (runner.isOnline(now)) "Online" else "Last seen ${RelativeTime.clock(runner.lastSeenAt, now)}"
}
