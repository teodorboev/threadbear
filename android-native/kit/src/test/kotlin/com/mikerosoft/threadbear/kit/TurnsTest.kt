package com.mikerosoft.threadbear.kit

import com.mikerosoft.threadbear.kit.Role.ASSISTANT
import com.mikerosoft.threadbear.kit.Role.TOOL
import com.mikerosoft.threadbear.kit.Role.USER
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

class TurnsTest {
    private fun replies(items: List<ThreadItem>) = items.mapNotNull { (it as? ThreadItem.Message)?.message?.seq }

    @Test fun workAfterTheSummaryShowsTheLastThingSaidAndDoesntCrash() {
        val messages = listOf(
            msg(1, USER, "Set up the API", 0.0),
            msg(2, TOOL, "Make the keys", 5.0),
            msg(3, TOOL, "threadbear: end_turn", 10.0),
            msg(4, ASSISTANT, "- Keys made\n- Install the app next", 11.0, summary = true),
            msg(5, TOOL, "Check for the app on the phone", 20.0),
            msg(6, ASSISTANT, "Installing it myself.", 30.0),
            msg(7, TOOL, "Tap Get", 40.0),
            msg(8, TOOL, "threadbear: end_turn", 50.0),
            msg(9, ASSISTANT, "It's installed, sign in and tell me.", 60.0),
        )
        val items = Turns.fold(messages, busy = false)
        assertEquals(9, (items.last() as ThreadItem.Message).message.seq)
        assertEquals(listOf(8), Turns.replyIndexes(messages))
    }

    @Test fun workAfterTheSummaryWithNothingSaidKeepsTheSummary() {
        val body = listOf(msg(1, ASSISTANT, "- Done", 0.0, summary = true), msg(2, TOOL, "One more check", 5.0))
        assertEquals(listOf(0), Turns.replyIndexes(body))
    }

    @Test fun aFinishedTurnFoldsItsWorkAndShowsTheSummary() {
        val items = Turns.fold(listOf(
            msg(1, USER, "Fix it", 0.0),
            msg(2, ASSISTANT, "Looking.", 5.0),
            msg(3, TOOL, "Run the tests", 10.0),
            msg(4, ASSISTANT, "Done.", 120.0),
            msg(5, TOOL, "threadbear: end_turn", 132.0),
            msg(6, ASSISTANT, "- Fixed it\n- Tests pass", 133.0, summary = true),
        ), busy = false)
        assertEquals(3, items.size)
        assertEquals("Worked for 2m 13s", assertIs<ThreadItem.Worked>(items[1]).worked.title)
        assertEquals(6, (items[2] as ThreadItem.Message).message.seq)
        assertEquals(items.size, items.map { it.id }.toSet().size)
    }

    @Test fun aLongerMessageAfterTheLastStepBeatsTheBullets() {
        val items = Turns.fold(listOf(
            msg(1, USER, "Why?", 0.0),
            msg(2, TOOL, "Read x.ts", 1.0),
            msg(3, ASSISTANT, "Because the cache is keyed by folder, so two threads in one folder share a run and the second never starts its own.", 30.0),
            msg(4, TOOL, "threadbear: end_turn", 31.0),
            msg(5, ASSISTANT, "- Explained it", 31.0, summary = true),
        ), busy = false)
        assertEquals(3, (items.last() as ThreadItem.Message).message.seq)
    }

    @Test fun theTurnStillRunningIsNotFoldedAndRunnerMessagesJoinTheTurnBefore() {
        val items = Turns.fold(listOf(
            msg(1, USER, "Go", 0.0),
            msg(2, TOOL, "Build", 1.0),
            msg(3, ASSISTANT, "Built.", 2.0),
            msg(4, USER, "The app restarted", 3.0, origin = "runner"),
            msg(5, TOOL, "Check it", 4.0),
            msg(6, ASSISTANT, "Checked.", 9.0),
            msg(7, USER, "Next", 20.0),
            msg(8, TOOL, "Run", 21.0),
        ), busy = true)
        assertEquals("Worked for 9s", assertIs<ThreadItem.Worked>(items[1]).worked.title)
        assertEquals(listOf(1, 3, 6, 7), replies(items))
        assertEquals(6, items.size)
        assertIs<ThreadItem.Tools>(items[5])
    }

