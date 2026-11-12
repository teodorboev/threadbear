package com.mikerosoft.threadbear.kit

import java.text.Normalizer

/**
 * What a search found, under the same headings as the inbox (ThreadSearch
 * in Inbox.swift), the put-away threads too. `olderArchived` is what
 * phone:search found past the den's newest 100: matched by Convex on whole
 * words, so kept as it comes, after the archived threads the phone has.
 */
data class ThreadSearch(
    val starred: List<PhoneThread> = emptyList(),
    val inbox: List<PhoneThread> = emptyList(),
    val running: List<PhoneThread> = emptyList(),
    /** The scheduled threads and the snoozed ones, as the Hibernating heading has them. */
    val hibernating: List<PhoneThread> = emptyList(),
    val archived: List<PhoneThread> = emptyList(),
) {
    val isEmpty get() = starred.isEmpty() && inbox.isEmpty() && running.isEmpty() && hibernating.isEmpty() && archived.isEmpty()

    companion object {
        fun of(
            query: String,
            sections: InboxSections,
            hibernating: List<PhoneThread>,
            archived: List<PhoneThread>,
            olderArchived: List<PhoneThread> = emptyList(),
            wakes: Map<String, ScheduleWake> = emptyMap(),
        ): ThreadSearch {
            val q = fold(query.trim())
            // A thread can sit in two lists for a moment while the runner moves
            // it (snoozed but not yet out of the inbox view): show it once.
            val seen = HashSet<String>()
            fun keep(threads: List<PhoneThread>) = threads.filter { matchesFolded(it, q) && seen.add(it.threadId) }
            val starred = keep(sections.starred)
            val inbox = keep(sections.inbox)
            val running = keep(sections.running)
            val asleep = InboxSections.hibernating(keep(sections.scheduled), keep(hibernating), wakes)
            val den = keep(archived) + olderArchived.filter { seen.add(it.threadId) }
            return ThreadSearch(starred, inbox, running, asleep, den)
        }

        /**
         * Part of the title or the last reply, as on the Mac, or the project
         * or folder, which on the phone's narrow rows is often all you can
         * remember a thread by. Case and accents don't matter.
         */
        fun matches(thread: PhoneThread, query: String) = matchesFolded(thread, fold(query.trim()))

        private fun matchesFolded(thread: PhoneThread, q: String) =
            listOf(thread.title, thread.preview, thread.placeText).any { fold(it).contains(q) }

        private val marks = Regex("\\p{M}+")

        /** Lower case with the accents off, so "séttings" finds "Settings". */
        private fun fold(text: String) = marks.replace(Normalizer.normalize(text, Normalizer.Form.NFD), "").lowercase()
    }
}
