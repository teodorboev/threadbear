package com.mikerosoft.threadbear.kit

import java.time.Instant

/** Messages sent from the phone that the runner hasn't applied yet (WaitingSends.swift). */
object WaitingSends {
    /**
     * The ones to show under the transcript, oldest first. A taken one stays
     * until its message is in the transcript, then drops out.
     */
    fun shown(waiting: List<WaitingSend>, messages: List<PhoneMessage>): List<WaitingSend> {
        val arrived = messages.filter { it.role == Role.USER && it.origin == "phone" }.toMutableList()
        return waiting.sortedBy { it.createdAt }.filter { send ->
            if (send.state != CommandState.TAKEN) return@filter true
            val sentAt = Instant.ofEpochMilli(send.createdAt.toLong()).minusSeconds(5)
            val words = mirrored(send)
            val i = arrived.indexOfFirst { it.at >= sentAt && it.text == words }
            if (i < 0) return@filter true
            arrived.removeAt(i)
            false
        }
    }

    /** The send's words as the transcript will have them. */
    fun mirrored(send: WaitingSend): String {
        val count = InlinePictures.unplaced(send.text, send.photos)
        if (count <= 0) return send.text
        val note = if (count == 1) "[1 picture]" else "[$count pictures]"
        return if (send.text.isEmpty()) note else "${send.text}\n\n$note"
    }

    fun note(send: WaitingSend, offlineRunner: String?): String =
        if (send.state == CommandState.PENDING && offlineRunner != null) "$offlineRunner is offline, sends when it's back" else "Sending…"

    fun offlineRunner(runnerId: String, runners: List<PhoneRunner>, now: Instant): String? {
        val runner = runners.firstOrNull { it.runnerId == runnerId } ?: return null
        return if (runner.isOnline(now)) null else runner.name
    }
}