    @Test fun aWakeUpAfterAReplyLeavesTheReplyOnShow() {
        val items = Turns.fold(listOf(
            msg(157, USER, "Generate an image", 0.0, turn = "a"),
            msg(160, TOOL, "Draw it", 16.0, turn = "a"),
            msg(164, TOOL, "threadbear: end_turn", 20.0, turn = "a"),
            msg(166, ASSISTANT, "Here's a test bear", 23.0, turn = "a"),
            msg(170, ASSISTANT, "That wait hit its limit", 56.0, turn = "b"),
            msg(171, TOOL, "threadbear: end_turn", 58.0, turn = "b"),
            msg(173, ASSISTANT, "Still waiting on you.", 60.0, turn = "b"),
        ), busy = false)
        assertEquals(listOf(157, 166, 173), replies(items))
        val worked = assertIs<ThreadItem.Worked>(items[1]).worked
        assertTrue(worked.items.any { (it as? ThreadItem.Message)?.message?.seq == 170 })
    }

    @Test fun aWakeUpsSummaryDoesntTakeTheReplysPlace() {
        val wake = listOf(
            msg(1, USER, "Fix it", 0.0, turn = "a"),
            msg(2, TOOL, "Build", 1.0, turn = "a"),
            msg(3, ASSISTANT, "- Fixed it", 2.0, summary = true, turn = "a"),
            msg(4, ASSISTANT, "- Nothing changed", 9.0, summary = true, turn = "b"),
        )
        assertEquals(listOf(1, 3, 4), replies(Turns.fold(wake, busy = false)))
        val restart = listOf(
            msg(1, USER, "Fix it", 0.0, turn = "a"),
            msg(2, TOOL, "Build", 1.0, turn = "a"),
            msg(3, ASSISTANT, "- Fixed it", 2.0, summary = true, turn = "a"),
            msg(4, USER, "The app restarted", 3.0, origin = "runner", turn = "c"),
            msg(5, ASSISTANT, "- Fixed and checked", 9.0, summary = true, turn = "c"),
        )
        assertEquals(listOf(1, 5), replies(Turns.fold(restart, busy = false)))
    }

    @Test fun rowsWithoutATurnAreOneTurn() {
        val items = Turns.fold(listOf(msg(1, USER, "Go", 0.0), msg(2, ASSISTANT, "One.", 1.0), msg(3, ASSISTANT, "Two.", 2.0)), busy = false)
        assertEquals(listOf(1, 3), replies(items))
    }

    @Test fun aQuickAnswerHasNothingToFold() {
        assertEquals(2, Turns.fold(listOf(msg(1, USER, "Hi", 0.0), msg(2, ASSISTANT, "Hello", 2.0)), busy = false).size)
    }

    @Test fun theLastShownSkipsThreadbearsOwnTools() {
        val messages = listOf(msg(1, ASSISTANT, "Done", 0.0), msg(2, TOOL, "threadbear: end_turn", 1.0))
        assertEquals(1, Turns.lastShown(messages)?.seq)
    }

    @Test fun gapsSayHowLongAndWhy() {
        assertEquals("Moments later", TurnGap.later(1.0))
        assertEquals("6 min later", TurnGap.later(360.0))
        assertEquals("2 h 5 min later", TurnGap.later(125 * 60.0))
        assertEquals("A day later", TurnGap.later(25 * 3600.0))
        assertEquals("Mine the repo finished", TurnGap.cause(listOf(WokeTask("agent", "Mine the repo"), WokeTask("shell", "npm test"))))
        assertEquals("2 background jobs finished", TurnGap.cause(listOf(WokeTask("shell", ""), WokeTask("shell", " "))))
    }
}
