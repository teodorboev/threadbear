package com.mikerosoft.threadbear.kit

import com.mikerosoft.threadbear.kit.InlineMarks.Link
import com.mikerosoft.threadbear.kit.InlineMarks.Mark
import com.mikerosoft.threadbear.kit.InlineMarks.Run
import java.lang.management.ManagementFactory
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class InlineMarksTest {
    private fun parse(text: String, links: Boolean = true) = InlineMarks.parse(text, links)
    private fun words(runs: List<Run>) = runs.joinToString("") { it.text }

    @Test fun marksAndLinks() {
        assertEquals(
            listOf(
                Run("Run "), Run(" npm test ", setOf(Mark.CODE)), Run(", "), Run("then", setOf(Mark.BOLD)), Run(" see "),
                Run("the ", link = Link("work:npm test (unit)", 0)), Run("log", setOf(Mark.ITALIC), Link("work:npm test (unit)", 0)),
                Run(" or "), Run("https://example.com/a", link = Link("https://example.com/a", 1)), Run(". "),
                Run("gone", setOf(Mark.STRIKE)), Run(" \\ * *"),
            ),
            parse("Run `npm test`, **then** see [the *log*](work:npm test (unit)) or https://example.com/a. ~~gone~~ \\\\ \\* \\*"),
        )
    }

    // Daybreak, 11 Nov 2026 (finding 97): a bare address was handed back to
    // the parser as its own words, found again, and so on until the stack ran
    // out, with links on or off.
    @Test fun aBareAddressIsItsOwnWordsOnce() {
        for (links in listOf(true, false)) {
            val runs = parse("https://example.com", links)
            assertEquals(listOf(Run("https://example.com", link = if (links) Link("https://example.com", 0) else null)), runs)
        }
        assertEquals(listOf(Run("See "), Run("https://a.b/c?d=e", link = Link("https://a.b/c?d=e", 0)), Run(".")), parse("See https://a.b/c?d=e."))
    }

    @Test fun nothingInsideALinkIsALink() {
        val url = Link("https://x.y", 0)
        assertEquals(listOf(Run("https://x.y", link = url)), parse("[https://x.y](https://x.y)"))
        assertEquals(listOf(Run("see https://evil.example", link = url)), parse("[see https://evil.example](https://x.y)"))
        // Off: words only, and still no loop.
        assertEquals(listOf(Run("https://x.y")), parse("[https://x.y](https://x.y)", links = false))
        assertEquals(listOf(Run("https://x.y", setOf(Mark.BOLD))), parse("**https://x.y**", links = false))
        assertEquals(listOf(Run("https://x.y", setOf(Mark.BOLD), url)), parse("[**https://x.y**](https://x.y)"))
    }

    @Test fun marksInsideMarksKeepThemAll() {
        assertEquals(
            listOf(Run("x", setOf(Mark.BOLD, Mark.STRIKE, Mark.ITALIC), Link("u", 0))),
            parse("[**~~_*x*_~~**](u)"),
        )
    }

    // Each of these is the largest a reply can be (10,000 bytes from a
    // secondary runner), shaped to make the scanner or the nesting work
    // hardest. A regex took 200 ms on the first and the square of the length
    // on the openers with no closers; each must take well under 100 ms of CPU.
    @Test fun hostileLinesStayCheap() {
        val n = 10_000
        val inputs = listOf(
            "`".repeat(n),
            "`".repeat(n / 2) + "a".repeat(n / 2),
            "`".repeat(n / 2) + "a" + "`".repeat(n / 2 - 1),
            "`".repeat(n / 3) + "a".repeat(n / 3) + "`".repeat(n / 3 - 1),
            (1..140).joinToString("a") { "`".repeat(141 - it) },
            (1..140).joinToString("a") { "`".repeat(it) },
            "https://example.com ".repeat(n / 20),
            "https://" + "a".repeat(n - 8),
            "https://" + ".".repeat(n - 8),
            "http://".repeat(n / 7),
            "*".repeat(n), "_".repeat(n), "~".repeat(n),
            "**a ".repeat(n / 4), "*a ".repeat(n / 3), "_a ".repeat(n / 3), "__a ".repeat(n / 4), "~~a ".repeat(n / 4),
            "[".repeat(n / 2) + "](".repeat(n / 4),
            "[".repeat(n - 4) + "](a",
            "[a](" + "(".repeat(n - 4),
            "[a](" + "(b)".repeat((n - 4) / 3),
            "[a](x " + "[b](y) ".repeat((n - 6) / 7),
            "[a] ".repeat(n / 4),
            "\\".repeat(n),
            "\\*".repeat(n / 2),
            (1..n / 10).joinToString("") { "[l$it](u) " },
            "**__~~*_".repeat(n / 8),
        )
        val bean = ManagementFactory.getThreadMXBean()
        val slow = mutableListOf<String>()
        repeat(2) { // the second time with the JIT warm, which is the one that counts
            slow.clear()
            for (text in inputs) {
                val start = bean.currentThreadCpuTime
                parse(text)
                val ms = (bean.currentThreadCpuTime - start) / 1_000_000
                if (ms >= 50) slow += "${text.take(12)}... $ms ms"
            }
        }
        assertTrue(slow.isEmpty(), slow.joinToString())
        // Cold, all at once, still well inside a second.
        val start = bean.currentThreadCpuTime
        for (text in inputs) InlineTokens(text).all()
        assertTrue((bean.currentThreadCpuTime - start) / 1_000_000 < 500)
    }

    // The scanner finds what this regex found (the one before it, with code
    // spans closed only by a run of as many backticks), on random text made
    // of the characters that matter.
    @Test fun theScannerFindsWhatTheRegexFound() {
        val regex = Regex(
            """\\([\\`*_{}\[\]()#+\-.!|~<>])|(?<!`)(`+)(?!`)(.+?)(?<!`)\2(?!`)|\*\*(.+?)\*\*|__(.+?)__|~~(.+?)~~|(?<![\w*])\*(?!\s)(.+?)(?<!\s)\*(?!\w)|(?<!\w)_(?!\s)(.+?)(?<!\s)_(?!\w)|\[([^\]\n]+)]\(((?:[^()\n]|\([^()\n]*\))+?)\)|(https?://[^\s<>()]+[^\s<>().,;:!?'"])""",
        )
        fun expected(text: String) = regex.findAll(text).map { m ->
            val g = m.groups
            val end = m.range.last + 1
            when {
                g[1] != null -> Token(m.range.first, end, Kind.ESCAPE, g[1]!!.value)
                g[3] != null -> Token(m.range.first, end, Kind.CODE, g[3]!!.value)
                g[4] != null -> Token(m.range.first, end, Kind.BOLD, g[4]!!.value)
                g[5] != null -> Token(m.range.first, end, Kind.BOLD, g[5]!!.value)
                g[6] != null -> Token(m.range.first, end, Kind.STRIKE, g[6]!!.value)
                g[7] != null -> Token(m.range.first, end, Kind.ITALIC, g[7]!!.value)
                g[8] != null -> Token(m.range.first, end, Kind.ITALIC, g[8]!!.value)
                g[9] != null -> Token(m.range.first, end, Kind.LINK, g[9]!!.value, g[10]!!.value)
                else -> Token(m.range.first, end, Kind.ADDRESS, g[11]!!.value)
            }
        }.toList()
        val pieces = listOf("*", "**", "_", "__", "~", "~~", "`", "``", "[", "]", "(", ")", "](", "\\", " ", "\n", "\r", " ", "\t",
            "a", "b", "1", ".", ",", "!", "'", "<", "#", "http://", "https://", "ht", "x.y", "é", " ")
        val random = kotlin.random.Random(11_11_2026)
        repeat(60_000) {
            val text = buildString { repeat(random.nextInt(0, 16)) { append(pieces[random.nextInt(pieces.size)]) } }
            assertEquals(expected(text), InlineTokens(text).all(), "on ${text.map { if (it.code < 32 || it.code > 126) "\\u%04x".format(it.code) else it.toString() }.joinToString("")}")
        }
    }
}
