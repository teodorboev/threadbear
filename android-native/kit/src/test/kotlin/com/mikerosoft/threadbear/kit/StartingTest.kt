package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals

/** The Starting rows a new thread shows until its runner has made it. */
class StartingTest {
    private fun row(message: String = "Fix the build\nand ship it", runnerId: String? = null, photos: Int = 0, failed: String? = null) =
        StartingThread("cmd", message, runnerId, photos, NOW, failed)
    private fun runner(id: String, secondsAgo: Long = 10) = PhoneRunner(id, id.uppercase(), lastSeenAt = NOW.minusSeconds(secondsAgo))

    @Test fun theTitleIsTheFirstLineOrThePhotos() {
        assertEquals("Fix the build", row().title)
        assertEquals("A photo", row("", photos = 1).title)
        assertEquals("3 photos", row("\n", photos = 3).title)
    }

    @Test fun theLineSaysWhereItStarts() {
        val m5 = runner("m5")
        val bruce = runner("bruce", 600)
        assertEquals("M5", row(runnerId = "m5").line(listOf(m5, bruce), NOW))
        assertEquals("BRUCE offline, starts when it's back", row(runnerId = "bruce").line(listOf(m5, bruce), NOW))
        // No runner picked, one runner: that one.
        assertEquals("M5", row().line(listOf(m5), NOW))
        // No runner picked among several: the backend picks.
        assertEquals("On its way", row().line(listOf(m5, bruce), NOW))
        assertEquals("Tap to try again · No such folder", row(failed = "No such folder").line(listOf(m5), NOW))
    }

    @Test fun aCommandsStatusSaysWhatBecomesOfTheRow() {
        assertEquals(StartingThread.Outcome.Started("t1"), StartingThread.outcome(CommandStatus(CommandState.DONE, threadId = "t1")))
        assertEquals(StartingThread.Outcome.Failed("No room"), StartingThread.outcome(CommandStatus(CommandState.FAILED, "No room")))
        assertEquals(StartingThread.Outcome.Failed("The runner couldn't start it."), StartingThread.outcome(CommandStatus(CommandState.FAILED)))
        assertEquals(StartingThread.Outcome.Gone, StartingThread.outcome(null))
        assertEquals(StartingThread.Outcome.Gone, StartingThread.outcome(CommandStatus(CommandState.DONE)))
        assertEquals(StartingThread.Outcome.Waiting, StartingThread.outcome(CommandStatus(CommandState.TAKEN)))
    }
}
