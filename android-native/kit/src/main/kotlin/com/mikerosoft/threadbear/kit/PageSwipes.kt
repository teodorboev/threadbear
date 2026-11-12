package com.mikerosoft.threadbear.kit

import kotlin.math.abs

/**
 * Forward, as in a browser, for the phone's stack of open threads
 * (PageSwipes.swift). The stack is already Back: going back pops it. What's
 * popped is kept here, so a swipe left on a thread can open it again, like
 * the Mac's swipes (Mike, 8 Nov: "swipe left and swipe right on the thread's
 * text to go back and forward, kind of like we do on the desktop").
 *
 * `ids`: last is the next one forward.
 */
data class ForwardStack(val ids: List<String> = emptyList()) {
    /** Where a swipe left goes, if anywhere. */
    val next: String? get() = ids.lastOrNull()

    /** The navigation path went from `from` to `to`, however it got there. */
    fun pathChanged(from: List<String>, to: List<String>): ForwardStack = when {
        // Back, one or more: the nearest comes forward first.
        to.size < from.size && from.subList(0, to.size) == to -> ForwardStack(ids + from.subList(to.size, from.size).asReversed())
        // Forward, by a swipe or by opening that same thread again, which is
        // the same step, so what's beyond it stays.
        to.size == from.size + 1 && to.subList(0, from.size) == from && to.last() == ids.lastOrNull() -> ForwardStack(ids.dropLast(1))
        // Somewhere new: as in a browser, there's no forward from here.
        to != from -> ForwardStack()
        else -> this
    }
}

/** A page swipe's progress: how far the finger's gone towards the line where letting go goes Back or Forward. */
object PageSwipe {
    /** Letting go past this far (in dp) goes; short of it, nothing happens. */
    const val LINE = 110f

    /** 0 to 1, full at the line. */
    fun progress(translation: Float): Float = minOf(1f, maxOf(0f, abs(translation) / LINE))

    /** Clearly sideways rather than a scroll: the way the Mac tells them apart before it commits to either. */
    fun isSideways(vx: Float, vy: Float): Boolean = abs(vx) > abs(vy) * 1.5f

    /**
     * A horizontally scrolling block (code, a table) under the finger that
     * can still scroll the way it's going keeps the swipe, so it scrolls
     * rather than leaving the page. `forward` is a swipe left, which scrolls
     * the content towards its end.
     */
    fun scrollerKeeps(forward: Boolean, offset: Float, contentWidth: Float, width: Float): Boolean {
        val end = contentWidth - width
        if (end <= 1) return false
        return if (forward) offset < end - 1 else offset > 1
    }
}
