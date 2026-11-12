package com.mikerosoft.threadbear.kit

/**
 * Mike's answer to a review (show_options on the Mac), read back out of the
 * message the runner sent for it (`reviewReply` in runner/src/reviews.ts),
 * so the bubble says what he picked rather than the review's id and the
 * wording the agent gets (ReviewAnswer.swift). The Mac does the same with
 * the review's id and draft from the event; the mirror only has the text, so
 * this goes by the runner's wording alone, and anything that doesn't fit it
 * is shown as it is.
 */
data class ReviewAnswer(
    /** The review's title. */
    val title: String,
    val lines: List<Line>,
) {
    /** `label`: small and quiet, over the line: what it's about. `text`: what he picked or wrote. */
    data class Line(val label: String, val text: String)

    companion object {
        /**
         * The runner's last paragraph names the options he passed over, for
         * the agent writing follow-ups (`PASSED_OVER_TAIL` in
         * runner/src/reviews.ts); his bubble leaves it out.
         */
        const val PASSED_OVER_TAIL = "Don't offer one of these later (a next step, a follow-up) as if it were new: say it's one they passed over."

        private val HEAD = Regex("""^Review: (.+?) \(([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\)(?:\n\n|$)""")

        // The id can't run past the first ". " (and below, the first " ("):
        // the plain lazy `.+?` tried every one of them with all that came
        // after, quadratic on a long line. Same matches.
        private val PICKED = Regex("""^Picked (.(?:(?!\. ).)*?)\. (.+?)\.?$""")
        private val NOTE = Regex("""^Notes on (.(?:(?! \().)*?) \((.+?)\): ([\s\S]+)$""")

        /**
         * "Review: <title> (<uuid>)", then "Picked <id>. <option>." (or
         * "Picked A + D. <option> + <option>.", or "Picked none of them."),
         * then his reply as he wrote it, then "Notes on <id> (<option>): <note>"
         * paragraphs (`reviewReply` in runner/src/reviews.ts).
         */
        fun parse(message: String): ReviewAnswer? {
            val text = withoutPassedOver(message)
            val head = HEAD.find(text) ?: return null
            val title = head.groupValues[1]
            var paragraphs = text.substring(head.range.last + 1).split("\n\n")
            if (paragraphs.lastOrNull() == "") paragraphs = paragraphs.dropLast(1)
            val lines = mutableListOf<Line>()
            paragraphs.firstOrNull()?.let { first ->
                if (first == "Picked none of them.") {
                    lines += Line(title, "None of them")
                    paragraphs = paragraphs.drop(1)
                } else {
                    val picked = PICKED.find(first)
                    if (picked != null) {
                        val id = picked.groupValues[1]
                        lines += Line(title, "Picked $id: ${bare(picked.groupValues[2], id)}")
                        paragraphs = paragraphs.drop(1)
                    }
                }
            }
            // Everything up to the first note is his reply, paragraphs and all.
            val reply = paragraphs.takeWhile { !it.startsWith("Notes on ") }
            if (reply.isNotEmpty()) {
                lines += Line(if (lines.isEmpty()) title else "", reply.joinToString("\n\n"))
                paragraphs = paragraphs.drop(reply.size)
            }
            // Paragraphs of a note stay with it: a new one starts only at "Notes on ".
            val notes = mutableListOf<String>()
            for (paragraph in paragraphs) {
                if (paragraph.startsWith("Notes on ") || notes.isEmpty()) notes += paragraph else notes[notes.size - 1] += "\n\n" + paragraph
            }
            for (part in notes) {
                val note = NOTE.find(part) ?: return null
                val id = note.groupValues[1]
                // With nothing picked, the review's title goes over the first note.
                lines += Line((if (lines.isEmpty()) title + "\n" else "") + "Notes on $id: ${bare(note.groupValues[2], id)}", note.groupValues[3])
            }
            return if (lines.isEmpty()) null else ReviewAnswer(title, lines)
        }

        internal fun withoutPassedOver(text: String): String {
            if (!text.endsWith(PASSED_OVER_TAIL)) return text
            val start = text.lastIndexOf("\n\nPassed over: ")
            return if (start < 0) text else text.substring(0, start)
        }

        /** Agents often title options "A: Summary first", which would read "Picked A: A: Summary first". */
        internal fun bare(option: String, id: String): String {
            for (sep in listOf(": ", ". ", " - ", ") ")) {
                if (option.startsWith(id + sep) && option.length > id.length + sep.length) return option.substring(id.length + sep.length)
            }
            return option
        }
    }
}
