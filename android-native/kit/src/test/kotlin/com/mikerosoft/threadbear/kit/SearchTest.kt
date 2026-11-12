package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue
import kotlinx.coroutines.test.runTest

/** searchLooksThroughEveryListLikeTheMac and friends (ThreadbearKitTests.swift). */
class SearchTest {
    @Test fun searchLooksThroughEveryListLikeTheMac() {
        val dates = thread("Dates on small screens", 1).copy(preview = "The **picker** fits now")
        val garden = thread("Release 1.2", 2).copy(projects = listOf(ProjectRef("p", "Garden planner")))
        val sections = InboxSections.build(listOf(dates, garden, thread("Busy picker", 0, status = ThreadStatus.RUNNING)), NOW)
        val asleep = thread("Old picker work", 50)
        val archived = thread("Rename the settings screen", 90)

        val picker = ThreadSearch.of("  PICKER ", sections, listOf(asleep), listOf(archived))
        assertEquals(listOf("Dates on small screens"), picker.inbox.map { it.threadId })
        assertEquals(listOf("Busy picker"), picker.running.map { it.threadId })
        assertEquals(listOf("Old picker work"), picker.hibernating.map { it.threadId })
        assertTrue(picker.archived.isEmpty())

        assertEquals(listOf("Release 1.2"), ThreadSearch.of("garden", sections, emptyList(), emptyList()).inbox.map { it.threadId })
        assertEquals(1, ThreadSearch.of("séttings", sections, emptyList(), listOf(archived)).archived.size)
        assertTrue(ThreadSearch.of("nothing like it", sections, listOf(asleep), listOf(archived)).isEmpty)
        // What phone:search found past the den's 100 comes after the phone's
        // own archived matches, once each, as Convex matched it (whole words).
        val old = thread("Picker from last year", 9000)
        val withOlder = ThreadSearch.of("picker", sections, emptyList(), listOf(archived, asleep), olderArchived = listOf(asleep, old))
        assertEquals(listOf("Old picker work", "Picker from last year"), withOlder.archived.map { it.threadId })
        // Mid-move, in the inbox and hibernating at once: shown once.
        assertTrue(ThreadSearch.of("dates", sections, listOf(dates), emptyList()).hibernating.isEmpty())
    }

    @Test fun aStarredMatchStaysUnderStarred() {
        val asks = thread("asks", 0, needsYou = true).copy(starredAt = NOW)
        val sections = InboxSections.build(listOf(asks, thread("plain", 0)), NOW)
        assertEquals(listOf("asks"), ThreadSearch.of("asks", sections, emptyList(), emptyList()).starred.map { it.threadId })
    }

    @Test fun searchAsksConvexForTheWords() = runTest {
        val t = FakeTransport()
        t.queries = { _, _ ->
            """[{"threadId":"old","runnerId":"r","title":"Old","cwd":"/w","accountId":"a","provider":"claude","status":"idle","needsYou":false,"unread":false,"preview":"","lastActivityAt":1000},{"bad":1}]"""
        }
        val found = PhoneBackend(t, "x").search("old")
        assertEquals(listOf("old"), found.map { it.threadId })
        assertEquals("phone:search", t.calls.single().first)
        assertEquals("\"old\"", t.calls.single().second["text"].toString())
    }
}
