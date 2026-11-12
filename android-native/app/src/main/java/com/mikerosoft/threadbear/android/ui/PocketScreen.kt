package com.mikerosoft.threadbear.android.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.layout.layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.zIndex
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.kit.PocketCard
import com.mikerosoft.threadbear.kit.PocketDeck
import com.mikerosoft.threadbear.kit.PocketIcon
import com.mikerosoft.threadbear.kit.PocketSwipe
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/**
 * The Pocket (PocketView.swift): a Wallet-style deck of the cards agents
 * filed for when Mike's out. Reads the cards saved on the phone, so it
 * needs no signal. Tap a card to bring it to the front, tap the front one to
 * fill the screen with it; swipe one left to remove it, with Undo.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PocketScreen(close: () -> Unit, openThread: (String) -> Unit, add: () -> Unit) {
    val model = LocalModel.current
    val shell = model.shell
    val cards by shell.pocket.collectAsStateWithLifecycle()
    var chosen by remember { mutableStateOf<String?>(null) }
    var showing by remember { mutableStateOf<PocketCard?>(null) }
    // Swiped out, with Undo showing. It only goes from the backend once that's
    // passed, so taking back a mis-swipe needs no connection.
    var going by remember { mutableStateOf<PocketCard?>(null) }
    var goingTimer by remember { mutableStateOf<Job?>(null) }
    val scope = rememberCoroutineScope()

    fun finishRemoving() {
        goingTimer?.cancel()
        goingTimer = null
        going?.let { shell.removePocketCard(it) }
        going = null
    }

    fun remove(card: PocketCard) {
        // Only one Undo at a time: the one before is final now.
        finishRemoving()
        going = card
        goingTimer = scope.launch {
            delay(5000)
            finishRemoving()
        }
    }

    // Anything that arrives while it's open has been seen too.
    LaunchedEffect(cards) { shell.markPocketSeen(cards) }
    // Leaving with Undo still up makes it final.
    DisposableEffect(Unit) { onDispose { finishRemoving() } }

    // No location here: the iPhone's "here" mark needs location and a map search.
    val deck = PocketDeck.ordered(cards.filter { it.id != going?.id })
    val open = deck.firstOrNull { it.id == chosen }?.id ?: deck.lastOrNull()?.id

    Scaffold(
        topBar = {
            TopAppBar(
                navigationIcon = { IconButton(onClick = close) { Icon(Line.Close, "Done", Modifier.size(22.dp)) } },
                title = { Text("Pocket") },
                actions = { IconButton(onClick = add) { Icon(Line.SquarePen, "Add a card", Modifier.size(21.dp)) } },
                colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
            )
        },
    ) { inner ->
        Box(Modifier.padding(inner).fillMaxSize()) {
            if (deck.isEmpty()) {
                Column(Modifier.fillMaxSize().padding(32.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.Center) {
                    Icon(Line.Wallet, null, Modifier.size(40.dp), tint = LocalSignals.current.faint)
                    Spacer(Modifier.size(12.dp))
                    Text("Nothing in your pocket", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.size(4.dp))
                    Text("Ask a thread to pop something in here.", style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            } else {
                val overlap = with(LocalDensity.current) { 14.dp.roundToPx() }
                Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(start = 14.dp, end = 14.dp, top = 4.dp, bottom = 96.dp)) {
                    deck.forEachIndexed { index, card ->
                        key(card.id) {
                            val isOpen = card.id == open
                            PocketRow(
                                card, isOpen,
                                Modifier.zIndex(index.toFloat())
                                    // Each card's foot tucks under the next one, as in Wallet.
                                    .then(if (index < deck.lastIndex) Modifier.layout { m, c ->
                                        val p = m.measure(c)
                                        layout(p.width, (p.height - overlap).coerceAtLeast(0)) { p.place(0, 0) }
                                    } else Modifier),
                                tap = { if (isOpen) showing = card else chosen = card.id },
                                remove = { remove(card) },
                                openThread = card.threadId?.let { id -> { openThread(id) } },
                            )
                        }
                    }
                }
            }
            AnimatedVisibility(
                going != null,
                modifier = Modifier.align(Alignment.BottomCenter).navigationBarsPadding(),
                enter = slideInVertically { it } + fadeIn(),
                exit = slideOutVertically { it } + fadeOut(),
            ) {
                val title = going?.title ?: ""
                Row(
                    Modifier.padding(horizontal = 16.dp, vertical = 8.dp).fillMaxWidth()
                        .clip(RoundedCornerShape(50)).background(MaterialTheme.colorScheme.surfaceContainerHighest)
                        .padding(start = 18.dp, end = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Removed $title", maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                    TextButton(onClick = {
                        goingTimer?.cancel()
                        goingTimer = null
                        going = null
                    }) { Text("Undo", fontWeight = FontWeight.SemiBold) }
                }
            }
        }
    }

    showing?.let { card ->
        PocketBigCard(card, done = { showing = null }, openThread = card.threadId?.let { id -> { showing = null; openThread(id) } })
    }
}

/**
 * A card and what a swipe does to it: it follows the finger left, with a
 * bin showing behind it, and past PocketSwipe's line it flies off. Up and
 * down is still the deck scrolling.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun PocketRow(card: PocketCard, isOpen: Boolean, modifier: Modifier, tap: () -> Unit, remove: () -> Unit, openThread: (() -> Unit)?) {
    val context = LocalContext.current
    val haptics = LocalHapticFeedback.current
    val density = LocalDensity.current
    val scope = rememberCoroutineScope()
    val offset = remember { Animatable(0f) }
    var menu by remember { mutableStateOf(false) }
    val removeAtPx = with(density) { PocketSwipe.REMOVE_AT.dp.toPx() }
    Box(modifier) {
        // The bin behind the card, showing as it slides away.
        Icon(
            Line.Trash, null,
            Modifier.align(Alignment.TopEnd).padding(22.dp).size(22.dp).graphicsLayer { alpha = (-offset.value / removeAtPx).coerceIn(0f, 1f) },
            tint = MaterialTheme.colorScheme.onSurface,
        )
        Box(
            Modifier.graphicsLayer { translationX = offset.value }
                .pointerInput(card.id) {
                    val tracker = VelocityTracker()
                    var buzzed = false
                    detectHorizontalDragGestures(
                        onDragStart = { tracker.resetTracking(); buzzed = false },
                        onHorizontalDrag = { change, dx ->
                            change.consume()
                            tracker.addPosition(change.uptimeMillis, change.position)
                            val next = (offset.value + dx).coerceAtMost(0f)
                            scope.launch { offset.snapTo(next) }
                            if (!buzzed && next <= -removeAtPx) { buzzed = true; haptics.performHapticFeedback(HapticFeedbackType.TextHandleMove) }
                        },
                        onDragEnd = {
                            val width = with(density) { offset.value.toDp().value.toDouble() }
                            // Where a flick would carry it, as the iPhone's predicted end.
                            val velocity = with(density) { tracker.calculateVelocity().x.toDp().value.toDouble() }
                            if (PocketSwipe.removes(width, width + velocity * 0.2)) {
                                scope.launch {
                                    offset.animateTo(with(density) { -600.dp.toPx() }, tween(180))
                                    remove()
                                }
                            } else {
                                scope.launch { offset.animateTo(0f, spring()) }
                            }
                        },
                        onDragCancel = { scope.launch { offset.animateTo(0f, spring()) } },
                    )
                }
                .combinedClickable(onClick = tap, onLongClick = { menu = true }),
        ) {
            PocketCardView(card, isOpen)
        }
        androidx.compose.material3.DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            ActionItem("Copy", Line.Copy) { menu = false; copyText(context, card.title, card.text) }
            if (openThread != null) ActionItem("Open thread", Line.MessageCircle) { menu = false; openThread() }
            ActionItem("Remove", Line.Trash) { menu = false; remove() }
        }
    }
}

/**
 * A luggage tag: a wash of the card's colour, and its icon big and faint
 * behind it, so a card can be picked out of the deck before it's read.
 */
