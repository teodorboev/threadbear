package com.mikerosoft.threadbear.kit

import com.mikerosoft.threadbear.kit.MarkdownBlock.Alignment
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** MarkdownBlocksTests.swift. */
class MarkdownBlocksTest {
    private fun parse(text: String) = MarkdownBlock.parse(text)
    private fun p(text: String) = MarkdownBlock.Paragraph(text)
    private fun li(text: String, marker: String = "•", depth: Int = 0, task: Boolean? = null) = MarkdownBlock.ListItem(text, marker, depth, task)
    private fun code(text: String) = MarkdownBlock.Code(text)
    private fun img(alt: String, src: String) = MarkdownBlock.Image(alt, src)
    private fun draft(kind: DraftBlock.Kind, source: String) = MarkdownBlock.Draft(DraftBlock(kind, source))
    private fun rich(kind: MarkdownBlock.RichKind, source: String) = MarkdownBlock.Rich(kind, source)
    private val slack = DraftBlock.Kind.SLACK

    @Test fun aPictureOnItsOwnLineOrAmongWords() {
        assertEquals(listOf(img("Back screws", "/Users/m/guide/back.png")), parse("![Back screws](/Users/m/guide/back.png)"))
        assertEquals(listOf(p("Before"), img("a", "~/a b.png"), p("after")), parse("Before ![a](<~/a b.png> \"title\") after"))
    }

    @Test fun aPictureInAListGoesUnderItsItem() {
        val text = """
            - First ![a](/a.png)
            - ![b](/b.png)
            - Third

              ![c](/c.png)
        """.trimIndent()
        assertEquals(listOf(li("First"), img("a", "/a.png"), img("b", "/b.png"), li("Third"), img("c", "/c.png")), parse(text))
    }

    @Test fun aPictureBeingTalkedAboutStaysWords() {
        val inSpan = "Write `![alt](/a.png)` like that"
        assertEquals(listOf(p(inSpan)), parse(inSpan))
        assertEquals(listOf(code("![alt](/a.png)")), parse("```\n![alt](/a.png)\n```"))
        assertEquals(listOf(p("A [link](/a.png) isn't one")), parse("A [link](/a.png) isn't one"))
    }

    @Test fun mermaidAndHtmlFencesAreRichBlocks() {
        assertEquals(listOf(rich(MarkdownBlock.RichKind.MERMAID, "flowchart TD\n  A --> B")), parse("```mermaid\nflowchart TD\n  A --> B\n```"))
        assertEquals(listOf(rich(MarkdownBlock.RichKind.HTML, "<b>hi</b>")), parse("~~~ HTML title=x\n<b>hi</b>\n~~~"))
        assertEquals(listOf(rich(MarkdownBlock.RichKind.MERMAID, "pie")), parse("```Mermaid\npie\n```"))
        // Anything else, or a word that only starts the same, stays code.
        assertEquals(listOf(code("let a = 1")), parse("```swift\nlet a = 1\n```"))
        assertEquals(listOf(code("<b>")), parse("```htmlx\n<b>\n```"))
        assertEquals(listOf(code("mermaid")), parse("```\nmermaid\n```"))
    }

    @Test fun slackEmailAndDraftFencesAreDrafts() {
        // Every line kept, blank ones too: none of it is joined up.
        assertEquals(
            listOf(p("Here it is:"), draft(slack, "*Update*\n• one\n• two\n\nThanks"), p("Send it?")),
            parse("Here it is:\n\n```slack\n*Update*\n• one\n• two\n\nThanks\n```\nSend it?"),
        )
        assertEquals(listOf(draft(DraftBlock.Kind.EMAIL, "Hi,\nBye")), parse("~~~Email to=x\nHi,\nBye\n~~~"))
        assertEquals(listOf(draft(DraftBlock.Kind.DRAFT, "hi")), parse("```draft\nhi\n```"))
        // Still streaming in, empty, or only starting the same: code.
        assertEquals(listOf(code("*Update*\n• one")), parse("```slack\n*Update*\n• one"))
        assertEquals(listOf(code("")), parse("```slack\n\n```"))
        assertEquals(listOf(code("hi")), parse("```slackbot\nhi\n```"))
    }

    @Test fun aDraftNeedsItsOwnClosingFence() {
        // Only a fence on its own closes it, at any indent.
        assertEquals(listOf(code("hello\n> ```")), parse("```slack\nhello\n> ```"))
        assertEquals(listOf(draft(slack, "hello")), parse("```slack\nhello\n   ```"))
        assertEquals(listOf(draft(slack, "hello")), parse("```slack\nhello\n    ```"))
        assertEquals(listOf(code("hello\n``` x")), parse("```slack\nhello\n``` x"))
        // A fence opening on a list item's own line: the block, in the item.
        assertEquals(listOf(draft(slack, "hello")), parse("- ```slack\n  hello\n  ```"))
        assertEquals(listOf(li("Run:"), code("tb land")), parse("- Run:\n1. ```sh\n   tb land\n   ```"))
        // A different fence character, or a shorter run, doesn't close it.
        assertEquals(listOf(code("hello\n~~~")), parse("```slack\nhello\n~~~"))
        assertEquals(listOf(code("hello\n```")), parse("````slack\nhello\n```"))
        assertEquals(listOf(draft(slack, "hello")), parse("````slack\nhello\n````"))
        assertEquals(listOf(draft(slack, "hi\nthere")), parse("```slack\r\nhi\r\nthere\r\n```\r\n"))
        assertEquals(listOf(MarkdownBlock.Quote(listOf(draft(slack, "hello")))), parse("> ```slack\n> hello\n> ```"))
        // Too big to be a message: code.
        val big = "a".repeat(DraftBlock.MAX_SOURCE + 1)
        assertEquals(listOf(code(big)), parse("```slack\n$big\n```"))
    }

