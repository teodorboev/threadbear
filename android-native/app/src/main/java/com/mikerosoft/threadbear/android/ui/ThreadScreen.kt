package com.mikerosoft.threadbear.android.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import androidx.compose.foundation.border
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import com.mikerosoft.threadbear.android.AppModel
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.android.LocalSend
import com.mikerosoft.threadbear.android.LocalSnackbar
import com.mikerosoft.threadbear.android.Push
import com.mikerosoft.threadbear.android.Loaded
import com.mikerosoft.threadbear.android.ThreadExtras
import com.mikerosoft.threadbear.android.ThreadViewModel
import com.mikerosoft.threadbear.android.knownThread
import com.mikerosoft.threadbear.kit.ApprovalOption
import com.mikerosoft.threadbear.kit.BackendApi
import com.mikerosoft.threadbear.kit.CardMentions
import com.mikerosoft.threadbear.kit.Hibernate
import com.mikerosoft.threadbear.kit.MessageImage
import com.mikerosoft.threadbear.kit.PendingApproval
import com.mikerosoft.threadbear.kit.PhoneMessage
import com.mikerosoft.threadbear.kit.PhoneThread
import com.mikerosoft.threadbear.kit.Plans
import com.mikerosoft.threadbear.kit.Role
import com.mikerosoft.threadbear.kit.SlashPalette
import com.mikerosoft.threadbear.kit.ThreadChange
import com.mikerosoft.threadbear.kit.ThreadItem
import com.mikerosoft.threadbear.kit.ThreadLinks
import com.mikerosoft.threadbear.kit.ThreadPictures
import com.mikerosoft.threadbear.kit.ThreadStatus
import com.mikerosoft.threadbear.kit.Turns
import com.mikerosoft.threadbear.kit.WaitingSend
import com.mikerosoft.threadbear.kit.WaitingSends
import java.time.Instant

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ThreadScreen(threadId: String, back: () -> Unit, open: (String) -> Unit) {
    val model = LocalModel.current
    val vm: ThreadViewModel = viewModel(key = threadId) { ThreadViewModel(model, threadId) }
    val loaded by vm.loaded.collectAsStateWithLifecycle()
    // The reply's words and photos (Composer.kt).
    val box = rememberPhotoBox()
    val thread = (loaded as? Loaded.Here)?.detail?.thread
    val stopping by model.stopping.collectAsStateWithLifecycle()
    val gate = rememberLinkGate()

    LaunchedEffect(thread?.isBusy) { if (thread?.isBusy == false) model.stopped(threadId) }

    // Every thread opened from the transcript (a thread: link, a chip, a reply
    // button) comes here, and its id from content: only one that can be a
    // thread's id becomes a route, the same rule as a notification's, as
    // "thread:a/b" made a route the navigation threw on (Codex review, 11 Nov).
    val push = remember(open) { { id: String -> Push.validThreadId(id)?.let(open) ?: model.note("That isn't a thread on this phone.") } }
    CompositionLocalProvider(LocalPushThread provides push, LocalButtonThread provides threadId) {
    Scaffold(
        snackbarHost = { SnackbarHost(LocalSnackbar.current) },
        topBar = { ThreadBar(thread, back) },
        bottomBar = {
            if (thread != null) Column {
                OverTheBox(thread, box.text.text.toString(), gate) { box.text.setTextAndPlaceCursorAtEnd(it) }
                val offline = model.offlineRunner(thread)
                Composer(
                    box = box,
                    placeholder = when {
                        offline != null -> "$offline is offline, this will wait"
                        thread.isBusy -> "Steer the agent"
                        else -> "Reply"
                    },
                    running = thread.isBusy,
                    canStop = model.apiVersion == 0 || model.apiVersion >= BackendApi.STOP,
                    stopping = threadId in stopping,
                    onSend = { message, photos ->
                        // Cleared at once, as the message shows below as it goes;
                        // a send that fails puts it all back if nothing's been typed since.
                        val typed = box.text.text.toString()
                        box.clear()
                        vm.send(message, photos = photos) { if (box.isEmpty) box.restore(typed, photos) else photos.forEach { it.abandon() } }
                    },
                    onStop = { model.stop(threadId) },
                )
            }
        },
    ) { inner ->
        when (val l = loaded) {
            Loaded.Loading -> Box(Modifier.padding(inner).fillMaxWidth()) {
                LinearProgressIndicator(Modifier.fillMaxWidth().height(1.dp), color = MaterialTheme.colorScheme.outline)
            }
            Loaded.Gone -> Box(Modifier.padding(inner).fillMaxSize(), contentAlignment = Alignment.Center) {
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text("This thread's gone", style = MaterialTheme.typography.titleMedium)
                    Text("It may have been deleted on the Mac.", color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            is Loaded.Here -> Transcript(vm, l.detail.thread, l.detail.messages, inner, back, gate) { if (box.isEmpty) box.text.setTextAndPlaceCursorAtEnd(it) }
        }
    }
    }
    LinkGateDialog(gate)
}

/**
 * Over the reply box: the slash palette while what's typed is a command
 * being typed, and the thread's
 * patch link buttons, as the iPhone stacks them over its composer.
 */
@Composable
private fun OverTheBox(thread: PhoneThread, draft: String, gate: LinkGate, fill: (String) -> Unit) {
    val model = LocalModel.current
    val extras = ThreadExtras.of(model, LocalContext.current)
    Column(Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.background), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        val query = SlashPalette.query(draft)
        if (query != null && extras.canUseSlashCommands) {
            SlashPaletteList(thread.threadId, thread.provider, query) { command ->
                fill(SlashPalette.fill(command, forArguments = !command.argumentHint.isNullOrBlank()))
            }
        }
        LinkChips(thread.links.orEmpty()) { gate.decide(it) }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ThreadBar(thread: PhoneThread?, back: () -> Unit) {
    val model = LocalModel.current
    val context = LocalContext.current
    var menu by remember { mutableStateOf(false) }
    var hibernate by remember { mutableStateOf(false) }
    val projects by model.projects.collectAsStateWithLifecycle()
    TopAppBar(
        navigationIcon = { IconButton(onClick = back) { Icon(Line.Back, "Back", Modifier.size(22.dp)) } },
        title = {
            if (thread != null) Row(verticalAlignment = Alignment.CenterVertically) {
                val ref = thread.projects?.firstOrNull()
                ProjectTile(ref?.let { r -> projects.firstOrNull { it.id == r.id } }, ref?.title ?: thread.folderName, 26.dp)
                Spacer(Modifier.width(10.dp))
                Text(thread.title.ifEmpty { "Untitled" }, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        },
        actions = {
            if (thread != null) Box {
                IconButton(onClick = { menu = true }) { Icon(Line.More, "More", Modifier.size(20.dp)) }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    if (thread.archivedAt != null) {
                        MenuItem("Back to the inbox", Line.Inbox) { menu = false; model.change(thread.threadId, ThreadChange.Unarchive) }
                    } else {
                        val asleep = thread.snoozedUntil != null || thread.snoozedUntilRestart == true
                        if (asleep) {
                            MenuItem("Wake", Line.Sun) { menu = false; model.change(thread.threadId, ThreadChange.Wake) }
                        } else {
                            MenuItem("Hibernate…", Line.Moon) { menu = false; hibernate = true }
                        }
                        MenuItem("Archive", Line.Archive) { menu = false; model.change(thread.threadId, ThreadChange.Archive); back() }
                        val starred = thread.starredAt != null
                        if (model.apiVersion == 0 || model.apiVersion >= BackendApi.STARS) {
                            MenuItem(if (starred) "Remove Star" else "Star", if (starred) Line.StarFilled else Line.Star) {
                                menu = false; model.change(thread.threadId, ThreadChange.Star(!starred))
                            }
                        }
                    }
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    // To point another thread at this one: pasted into a message
                    // here or on the Mac, it goes in as a link to it.
                    MenuItem("Copy Thread ID", Line.Copy) {
                        menu = false
                        val clipboard = context.getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
                        clipboard.setPrimaryClip(ClipData.newPlainText("Thread ID", thread.threadId))
                    }
                    HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
                    Info(Line.Folder, thread.cwd)
                    Info(Line.Paw, listOfNotNull(thread.accountId, thread.model).joinToString(" · "))
                    thread.costUsd?.takeIf { it > 0 }?.let { Info(Line.Zap, "$%.2f so far".format(it)) }
                }
                DropdownMenu(expanded = hibernate, onDismissRequest = { hibernate = false }) {
                    for (p in Hibernate.presets(Instant.now())) {
                        MenuItem(p.title, Line.Moon) { hibernate = false; model.change(thread.threadId, ThreadChange.Snooze(p.date)); back() }
                    }
                    MenuItem("Until the runner restarts", Line.Moon) {
                        hibernate = false; model.change(thread.threadId, ThreadChange.SnoozeUntilRestart); back()
                    }
                }
            }
        },
        colors = TopAppBarDefaults.topAppBarColors(containerColor = MaterialTheme.colorScheme.background),
    )
}

@Composable
private fun MenuItem(text: String, icon: androidx.compose.ui.graphics.vector.ImageVector, onClick: () -> Unit) {
    DropdownMenuItem(text = { Text(text) }, leadingIcon = { Icon(icon, null, Modifier.size(18.dp)) }, onClick = onClick)
}

/** A line of what the thread is, under the menu's actions: not something to tap. */
@Composable
private fun Info(icon: androidx.compose.ui.graphics.vector.ImageVector, text: String) {
    DropdownMenuItem(
        text = { Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) },
        leadingIcon = { Icon(icon, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant) },
        onClick = {}, enabled = false,
    )
}

/** One row of the transcript, bottom first (the list is drawn upside down so it sits on the newest). */
private sealed class Row_ {
    abstract val key: String
    data class Item(val item: ThreadItem) : Row_() { override val key get() = "i:${item.id}" }
    data class Pinned(val image: MessageImage) : Row_() { override val key get() = "p:${image.seq}:${image.url}" }
    data class Waiting(val send: WaitingSend, val says: Boolean) : Row_() { override val key get() = "w:${send.commandId}" }
    data class Local(val send: LocalSend) : Row_() { override val key get() = "l:${send.key}" }
    data object Card : Row_() { override val key get() = "card" }
    data object Earlier : Row_() { override val key get() = "earlier" }
}

@Composable
private fun Transcript(
    vm: ThreadViewModel,
    thread: PhoneThread,
    threadMessages: List<PhoneMessage>,
    inner: PaddingValues,
    leave: () -> Unit,
    gate: LinkGate,
    giveBack: (String) -> Unit,
) {
    val model = LocalModel.current
    val extras = ThreadExtras.of(model, LocalContext.current)
    val push = LocalPushThread.current
    val page by vm.page.collectAsStateWithLifecycle()
    LaunchedEffect(page) { page?.let(vm::takeLive) }
    val history by vm.history.collectAsStateWithLifecycle()
    val late by vm.lateImages.collectAsStateWithLifecycle()
    LaunchedEffect(late) { vm.takeLateImages(late) }
    val loadingChunk by vm.loadingChunk.collectAsStateWithLifecycle()
    val loadingBefore by vm.loadingBefore.collectAsStateWithLifecycle()
    val waiting by vm.waiting.collectAsStateWithLifecycle()
    val locals by vm.locals.collectAsStateWithLifecycle()
    val cancelling by vm.cancelling.collectAsStateWithLifecycle()
    val answering by vm.answering.collectAsStateWithLifecycle()
    val runners by model.runners.collectAsStateWithLifecycle()
    val now = rememberNow(1000)

    // From API 2 the history (the live page, tool lines too, with older
    // chunks before it); until it arrives, and on an older backend,
    // phone:thread's messages.
    val h = history
    val messages = h?.messages ?: threadMessages
    val items = remember(messages, thread.isBusy) { Turns.fold(messages, thread.isBusy) }
    val pictures = remember(h) { ThreadPictures(h?.images.orEmpty()) }
    val pins = remember(pictures, items) { pictures.pinned(items) }
    val paged = h != null
    val canLoadMore = if (h != null) h.hasMore else thread.hasEarlier(threadMessages.firstOrNull()?.seq)
    val loadingMore = if (paged) loadingChunk else loadingBefore != null && loadingBefore == threadMessages.firstOrNull()?.seq
    val loadMore = { if (paged) vm.loadChunk() else vm.loadEarlier(threadMessages.firstOrNull()?.seq) }
    val offline = remember(runners, thread.runnerId, now) { WaitingSends.offlineRunner(thread.runnerId, runners, now) }
    val macAsleep = runners.isNotEmpty() && runners.none { it.isOnline(now) }
    val shown = remember(waiting, messages) { WaitingSends.shown(waiting, messages) }
    val listed = shown.map { it.commandId }.toSet()
    val pendingLocals = locals.filter { it.commandId == null || it.commandId !in listed }

    var workLink by remember { mutableStateOf<ThreadLinks.Hit?>(null) }
    var planShown by remember { mutableStateOf<PhoneMessage?>(null) }

    /**
     * A link in a message. A work: link opens the turn's working over the
     * thread with the step lit; a thread: link opens that thread on top of
     * this one; a review only opens on the Mac, so that one says so rather
     * than doing nothing; anything else goes through the link gate.
     */
    fun follow(url: String, seq: Int?) {
        when (val target = ThreadLinks.target(url)) {
            null -> {
                // A relative link (`[plan](shorts-plan/candidates.md)`) is a
                // file in the thread's folder on the Mac; the phone can't open it.
                if (!Regex("^[A-Za-z][A-Za-z0-9+.-]*:").containsMatchIn(url) || url.startsWith("file:", ignoreCase = true)) {
                    model.note("That file opens on the Mac.")
                } else {
                    gate.decide(url)
                }
            }
            // Agents may give only the start of the id.
            is ThreadLinks.Target.Thread -> push(model.inbox.value?.firstOrNull { it.threadId.startsWith(target.id) }?.threadId ?: target.id)
            ThreadLinks.Target.Review -> model.note("That review opens on the Mac.")
            is ThreadLinks.Target.Button -> {
                // One partway through a line, drawn as a link. An id the row doesn't carry is just its words.
                val button = messages.firstOrNull { it.seq == seq }?.buttons?.firstOrNull { it.id == target.id } ?: return
                extras.tap(button, thread.threadId, push)
            }
            is ThreadLinks.Target.Step -> workLink = ThreadLinks.find(target.quote, seq, items) ?: return
        }
    }

    val rows = remember(items, shown, pendingLocals, canLoadMore, pins) {
        buildList {
            add(Row_.Card)
            pendingLocals.reversed().forEach { add(Row_.Local(it)) }
            shown.reversed().forEachIndexed { i, s -> add(Row_.Waiting(s, says = i == 0)) }
            pins.trailing.reversed().forEach { add(Row_.Pinned(it)) }
            items.reversed().forEach { add(Row_.Item(it)) }
            if (canLoadMore) add(Row_.Earlier)
        }
    }
    val state = rememberLazyListState()
    val lastSeq = messages.lastOrNull()?.seq
    LaunchedEffect(lastSeq, pendingLocals.size) {
        if (state.firstVisibleItemIndex <= 2) state.animateScrollToItem(0)
    }

    CompositionLocalProvider(
        LocalThreadLink provides ::follow,
        LocalThreadPictures provides pictures,
        LocalOpenPlan provides { planShown = it },
    ) {
        PageSwipes(back = leave) {
            LazyColumn(
                state = state,
                reverseLayout = true,
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = inner.calculateTopPadding() + 8.dp, bottom = inner.calculateBottomPadding() + 12.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp, Alignment.Bottom),
            ) {
                items(rows, key = { it.key }) { row ->
                    when (row) {
                        Row_.Card -> StatusCard(vm, thread, messages, pendingLocals.isEmpty() && shown.isEmpty(), leave)
                        is Row_.Local -> WaitingMessageView(row.send.text, null, offline, says = true, cancelling = false, cancel = null)
                        is Row_.Waiting -> WaitingMessageView(
                            WaitingSends.mirrored(row.send), row.send, offline, says = row.says,
                            cancelling = row.send.commandId in cancelling,
                            cancel = { vm.cancel(row.send, giveBack) },
                        )
                        is Row_.Pinned -> PinnedPicture(row.image)
                        is Row_.Item -> ItemView(row.item, pins.under)
                        Row_.Earlier -> {
                            EarlierButton(loadingMore, waitingForMac = !paged && macAsleep, load = loadMore)
                            // Scrolling up to it loads by itself, like any chat. Not on a
                            // short thread, where it's on screen at once and nobody asked.
                            // Rows, not messages: one long finished turn is dozens of
                            // messages folded into a line.
                            LaunchedEffect(Unit) { if (items.size >= 20) loadMore() }
                        }
                    }
                }
            }
        }
    }

    workLink?.let { hit ->
        CompositionLocalProvider(LocalThreadPictures provides pictures, LocalThreadLink provides ::follow) {
            WorkingSheet(hit) { workLink = null }
        }
    }
    planShown?.let { plan ->
        val pending = thread.pendingApproval
        // Go ahead only on the card that waits on this very plan, when it can be allowed from here.
        val waitsOnIt = Plans.plan(pending, messages)?.seq == plan.seq
        val approvalId = if (waitsOnIt && Plans.canGoAhead(pending)) pending?.id else null
        CompositionLocalProvider(LocalThreadPictures provides pictures, LocalThreadLink provides ::follow) {
            PlanSheet(
                plan, approvalId,
                approving = approvalId != null && answering?.first == approvalId,
                canSendBack = Plans.canSendBack(plan, pending, messages),
                goAhead = { vm.respond(it, ApprovalOption.ALLOW) },
                sendBack = { text, done -> vm.send(text, onSent = { done(true) }, onFailed = { done(false) }) },
            ) { planShown = null }
        }
    }
}

/** Below the newest message: what the thread is doing, or the card waiting on Mike. */
@Composable
private fun StatusCard(vm: ThreadViewModel, thread: PhoneThread, messages: List<PhoneMessage>, nothingOnItsWay: Boolean, leave: () -> Unit) {
    val model = LocalModel.current
    val signals = LocalSignals.current
    val colors = MaterialTheme.colorScheme
    val answering by vm.answering.collectAsStateWithLifecycle()
    val answeredAsks by vm.answeredAsks.collectAsStateWithLifecycle()
    val answeredDone by vm.answeredDone.collectAsStateWithLifecycle()
    val pending = thread.pendingApproval
    // The choice tapped on this card, spinning until it goes.
    fun tapped(id: String) = answering?.takeIf { it.first == id }?.second
    val lastSeq = messages.lastOrNull()?.seq
    val asks = thread.asks.orEmpty()
    val showsAsks = asks.isNotEmpty() && nothingOnItsWay && answeredAsks != thread.asksSeq && thread.waitingOnMike
    val done = thread.status == ThreadStatus.IDLE && !thread.needsYou && thread.archivedAt == null && thread.working == null &&
        Turns.lastShown(messages)?.role == Role.ASSISTANT && nothingOnItsWay && answeredDone != lastSeq
    var picks by remember(thread.asksSeq) { mutableStateOf(mapOf<Int, Int>()) }
    var ticked by remember(lastSeq) { mutableStateOf(setOf<Int>()) }
    val plan = remember(pending, messages) { Plans.plan(pending, messages) }
    when {
        pending is PendingApproval.Approval && plan != null -> PlanApprovalCard(pending, plan, tapped(pending.id)) { vm.respond(pending.id, it) }
        pending is PendingApproval.Approval -> ApprovalCard(pending, tapped(pending.id)) { vm.respond(pending.id, it) }
        pending is PendingApproval.Questions -> QuestionCard(pending, tapped(pending.id)) { chosen, typed -> vm.answer(pending.id, pending.questions, chosen, typed) }
        pending is PendingApproval.Secret -> SecretCard(
            pending,
            com.mikerosoft.threadbear.kit.RunnerKeyPins.check(pending.key, model.runnerKey(thread.runnerId)),
            tapped(pending.id),
        ) { vm.secret(pending, thread.runnerId, it) }
        // A kind this build doesn't know.
        pending is PendingApproval.Other -> CardFrame(signals.needsYou) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Line.Laptop, null, Modifier.size(16.dp), tint = colors.onSurfaceVariant)
                Spacer(Modifier.width(8.dp))
                Text("Answer this on the Mac.", style = MaterialTheme.typography.bodyMedium, color = colors.onSurfaceVariant)
            }
        }
        // Its own id per status, so the seconds start again when a stopped one runs.
        thread.isBusy || thread.working != null -> androidx.compose.runtime.key(thread.status == ThreadStatus.RUNNING) { WorkingLine(thread.working) }
        showsAsks -> AsksCard(
            asks, picks, busy = answeredAsks == thread.asksSeq,
            pick = { i, c -> picks = if (c == null) picks - i else picks + (i to c) },
            send = { vm.sendAsks(thread, picks) },
        )
        thread.status == ThreadStatus.ERROR -> Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Line.Alert, null, Modifier.size(16.dp), tint = signals.failed)
            Spacer(Modifier.width(8.dp))
            Text(
                if (thread.waitingForNetwork != null) "Lost the connection. It carries on by itself." else "The last turn failed. Send a message to try again.",
                style = MaterialTheme.typography.bodyMedium, color = colors.onSurfaceVariant,
            )
        }
        done -> DoneCard(
            thread.next.orEmpty(), ticked, busy = answeredDone == lastSeq,
            tick = { i -> ticked = if (i in ticked) ticked - i else ticked + i },
            send = { val next = thread.next.orEmpty(); vm.sendFollowUps(ticked.sorted().mapNotNull { next.getOrNull(it) }, lastSeq) },
            archive = { model.change(thread.threadId, ThreadChange.Archive); leave() },
        )
        else -> Spacer(Modifier.height(1.dp))
    }
}