@Composable
private fun PocketCardView(card: PocketCard, isOpen: Boolean) {
    val icon = PocketIcon.named(card.icon)
    val colour = Color(icon.argb)
    val shape = RoundedCornerShape(15.dp)
    Box(
        Modifier.fillMaxWidth().heightIn(min = if (isOpen) 300.dp else 80.dp)
            .shadow(6.dp, shape, clip = false, ambientColor = Color.Black.copy(alpha = 0.18f), spotColor = Color.Black.copy(alpha = 0.18f))
            .clip(shape)
            // A solid fill under the wash, as the cards overlap.
            .background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .background(Brush.linearGradient(listOf(colour.copy(alpha = 0.28f), colour.copy(alpha = 0.08f))))
            .animateContentSize(),
    ) {
        // The big icon under the words: out past the edge when open; on a
        // folded card, left of its label and cut off by the top edge.
        Icon(
            Line.pocket(icon.name), null,
            Modifier.align(if (isOpen) Alignment.BottomEnd else Alignment.TopEnd)
                .offset(x = if (isOpen) 22.dp else (-92).dp, y = if (isOpen) 22.dp else (-12).dp)
                .size(if (isOpen) 180.dp else 76.dp),
            tint = colour.copy(alpha = 0.16f),
        )
        Column(Modifier.fillMaxWidth().padding(15.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Line.pocket(icon.name), null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurface)
                Spacer(Modifier.width(9.dp))
                Text(card.title, style = MaterialTheme.typography.titleMedium, fontWeight = FontWeight.SemiBold, maxLines = 1,
                    overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                Spacer(Modifier.weight(1f).width(8.dp))
                card.source?.let {
                    Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1,
                        overflow = TextOverflow.Ellipsis)
                }
            }
            if (isOpen) {
                Spacer(Modifier.size(10.dp))
                Text(card.text, style = MaterialTheme.typography.titleLarge.copy(fontWeight = FontWeight.Medium), modifier = Modifier.fillMaxWidth())
            }
        }
    }
}