    // Cross-lab review, round three (6 Nov).
    @Test fun backticksAfterABacktickFenceMakeItWords() {
        assertEquals(listOf(li("```literal```"), li("next")), parse("- ```literal```\n- next"))
        assertEquals(listOf(p("```a``` b next")), parse("```a``` b\nnext"))
        // A tilde fence's info string may have backticks.
        assertEquals(listOf(draft(slack, "hi")), parse("~~~slack `x`\nhi\n~~~"))
    }

    @Test fun aClosingFenceClosesAtAnyIndent() {
        assertEquals(listOf(draft(slack, "hello")), parse("   ```slack\n   hello\n    ```"))
        assertEquals(listOf(draft(slack, "hello")), parse("  ```slack\n  hello\n```"))
        assertEquals(listOf(draft(slack, "hello")), parse("- ```slack\n  hello\n```"))
        assertEquals(listOf(draft(slack, "hello")), parse("- ```slack\n  hello\n     ```"))
        assertEquals(listOf(li("Steps:"), draft(slack, "hello")), parse("- Steps:\n\n  ```slack\n  hello\n  ```"))
        assertEquals(listOf(li("a"), li("b", depth = 1), code("code")), parse("- a\n  - b\n\n    ```\n    code\n    ```"))
        // Round four: a reply's words after the block stay words, under a
        // nested list or after a heading that ended one.
        assertEquals(
            listOf(li("root"), li("middle", depth = 1), li("child", depth = 2), code("let x = 1"), p("After.")),
            parse("- root\n  - middle\n    - child\n\n    ```swift\n    let x = 1\n    ```\n\nAfter."),
        )
        assertEquals(
            listOf(li("item"), MarkdownBlock.Heading(1, "Heading"), code("let x = 1"), p("After.")),
            parse("- item\n\n# Heading\n\n  ```swift\n  let x = 1\n```\n\nAfter."),
        )
    }

    @Test fun aFencedItemKeepsTheListsDepths() {
        assertEquals(listOf(code("echo hi"), li("child", depth = 1), li("sibling")), parse("- ```sh\n  echo hi\n  ```\n  - child\n- sibling"))
    }

    @Test fun ordinaryListsAndCodeAreAsBefore() {
        assertEquals(listOf(li("one"), li("two", depth = 1), li("three")), parse("- one\n  - two\n- three"))
        assertEquals(listOf(code("let a = 1"), p("after")), parse("```swift\nlet a = 1\n```\nafter"))
        assertEquals(listOf(code("```\ninner\n```")), parse("````\n```\ninner\n```\n````"))
        assertEquals(listOf(p("Use `a` and ``b``")), parse("Use `a` and ``b``"))
    }

    @Test fun aFenceStillStreamingInOrTooBigStaysCode() {
        // The reply so far, its fence not closed yet: drawn once it is.
        assertEquals(listOf(code("flowchart TD\n  A -->")), parse("```mermaid\nflowchart TD\n  A -->"))
        val big = "a".repeat(MarkdownBlock.MAX_RICH_SOURCE + 1)
        assertEquals(listOf(code(big)), parse("```html\n$big\n```"))
        assertEquals(listOf(code("")), parse("```mermaid\n\n```"))
    }

    @Test fun aRichBlockInAListKeepsItsSource() {
        assertEquals(
            listOf(li("Steps:"), rich(MarkdownBlock.RichKind.MERMAID, "flowchart LR\n  A --> B")),
            parse("- Steps:\n\n  ```mermaid\n  flowchart LR\n    A --> B\n  ```"),
        )
    }

    @Test fun aPictureInAQuoteStaysInTheQuote() {
        assertEquals(listOf(MarkdownBlock.Quote(listOf(img("a", "/a.png")))), parse("> ![a](/a.png)"))
    }

    @Test fun tablesBecomeRowsAndCells() {
        val blocks = parse(
            """
            Here's the spread:

            | Option | Cost | Notes |
            |:--|:-:|--:|
            | **A** | $5 | fast \| cheap |
            | B | $9 |
            """.trimIndent(),
        )
        assertEquals(
            listOf(
                p("Here's the spread:"),
                MarkdownBlock.Table(
                    listOf("Option", "Cost", "Notes"), listOf(Alignment.LEADING, Alignment.CENTER, Alignment.TRAILING),
                    listOf(listOf("**A**", "$5", "fast | cheap"), listOf("B", "$9", "")),
                ),
            ),
            blocks,
        )
    }

