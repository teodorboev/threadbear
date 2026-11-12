package com.mikerosoft.threadbear.kit

import kotlin.math.abs
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** The full-screen picture on a tall phone: Mike couldn't see the top of a big picture on the iPhone, which sat under the notch and the buttons. */
class PictureFramingTest {
    private val screen = FSize(430f, 932f)
    private val framing = PictureFraming(top = 111f, bottom = 34f)
    private fun near(a: Float, b: Float) = abs(a - b) < 0.01f

    @Test fun aTallPictureOpensWhollyBelowTheNotch() {
        val size = framing.fitted(FSize(800f, 4000f), screen)
        assertTrue(near(size.height, 932f - 111f - 34f))
        assertTrue(near(size.width, size.height / 5))
        // Its top edge starts where the clear part does.
        val insets = framing.insets(size, screen)
        assertTrue(near(insets.top, 111f))
        assertTrue(near(insets.bottom, 34f))
        assertTrue(near(insets.left, (430f - size.width) / 2))
    }

    @Test fun aWidePictureFillsTheWidthCentredInTheClearPart() {
        val size = framing.fitted(FSize(1600f, 900f), screen)
        assertTrue(near(size.width, 430f))
        val insets = framing.insets(size, screen)
        assertTrue(near(insets.left, 0f))
        // As much room above as below, once the margins are taken off.
        assertTrue(near(insets.top - framing.top, insets.bottom - framing.bottom))
        assertTrue(near(insets.top + size.height + insets.bottom, 932f))
    }

    @Test fun aZoomedPictureCanStillBeDraggedClearOfTheNotch() {
        assertEquals(framing, framing.insets(FSize(2000f, 4000f), screen))
    }

    @Test fun noPictureOrNoRoomDoesNotDivideByZero() {
        assertEquals(FSize(430f, 787f), framing.fitted(FSize.ZERO, screen))
        assertEquals(FSize(430f, 0f), PictureFraming(top = 2000f).fitted(FSize(10f, 10f), screen))
    }
}
