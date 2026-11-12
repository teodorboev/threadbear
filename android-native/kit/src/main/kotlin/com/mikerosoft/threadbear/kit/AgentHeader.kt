package com.mikerosoft.threadbear.kit

/**
 * The line the runner starts a message from another thread's agent with,
 * `[Sent by the agent in the thread "<title>" (<id8>)]`, or the `then` of a
 * thread's own move, `[Sent by the agent before moving to <folder>]`
 * (docs/protocol.md, "Threadbear tools"; AgentHeader.swift). The runner
 * gives these no origin, so this line is the only thing saying they aren't
 * Mike's, and the phone draws them as a card rather than his bubble, as the
 * Mac does.
 */
data class AgentHeader(
    /** The sending thread's title; empty for a move's `then`, which this thread's agent sent. */
    val title: String,
    /** The sending thread's id (its first 8 characters); empty for a move's `then`. */
    val threadId: String,
    /** For a move's `then`: the folder the thread moved to. */
    val movedTo: String?,
    /** The message without the header line. */
    val body: String,
) {
    companion object {
        private const val THREAD = "[Sent by the agent in the thread \""
        private const val MOVE = "[Sent by the agent before moving to "
        private val NEWLINES = charArrayOf('\n', '\r', '\u000B', '\u000C', '\u0085', ' ', ' ')

        fun parse(text: String): AgentHeader? {
            val lines = text.split("\n", limit = 2)
            val first = lines[0].trimSpaces()
            val body = if (lines.size > 1) lines[1].trim(*NEWLINES) else ""
            if (first.startsWith(THREAD) && first.endsWith(")]")) {
                val inner = first.substring(THREAD.length, first.length - 2)
                // The title may hold quotes and brackets; the id never holds `" (`.
                val split = inner.lastIndexOf("\" (")
                if (split < 0) return null
                return AgentHeader(inner.substring(0, split), inner.substring(split + 3), null, body)
            }
            if (first.startsWith(MOVE) && first.endsWith("]") && first.length > MOVE.length + 1) {
                return AgentHeader("", "", first.substring(MOVE.length, first.length - 1), body)
            }
            return null
        }
    }
}

/**
 * Set for a user message an agent wrote (another thread's, or this one's
 * before a move). Only for messages with no origin, as the Mac.
 */
val PhoneMessage.agentHeader: AgentHeader?
    get() = if (role == Role.USER && origin == null) AgentHeader.parse(text) else null
