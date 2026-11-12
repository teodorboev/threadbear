package com.mikerosoft.threadbear.kit

import java.net.URI
import java.time.Instant
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** The ThreadPicturesTests suite in MarkdownBlocksTests.swift. */
class ThreadPicturesTest {
    private val at = Instant.ofEpochSecond(0)
    private fun reply(seq: Int): ThreadItem = ThreadItem.Message(PhoneMessage(seq = seq, role = Role.ASSISTANT, text = "", at = at))

    @Test fun aMessagesOwnPictureComesFirst() {
        val old = MessageImage(seq = 5, src = "/a.png", url = "https://x/old")
        val new = MessageImage(seq = 9, src = "/a.png", url = "https://x/new")
        val pictures = ThreadPictures(listOf(old, new, MessageImage(seq = 9, src = "/b.png", url = "https://x/b")))
        assertEquals(old, pictures.image("/a.png", 5))
        assertEquals(new, pictures.image("/a.png", 9))
        assertEquals(old, pictures.image("/a.png", 7))
        assertEquals(new, pictures.image("/a.png", 1))
        assertEquals(new, pictures.image("/a.png", null))
        assertNull(pictures.image("/missing.png", 9))
    }

    @Test fun onlyAWebPictureNeedsNoUpload() {
        assertEquals("x.io", ThreadPictures.webUrl("https://x.io/a.png")?.let { URI(it).host })
        assertNull(ThreadPictures.webUrl("/Users/m/a.png"))
        assertNull(ThreadPictures.webUrl("http://x.io/a.png"))
    }

    @Test fun aPinGoesUnderTheNextReplyOnShow() {
        val first = MessageImage(seq = 3, pinned = true, url = "https://x/first")
        val second = MessageImage(seq = 8, pinned = true, url = "https://x/second")
        val notYet = MessageImage(seq = 20, pinned = true, url = "https://x/late")
        // A shared thread's tool picture is neither, and doesn't show.
        val tool = MessageImage(seq = 4, url = "https://x/tool")
        val items = listOf(
            ThreadItem.Message(PhoneMessage(seq = 1, role = Role.USER, text = "", at = at)),
            ThreadItem.Tools(listOf(PhoneMessage(seq = 3, role = Role.TOOL, text = "", at = at))),
            reply(5),
            ThreadItem.Tools(listOf(PhoneMessage(seq = 8, role = Role.TOOL, text = "", at = at))),
            reply(10),
        )
        val placed = ThreadPictures(listOf(first, tool, second, notYet)).pinned(items)
        assertEquals(mapOf(5 to listOf(first), 10 to listOf(second)), placed.under)
        assertEquals(listOf(notYet), placed.trailing)
    }

    @Test fun aPageFromAnOlderBackendHasNoPictures() {
        assertEquals(emptyList(), MessagePage.read(parseJson("""{"messages":[],"hasMore":false}""") as JsonObject).images)
        val with = """{"messages":[],"hasMore":false,"images":[{"seq":4,"src":"/a.png","url":"https://x/a","width":400,"height":300,"somethingNew":1}]}"""
        assertEquals(
            MessageImage(seq = 4, src = "/a.png", url = "https://x/a", width = 400, height = 300),
            MessagePage.read(parseJson(with) as JsonObject).images.first(),
        )
    }
}
