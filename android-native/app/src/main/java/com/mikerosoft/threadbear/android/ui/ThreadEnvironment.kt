package com.mikerosoft.threadbear.android.ui

import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.staticCompositionLocalOf
import com.mikerosoft.threadbear.kit.ThreadPictures

// What the open thread hands down to its rows, as the iPhone's environment
// values do (ThreadView.swift's EnvironmentValues extension).

/**
 * What a tap on a link in message `seq` does, set by the open thread.
 * Outside one, the link gate decides (a web page opens, anything else asks).
 */
val LocalThreadLink = staticCompositionLocalOf<(url: String, seq: Int?) -> Unit> { { _, _ -> } }

/** The open thread's pictures, for MarkdownText to find its images in. */
val LocalThreadPictures = compositionLocalOf { ThreadPictures() }

/** The open thread, whose reply buttons a tap presses. Null outside one. */
val LocalButtonThread = compositionLocalOf<String?> { null }

/** Opens a thread on top of this one, so Back comes back here. */
val LocalPushThread = staticCompositionLocalOf<(String) -> Unit> { {} }

/** The step a work: link lit up in its sheet, for a moment. */
val LocalLitStep = compositionLocalOf<Int?> { null }
