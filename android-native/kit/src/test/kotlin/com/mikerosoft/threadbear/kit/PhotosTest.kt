package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest

/** What the composer adds to InlinePictures (its own tests are InlinePicturesTest). */
class PhotoMarksTest {
    private val m = InlinePictures.MARK_STRING

    /** A chip deleted or cut in the words takes its photo along; one typed past isn't touched. */
    @Test fun anEditSaysWhichPhotosItTookOut() {
        val text = "a $m b $m c $m"
        // Backspace over the second chip (at 6).
        assertEquals(listOf(1), InlinePictures.removedMarks(text, listOf(6..6)))
        // A selection over the first two.
        assertEquals(listOf(0, 1), InlinePictures.removedMarks(text, listOf(1..7)))
        // Typing between them takes none.
        assertEquals(emptyList(), InlinePictures.removedMarks(text, listOf(3 until 3)))
        assertEquals(emptyList(), InlinePictures.removedMarks(text, emptyList()))
    }

    /** What the composer sends is what the message side reads back as placed pictures. */
    @Test fun sentMarkersAreThePlacedPictures() {
        val sent = InlinePictures.sentText("this $m and $m")
        assertEquals(0, InlinePictures.unplaced(sent, 2))
    }
}

class PhotoPrepTest {
    @Test fun aBigPhotoShrinksToTheLongEdge() {
        assertEquals(2048 to 1536, PhotoPrep.fitted(4000, 3000))
        assertEquals(1536 to 2048, PhotoPrep.fitted(3000, 4000))
        assertEquals(PhotoPrep.THUMBNAIL_PIXELS to PhotoPrep.THUMBNAIL_PIXELS * 3 / 4, PhotoPrep.fitted(4000, 3000, PhotoPrep.THUMBNAIL_PIXELS))
        // A sliver keeps a pixel.
        assertEquals(2048 to 1, PhotoPrep.fitted(100_000, 2))
    }

    /** A decode its waiter gave up on still holds its turn until it ends, once (Codex's second pass). */
    @Test fun aTurnIsHeldUntilTheWorkEnds() = runTest {
        val first = PhotoPrep.takeTurn()
        val second = PhotoPrep.takeTurn()
        assertEquals(0, PhotoPrep.freeTurns)
        first()
        first()
        assertEquals(1, PhotoPrep.freeTurns)
        second()
        assertEquals(2, PhotoPrep.freeTurns)
    }

    @Test fun aSmallPhotoIsNotBlownUp() {
        assertEquals(300 to 200, PhotoPrep.fitted(300, 200))
    }

    /** A 10,240 px square is 105 MB of pixels: refused before it's decoded (Daybreak, 7 Nov). */
    @Test fun aHugePictureIsRefusedBeforeItsDecoded() {
        assertFalse(PhotoPrep.fits(10_240, 10_240))
        assertFalse(PhotoPrep.fits(2_000, 2_000, PhotoPrep.MAX_SOURCE_BYTES + 1))
        assertFalse(PhotoPrep.fits(0, 100))
        // The camera's 48 MP fits, and a size not known is no reason to refuse.
        assertTrue(PhotoPrep.fits(8_064, 6_048))
        assertTrue(PhotoPrep.fits(2_000, 2_000, null))
    }
}

class PhotoUploadTest {
    private fun TestScope.uploads() = CoroutineScope(SupervisorJob() + StandardTestDispatcher(testScheduler))

    @Test fun aPhotoNeverSentIsTakenBack() = runTest {
        val discards = mutableListOf<String>()
        val upload = PhotoUpload(uploads(), { "id1" }, { discards += it })
        assertEquals("id1", upload.storageId())
        assertEquals(PhotoUpload.State.UP, upload.state.value)
        upload.drop()
        advanceUntilIdle()
        assertEquals(listOf("id1"), discards)
        // Once only.
        upload.drop()
        advanceUntilIdle()
        assertEquals(listOf("id1"), discards)
    }

    @Test fun aSentPhotoIsLeftForTheBackend() = runTest {
        val discards = mutableListOf<String>()
        val upload = PhotoUpload(uploads(), { "id1" }, { discards += it })
        upload.storageId()
        upload.keep()
        upload.drop()
        advanceUntilIdle()
        assertTrue(discards.isEmpty())
    }

