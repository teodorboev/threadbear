package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** CardMentionsTests.swift. */
class CardMentionsTest {
    @Test fun onlyNotionLinksAreCards() {
        val text = "Look at [Fix \\[the\\] button](https://app.notion.com/p/abc123) and [Docs](https://example.com/x)"
        val found = CardMentions.find(text)
        assertEquals(listOf("Fix [the] button"), found.map { it.title })
        assertEquals("https://app.notion.com/p/abc123", found.first().url)
        assertEquals(text.indexOf("[Fix"), found.first().start)
        assertEquals(text.indexOf(" and"), found.first().end)
        assertTrue(CardMentions.find("![a picture](https://www.notion.so/t3)").isEmpty())
        assertTrue(CardMentions.find("[c](https://notion.so.evil.com/t3) [d](http://www.notion.so/t3)").isEmpty())
    }

    @Test fun threadLinksAreChipsToo() {
        val found = CardMentions.find("See [Fix the chip](thread:0cf430ac-f429) and [x](thread:a b)")
        assertEquals(listOf("Fix the chip"), found.map { it.title })
        assertTrue(found.first().isThread)
    }

    @Test fun aPastedThreadIdGoesInAsALink() {
        val id = "0cf430ac-f429-4b1e-9d8a-1c2b3d4e5f60"
        val known = setOf(id)
        assertEquals(id, CardMentions.pastedThreadId("  $id\n", known::contains))
        assertEquals(id, CardMentions.pastedThreadId("thread:$id", known::contains))
        assertEquals(id, CardMentions.pastedThreadId(id.uppercase(), known::contains))
        // Not one of his threads, or not only an id: the words as they are.
        assertNull(CardMentions.pastedThreadId("11111111-2222-3333-4444-555555555555", known::contains))
        assertNull(CardMentions.pastedThreadId("see $id", known::contains))
        val link = CardMentions.threadReference(id, "Fix [the] chip")
        assertEquals("[Fix \\[the\\] chip](thread:$id)", link)
        assertEquals("Fix [the] chip", CardMentions.find(link).first().title)
        assertEquals("[Thread](thread:$id)", CardMentions.threadReference(id, ""))
    }

    // Not in the Swift suite: Daybreak's 7 Nov case, many `[` with no `](`, stays quick.
    @Test fun manyOpenBracketsStayQuick() {
        val start = System.nanoTime()
        CardMentions.find("[".repeat(32_000) + " notion.")
        CardMentions.find("[a](https://notion.so/" + "[a](".repeat(8_000))
        assertTrue((System.nanoTime() - start) / 1e9 < 2)
    }
}
