package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest

class NewThreadBackendTest {
    @Test fun createSendsOnlyWhatsChosen() = runTest {
        val t = FakeTransport()
        val b = PhoneBackend(t, "tok")
        b.create("hi", null, null, emptyList(), NewThreadSettings())
        // Nothing picked: an older backend refuses fields it doesn't know.
        assertEquals(setOf("token", "text"), t.calls.last().second.keys)
        b.create("look [Picture 1]", "bruce", null, listOf("s1"), NewThreadSettings("opus", "high"))
        val args = t.calls.last().second
        assertEquals("phone:create", t.calls.last().first)
        assertEquals("""["s1"]""", args["images"].toString())
        assertEquals("\"opus\"", args["model"].toString())
        assertEquals("\"high\"", args["effort"].toString())
        assertEquals("\"bruce\"", args["runnerId"].toString())
    }

    @Test fun accountsAreLive() = runTest {
        val t = FakeTransport()
        t.subscriptions = { _, _ -> listOf("""[{"id":"claude-1","runnerId":"m5","provider":"claude","title":"me","models":[{"id":"opus","title":"Opus"}]}]""") }
        val accounts = PhoneBackend(t, "tok").accounts().first()
        assertEquals("phone:accounts", t.calls.last().first)
        assertEquals(listOf("opus"), accounts.single().models.map { it.id })
    }

    @Test fun aPhotoGoesUpToTheURLItsGiven() = runTest {
        val t = FakeTransport()
        t.mutations = { name, _ -> if (name == "phone:imageUploadUrl") "\"https://x.convex.cloud/upload?t=1\"" else "null" }
        var posted: Pair<String, Int>? = null
        val post = PhotoPost { url, jpeg, progress -> posted = url to jpeg.size; progress(0.5f); """{"storageId":"kg1"}""" }
        var heard = 0f
        val b = PhoneBackend(t, "tok")
        assertEquals("kg1", b.uploadPhoto(ByteArray(10), post) { heard = it })
        assertEquals("https://x.convex.cloud/upload?t=1" to 10, posted)
        assertEquals(0.5f, heard)
        b.discardPhoto("kg1")
        assertEquals("phone:discardImage", t.calls.last().first)
        assertEquals("\"kg1\"", t.calls.last().second["storageId"].toString())
        // An answer with no id is a failed upload, not a crash.
        assertFailsWith<TransportError.Server> { b.uploadPhoto(ByteArray(1), PhotoPost { _, _, _ -> "<html>" }) }
    }
}