    @Test fun aFailedUploadHasNothingToTakeBack() = runTest {
        val discards = mutableListOf<String>()
        val scope = uploads()
        val upload = PhotoUpload(scope, { throw TransportError.Network("no signal") }, { discards += it })
        assertFailsWith<TransportError.Network> { upload.storageId() }
        assertEquals(PhotoUpload.State.FAILED, upload.state.value)
        upload.drop()
        advanceUntilIdle()
        assertTrue(discards.isEmpty())
        // The failure stayed with the photo: the scope goes on.
        assertEquals("id2", PhotoUpload(scope, { "id2" }, {}).storageId())
    }

    /** Taken out while it's still going: it stops, and there's no copy to take back. */
    @Test fun aPhotoDroppedOnItsWayStops() = runTest {
        val discards = mutableListOf<String>()
        val started = CompletableDeferred<Unit>()
        val upload = PhotoUpload(uploads(), { p -> p(0.5f); started.complete(Unit); awaitCancellation() }, { discards += it })
        started.await()
        assertEquals(0.5f, upload.progress.value)
        upload.drop()
        advanceUntilIdle()
        assertTrue(discards.isEmpty())
    }
}

class DictationTextTest {
    /** The wipe of 31 Oct: the recognizer started over after a pause, and what came before went. */
    @Test fun aRestartKeepsWhatCameBefore() {
        val carry = Carry()
        assertEquals("hello", carry.add("hello"))
        assertEquals("hello there", carry.add("hello there"))
        assertEquals("hello there and", carry.add("and"))
        assertEquals("hello there and then", carry.add("and then"))
        // A correction that starts the same way is the same words, not new ones.
        val again = Carry()
        again.add("write a test")
        assertEquals("write it", again.add("write it"))
    }

    /** Android ends each session at a pause and the app starts the next: what one ended on stays. */
    @Test fun aSessionsWordsStayForTheNext() {
        val carry = Carry()
        carry.add("make the")
        assertEquals("make the button", carry.commit("make the button"))
        // Even when the next starts with the same word.
        assertEquals("make the button make", carry.add("make"))
        assertEquals("make the button make it blue", carry.commit("make it blue"))
        assertEquals("make the button make it blue", carry.text)
    }

    @Test fun wordsGoInAtTheCursor() {
        val spot = DictationSpot.at("fix the bug", 7)
        val first = spot.place("fix the bug", "login")
        assertEquals(DictationSpot.Placed("fix the login bug", 13), first)
        // The next transcript replaces the last, not adds to it.
        assertEquals("fix the login page bug", spot.place(first.text, "login page").text)
        // Just before a word: the space goes after.
        assertEquals(DictationSpot.Placed("fix the login bug", 13), DictationSpot.at("fix the bug", 8).place("fix the bug", "login"))
        // At the end of words with no space: one goes before.
        val end = DictationSpot.at("hello", 5)
        assertEquals(DictationSpot.Placed("hello world", 11), end.place("hello", "world"))
        // Nothing heard yet: nothing changes.
        assertEquals(DictationSpot.Placed("hello", 5), DictationSpot.at("hello", 5).place("hello", ""))
    }

    /** Typed over meanwhile: the words carry on at the end rather than overwrite it. */
    @Test fun editedAroundTheWordsCarryOnAtTheEnd() {
        val spot = DictationSpot.at("", 0)
        val one = spot.place("", "one")
        assertEquals("one", one.text)
        assertEquals("on one two", spot.place("on", "one two").text)
    }

    @Test fun theBarsStretchToWhatTheyHeard() {
        val levels = MicLevels(count = 4)
        assertEquals(listOf(0f, 0f, 0f, 0f), levels.shown)
        levels.push(-2f)
        levels.push(4f)
        levels.push(10f)
        levels.push(40f)
        assertEquals(listOf(0f, 0.5f, 1f, 1f), levels.history)
        assertEquals(1f, levels.shown.last())
        levels.reset()
        assertEquals(4, levels.history.size)
    }
}
