package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Swipe left on a thread to go forward to the one you just backed out of. */
class PageSwipesTest {
    @Test fun goingBackKeepsWhatWasPoppedNearestFirst() {
        var forward = ForwardStack()
        forward = forward.pathChanged(listOf("a", "b", "c"), listOf("a"))
        assertEquals("b", forward.next)
        forward = forward.pathChanged(listOf("a"), listOf("a", "b"))
        assertEquals("c", forward.next)
        forward = forward.pathChanged(listOf("a", "b"), listOf("a", "b", "c"))
        assertNull(forward.next)
    }

    @Test fun openingTheSameThreadAgainIsForward() {
        var forward = ForwardStack().pathChanged(listOf("a", "b"), emptyList())
        // Tapped a's row in the inbox: b is still forward from it.
        forward = forward.pathChanged(emptyList(), listOf("a"))
        assertEquals("b", forward.next)
    }

    @Test fun goingSomewhereElseClearsForward() {
        var forward = ForwardStack().pathChanged(listOf("a", "b"), listOf("a"))
        forward = forward.pathChanged(listOf("a"), listOf("a", "x"))
        assertNull(forward.next)
        forward = forward.pathChanged(listOf("a", "x"), listOf("a"))
        forward = forward.pathChanged(listOf("a"), listOf("y"))
        assertNull(forward.next)
    }

    @Test fun noChangeLeavesItAlone() {
        val forward = ForwardStack().pathChanged(listOf("a", "b"), listOf("a")).pathChanged(listOf("a"), listOf("a"))
        assertEquals("b", forward.next)
    }

    @Test fun progressFillsToTheLine() {
        assertEquals(0f, PageSwipe.progress(0f))
        assertEquals(0.5f, PageSwipe.progress(-PageSwipe.LINE / 2))
        assertEquals(1f, PageSwipe.progress(PageSwipe.LINE * 3))
    }

    @Test fun onlyClearlySidewaysCounts() {
        assertTrue(PageSwipe.isSideways(-300f, 40f))
        assertFalse(PageSwipe.isSideways(-100f, 90f))
        assertFalse(PageSwipe.isSideways(0f, 0f))
    }

    @Test fun aCodeBlockThatCanStillScrollKeepsTheSwipe() {
        // 300 wide in a 200 wide view, at the start: left scrolls it, right leaves.
        assertTrue(PageSwipe.scrollerKeeps(forward = true, offset = 0f, contentWidth = 300f, width = 200f))
        assertFalse(PageSwipe.scrollerKeeps(forward = false, offset = 0f, contentWidth = 300f, width = 200f))
        // Scrolled to its end: left goes forward.
        assertFalse(PageSwipe.scrollerKeeps(forward = true, offset = 100f, contentWidth = 300f, width = 200f))
        // Nothing to scroll.
        assertFalse(PageSwipe.scrollerKeeps(forward = true, offset = 0f, contentWidth = 200f, width = 200f))
    }
}
