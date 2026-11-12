package com.mikerosoft.threadbear.kit

import kotlin.math.max
import kotlin.math.min

data class FSize(val width: Float, val height: Float) {
    companion object {
        val ZERO = FSize(0f, 0f)
    }
}

/**
 * How a picture sits in the phone's full-screen viewer (PictureFraming.swift).
 * `top`, `left`, `bottom` and `right` are the room to keep clear of it: the
 * status bar and camera cutout, the Close and Share buttons, the gesture bar.
 * So the whole picture shows when it opens, whatever its shape, while a
 * zoomed one can still use the whole screen as you drag it about. Here
 * rather than in the view so a test can hold it.
 */
data class PictureFraming(val top: Float = 0f, val left: Float = 0f, val bottom: Float = 0f, val right: Float = 0f) {
    /** The picture's size when it opens: as big as fits in the clear part of the screen, which can be bigger than its own pixels. */
    fun fitted(picture: FSize, screen: FSize): FSize {
        val clear = clearSize(screen)
        if (picture.width <= 0 || picture.height <= 0 || clear.width <= 0 || clear.height <= 0) return clear
        val fit = min(clear.width / picture.width, clear.height / picture.height)
        return FSize(picture.width * fit, picture.height * fit)
    }

    /**
     * The room around a picture at this size: centred in the clear part while
     * it's smaller than that, and able to be dragged clear of the cutout and
     * buttons, so no edge of it ends up stuck under them.
     */
    fun insets(picture: FSize, screen: FSize): PictureFraming {
        val clear = clearSize(screen)
        val x = max(0f, (clear.width - picture.width) / 2)
        val y = max(0f, (clear.height - picture.height) / 2)
        return PictureFraming(top + y, left + x, bottom + y, right + x)
    }

    private fun clearSize(screen: FSize) = FSize(max(0f, screen.width - left - right), max(0f, screen.height - top - bottom))
}
