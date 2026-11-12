package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonElement

/** Answers by function name; records every call. */
class FakeTransport : ConvexTransport {
    val calls = mutableListOf<Pair<String, Map<String, JsonElement>>>()
    var queries: (String, Map<String, JsonElement>) -> String = { _, _ -> "null" }
    var mutations: (String, Map<String, JsonElement>) -> String = { _, _ -> "\"cmd1\"" }
    var subscriptions: (String, Map<String, JsonElement>) -> List<String> = { _, _ -> emptyList() }
    var subscriptionError: TransportError? = null
    override val connected: StateFlow<Boolean> = MutableStateFlow(true)

    override suspend fun query(name: String, args: Map<String, JsonElement>): String { calls += name to args; return queries(name, args) }
    override suspend fun mutation(name: String, args: Map<String, JsonElement>): String { calls += name to args; return mutations(name, args) }
    override fun subscribe(name: String, args: Map<String, JsonElement>): Flow<String> = flow {
        calls += name to args
        subscriptions(name, args).forEach { emit(it) }
        subscriptionError?.let { throw it }
    }
}

class BackendTest {
    private fun row(id: String, at: Long, extra: String = "") =
        """{"threadId":"$id","runnerId":"r","title":"$id","cwd":"/w","accountId":"a","provider":"claude","status":"idle","needsYou":false,"unread":false,"preview":"","lastActivityAt":$at$extra}"""

    @Test fun everyCallCarriesTheToken() = runTest {
        val t = FakeTransport()
        val b = PhoneBackend(t, "tb_secret")
        assertEquals("cmd1", b.send("t1", "hi"))
        assertEquals("cmd1", b.interrupt("t1"))
        assertEquals(listOf("phone:send", "phone:interrupt"), t.calls.map { it.first })
        t.calls.forEach { assertEquals("\"tb_secret\"", it.second["token"].toString()) }
        // No answers or images: an older backend refuses the fields.
        assertEquals(setOf("token", "threadId", "text"), t.calls[0].second.keys)
        b.send("t1", "yes", answers = listOf(AskAnswer("Merge?", "Merge")))
        assertEquals("""[{"action":"Merge?","answer":"Merge"}]""", t.calls.last().second["answers"].toString())
    }

    @Test fun aThreadThatIsntThereIsNull() = runTest {
        val t = FakeTransport()
        t.subscriptionError = TransportError.NotFound("No such thread")
        val values = PhoneBackend(t, "x").thread("gone").toList()
        assertEquals(listOf(null), values)
    }

    @Test fun otherErrorsEndTheFlow() = runTest {
        val t = FakeTransport()
        t.subscriptionError = TransportError.Unauthorized("expired")
        assertFailsWith<TransportError.Unauthorized> { PhoneBackend(t, "x").runners().toList() }
    }

    @Test fun convexErrorsAreSortedByKind() {
        assertIs<TransportError.Unauthorized>(TransportError.fromConvexError("""{"kind":"auth","message":"Token expired"}"""))
        assertIs<TransportError.NotFound>(TransportError.fromConvexError("""{"kind":"notFound","message":"No thread"}"""))
        val other = TransportError.fromConvexError("plain words")
        assertIs<TransportError.Server>(other)
        assertEquals("plain words", other.message)
    }

    @Test fun theInboxIsReadOnceThenFollowedByChanges() = runTest {
        val t = FakeTransport()
        t.queries = { name, _ ->
            assertEquals("phone:threadsAt", name)
            """{"at":100000,"rows":[${row("a", 1000)},${row("b", 2000)},${row("c", 3000)}]}"""
        }
        t.subscriptions = { _, args ->
            if (args["since"].toString() == "95000.0") listOf(
                """{"rows":[{"thread":0,"view":"inbox","updatedMs":101000},${row("a", 9000, ",\"view\":\"inbox\",\"updatedMs\":99000")}, ${row("b", 2000, ",\"view\":\"den\",\"updatedMs\":99000")}],"gone":["c"]}""",
            ) else emptyList()
        }
        val lists = PhoneBackend(t, "x").inboxChanges().toList()
        // The first read as the server ordered it; changes newest first.
        assertEquals(listOf(listOf("a", "b", "c"), listOf("a")), lists.map { l -> l.map { it.threadId } })
        // It asked from a margin before what it had read.
        assertEquals(listOf("phone:threadsAt", "phone:threadsSince"), t.calls.map { it.first })
        assertNull(t.calls[1].second["view"])
    }

    @Test fun aWaitingListAndCancel() = runTest {
        val t = FakeTransport()
        t.subscriptions = { _, _ -> listOf("""[{"commandId":"c1","state":"pending","text":"hi","photos":0,"createdAt":5},{"commandId":"c2","state":"weird","text":"x","photos":0,"createdAt":6}]""") }
        t.mutations = { _, _ -> "true" }
        val b = PhoneBackend(t, "x")
        assertEquals(listOf("c1"), b.waiting("t").toList().single().map { it.commandId })
        assertEquals(true, b.cancel("c1"))
    }
}

class BackendPicturesAndHistoryTest {
    @Test fun lateImagesAndLoadEarlierGoToTheirFunctions() = runTest {
        val t = FakeTransport()
        t.subscriptions = { _, _ -> listOf("""[{"seq":922,"url":"https://x/922.png"},{"seq":"bad","url":"x"}]""") }
        val b = PhoneBackend(t, "x")
        assertEquals(listOf(922), b.images("t1", 921).toList().single().map { it.seq })
        assertEquals("phone:images", t.calls.last().first)
        assertEquals("921", t.calls.last().second["from"].toString())
        assertEquals("cmd1", b.loadEarlier("t1", 40))
        assertEquals("phone:loadEarlier", t.calls.last().first)
        assertEquals("40", t.calls.last().second["beforeSeq"].toString())
    }
}
