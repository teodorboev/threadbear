package com.mikerosoft.threadbear.kit

import com.mikerosoft.threadbear.kit.DraftBlock.Span
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * ```slack, ```email and ```draft blocks (DraftBlockTests.swift): text for
 * Mike to send elsewhere, shown line for line with a Copy button. The Mac's
 * DraftBlockTests and the website's draft.test.ts check the same cases.
 */
class DraftBlockTest {
    @Test fun onlyTheDraftFencesCount() {
        assertEquals(DraftBlock.Kind.SLACK, DraftBlock.from("slack", "hi\n")?.kind)
        assertEquals(DraftBlock.Kind.EMAIL, DraftBlock.from("Email subject=x", "hi")?.kind)
        assertEquals(DraftBlock.Kind.DRAFT, DraftBlock.from("draft", "hi")?.kind)
        assertNull(DraftBlock.from("slackbot", "hi"))
        assertNull(DraftBlock.from(null, "hi"))
        assertNull(DraftBlock.from("slack", " \n\n"))
        assertEquals("a\nb", DraftBlock.from("slack", "a\r\nb\n\n")?.source)
        assertEquals("Slack draft", DraftBlock(DraftBlock.Kind.SLACK, "x").label)
        assertEquals("Email draft", DraftBlock(DraftBlock.Kind.EMAIL, "x").label)
        assertEquals("Draft", DraftBlock(DraftBlock.Kind.DRAFT, "x").label)
    }

    @Test fun everyLineStaysALine() {
        val draft = DraftBlock(DraftBlock.Kind.DRAFT, "*Threadbear, 6 Nov*\n• one\n• two\n\nThanks")
        assertEquals(listOf("*Threadbear, 6 Nov*", "• one", "• two", "", "Thanks"), draft.lines.map { l -> l.spans.joinToString("") { it.text } })
        // Only a Slack draft reads Slack's formatting.
        assertFalse(draft.lines[0].spans[0].bold)
        assertEquals(draft.source, draft.plainText)
    }

    @Test fun slackFormatting() {
        assertEquals(
            listOf(
                Span("Shipped", bold = true), Span(" "), Span("today", italic = true), Span(": "),
                Span("old", strike = true), Span(" "), Span("tb land", code = true), Span(" and "),
                Span("both", bold = true, italic = true),
            ),
            DraftBlock.slackSpans("*Shipped* _today_: ~old~ `tb land` and *_both_*"),
        )
    }

    @Test fun markersInsideWordsAndLoneOnesStayAsWritten() {
        for (text in listOf("2*3*4", "snake_case_name", "a * b * c", "*open only", "price: $5_000_")) {
            assertEquals(listOf(Span(text)), DraftBlock.slackSpans(text), text)
        }
    }

    @Test fun markdownBoldIsSlackBoldWithAStarEitherSide() {
        // Markdown's **bold** is Slack's *bold* with a star either side,
        // which is how Slack shows it too.
        assertEquals(listOf(Span("*"), Span("x", bold = true), Span("*")), DraftBlock.slackSpans("**x**"))
    }

    @Test fun linksAndMentions() {
        val spans = DraftBlock.slackSpans("See <https://example.com/a?b=1|the docs>, <https://x.dev> or <mailto:m@x.dev>; cc <@U123> in <#C9|general> <!here> & 1 < 2 &amp; <b>")
        assertEquals(listOf("https://example.com/a?b=1", "https://x.dev", "mailto:m@x.dev"), spans.mapNotNull { it.link })
        assertEquals("See the docs, https://x.dev or m@x.dev; cc @U123 in #general @here & 1 < 2 & <b>", spans.joinToString("") { it.text })
        // Other schemes aren't links.
        assertNull(DraftBlock.slackSpans("<javascript:alert(1)|x>").first().link)
        assertEquals("<javascript:alert(1)|x>", DraftBlock.slackSpans("<javascript:alert(1)|x>").joinToString("") { it.text })
    }

    @Test fun copyKeepsSlacksMarkersButSpellsOutLinks() {
        val draft = DraftBlock(DraftBlock.Kind.SLACK, "*Done* <https://x.dev/pr/1|the PR>\n<https://x.dev>\n> quoted &lt;tag&gt;")
        assertEquals("*Done* the PR (https://x.dev/pr/1)\nhttps://x.dev\n> quoted <tag>", draft.plainText)
        assertEquals(
            "<meta charset=\"utf-8\"><div><b>Done</b> <a href=\"https://x.dev/pr/1\">the PR</a><br><a href=\"https://x.dev\">https://x.dev</a><blockquote>quoted &lt;tag&gt;</blockquote></div>",
            draft.html,
        )
        assertTrue(draft.lines[2].quote)
    }

    @Test fun htmlIsEscaped() {
        val draft = DraftBlock(DraftBlock.Kind.EMAIL, "Hi <script>alert(\"x\")</script> & bye")
        assertEquals("<meta charset=\"utf-8\"><div>Hi &lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; bye</div>", draft.html)
    }

    // Cross-lab review, 6 Nov.
    @Test fun tokensInsideCodeStayAsWrittenAndALinkCanBeBold() {
        assertEquals(listOf(Span("<@U123>", code = true), Span(" and "), Span("a < b", code = true)), DraftBlock.slackSpans("`<@U123>` and `a < b`"))
        val url = "https://x.dev"
        assertEquals(listOf(Span("See ", bold = true), Span("docs", bold = true, link = url)), DraftBlock.slackSpans("*See <https://x.dev|docs>*"))
        assertEquals(listOf(Span("a", link = url), Span("b", italic = true)), DraftBlock.slackSpans("<https://x.dev|a>_b_"))
        // Copied as plain text, code keeps its tokens as typed too.
        assertEquals(
            "`<@U1>` @U1 `<https://x.dev|docs>` docs (https://x.dev)",
            DraftBlock(DraftBlock.Kind.SLACK, "`<@U1>` <@U1> `<https://x.dev|docs>` <https://x.dev|docs>").plainText,
        )
    }

    @Test fun hostileLinesStayQuick() {
        val start = System.nanoTime()
        DraftBlock.slackSpans("<".repeat(DraftBlock.MAX_FORMATTED_LINE))
        DraftBlock.slackSpans(" *a".repeat(DraftBlock.MAX_FORMATTED_LINE / 3))
        DraftBlock(DraftBlock.Kind.SLACK, "<".repeat(DraftBlock.MAX_SOURCE)).plainText
        DraftBlock(DraftBlock.Kind.SLACK, "<".repeat(DraftBlock.MAX_SOURCE - 1) + ">").plainText
        assertTrue((System.nanoTime() - start) / 1e9 < 2)
        // A line past the limit is shown as written; a draft past its limit stays code.
        val long = "*" + "a".repeat(DraftBlock.MAX_FORMATTED_LINE) + "*"
        assertEquals(listOf(Span(long)), DraftBlock.slackSpans(long))
        assertNull(DraftBlock.from("slack", "a".repeat(DraftBlock.MAX_SOURCE + 1)))
        // A placeholder character in the text itself turns formatting off for the line.
        assertEquals(listOf(Span("*a* ")), DraftBlock.slackSpans("*a* "))
    }

    @Test fun copiedHtmlKeepsIndentsAndLinedUpSpaces() {
        val draft = DraftBlock(DraftBlock.Kind.EMAIL, "    • nested\nA    B")
        assertEquals("<meta charset=\"utf-8\"><div>&nbsp;&nbsp;&nbsp;&nbsp;• nested<br>A &nbsp;&nbsp;&nbsp;B</div>", draft.html)
    }
}
