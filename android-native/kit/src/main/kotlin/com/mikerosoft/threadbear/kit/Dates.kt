package com.mikerosoft.threadbear.kit

import java.time.Instant
import java.time.OffsetDateTime
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull

/**
 * Dates in Convex rows (FlexibleDate.swift): milliseconds, as Convex code
 * stores them, or ISO strings, as the runner writes them. Held to
 * JavaScript's own range (±8.64e15 ms, years -271821 to 275760), so one
 * thread's bad date can't break the inbox. Both kinds: an ISO string like
 * "+999999999-12-31T23:59:59Z" parsed, then threw in toEpochMilli() or
 * atZone() when its row was drawn (Daybreak, 11 Nov 2026). Held, not
 * refused, as the numbers are, so the row still shows.
 */
object FlexibleDate {
    private const val LIMIT = 8.64e15
    val EARLIEST: Instant = Instant.ofEpochMilli(-LIMIT.toLong())
    val LATEST: Instant = Instant.ofEpochMilli(LIMIT.toLong())

    fun read(value: JsonElement?): Instant? {
        val p = value as? JsonPrimitive ?: return null
        if (!p.isString) {
            val ms = p.doubleOrNull ?: return null
            if (!ms.isFinite()) return null
            return Instant.ofEpochMilli(ms.coerceIn(-LIMIT, LIMIT).toLong())
        }
        return parseIso(p.content)
    }

    /** Compared as Instants, before anything turns it into milliseconds or a local date. */
    fun held(date: Instant): Instant = if (date < EARLIEST) EARLIEST else if (date > LATEST) LATEST else date

    fun parseIso(text: String): Instant? {
        // Longer than any time the runner writes (an offset and nine
        // fraction digits come to 35): no reason to hand the parser more.
        if (text.length > 64) return null
        val parsed = try {
            Instant.parse(text)
        } catch (_: Exception) {
            try {
                OffsetDateTime.parse(text).toInstant()
            } catch (_: Exception) {
                null
            }
        }
        return parsed?.let(::held)
    }
}

fun JsonObject.date(key: String): Instant? = FlexibleDate.read(this[key])
fun JsonObject.needDate(key: String): Instant = date(key) ?: throw BadRow("No date $key")
