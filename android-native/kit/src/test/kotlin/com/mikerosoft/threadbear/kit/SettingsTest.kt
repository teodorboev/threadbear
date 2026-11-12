package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.test.runTest

class SettingsTest {
    private fun runner(id: String, secondsAgo: Long = 10, placed: Boolean? = null) =
        PhoneRunner(id, id.uppercase(), lastSeenAt = NOW.minusSeconds(secondsAgo), placed = placed)

    @Test fun notifyModesReadAndSayTheMacWords() {
        assertEquals(NotifyMode.ALWAYS, NotifyMode.of("always"))
        // Nothing saved yet, or a word this build doesn't know: away.
        assertEquals(NotifyMode.AWAY, NotifyMode.of(null))
        assertEquals(NotifyMode.AWAY, NotifyMode.of("sometimes"))
        assertEquals(listOf("When I'm away from the Mac", "Always", "Never"), NotifyMode.entries.map { it.label })
    }

    @Test fun aDraggedOrderHoldsUntilTheBackendAgrees() {
        val list = listOf(runner("m5"), runner("bruce"), runner("cloud"))
        val dragged = listOf("cloud", "m5", "bruce")
        // A heartbeat from before the backend had it: still the dragged order.
        val early = RunnerOrder.receive(list, dragged)
        assertFalse(early.caughtUp)
        assertEquals(dragged, early.runners.map { it.runnerId })
        assertTrue(early.runners.all { it.placed == true })
        // Back in that order: the backend's list, and the pending order let go.
        val agreed = RunnerOrder.receive(dragged.map { runner(it) }, dragged)
        assertTrue(agreed.caughtUp)
        assertEquals(dragged, agreed.runners.map { it.runnerId })
    }

    @Test fun aNewRunnerMeanwhileGoesAtTheEnd() {
        val list = listOf(runner("pixel"), runner("m5"), runner("bruce"))
        val got = RunnerOrder.receive(list, listOf("bruce", "m5"))
        assertEquals(listOf("bruce", "m5", "pixel"), got.runners.map { it.runnerId })
        assertNull(got.runners.last().placed)
        // A runner gone from the list doesn't hold it up.
        assertTrue(RunnerOrder.receive(listOf(runner("m5")), listOf("bruce", "m5")).caughtUp)
    }

    @Test fun noDragIsTheBackendsList() {
        val list = listOf(runner("b"), runner("a"))
        assertEquals(list, RunnerOrder.receive(list, null).runners)
        assertEquals(listOf(true, true), RunnerOrder.placed(list).map { it.placed })
    }

    @Test fun offlineOnlyOnceEveryRunnerIs() {
        assertNull(RunnerOrder.offlineText(emptyList(), NOW))
        assertNull(RunnerOrder.offlineText(listOf(runner("m5", 600), runner("bruce")), NOW))
        val one = RunnerOrder.offlineText(listOf(runner("m5", 600)), NOW)!!
        assertTrue(one.startsWith("Mac offline since "), one)
        val two = RunnerOrder.offlineText(listOf(runner("m5", 900), runner("bruce", 600)), NOW)!!
        assertTrue(two.startsWith("BRUCE offline since "), two)
        assertEquals("Online", RunnerOrder.status(runner("m5"), NOW))
        assertTrue(RunnerOrder.status(runner("m5", 600), NOW).startsWith("Last seen "))
    }

    @Test fun settingsCallsCarryTheirArguments() = runTest {
        val t = FakeTransport()
        t.mutations = { _, _ -> "null" }
        val b = PhoneBackend(t, "x")
        b.setNotifications("tok", NotifyMode.OFF)
        b.setRunnerOrder(listOf("a", "b"))
        assertEquals(listOf("phone:setNotifications", "phone:setRunnerOrder"), t.calls.map { it.first })
        assertEquals("\"off\"", t.calls[0].second["notify"].toString())
        assertEquals("\"tok\"", t.calls[0].second["apnsToken"].toString())
        assertEquals("""["a","b"]""", t.calls[1].second["runnerIds"].toString())
    }
}