    @Test fun aPipeInProseIsNotATable() {
        assertEquals(listOf(p("Use a | b here and carry on")), parse("Use a | b here\nand carry on"))
    }

    @Test fun nestedListsKeepTheirDepth() {
        val blocks = parse(
            """
            - one
              - one a
                - one a i
            - two
            1. first
            2) second
            + plus
            """.trimIndent(),
        )
        assertEquals(
            listOf(
                li("one"), li("one a", depth = 1), li("one a i", depth = 2), li("two"),
                li("first", marker = "1."), li("second", marker = "2."), li("plus"),
            ),
            blocks,
        )
    }

    @Test fun aWrappedListItemStaysOneItem() {
        val blocks = parse(
            """
            - **Tables** show up as raw
              pipes on the phone.
            - Quotes too.

              A second paragraph in it.

            After the list.
            """.trimIndent(),
        )
        assertEquals(
            listOf(li("**Tables** show up as raw pipes on the phone."), li("Quotes too.\n\nA second paragraph in it."), p("After the list.")),
            blocks,
        )
    }

    @Test fun taskListsHaveTheirBoxes() {
        assertEquals(listOf(li("todo", task = false), li("done", task = true)), parse("- [ ] todo\n- [x] done"))
    }

    @Test fun quotesRulesAndHeadings() {
        val blocks = parse(
            """
            ## What I found ##
            > It's **broken**
            > - on the phone
            ---
            * * *
            Done
            """.trimIndent(),
        )
        assertEquals(
            listOf(
                MarkdownBlock.Heading(2, "What I found"),
                MarkdownBlock.Quote(listOf(p("It's **broken**"), li("on the phone"))),
                MarkdownBlock.Rule,
                MarkdownBlock.Rule,
                p("Done"),
            ),
            blocks,
        )
    }

    @Test fun codeFencesKeepTheirInsides() {
        val blocks = parse(
            """
            1. Run it:
               ```sh
               tb land
                 --dry-run
               ```
            ~~~
            | not a table |
            |---|
            ~~~
            """.trimIndent(),
        )
        assertEquals(listOf(li("Run it:", marker = "1."), code("tb land\n  --dry-run"), code("| not a table |\n|---|")), blocks)
    }

    @Test fun paragraphsJoinSoftBreaksAndKeepHardOnes() {
        assertEquals(listOf(p("one two\nthree\nfour"), p("next")), parse("one\ntwo  \nthree\\\nfour\n\nnext"))
    }

    @Test fun anUnclosedFenceWhileStreamingIsStillCode() {
        assertEquals(listOf(p("Look:"), code("let a = 1")), parse("Look:\n```\nlet a = 1"))
    }

    // Not in the Swift suite: the limits that keep hostile replies cheap.
    @Test fun quotesStopNestingAtTheirDepthAndTablesAtTheirCells() {
        var blocks = parse(">".repeat(20) + " deep")
        var depth = 0
        while (blocks.singleOrNull() is MarkdownBlock.Quote) { blocks = (blocks.single() as MarkdownBlock.Quote).blocks; depth++ }
        assertEquals(MarkdownBlock.MAX_QUOTE_DEPTH, depth)
        val rows = (1..3000).joinToString("\n") { "| $it |" }
        val table = parse("| A |\n|---|\n$rows").first() as MarkdownBlock.Table
        assertEquals(MarkdownBlock.MAX_TABLE_CELLS - 1, table.rows.size)
    }

    // Daybreak, 11 Nov 2026 (finding 98): an 1,800-cell header, a one-cell
    // divider and 1,000 one-cell rows came to 1.8 million cells to draw. Its
    // divider doesn't match the header, so it's words; one whose divider does
    // match still comes to no more cells than MAX_TABLE_CELLS, every table of
    // the reply together.
    @Test fun raggedAndWideTablesStayWithinTheirCells() {
        val ragged = "|" + (1..1800).joinToString("|") { "h" } + "|\n|---|\n" + (1..1000).joinToString("\n") { "|x|" }
        assertTrue(ragged.length < 8_000) // within a runner's message limit
        assertTrue(parse(ragged).none { it is MarkdownBlock.Table })

        for (width in listOf(24, 999, 1000, 1001, 1800)) {
            val header = "|" + (1..width).joinToString("|") { "h" } + "|"
            val divider = "|" + (1..width).joinToString("|") { "-" } + "|"
            val text = listOf(header, divider).plus((1..1000).map { "|x|" }).joinToString("\n")
            val again = (1..5).joinToString("\n\n") { text }
            val tables = parse(again).filterIsInstance<MarkdownBlock.Table>()
            val cells = tables.sumOf { it.header.size + it.rows.sumOf { r -> r.size } }
            assertTrue(cells <= MarkdownBlock.MAX_TABLE_CELLS, "width $width: $cells cells")
            assertTrue(tables.all { t -> t.rows.all { it.size == t.header.size } })
        }
    }
}
