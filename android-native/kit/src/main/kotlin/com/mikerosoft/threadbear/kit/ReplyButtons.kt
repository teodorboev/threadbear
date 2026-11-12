package com.mikerosoft.threadbear.kit

/**
 * An agent's `[title](button:<id>)` links drawn as buttons (ReplyButtons.swift,
 * docs/protocol.md "Reply buttons", backend API 59): blue starts a thread,
 * purple sends a message, green with a tick once pressed. The web page's
 * reply-buttons.ts says the same; keep them alike. A row carries what a
 * button says and where it goes (MessageButton), never the brief or message
 * behind it, which stay on the Mac.
 */
object ReplyButtons {
    data class Link(val id: String, val title: String)
    data class Trailing(val text: String, val links: List<Link>)

    private val trailingLink = Regex("""(?s)(.*?)[\s,;·|]*\[([^\]\n]+)\]\(button:([a-z0-9]{8})\)[\s.,;·|]*""")

    /**
     * A line's button links at its end, peeled off so they can be drawn as
     * real buttons after its words ("- Pebble form due. [Fill it in](button:x)").
     * A link with words after it stays in the text, where it's drawn as a link.
     */
    fun trailing(text: String): Trailing {
        var rest = text
        val links = mutableListOf<Link>()
        while (true) {
            val match = trailingLink.matchEntire(rest) ?: break
            links.add(0, Link(match.groupValues[3], match.groupValues[2]))
            rest = match.groupValues[1]
        }
        return Trailing(rest.trim { it == ' ' || it == '\t' }, links)
    }

    private val anyButton = Regex("""\[([^\]\n]+)\]\(button:([^)\s]*)\)""")

    /**
     * The text with every button link whose id isn't in `except` as just its
     * title: an older runner's row, or a button the thread no longer keeps.
     */
    fun unlinking(text: String, except: Set<String>): String {
        if (!text.contains("](button:")) return text
        return anyButton.replace(text) { if (it.groupValues[2] in except) it.value else it.groupValues[1] }
    }

    sealed class Look {
        data object Ready : Look()
        data object Pressing : Look()
        data class Pressed(val threadId: String?) : Look()
        data class Failed(val note: String) : Look()
    }

    /**
     * What a button shows: the row's state, with this phone's own press on
     * top until the row catches up. `asking`: a press is on its way to the
     * backend, or its command hasn't finished.
     */
    fun look(button: MessageButton, status: CommandStatus?, asking: Boolean): Look {
        button.pressedThreadId?.let { return Look.Pressed(it) }
        if (status?.state == CommandState.DONE) return Look.Pressed(status.threadId)
        if (asking || status?.state == CommandState.PENDING || status?.state == CommandState.TAKEN) return Look.Pressing
        if (status?.state == CommandState.FAILED) return Look.Failed(status.note ?: "The runner couldn't press it.")
        button.failed?.let { return Look.Failed(it) }
        return Look.Ready
    }

    /** What a screen reader says, and where it goes. */
    fun hint(button: MessageButton): String = when (button.kind) {
        "start" -> button.target?.let { "Starts a thread in $it" } ?: "Starts a thread"
        else -> button.target?.let { "Sends a message to $it" } ?: "Sends a message"
    }
}
