package com.mikerosoft.threadbear.kit

import java.time.Instant

val NOW: Instant = Instant.parse("2026-11-03T04:00:00Z")
val T0: Instant = Instant.ofEpochSecond(1_800_000_000)

fun thread(
    id: String,
    minutesAgo: Long,
    status: ThreadStatus = ThreadStatus.IDLE,
    needsYou: Boolean = false,
    approval: Boolean = false,
    snoozed: Boolean = false,
    working: String? = null,
) = PhoneThread(
    threadId = id, title = id, status = status, needsYou = needsYou, unread = needsYou,
    lastActivityAt = NOW.minusSeconds(minutesAgo * 60),
    snoozedUntil = if (snoozed) NOW.plusSeconds(600) else null, working = working,
    pendingApproval = if (approval) PendingApproval.Approval("x", "", "", listOf(ApprovalOption.ALLOW), null) else null,
)

fun msg(seq: Int, role: Role, text: String, at: Double, origin: String? = null, summary: Boolean = false, turn: String? = null) =
    PhoneMessage(seq, role, text, origin, if (role == Role.TOOL) "command" else null, summary, turn, at = T0.plusMillis((at * 1000).toLong()))
