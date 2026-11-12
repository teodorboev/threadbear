package com.mikerosoft.threadbear.kit

import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** AgentHeader.swift has no suite of its own; these hold its two forms. */
class AgentHeaderTest {
    private fun user(text: String, origin: String? = null) =
        PhoneMessage(seq = 1, role = Role.USER, text = text, origin = origin, at = Instant.ofEpochSecond(0))

    @Test fun anotherThreadsAgentWithQuotesInItsTitle() {
        val header = user("[Sent by the agent in the thread \"Fix \"the\" (chip)\" (0cf430ac)]\nCan you look at this?\n").agentHeader
        assertEquals(AgentHeader("Fix \"the\" (chip)", "0cf430ac", null, "Can you look at this?"), header)
    }

    @Test fun aMovesThen() {
        val header = AgentHeader.parse("[Sent by the agent before moving to /Users/m/dev/me/threadbear]\n\nCarry on with the tests")
        assertEquals(AgentHeader("", "", "/Users/m/dev/me/threadbear", "Carry on with the tests"), header)
        assertNull(AgentHeader.parse("[Sent by the agent before moving to ]"))
    }

    @Test fun mikesOwnWordsHaveNone() {
        assertNull(user("Just a message").agentHeader)
        assertNull(user("[Sent by the agent in the thread \"x\" (abc)]", origin = "phone").agentHeader)
        assertNull(PhoneMessage(seq = 1, role = Role.ASSISTANT, text = "[Sent by the agent in the thread \"x\" (abc)]", at = Instant.ofEpochSecond(0)).agentHeader)
        assertEquals("abc", user("[Sent by the agent in the thread \"x\" (abc)]").agentHeader?.threadId)
    }
}
