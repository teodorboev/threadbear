package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.serialization.json.JsonObject

class PlansTest {
    private fun m(seq: Int, role: Role, text: String, at: Double, plan: Boolean = false) =
        PhoneMessage(seq, role, text, toolKind = if (role == Role.TOOL) "command" else null, at = T0.plusMillis((at * 1000).toLong()), plan = plan)

    private fun card(name: String = "ExitPlanMode", options: List<ApprovalOption> = listOf(ApprovalOption.ALLOW, ApprovalOption.DENY), planSeq: Int? = null) =
        PendingApproval.Approval("a", name, "Finish the plan", options, null, planSeq)

    @Test fun titleIsTheFirstHeadingWithoutItsMarks() {
        assertEquals("Bots and webhook triggers", Plans.title("# Bots and **webhook** triggers\n\nSteps"))
        assertEquals("Just do x", Plans.title("Just do `x`\nthen y"))
        assertEquals("Plan", Plans.title("  \n"))
        assertEquals("## Context\nWhy", Plans.body("\n# Title\n\n## Context\nWhy"))
        assertEquals("No heading\nmore", Plans.body("No heading\nmore"))
    }

    @Test fun fileNameIsTheTitleInWords() {
        assertEquals("bots-webhook-triggers-for-threadbear.md", Plans.fileName("# Bots & webhook triggers for Threadbear"))
        assertEquals("plan.md", Plans.fileName("# !!!"))
    }

    @Test fun onlyAnExitPlanModeCardWaitsForGoAhead() {
        assertTrue(Plans.waiting(card()))
        assertFalse(Plans.waiting(card(name = "Bash", options = listOf(ApprovalOption.ALLOW))))
        assertFalse(Plans.waiting(null))
        // Go ahead only where the runner offers Allow on it.
        assertTrue(Plans.canGoAhead(card()))
        assertFalse(Plans.canGoAhead(card(options = listOf(ApprovalOption.DENY))))
        assertFalse(Plans.canGoAhead(card(name = "Bash", options = listOf(ApprovalOption.ALLOW))))
    }

    /** Codex review, 11 Nov: the card for plan B can reach the phone before B's row, and pairing it with the newest plan showed A with B's Go ahead. */
    @Test fun aCardPairsOnlyWithThePlanItNames() {
        val messages = listOf(m(3, Role.ASSISTANT, "# A", 0.0, plan = true), m(9, Role.ASSISTANT, "# B", 5.0, plan = true))
        assertEquals("# B", Plans.plan(card(planSeq = 9), messages)?.text)
        assertNull(Plans.plan(card(planSeq = 12), messages), "its row isn't here yet")
        assertNull(Plans.plan(card(planSeq = null), messages), "an older runner's card names none")
        assertNull(Plans.plan(card(name = "Bash", options = listOf(ApprovalOption.ALLOW), planSeq = 9), messages))
        val row = """{"approvalId": "a", "kind": "approval", "name": "ExitPlanMode", "title": "Finish the plan", "options": ["deny"], "planSeq": 9.0}"""
        val decoded = PendingApproval.read(parseJson(row) as JsonObject)
        assertEquals(9, Plans.plan(decoded, messages)?.seq)
        // Sent back only from the newest plan, and not while a card names another.
        assertTrue(Plans.canSendBack(messages[1], card(planSeq = 9), messages))
        assertFalse(Plans.canSendBack(messages[1], card(planSeq = 12), messages))
        assertFalse(Plans.canSendBack(messages[0], null, messages))
        assertTrue(Plans.canSendBack(messages[1], null, messages))
    }

    @Test fun rowsCarryThePlanMark() {
        val row = """{"seq": 4.0, "role": "assistant", "text": "# P", "plan": true, "at": "2026-11-10T00:00:00.000Z"}"""
        assertTrue(PhoneMessage.read(parseJson(row) as JsonObject).plan)
        val old = """{"seq": 4.0, "role": "assistant", "text": "x", "at": "2026-11-10T00:00:00.000Z"}"""
        assertFalse(PhoneMessage.read(parseJson(old) as JsonObject).plan)
    }

    /** A plan sent back, then the agent's reply: the plan stays in view, not folded into the turn's working with the steps around it. */
    @Test fun aFinishedTurnKeepsItsPlanInView() {
        val messages = listOf(
            m(1, Role.USER, "Plan the bots", 0.0),
            m(2, Role.TOOL, "Read the schema", 5.0),
            m(3, Role.ASSISTANT, "# Bots\n\n1. Add a table", 10.0, plan = true),
            m(4, Role.TOOL, "Finish the plan", 11.0),
            m(5, Role.ASSISTANT, "Waiting on your go.", 12.0),
        )
        val shown = Turns.fold(messages, busy = false).mapNotNull { (it as? ThreadItem.Message)?.message?.seq }
        assertEquals(listOf(1, 3, 5), shown)
        assertEquals(3, Plans.latest(messages)?.seq)
    }
}
