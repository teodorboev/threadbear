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

    // Daybreak findings 129 and 133: a web address written as a picture is a
    // link to open, never a picture the app fetches. Only the thread's own
    // stored pictures (image) are drawn.
    @Test fun aWebPictureIsOnlyALink() {
        assertEquals("x.io", ThreadPictures.webLink("https://x.io/a.png")?.let { URI(it).host })
        assertEquals("192.168.1.1", ThreadPictures.webLink("http://192.168.1.1/admin?x=1")?.let { URI(it).host })
        assertNull(ThreadPictures.webLink("/Users/m/a.png"))
        assertNull(ThreadPictures.webLink("file:///etc/passwd"))
        assertNull(ThreadPictures.webLink("https://"))
        val pictures = ThreadPictures(listOf(MessageImage(seq = 3, src = "/a.png", url = "https://store/a")))
        assertNull(pictures.image("https://127.0.0.1/a.png", 3))
        assertNull(pictures.image("https://attacker.example/a.png", 3))
    }

    // Daybreak finding 130, by pins and pasted photos rather than Markdown: a
    // runner key can attach hundreds of stored pictures to one message.
    @Test fun oneMessageShowsTenPinnedOrPastedPicturesAtMost() {
        val pins = (0 until 300).map { MessageImage(seq = 1, pinned = true, url = "https://store/pin-$it") }
        val late = (0 until 300).map { MessageImage(seq = 9, pinned = true, url = "https://store/late-$it") }
        val pasted = (1..300).map { MessageImage(seq = 2, src = "picture:$it", url = "https://store/p-$it") }
        val pictures = ThreadPictures(pins + late + pasted)
        val placed = pictures.pinned(listOf(reply(1)))
        assertEquals(MarkdownBlock.MAX_PICTURES, placed.under[1]?.size)
        assertEquals("https://store/pin-0", placed.under[1]?.first()?.url)
        assertEquals(MarkdownBlock.MAX_PICTURES, placed.trailing.size)
        assertEquals((1..MarkdownBlock.MAX_PICTURES).map { "https://store/p-$it" }, pictures.pasted(2).map { it.url })
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