/**
 * One card filling the screen, black on white, to read from or hold up to
 * someone across a counter. The screen stays on while it's up.
 */
@Composable
private fun PocketBigCard(card: PocketCard, done: () -> Unit, openThread: (() -> Unit)?) {
    val context = LocalContext.current
    Dialog(onDismissRequest = done, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        val view = LocalView.current
        DisposableEffect(Unit) {
            view.keepScreenOn = true
            onDispose { view.keepScreenOn = false }
        }
        Column(Modifier.fillMaxSize().background(Color.White).statusBarsPadding().navigationBarsPadding()) {
            // A long card shrinks to fit rather than scrolling off.
            var size by remember(card.id) { mutableStateOf(34f) }
            Box(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 26.dp), contentAlignment = Alignment.CenterStart) {
                SelectionContainer {
                    Text(
                        card.text, color = Color.Black, fontSize = size.sp, lineHeight = (size * 1.2f).sp, fontWeight = FontWeight.Bold,
                        onTextLayout = { r: TextLayoutResult -> if (r.didOverflowHeight && size > 12f) size *= 0.9f },
                    )
                }
            }
            Row(Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                val blue = Color(0xFF007AFF)
                TextButton(onClick = done) { Text("Done", color = blue) }
                Spacer(Modifier.weight(1f))
                TextButton(onClick = { copyText(context, card.title, card.text) }) { Text("Copy", color = blue) }
                if (openThread != null) {
                    Spacer(Modifier.weight(1f))
                    TextButton(onClick = openThread) { Text("Open thread", color = blue) }
                }
            }
        }
    }
}
