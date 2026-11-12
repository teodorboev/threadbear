package com.mikerosoft.threadbear.kit

import java.time.Instant
import kotlin.math.roundToLong

/** A thread as shown: messages, with the tool calls between two of them gathered. */
sealed class ThreadItem {
    abstract val id: Long

    data class Message(val message: PhoneMessage) : ThreadItem() {
        override val id get() = message.seq.toLong()
    }

    data class Tools(val items: List<PhoneMessage>) : ThreadItem() {
        override val id get() = (items.firstOrNull()?.seq ?: 0).toLong()
    }

    /** A finished turn's work, folded into one "Worked for 2m 13s" line. */
    data class Worked(val worked: WorkedTurn) : ThreadItem() {
        override val id get() = -(worked.items.firstOrNull()?.id ?: 0L) - 1
    }

    /** "6 min later · Mine the repo finished" before a turn the agent started itself. */
    data class Gap(val gap: TurnGap) : ThreadItem() {
        override val id get() = -gap.seq.toLong() - 1_000_000_000L
    }

    companion object {
        fun group(messages: List<PhoneMessage>): List<ThreadItem> {
            val out = mutableListOf<ThreadItem>()
            for (m in messages) {
                val last = out.lastOrNull()
                if (m.role == Role.TOOL) {
                    if (last is Tools) out[out.size - 1] = Tools(last.items + m) else out += Tools(listOf(m))
                } else {
                    out += Message(m)
                }
            }
            return out
        }
    }
}

/** A finished turn's working: the messages and tool lines before its reply. */
data class WorkedTurn(val items: List<ThreadItem>, val durationSeconds: Double?) {
    val title: String
        get() {
            val d = durationSeconds
            if (d == null || d < 1) return "Worked"
            val s = d.saturatingInt()
            if (s < 60) return "Worked for ${s}s"
            if (s < 3600) return if (s % 60 == 0) "Worked for ${s / 60}m" else "Worked for ${s / 60}m ${s % 60}s"
            return if ((s % 3600) / 60 == 0) "Worked for ${s / 3600}h" else "Worked for ${s / 3600}h ${(s % 3600) / 60}m"
        }
}

/** The wait before a turn the agent started by itself (TurnGap.swift). */
data class TurnGap(val seq: Int, val from: Instant, val to: Instant, val wokeBy: List<WokeTask> = emptyList()) {
    val label: String
        get() {
            val time = later(maxOf(0.0, (to.toEpochMilli() - from.toEpochMilli()) / 1000.0))
            val cause = cause(wokeBy) ?: return time
            return "$time · $cause"
        }

    companion object {
        fun later(interval: Double): String {
            val seconds = interval.roundToLong()
            if (seconds < 60) return if (seconds < 2) "Moments later" else "${seconds}s later"
            val minutes = (seconds / 60.0).roundToLong()
            if (minutes < 60) return "$minutes min later"
            val hours = minutes / 60
            val rest = minutes % 60
            if (hours < 24) return if (rest == 0L) "$hours h later" else "$hours h $rest min later"
            val days = hours / 24
            return if (days == 1L) "A day later" else "$days days later"
        }

        fun cause(tasks: List<WokeTask>): String? {
            if (tasks.isEmpty()) return null
            val agents = tasks.filter { it.kind == "agent" }
            val shown = agents.ifEmpty { tasks }
            val names = shown.map { it.description.trim() }.filter { it.isNotEmpty() }
            val noun = if (agents.isEmpty()) "background job" else "sub-agent"
            return when {
                names.isEmpty() && shown.size == 1 -> "A $noun finished"
                names.isEmpty() -> "${shown.size} ${noun}s finished"
                names.size == 1 && shown.size == 1 -> "${names[0]} finished"
                names.size == 2 && shown.size == 2 -> "${names[0]} and ${names[1]} finished"
                else -> "${names[0]} and ${shown.size - 1} more finished"
            }
        }
    }
}

