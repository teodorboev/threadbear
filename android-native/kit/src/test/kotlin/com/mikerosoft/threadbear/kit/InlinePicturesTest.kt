package com.mikerosoft.threadbear.kit

import com.mikerosoft.threadbear.kit.InlinePictures.Piece
import kotlin.test.Test
import kotlin.test.assertEquals

/** docs/protocol.md, "Pictures in a message", on the phone (InlinePicturesTests.swift). */
class InlinePicturesTest {
    private val m = InlinePictures.MARK_STRING

    @Test fun photosGoInWhereTheCursorIsWithASpaceEitherSide() {
        val one = InlinePictures.inserting(1, "compare this with that", 12, 0)
        assertEquals("compare this $m with that", one.text)
        assertEquals(0, one.firstIndex)
        assertEquals(14, one.cursor)
        // Two more at the end: after the first photo, spaced.
        val more = InlinePictures.inserting(2, one.text, one.text.length, 0)
        assertEquals("compare this $m with that $m $m ", more.text)
        assertEquals(1, more.firstIndex)
        // In place of a selection, and clamped past the end.
        assertEquals("a $m word", InlinePictures.inserting(1, "a bad word", 2, 3).text)
        assertEquals("$m ", InlinePictures.inserting(1, "", 9, 4).text)
    }

    @Test fun marksGoAsNumberedMarkersAndPhotosAloneAsNoWords() {
        assertEquals("this [Picture 1] like [Picture 2]", InlinePictures.sentText("this $m like $m"))
        assertEquals("", InlinePictures.sentText("$m $m "))
        assertEquals("plain", InlinePictures.sentText("plain"))
    }

    @Test fun eachPhotoHasOneMark() {
        assertEquals("look $m $m", InlinePictures.normalized("look", 2))
        assertEquals("a $m b ", InlinePictures.normalized("a $m b $m", 1))
        assertEquals("this that", InlinePictures.removingMark(0, "this $m that"))
    }

    /** A message's markers become chips, the first naming each number. */
    @Test fun aMessageSplitsIntoWordsAndChips() {
        assertEquals(
            listOf(Piece.Words("this "), Piece.Picture(1), Piece.Words(" like "), Piece.Picture(2), Piece.Words(", see [Picture 1] again")),
            InlinePictures.pieces("this [Picture 1] like [Picture 2], see [Picture 1] again"),
        )
        assertEquals(listOf(Piece.Picture(1)), InlinePictures.pieces("[Picture 1]"))
        assertEquals(0, InlinePictures.unplaced("this [Picture 1] and [Picture 2]", 2))
        assertEquals(2, InlinePictures.unplaced("see [Picture 2]", 3))
        assertEquals(listOf(Piece.Words("no pictures [picture 1] [Picture 0]")), InlinePictures.pieces("no pictures [picture 1] [Picture 0]"))
    }

    /** The runner's "[2 pictures]" line goes once the photos are there to show, and only then. */
    @Test fun sentPhotosReplaceTheirLine() {
        fun photo(seq: Int, n: Int) = MessageImage(seq = seq, src = "picture:$n", url = "https://x/$seq-$n")
        val both = ThreadPictures(listOf(photo(5, 2), photo(5, 1), photo(4, 1), MessageImage(seq = 5, src = "/a.png", url = "https://x/a")))
        assertEquals(listOf("https://x/5-1", "https://x/5-2"), both.pasted(5).map { it.url })
        assertEquals("look", both.words("look\n\n[2 pictures]", 5))
        assertEquals("", both.words("[2 pictures]", 5))
        // One kept on the Mac: the line stays.
        assertEquals("look\n\n[2 pictures]", ThreadPictures(listOf(photo(5, 1))).words("look\n\n[2 pictures]", 5))
        // A marker places picture 1, so the line counts only picture 2.
        assertEquals("this [Picture 1]", ThreadPictures(listOf(photo(5, 2))).words("this [Picture 1]\n\n[1 picture]", 5))
        assertEquals("plain words", ThreadPictures().words("plain words", 5))
    }
}
