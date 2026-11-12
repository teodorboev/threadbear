package com.mikerosoft.threadbear.kit

import java.time.Instant
import java.time.ZoneId
import java.time.ZoneOffset
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * Daybreak, 11 Nov 2026 (findings 99 and 100): ISO strings parsed past the
 * range numbers were held to, then threw in toEpochMilli() (a thread's
 * lastActivityAt, RelativeTime.short) or atZone() (a schedule's nextRunAt,
 * RelativeTime.clock) while the inbox drew its rows. A secondary runner can
 * write either into its own thread or schedule.
 */
class DateRangeTest {
    private val zones = listOf(ZoneOffset.UTC, ZoneId.of("America/Los_Angeles"), ZoneId.of("Asia/Tokyo"), ZoneId.of("Pacific/Kiritimati"))
    private val far = listOf(
        "+999999999-12-31T23:59:59Z",
        "+1000000000-06-01T00:00:00Z",
        "-999999999-01-01T00:00:00Z",
        "-1000000000-01-01T00:00:00Z",
        "+999999999-12-31T23:59:59+18:00",
        "-999999999-01-01T00:00:00-18:00",
        "275761-01-01T00:00:00Z",
    )

    @Test fun isoDatesAreHeldToTheSameRangeAsNumbers() {
        for (text in far) {
            val date = FlexibleDate.read(JsonPrimitive(text)) ?: continue
            assertTrue(date >= FlexibleDate.EARLIEST && date <= FlexibleDate.LATEST, "$text read as $date")
            date.toEpochMilli()
            for (zone in zones) date.atZone(zone)
        }
        assertEquals(FlexibleDate.LATEST, FlexibleDate.read(JsonPrimitive("+999999999-12-31T23:59:59Z")))
        assertEquals(FlexibleDate.EARLIEST, FlexibleDate.read(JsonPrimitive("-999999999-01-01T00:00:00Z")))
        assertEquals(FlexibleDate.LATEST, FlexibleDate.read(JsonPrimitive(8.64e15)))
        assertEquals(FlexibleDate.LATEST, FlexibleDate.read(JsonPrimitive(1e300)))
        // Ordinary ones are as written.
        assertEquals(Instant.parse("2026-11-11T03:06:54.388Z"), FlexibleDate.read(JsonPrimitive("2026-11-11T03:06:54.388Z")))
        assertEquals(Instant.parse("2026-11-11T02:06:54Z"), FlexibleDate.read(JsonPrimitive("2026-11-11T03:06:54+01:00")))
        assertNull(FlexibleDate.read(JsonPrimitive("2026-11-11T03:06:54" + "0".repeat(100) + "Z")))
    }

    // The row a scoped runner could write (finding 99): idle, so its trailing
    // word is the time, lastActivityAt far past what milliseconds can hold.
    @Test fun aFarLastActivityStillDrawsItsRow() {
        for (text in far) {
            val list = rows(threadRow("t", lastActivityAt = text))
            val t = list.singleOrNull() ?: continue // refused is fine too; throwing isn't
            assertEquals(RowStatus.Kind.TIME, RowStatus.kind(t))
            for (zone in zones) {
                assertTrue(RowStatus.text(t, NOW, zone).isNotEmpty())
                InboxSections.build(list, NOW)
            }
        }
    }

    // Finding 100: a schedule's nextRunAt, drawn by RelativeTime.clock.
    @Test fun aFarScheduleStillDrawsItsClock() {
        for (text in far) {
            val wake = ScheduleWake.read(buildJsonObject { put("scheduleId", "s"); put("threadId", "t"); put("nextRunAt", text) })
            val at = wake.nextRunAt ?: continue
            val wakes = InboxSections.wakes(listOf(wake))
            InboxSections.build(listOf(thread("t", 1)), NOW, wakes)
            for (zone in zones) assertTrue(RelativeTime.clock(at, NOW, zone).isNotEmpty(), text)
        }
    }

    // Whatever made the Instant: the formatters hold it themselves.
    @Test fun theFormattersNeverThrowOnAnyInstant() {
        for (date in listOf(Instant.MAX, Instant.MIN, Instant.EPOCH)) {
            for (zone in zones) {
                RelativeTime.short(date, NOW, zone)
                RelativeTime.clock(date, NOW, zone)
                RelativeTime.short(NOW, date, zone)
                RelativeTime.clock(NOW, date, zone)
            }
        }
        assertEquals("now", RelativeTime.short(Instant.MAX, NOW, ZoneOffset.UTC))
        assertNotNull(RelativeTime.short(Instant.MIN, NOW, ZoneOffset.UTC))
    }

    private fun rows(vararg o: JsonObject) = JsonArray(o.toList()).lossyList(PhoneThread::read)

    private fun threadRow(id: String, lastActivityAt: String): JsonObject = Json.parseToJsonElement(
        """{"threadId":"$id","runnerId":"r","title":"t","cwd":"/x","accountId":"a","provider":"claude","status":"idle",
           "needsYou":false,"unread":false,"preview":"","lastActivityAt":"$lastActivityAt","starredAt":"2026-11-01T00:00:00Z"}""",
    ) as JsonObject
}
