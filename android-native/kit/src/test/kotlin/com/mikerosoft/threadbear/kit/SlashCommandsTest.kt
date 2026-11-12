package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonObject

/** The phone's slash command palette (backend API 61): the list comes back as a slashCommands command's result. */
class SlashCommandsTest {
    private fun status(json: String) = CommandStatus.read(parseJson(json) as JsonObject)

    @Test fun aCommandsResultDecodesAsTheList() {
        val s = status("""{"state":"done","result":[{"name":"compact","source":"cli"},{"name":"deslop","description":"Clean","source":"skill","scope":"user","path":"/u/.claude/skills/deslop/SKILL.md"}]}""")
        assertEquals(listOf("compact", "deslop"), s.commands?.map { it.name })
        assertEquals("/u/.claude/skills/deslop/SKILL.md", s.commands?.last()?.path)
    }

    @Test fun anyOtherResultReadsAsNone() {
        for (json in listOf("""{"state":"done","result":{"items":[]}}""", """{"state":"pending"}""", """{"state":"done","result":[{"title":"x"}]}""")) {
            assertNull(status(json).commands, json)
        }
    }

    @Test fun theBackendAsksTheThreadsRunnerAndReadsTheAnswer() = runTest {
        val t = FakeTransport()
        t.subscriptions = { _, _ -> listOf("""{"state":"done","result":[{"name":"compact","source":"cli"}]}""") }
        val b = PhoneBackend(t, "x")
        val id = b.slashCommands("t1")
        assertEquals("phone:slashCommands", t.calls.last().first)
        assertEquals("\"t1\"", t.calls.last().second["threadId"].toString())
        val s = b.command(id).first()
        assertEquals(CommandState.DONE, s?.state)
        assertTrue(s?.commands?.isNotEmpty() == true)
    }

    private val commands = listOf(
        SlashCommand("compact", "Compact the conversation", source = "cli"),
        SlashCommand("model", "Pick the model", argumentHint = "[name]", source = "threadbear"),
        SlashCommand("deslop", "Clean up code", source = "skill", scope = "user", path = "/u/.claude/skills/deslop/SKILL.md"),
        SlashCommand("code-review", "Review the diff", argumentHint = " [low|high] ", source = "skill", userOnly = true),
    )

    @Test fun matchesRankNamesBeforeDescriptions() {
        // "compact" and "code-review" start with it, in the runner's order; "deslop" only says "code".
        assertEquals(listOf("compact", "code-review", "deslop"), SlashPalette.matches(commands, "co", SlashPalette.Category.ALL, emptyList()).map { it.name })
        assertEquals(listOf("deslop", "code-review"), SlashPalette.matches(commands, "", SlashPalette.Category.SKILLS, emptyList()).map { it.name })
        // Recent is in the order they were used.
        assertEquals(listOf("deslop", "compact"), SlashPalette.matches(commands, "", SlashPalette.Category.RECENT, listOf("deslop", "compact")).map { it.name })
    }

    @Test fun countsLeaveOutAnEmptyRecent() {
        assertEquals(listOf(SlashPalette.Category.ALL, SlashPalette.Category.THREADBEAR, SlashPalette.Category.CLI, SlashPalette.Category.SKILLS),
            SlashPalette.counts(commands, "", emptyList()).map { it.first })
        assertEquals(4 to 1, SlashPalette.counts(commands, "", listOf("model")).let { it[0].second to it.last().second })
    }

    @Test fun whatsTypedAndWhatAPickLeaves() {
        assertEquals("co", SlashPalette.query("/co"))
        assertEquals("", SlashPalette.query("/"))
        assertNull(SlashPalette.query("/co x"))
        assertNull(SlashPalette.query("/Users/m"))
        assertNull(SlashPalette.query("hi"))
        assertEquals(SlashPalette.Typed("td", 9), SlashPalette.typed("then use /td"))
        assertTrue(SlashPalette.typed("then use /td")!!.midMessage)
        assertNull(SlashPalette.typed("then use /td "))
        assertNull(SlashPalette.typed("see /Users/m"))
        assertEquals("/model ", SlashPalette.fill(commands[1], forArguments = true))
        assertEquals("/compact", SlashPalette.fill(commands[0], forArguments = false))
        assertEquals("/model [name]", commands[1].usage)
        assertEquals("/code-review [low|high]", commands[3].usage)
        // Partway through a message, only skills the agent may run.
        assertEquals(listOf("deslop"), SlashPalette.midMessage(commands).map { it.name })
        assertEquals(listOf("b", "a", "c"), SlashPalette.used("b", listOf("a", "b", "c")))
        assertEquals(8, SlashPalette.used("z", (1..10).map { "$it" }).size)
    }

    @Test fun originsAndDescriptions() {
        assertEquals("Your skill", SlashPalette.origin(commands[2], "claude"))
        assertEquals("Claude Code's own", SlashPalette.origin(commands[0], "claude"))
        assertEquals("Codex's own", SlashPalette.origin(commands[0], "codex"))
        assertEquals("Threadbear's own: it changes this thread's settings", SlashPalette.origin(commands[1], "claude"))
        assertEquals("From the vercel plugin", SlashPalette.origin(SlashCommand("x", source = "cli", scope = "plugin", plugin = "vercel"), "claude"))
        assertEquals("Claude Code", SlashPalette.Category.CLI.title("claude"))
        assertEquals("Gemini", SlashPalette.cliName("gemini"))
        // Links keep their words but stop being links (Daybreak, 8 Nov); the rest of the marks stay.
        assertEquals("Run **this** with the script and https://x.dev", SlashPalette.description("Run **this** with [the script](file:///tmp/run.sh) and <https://x.dev>"))
        assertEquals("~/dev/a", SlashPalette.shortPath("/Users/m/dev/a", home = "/Users/m"))
        assertEquals("/Users/mm/a", SlashPalette.shortPath("/Users/mm/a", home = "/Users/m"))
    }
}