/** A thread as the Mac shows it once its turns are over (Turns.swift). */
object Turns {
    fun fold(messages: List<PhoneMessage>, busy: Boolean): List<ThreadItem> {
        val turns = mutableListOf(mutableListOf<PhoneMessage>())
        for (m in messages) {
            if (m.role == Role.USER && m.origin != "runner" && turns.last().isNotEmpty()) turns += mutableListOf<PhoneMessage>()
            turns.last() += m
        }
        val out = mutableListOf<ThreadItem>()
        for ((i, turn) in turns.withIndex()) {
            if (turn.isEmpty()) continue
            val starter = turn.first().takeIf { it.role == Role.USER && it.origin != "runner" }
            val body = if (starter == null) turn else turn.drop(1)
            if (starter != null) out += ThreadItem.Message(starter)
            if (busy && i == turns.size - 1) { out += shownWhole(body); continue }
            // A plan stays out of the fold wherever it is in the turn: it's
            // what Go ahead approves, and a sent-back one is what he asked
            // to change.
            val replyOnly = replies(body)
            val reply = if (replyOnly.isEmpty()) emptyList() else (replyOnly.toSet() + body.indices.filter { body[it].plan }).toList()
            if (reply.isEmpty()) { out += shownWhole(body); continue }
            val replySet = reply.toSet()
            val folded = body.filterIndexed { k, _ -> k !in replySet }
            if (folded.isNotEmpty() && folded.any { it.role == Role.TOOL || it.role == Role.ASSISTANT }) {
                val end = body[reply.max()].at
                val duration = starter?.let { (end.toEpochMilli() - it.at.toEpochMilli()) / 1000.0 }
                out += ThreadItem.Worked(WorkedTurn(ThreadItem.group(folded), duration))
            } else {
                out += ThreadItem.group(folded)
            }
            val walls = dividers(body, replySet)
            for (r in reply.sorted()) {
                val k = walls.keys.filter { it <= r }.maxOrNull()
                val gap = k?.let { walls[it] }
                if (gap != null && ThreadItem.Gap(gap) !in out) out += ThreadItem.Gap(gap)
                out += ThreadItem.Message(body[r])
            }
        }
        return out
    }

    internal fun shownWhole(body: List<PhoneMessage>): List<ThreadItem> {
        val walls = dividers(body, body.indices.toSet())
        val out = mutableListOf<ThreadItem>()
        var start = 0
        for (k in walls.keys.sorted()) {
            out += ThreadItem.group(body.subList(start, k))
            out += ThreadItem.Gap(walls.getValue(k))
            start = k
        }
        return out + ThreadItem.group(body.subList(start, body.size))
    }

    internal fun waits(body: List<PhoneMessage>): Map<Int, TurnGap> {
        val out = HashMap<Int, TurnGap>()
        var i = 1
        while (i < body.size) {
            val starts = body[i].turn != null && body[i].turn != body[i - 1].turn && body[i].role != Role.USER
            if (!starts) { i++; continue }
            var end = i + 1
            while (end < body.size && body[end].turn == body[i].turn && body[end].role != Role.USER) end++
            val woke = body.subList(i, end).firstNotNullOfOrNull { it.wokeBy } ?: emptyList()
            out[i] = TurnGap(body[i].seq, body[i - 1].at, body[i].at, woke)
            i = end
        }
        return out
    }

    internal fun dividers(body: List<PhoneMessage>, shown: Set<Int>): Map<Int, TurnGap> {
        val gaps = waits(body)
        val out = HashMap<Int, TurnGap>()
        var shownBefore = false
        var waiting: Pair<Int, TurnGap>? = null
        for (k in body.indices) {
            val gap = gaps[k]
            if (gap != null && shownBefore) {
                val earlier = waiting?.second
                waiting = if (earlier != null) k to TurnGap(gap.seq, earlier.from, gap.to, earlier.wokeBy + gap.wokeBy) else k to gap
            }
            if (k !in shown || body[k].role == Role.USER) continue
            waiting?.let { out[it.first] = it.second }
            waiting = null
            shownBefore = true
        }
        return out
    }

    internal fun replies(body: List<PhoneMessage>): List<Int> {
        val turns = mutableListOf<IntRange>()
        var start = 0
        for (i in 1 until body.size) {
            val newTurn = body[i].turn != null && body[i].turn != body[i - 1].turn
            if (body[i].role == Role.USER || newTurn) {
                turns += start until i
                start = i
            }
        }
        if (body.isNotEmpty()) turns += start until body.size
        val covers = turns.indexOfLast { turn ->
            val wakeUp = turn.first > 0 && body[turn.first].role != Role.USER
            !wakeUp && turn.any { body[it].role == Role.ASSISTANT && body[it].summary }
        }
        return turns.drop(if (covers < 0) 0 else covers).flatMap { turn ->
            replyIndexes(body.subList(turn.first, turn.last + 1)).map { it + turn.first }
        }
    }

    internal fun replyIndexes(body: List<PhoneMessage>): List<Int> {
        val summary = body.indexOfLast { it.role == Role.ASSISTANT && it.summary }
        val lastStep = body.indexOfLast(::isStep)
        if (summary >= 0 && lastStep < summary) {
            val words = ((lastStep + 1) until summary).filter { body[it].role == Role.ASSISTANT && !body[it].summary }
            val length = words.sumOf { body[it].text.length }
            return if (length > body[summary].text.length) words else listOf(summary)
        }
        val last = body.indexOfLast { it.role == Role.ASSISTANT }
        return if (last >= 0) listOf(last) else emptyList()
    }

    /** The last message the thread view shows: not one of Threadbear's own tool rows. */
    fun lastShown(messages: List<PhoneMessage>): PhoneMessage? = messages.lastOrNull { it.role != Role.TOOL || isStep(it) }

    internal fun isStep(m: PhoneMessage): Boolean = when (m.role) {
        Role.USER -> true
        Role.ASSISTANT -> false
        Role.TOOL -> !(m.text.startsWith("threadbear: ") || m.text.startsWith("Find tools: "))
    }
}
