package com.mikerosoft.threadbear.kit

import com.mikerosoft.threadbear.kit.ReviewAnswer.Line
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** ReviewAnswerTests.swift. */
class ReviewAnswerTest {
    private val id = "74d1e391-cbc6-4b23-bd6b-cfed7cb79488"

    @Test fun aPickShowsTheOptionNotTheId() {
        val answer = ReviewAnswer.parse("Review: End of turn: summary + what's next ($id)\n\nPicked A. A: summary is the agent's last message.")
        assertEquals("End of turn: summary + what's next", answer?.title)
        assertEquals(listOf(Line("End of turn: summary + what's next", "Picked A: summary is the agent's last message")), answer?.lines)
    }

    // The runner names the options he passed over for the agent; his bubble doesn't.
    @Test fun whatHePassedOverIsLeftOut() {
        val text = "Review: Run commands ($id)\n\nPicked A. Small play button.\n\nNotes on A (Small play button): Nice\n\nPassed over: B. Run card. ${ReviewAnswer.PASSED_OVER_TAIL}"
        assertEquals(
            listOf(Line("Run commands", "Picked A: Small play button"), Line("Notes on A: Small play button", "Nice")),
            ReviewAnswer.parse(text)?.lines,
        )
    }

    @Test fun notesKeepTheirParagraphsAndBrackets() {
        val text = "Review: Picker ($id)\n\nPicked B. One picker.\n\nNotes on A (Auto (smart)): Too clever.\n\nSecond thought.\n\nNotes on B (One picker): Ship it"
        assertEquals(
            listOf(
                Line("Picker", "Picked B: One picker"),
                Line("Notes on A: Auto (smart)", "Too clever.\n\nSecond thought."),
                Line("Notes on B: One picker", "Ship it"),
            ),
            ReviewAnswer.parse(text)?.lines,
        )
    }

    @Test fun onlyNotesPutTheTitleOverTheFirst() {
        assertEquals(listOf(Line("Picker\nNotes on D: Moving", "Keep this")), ReviewAnswer.parse("Review: Picker ($id)\n\nNotes on D (Moving): Keep this")?.lines)
    }

    @Test fun otherMessagesAreLeftAlone() {
        assertNull(ReviewAnswer.parse("Review: this PR please"))
        assertNull(ReviewAnswer.parse("Review: Picker (not-a-uuid)\n\nPicked A. Thing."))
        assertNull(ReviewAnswer.parse("Review: Picker ($id)"))
    }

    // Since 4 Nov a review takes several picks, none, and a reply in his words.
    @Test fun severalPicksAndAReply() {
        val text = "Review: Diagrams ($id)\n\nPicked A + D. Entity board + Sandbox.\n\nThe board for the app.\n\nThe sandbox for the video.\n\nNotes on C (Map): Too busy"
        assertEquals(
            listOf(
                Line("Diagrams", "Picked A + D: Entity board + Sandbox"),
                Line("", "The board for the app.\n\nThe sandbox for the video."),
                Line("Notes on C: Map", "Too busy"),
            ),
            ReviewAnswer.parse(text)?.lines,
        )
    }

    @Test fun noneOrOnlyAReply() {
        assertEquals(listOf(Line("Diagrams", "None of them")), ReviewAnswer.parse("Review: Diagrams ($id)\n\nPicked none of them.")?.lines)
        assertEquals(listOf(Line("Diagrams", "What are these for?")), ReviewAnswer.parse("Review: Diagrams ($id)\n\nWhat are these for?")?.lines)
    }
}
