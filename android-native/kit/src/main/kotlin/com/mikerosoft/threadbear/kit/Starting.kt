package com.mikerosoft.threadbear.kit

import java.time.Duration
import java.time.Instant

/**
 * A new thread asked for from this phone that its runner hasn't made yet
 * (AppModel.starting on the iPhone). The sheet closes as soon as the
 * backend has the request, and these stand in for them at the top of the
 * inbox: the runner reports a new thread only once its row is in Convex,
 * which waits behind any sync it's in the middle of, and on 7 Nov the
 * sheet's spinner sat there 22 seconds after the M5's runner had restarted.
 */
data class StartingThread(
    /** The create command's id. */
    val id: String,
    /** What went to the runner. */
    val message: String,
    /** Nil when the backend picks (only one runner). */
    val runnerId: String? = null,
    /** How many photos went with it, for a row with no words. */
    val photos: Int = 0,
    val startedAt: Instant,
    /** Why the runner couldn't start it. */
    val failed: String? = null,
) {
    /** The first line of what was asked, or the photos. */
    val title: String
        get() {
            val first = message.lineSequence().firstOrNull { it.isNotBlank() }?.trim() ?: ""
            if (first.isNotEmpty()) return first
            return if (photos == 1) "A photo" else "$photos photos"
        }

    /**
     * Under the title: where it starts, or why it hasn't. Its Mac asleep: it
     * starts when it's back, maybe much later. No runner picked: the
     * backend's, which with one runner is that one.
     */
    fun line(runners: List<PhoneRunner>, now: Instant): String {
        failed?.let { return "Tap to try again · $it" }
        val picked = runnerId?.let { id -> runners.firstOrNull { it.runnerId == id } }
        val runner = picked ?: runners.singleOrNull() ?: return RunnerOrder.offlineText(runners, now) ?: "On its way"
        if (!runner.isOnline(now)) return "${runner.name} offline, starts when it's back"
        return runner.name
    }

    /** What its create command's status means for the row. */
    sealed class Outcome {
        /** Made: the row goes, and the thread may open. */
        data class Started(val threadId: String) : Outcome()
        data class Failed(val note: String) : Outcome()
        /**
         * The command's gone from the backend (pruned long after), or done
         * without saying which thread: nothing left to wait for, and the
         * thread, if any, is in the list.
         */
        data object Gone : Outcome()
        data object Waiting : Outcome()
    }

    companion object {
        /**
         * A thread that started this soon after it was asked for opens, as
         * the sheet used to wait to do. One that took longer (its Mac was
         * asleep) just turns up in the list, rather than pulling you out of
         * what you're on.
         */
        val OPENS_WITHIN: Duration = Duration.ofSeconds(60)

        fun outcome(status: CommandStatus?): Outcome = when {
            status?.threadId != null -> Outcome.Started(status.threadId)
            status?.state == CommandState.FAILED -> Outcome.Failed(status.note ?: "The runner couldn't start it.")
            status == null || status.state == CommandState.DONE -> Outcome.Gone
            else -> Outcome.Waiting
        }
    }
}
