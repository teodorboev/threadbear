package com.mikerosoft.threadbear.kit

import java.io.File
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest

/** PocketTests.swift. */
class PocketTest {
    private fun card(id: String, at: String, place: String? = null) = PocketCard(id, id, "t", place = place, updatedAt = at)
    private fun tempFile(): File = Files.createTempDirectory("pocket").resolve("pocket.json").toFile()

    @Test fun newestGoesToTheFront() {
        val deck = PocketDeck.ordered(listOf(card("b", "2026-10-02"), card("c", "2026-10-03"), card("a", "2026-10-01")))
        assertEquals(listOf("a", "b", "c"), deck.map { it.id })
    }

    @Test fun aCardForHereBeatsANewerOne() {
        val deck = PocketDeck.ordered(listOf(card("old", "2026-10-01", place = "post office"), card("new", "2026-10-03")), here = setOf("old"))
        assertEquals("old", deck.last().id)
    }

    @Test fun nothingSeenYetMeansEveryCardIsNew() {
        assertTrue(PocketDeck.hasNew(listOf(card("a", "2026-10-01")), null))
        assertFalse(PocketDeck.hasNew(emptyList(), null))
    }

    @Test fun onlyACardFiledOrChangedSinceIsNew() {
        val seen = "2026-10-02"
        assertFalse(PocketDeck.hasNew(listOf(card("a", "2026-10-01"), card("b", seen)), seen))
        assertTrue(PocketDeck.hasNew(listOf(card("a", "2026-10-01"), card("b", "2026-10-03")), seen))
    }

    @Test fun lookingAtTheCardsLeavesNothingNew() {
        val cards = listOf(card("a", "2026-10-01"), card("b", "2026-10-03"))
        val seen = PocketDeck.seenUpTo(cards, "2026-10-02")
        assertEquals("2026-10-03", seen)
        assertFalse(PocketDeck.hasNew(cards, seen))
    }

    @Test fun removingTheNewestCardDoesNotMakeAnOlderOneNew() {
        val older = listOf(card("a", "2026-10-01"))
        val seen = PocketDeck.seenUpTo(older, "2026-10-03")
        assertEquals("2026-10-03", seen)
        assertFalse(PocketDeck.hasNew(older, seen))
    }

    @Test fun anEmptyPocketSeenForTheFirstTimeRemembersNothing() {
        assertNull(PocketDeck.seenUpTo(emptyList(), null))
    }

    @Test fun savedCardsComeBackWithNoBackend() {
        val store = PocketStore(tempFile())
        assertTrue(store.load().isEmpty())
        val cards = listOf(card("a", "2026-10-01", place = "post office"))
        store.save(cards)
        assertEquals(cards, store.load())
        store.clear()
        assertTrue(store.load().isEmpty())
    }

    @Test fun aDamagedFileIsNoCards() {
        val file = tempFile()
        file.writeText("nope")
        assertTrue(PocketStore(file).load().isEmpty())
    }

    @Test fun decodesWhatThePhoneQueryAnswers() {
        val cards = parseJson("""[{"id":"a","title":"A","text":"x","updatedAt":"2026-10-01"},{"id":"b"}]""").lossyList(PocketCard::read)
        assertEquals(listOf("A"), cards.map { it.title })
        assertNull(cards[0].threadId)
    }

    @Test fun removingGoesStraightToTheBackend() = runTest {
        val t = FakeTransport()
        PhoneBackend(t, "tb_x").removePocketCard("a")
        assertEquals("phone:removePocketCard", t.calls.first().first)
        assertEquals("\"a\"", t.calls.first().second["id"].toString())
    }

    @Test fun theDeckIsLive() = runTest {
        val t = FakeTransport()
        t.subscriptions = { name, _ ->
            assertEquals("phone:pocket", name)
            listOf("""[{"id":"a","title":"A","text":"x","icon":"plane","updatedAt":"2026-10-01"}]""")
        }
        assertEquals("plane", PhoneBackend(t, "x").pocket().first().single().icon)
    }

    @Test fun anIconItDoesNotKnowIsThePlainOne() {
        assertEquals("parcel", PocketIcon.named("parcel").name)
        assertEquals(PocketIcon.PLAIN, PocketIcon.named("unicorn"))
        assertEquals(PocketIcon.PLAIN, PocketIcon.named(null))
        assertEquals(0xFF7FB2F0, PocketIcon.named("plane").argb)
    }

    @Test fun aCardKeepsItsIconInTheFile() {
        val store = PocketStore(tempFile())
        store.save(listOf(PocketCard("a", "A", "t", icon = "plane", updatedAt = "x")))
        assertEquals("plane", store.load().first().icon)
        store.clear()
    }

    @Test fun aSwipeRemovesOnlyFarEnoughLeftOrFlicked() {
        assertTrue(PocketSwipe.removes(-120.0, -120.0))
        assertTrue(PocketSwipe.removes(-40.0, -300.0))
        assertFalse(PocketSwipe.removes(-40.0, -60.0))
        assertFalse(PocketSwipe.removes(200.0, 400.0))
        assertTrue(PocketSwipe.isSideways(60.0, 10.0))
        assertFalse(PocketSwipe.isSideways(30.0, 40.0))
    }
}
