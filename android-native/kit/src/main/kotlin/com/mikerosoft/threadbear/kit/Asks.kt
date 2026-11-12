package com.mikerosoft.threadbear.kit

/** An ask answered, as phone:send takes it. */
data class AskAnswer(val action: String, val answer: String) {
    companion object {
        /** The message the Mac sends (Decisions.message): each picked ask, then the rest. */
        fun message(asks: List<PhoneAsk>, picks: Map<Int, Int>, extra: String = ""): String {
            val parts = answers(asks, picks).map { (i, a) ->
                if (asks.size == 1) "${a.action}\n${a.answer}"
                else "${i + 1}. ${a.action}\n   " + a.answer.replace("\n", "\n   ")
            }.toMutableList()
            val more = extra.trim()
            if (more.isNotEmpty()) parts += more
            return parts.joinToString("\n\n")
        }

        /** Follow-ups ticked on the done card: one on its own, several as a list. */
        fun followUpMessage(followUps: List<String>, extra: String = ""): String {
            var body = if (followUps.size == 1) followUps[0] else followUps.joinToString("\n") { "- $it" }
            val more = extra.trim()
            if (more.isNotEmpty()) body += (if (body.isEmpty()) "" else "\n\n") + more
            return body
        }

        fun answers(asks: List<PhoneAsk>, picks: Map<Int, Int>): List<Pair<Int, AskAnswer>> =
            asks.withIndex().mapNotNull { (i, ask) ->
                val n = picks[i] ?: return@mapNotNull null
                val choice = ask.choices.getOrNull(n) ?: return@mapNotNull null
                i to AskAnswer(ask.action, choice)
            }
    }
}

/** The answers map for a question card, as the Mac builds it (Answers in Backend.swift). */
object Answers {
    fun build(questions: List<Question>, chosen: Map<Int, Set<String>>, typed: String): Map<String, String> {
        val out = LinkedHashMap<String, String>()
        val t = typed.trim()
        val last = questions.size - 1
        for ((index, q) in questions.withIndex()) {
            var parts = q.options.map { it.label }.filter { chosen[index]?.contains(it) == true }
            if (t.isNotEmpty() && index == last) parts = if (q.multiSelect) parts + t else listOf(t)
            if (parts.isNotEmpty()) out[q.question] = parts.joinToString(", ")
        }
        return out
    }

    /** One line for the transcript: "Which database? Convex · Tests? Yes". */
    fun summary(questions: List<Question>, answers: Map<String, String>): String =
        questions.mapNotNull { q -> answers[q.question]?.let { if (questions.size == 1) it else "${q.header ?: q.question} $it" } }
            .joinToString(" · ")
}
