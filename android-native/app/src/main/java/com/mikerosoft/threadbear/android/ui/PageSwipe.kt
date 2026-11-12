package com.mikerosoft.threadbear.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChange
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalViewConfiguration
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavController
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.android.knownThread
import com.mikerosoft.threadbear.kit.ForwardStack
import com.mikerosoft.threadbear.kit.PageSwipe
import kotlin.math.abs

/**
 * Where a swipe left on a thread goes: the thread last backed out of, as in
 * a browser, and how to open it (the iPhone's PageForward).
 */
class PageForward(val threadId: String, val go: () -> Unit)

val LocalPageForward = compositionLocalOf<PageForward?> { null }

/**
 * Forward for the stack of open threads, kept from the navigation's own
 * back stack however it changes (a swipe, Back, the system's back gesture,
 * a thread link), as the iPhone's ForwardStack follows its path.
 */
@Composable
fun rememberPageForward(nav: NavController): PageForward? {
    val entries by nav.currentBackStack.collectAsStateWithLifecycle()
    val path = entries.filter { it.destination.route == "thread/{id}" }.mapNotNull { it.arguments?.getString("id") }
    var stack by remember { mutableStateOf(ForwardStack()) }
    var last by remember { mutableStateOf(emptyList<String>()) }
    LaunchedEffect(path) {
        stack = stack.pathChanged(last, path)
        last = path
    }
    val next = stack.next ?: return null
    // Ids from the back stack were routes already; checked again all the same.
    if (com.mikerosoft.threadbear.android.Push.validThreadId(next) == null) return null
    return remember(next) { PageForward(next) { nav.navigate("thread/$next") } }
}

/**
 * Swipe left on a thread's messages to go Forward, right to go Back, as the
 * Mac's trackpad swipes do, with the same card in the middle saying where
 * letting go will land (Mike picked it for the Mac on 8 Nov). Android's own
 * back gesture is the screen's edge only, as the iPhone's was before iOS
 * 26, so this does both. Only a drag that's clearly sideways counts; one a
 * code block or table scrolling sideways has taken stays theirs.
 */
@Composable
fun PageSwipes(back: () -> Unit, content: @Composable BoxScope.() -> Unit) {
    val forward = LocalPageForward.current
    val haptics = LocalHapticFeedback.current
    val slop = LocalViewConfiguration.current.touchSlop
    var drag by remember { mutableStateOf<Pair<Boolean, Float>?>(null) }
    val currentForward by rememberUpdatedState(forward)
    val currentBack by rememberUpdatedState(back)
    Box(
        Modifier.fillMaxSize().pointerInput(Unit) {
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Main)
                var dx = 0f
                var dy = 0f
                var going: Boolean? = null
                var armed = false
                while (true) {
                    val event = awaitPointerEvent(PointerEventPass.Main)
                    val change = event.changes.firstOrNull { it.id == down.id } ?: break
                    if (!change.pressed) {
                        val f = going
                        drag = null
                        if (f != null && armed) { if (f) currentForward?.go?.invoke() else currentBack() }
                        break
                    }
                    if (going == null) {
                        // Taken by something under the finger (a scroll, sideways or down): theirs.
                        if (change.isConsumed) break
                        val d = change.positionChange()
                        dx += d.x
                        dy += d.y
                        if (abs(dx) < slop && abs(dy) < slop) continue
                        if (!PageSwipe.isSideways(dx, dy)) break
                        val f = dx < 0
                        if (f && currentForward == null) break
                        going = f
                    } else {
                        dx += change.positionChange().x
                    }
                    change.consume()
                    val f = going ?: break
                    // Only the way it started counts: swung back past where it began is nothing.
                    val progress = PageSwipe.progress((if (f) minOf(0f, dx) else maxOf(0f, dx)).toDp().value)
                    if (progress >= 1f && !armed) haptics.performHapticFeedback(HapticFeedbackType.SegmentTick)
                    armed = progress >= 1f
                    drag = f to progress
                }
                drag = null
            }
        },
    ) {
        content()
        val d = drag
        if (d != null) {
            val (f, progress) = d
            val title = if (f) forward?.let { LocalModel.current.knownThread(it.threadId) } else null
            SwipeCard(
                forward = f,
                title = if (f) title?.title?.ifEmpty { "Untitled" } ?: "Thread" else "Inbox",
                detail = if (f) title?.placeText else null,
                progress = progress,
                // In as soon as it's clear it's a swipe, not with the line.
                modifier = Modifier.align(Alignment.Center).alpha(minOf(1f, 0.4f + progress * 2)),
            )
        }
    }
}

/** The Mac's SwipeNavigationArrow card, at the phone's sizes. */
@Composable
private fun SwipeCard(forward: Boolean, title: String, detail: String?, progress: Float, modifier: Modifier) {
    val colors = MaterialTheme.colorScheme
    val accent = LocalSignals.current.accent
    val armed = progress >= 1f
    val shape = RoundedCornerShape(14.dp)
    Column(
        modifier.padding(horizontal = 32.dp).widthIn(max = 320.dp).fillMaxWidth().shadow(20.dp, shape)
            .clip(shape).background(colors.surfaceContainerHigh)
            .border(if (armed) 1.dp else 0.5.dp, if (armed) accent else colors.outline, shape)
            .padding(horizontal = 18.dp, vertical = 16.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(34.dp).clip(CircleShape).background(if (armed) accent else colors.onSurface.copy(alpha = 0.08f)), contentAlignment = Alignment.Center) {
                Icon(if (forward) Line.ChevronRight else Line.ChevronLeft, null, Modifier.size(16.dp),
                    tint = if (armed) androidx.compose.ui.graphics.Color.White else colors.onSurfaceVariant)
            }
            Spacer(Modifier.width(12.dp))
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(if (forward) "Forward to" else "Back to", style = MaterialTheme.typography.labelMedium, color = colors.onSurfaceVariant)
                Text(title, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.SemiBold, maxLines = 2, overflow = TextOverflow.Ellipsis)
                if (detail != null) Text(detail, style = MaterialTheme.typography.labelMedium, color = colors.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        Box(Modifier.fillMaxWidth().height(3.dp).clip(CircleShape).background(colors.onSurface.copy(alpha = 0.1f))) {
            Box(Modifier.fillMaxWidth(progress).height(3.dp).clip(CircleShape).background(if (armed) accent else colors.onSurface.copy(alpha = 0.45f)))
        }
    }
}
