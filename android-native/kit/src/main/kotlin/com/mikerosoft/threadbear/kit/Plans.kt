package com.mikerosoft.threadbear.kit

/** A Plan first plan on the phone (backend API 72; Plans.swift): its title, the card it waits on, and the file it's shared as. */
object Plans {
    /** The tool Claude puts its plan to Mike with. Its approval is the plan's Go ahead (allow) and Send back (deny). */
    const val TOOL_NAME = "ExitPlanMode"

    /** Whether a thread's open card is a plan waiting for Go ahead. */
    fun waiting(pending: PendingApproval?): Boolean = pending is PendingApproval.Approval && pending.name == TOOL_NAME

    /**
     * Whether this plan can be sent back from its sheet: it's the newest
     * here, and no waiting card names a different one (whose row hasn't come
     * yet: a message now would send that one back, Codex review, 11 Nov).
     */
    fun canSendBack(plan: PhoneMessage, pending: PendingApproval?, messages: List<PhoneMessage>): Boolean {
        if (latest(messages)?.seq != plan.seq) return false
        if (waiting(pending)) {
            val planSeq = (pending as PendingApproval.Approval).planSeq
            if (planSeq != null) return planSeq == plan.seq
        }
        return true
    }

    /**
     * The plan a waiting card is for: the row its planSeq names, never just
     * the newest plan, as the card can go up before its plan's row and an
     * older plan's Go ahead would approve this one (Codex review, 11 Nov).
     * Null until that row is here, and for a card from an older runner.
     */
    fun plan(pending: PendingApproval?, messages: List<PhoneMessage>): PhoneMessage? {
        if (!waiting(pending)) return null
        val planSeq = (pending as PendingApproval.Approval).planSeq ?: return null
        return messages.lastOrNull { it.plan && it.seq == planSeq }
    }

    /** Whether that card's Go ahead can be given from the phone: the runner offers only Deny there when the line it shows isn't all of it. */
    fun canGoAhead(pending: PendingApproval?): Boolean =
        waiting(pending) && ApprovalOption.ALLOW in (pending as PendingApproval.Approval).options

    /** The newest plan in a thread's messages. */
    fun latest(messages: List<PhoneMessage>): PhoneMessage? = messages.lastOrNull { it.plan }

    /** The plan's first heading, else its first line, without Markdown marks. */
    fun title(text: String): String {
        val lines = text.split('\n').filter { it.isNotEmpty() }.map { it.trim { c -> c == ' ' || c == '\t' } }
        val line = lines.firstOrNull { it.startsWith("#") } ?: lines.firstOrNull { it.isNotEmpty() } ?: ""
        val bare = line.dropWhile { it == '#' }.trim { it == ' ' || it == '\t' }.replace("**", "").replace("`", "")
        return if (bare.isEmpty()) "Plan" else bare.take(120)
    }

    /** The plan without its title line, for the card's few lines under it. */
    fun body(text: String): String {
        val lines = text.split('\n').toMutableList()
        val first = lines.indexOfFirst { it.isNotBlank() }
        if (first >= 0 && lines[first].trim().startsWith("#")) repeat(first + 1) { lines.removeAt(0) }
        return lines.joinToString("\n").trim()
    }

    /** A file name for sharing: the title in lowercase words, `plan.md` if there are none. */
    fun fileName(text: String): String {
        val words = title(text).lowercase().split(Regex("[^\\p{L}\\p{N}]+")).filter { it.isNotEmpty() }
        val slug = words.take(8).joinToString("-")
        return (slug.ifEmpty { "plan" }) + ".md"
    }
}
