package com.mikerosoft.threadbear.kit

import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject

class NewThreadChoicesTest {
    @Test fun accountsReadWithAndWithoutModels() {
        val json = """
        [{"id":"claude-3","provider":"claude","title":"three@example.com","usage":{"windows":[],"readAt":"x","source":"live"},
          "models":[{"id":"opus","title":"Opus 5.5","effortLevels":["low","high","max"]}],"defaultEffort":"high"},
         {"id":"codex-main","provider":"codex","title":"Codex · me"},
         {"title":"no id"}]
        """
        val accounts = parseJson(json).lossyList(PhoneAccount::read)
        assertEquals(listOf("claude-3", "codex-main"), accounts.map { it.id })
        assertEquals(listOf(PhoneModelChoice("opus", "Opus 5.5", listOf("low", "high", "max"))), accounts[0].models)
        assertTrue(accounts[1].models.isEmpty())
        // Only the id is needed: the rest falls back.
        val bare = PhoneAccount.read(parseJson("""{"id":"x"}""") as JsonObject)
        assertEquals(PhoneAccount("x", null, "claude", "x"), bare)
    }

    @Test fun modelsComeClaudeFirstAndOnceEach() {
        val accounts = listOf(
            PhoneAccount("codex-main", provider = "codex", title = "C", models = listOf(PhoneModelChoice("astra", "Astra"))),
            PhoneAccount("claude-1", provider = "claude", title = "1", models = listOf(PhoneModelChoice("opus", "Opus"), PhoneModelChoice("haiku", "Haiku"))),
            PhoneAccount("claude-2", provider = "claude", title = "2", models = listOf(PhoneModelChoice("opus", "Opus"))),
        )
        assertEquals(listOf("opus", "haiku", "astra"), NewThreadChoices.models(accounts).map { it.id })
    }

    @Test fun effortsFollowTheModel() {
        val codex = PhoneModelChoice("astra", "Astra", listOf("low", "medium", "ultra", "weird"))
        assertEquals(listOf("low", "medium", "ultra"), NewThreadChoices.efforts(codex))
        assertEquals(listOf("low", "medium", "high", "xhigh", "max"), NewThreadChoices.efforts(null))
        assertTrue(NewThreadChoices.keeps("ultra", codex))
        assertFalse(NewThreadChoices.keeps("ultra", null))
        assertTrue(NewThreadChoices.keeps(null, null))
        assertEquals("Extra high", NewThreadChoices.effortTitle("xhigh"))
        assertEquals("Turbo", NewThreadChoices.effortTitle("turbo"))
    }

    private val now: Instant = Instant.ofEpochSecond(1_000_000)
    private fun runner(id: String, name: String, secondsAgo: Long = 0, draining: Boolean? = null, placed: Boolean? = null, main: Boolean? = null) =
        PhoneRunner(id, name, lastSeenAt = now.minusSeconds(secondsAgo), draining = draining, placed = placed, main = main)

    /**
     * The Mac asleep on 6 Nov left the phone with nowhere to start a thread:
     * the picker starts on the first runner online in Mike's order.
     */
    @Test fun runnerIsTheFirstOnlineInOrder() {
        val asleep = runner("laptop", "Laptop", secondsAgo = 600, placed = true, main = true)
        val draining = runner("build-box", "Build box", draining = true, placed = true)
        val mini = runner("mac-mini", "Mac mini", placed = true)
        assertEquals("mac-mini", NewThreadChoices.runner(listOf(asleep, draining, mini), now)?.runnerId)
        assertEquals("mac-mini", NewThreadChoices.runner(listOf(mini, asleep), now)?.runnerId)
        // None online: the main Mac, to wait for.
        assertEquals("laptop", NewThreadChoices.runner(listOf(asleep, draining), now)?.runnerId)
        assertNull(NewThreadChoices.runner(emptyList(), now))
    }

    /**
     * A Mac with a runner key only gets Mike's words when he's put it in his
     * order (Codex's review): with no order, the main Mac, asleep or not.
     */
    @Test fun anUnplacedRunnerIsNeverPickedForHim() {
        val asleep = runner("laptop", "Laptop", secondsAgo = 600, main = true)
        val mini = runner("mac-mini", "Mac mini")
        assertEquals("laptop", NewThreadChoices.runner(listOf(mini, asleep), now)?.runnerId)
        // No main Mac known: nothing picked, so the backend decides.
        assertNull(NewThreadChoices.runner(listOf(mini, runner("laptop", "Laptop")), now))
    }

    @Test fun modelsFollowTheRunner() {
        val accounts = listOf(
            PhoneAccount("claude-laptop", runnerId = "laptop", provider = "claude", title = "1", models = listOf(PhoneModelChoice("opus", "Opus"))),
            PhoneAccount("codex-mini", runnerId = "mac-mini", provider = "codex", title = "C", models = listOf(PhoneModelChoice("astra", "Astra"))),
        )
        assertEquals(listOf("astra"), NewThreadChoices.models(accounts, "mac-mini").map { it.id })
        assertEquals(listOf("opus", "astra"), NewThreadChoices.models(accounts, null).map { it.id })
        // A runner with no logins mirrored yet offers none, rather than another Mac's.
        assertTrue(NewThreadChoices.models(accounts, "cloud").isEmpty())
    }

    @Test fun offlineSpeaksUpOnlyWhenEveryRunnerIs() {
        val zone = java.time.ZoneId.systemDefault()
        val clock = RelativeTime.clock(now.minusSeconds(600), now, zone)
        assertNull(NewThreadChoices.offlineText(emptyList(), now))
        assertEquals("Mac offline since $clock", NewThreadChoices.offlineText(listOf(runner("laptop", "Laptop", 600)), now))
        assertEquals("Mac mini offline since $clock", NewThreadChoices.offlineText(listOf(runner("laptop", "Laptop", 900), runner("mac-mini", "Mac mini", 600)), now))
        assertNull(NewThreadChoices.offlineText(listOf(runner("laptop", "Laptop", 900), runner("mac-mini", "Mac mini")), now))
        assertEquals("Laptop offline since $clock", NewThreadChoices.offlineText(runner("laptop", "Laptop", 600), now))
        assertNull(NewThreadChoices.offlineText(runner("laptop", "Laptop"), now))
    }
}

class ProviderInfoTest {
    @Test fun builtInsAndStrangers() {
        Providers.reset()
        assertEquals("Claude Code", Providers.info("claude").cliName)
        assertEquals("claude", Providers.info("").id)
        assertEquals("codex", Providers.info("codex").effort)
        // Never heard of: its id for a name, and nothing it might not have.
        val odd = Providers.info("gemini")
        assertEquals("Gemini", odd.name)
        assertEquals("none", odd.effort)
        assertFalse(odd.rewind)
        assertNull(Providers.known("gemini"))
    }

    @Test fun aRunnerSaysWhatItsProvidersAre() {
        Providers.reset()
        val told = (parseJson("""[{"id":"gemini","name":"Gemini","effort":"claude","rewind":true,"sandbox":"yes"},{"name":"no id"}]""") as JsonArray)
            .lossy { ProviderInfo.read(it as JsonObject) }
        assertEquals(1, told.size)
        // A wrong type reads as absent.
        assertFalse(told[0].sandbox)
        assertEquals("gemini", told[0].cliName)
        Providers.update(told)
        assertEquals("claude", Providers.info("gemini").effort)
        assertTrue(Providers.info("gemini").rewind)
        Providers.reset()
    }
}
