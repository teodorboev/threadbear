package com.mikerosoft.threadbear.android.ui

import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.VisibilityThreshold
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyItemScope
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.draw.scale
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.mikerosoft.threadbear.android.LocalModel
import com.mikerosoft.threadbear.android.LocalSnackbar
import com.mikerosoft.threadbear.kit.BackendApi
import com.mikerosoft.threadbear.kit.InboxSections
import com.mikerosoft.threadbear.kit.PhoneProject
import com.mikerosoft.threadbear.kit.PhoneSection
import com.mikerosoft.threadbear.kit.PhoneThread
import com.mikerosoft.threadbear.kit.ProjectMark
import com.mikerosoft.threadbear.kit.RelativeTime
import com.mikerosoft.threadbear.kit.RowStatus
import com.mikerosoft.threadbear.kit.RunnerOrder
import com.mikerosoft.threadbear.kit.ScheduleWake
import com.mikerosoft.threadbear.kit.StartingThread
import com.mikerosoft.threadbear.kit.ThreadChange
import com.mikerosoft.threadbear.kit.ThreadSearch
import com.mikerosoft.threadbear.kit.changedHeadings
import com.mikerosoft.threadbear.kit.isInboxZero
import java.time.Instant
import kotlinx.coroutines.delay

/** Now, every few seconds: the rows' times and working clocks follow it. */
@Composable
fun rememberNow(everyMs: Long = 5000): Instant {
    val now by produceState(Instant.now()) {
        while (true) { delay(everyMs); value = Instant.now() }
    }
    return now
}

/** What the rows around the inbox need to draw and act. */
private class RowContext(
    val now: Instant,
    val projects: Map<String, PhoneProject>,
    val wakes: Map<String, ScheduleWake>,
    val open: (String) -> Unit,
    val hibernate: (PhoneThread) -> Unit,
    /** Rows glide to their new places only when a thread changed heading (InboxMoves). */
    val glide: Boolean,
)

@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
fun InboxScreen(
    open: (String) -> Unit,
    newThread: () -> Unit = {},
    settings: () -> Unit = {},
    pocket: () -> Unit = {},
) {
    val model = LocalModel.current
    val shell = model.shell
    val inbox by model.inbox.collectAsStateWithLifecycle()
    val wakes by model.wakes.collectAsStateWithLifecycle()
    val counts by model.counts.collectAsStateWithLifecycle()
    val folds by model.folds.collectAsStateWithLifecycle()
    val connected by model.connected.collectAsStateWithLifecycle()
    val projects by model.projects.collectAsStateWithLifecycle()
    val runners by model.runners.collectAsStateWithLifecycle()
    // Read so the Pocket button and the star follow the backend's version.
    val identity by model.identity.collectAsStateWithLifecycle()
    val snoozed by model.putAway(PhoneSection.HIBERNATING).collectAsStateWithLifecycle()
    val den by model.putAway(PhoneSection.ARCHIVED).collectAsStateWithLifecycle()
    val archiving by shell.archiving.collectAsStateWithLifecycle()
    val tucks by shell.tucks.collectAsStateWithLifecycle()
    val back by shell.bringingBack.collectAsStateWithLifecycle()
    val starting by shell.starting.collectAsStateWithLifecycle()
    val cards by shell.pocket.collectAsStateWithLifecycle()
    val seen by shell.pocketSeenUpTo.collectAsStateWithLifecycle()
    val now = rememberNow()
    val scroll = TopAppBarDefaults.pinnedScrollBehavior()
    var menu by remember { mutableStateOf(false) }
    var query by rememberSaveable { mutableStateOf("") }
    var hibernating by remember { mutableStateOf<PhoneThread?>(null) }

    LaunchedEffect(inbox) { inbox?.let { shell.inboxArrived(it) } }
    LaunchedEffect(counts) { shell.countsArrived(counts?.hibernating) }

    val wakeMap = remember(wakes) { InboxSections.wakes(wakes) }
    val sections = remember(inbox, wakeMap, archiving, tucks, now) { shell.sections(inbox ?: emptyList(), now, wakeMap) }
    // A list update that only changes a title or a working line leaves the rows
    // still, as they can come in bursts; a thread changing heading glides.
    // Held outside Compose state, so noting it doesn't redraw the list and
    // switch the glide off mid-move.
    val previous = remember { arrayOfNulls<InboxSections>(1) }
    val glide = remember(sections) { previous[0]?.changedHeadings(sections) ?: false }
    SideEffect { previous[0] = sections }
    val projectById = remember(projects) { projects.associateBy { it.id } }
    val rows = RowContext(now, projectById, wakeMap, open, { hibernating = it }, glide)
    val searching = query.isNotBlank()
    // The button shows once the backend has a Pocket, or there are cards from before.
    val showsPocket = cards.isNotEmpty() || (identity?.apiVersion ?: 0) >= BackendApi.POCKET

    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        snackbarHost = { SnackbarHost(LocalSnackbar.current) },
        topBar = {
            Column(Modifier.background(MaterialTheme.colorScheme.background)) {
                TopAppBar(
                    navigationIcon = {
                        Box {
                            val update by com.mikerosoft.threadbear.android.Updates.available.collectAsStateWithLifecycle()
                            val context = androidx.compose.ui.platform.LocalContext.current
                            IconButton(onClick = { menu = true }) {
                                // A small accent dot when a newer release is out: the menu says what.
                                Box {
                                    Icon(Line.Menu, if (update != null) "Menu, update available" else "Menu", Modifier.size(22.dp))
                                    if (update != null) Box(Modifier.align(Alignment.TopEnd).size(7.dp).clip(CircleShape).background(LocalSignals.current.accent))
                                }
                            }
                            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                                ActionItem("Settings", Line.Settings) { menu = false; settings() }
                                update?.let { u ->
                                    ActionItem("Update to ${com.mikerosoft.threadbear.android.Updates.label(u.date)}", Line.Download) {
                                        menu = false; com.mikerosoft.threadbear.android.Updates.open(context, u)
                                    }
                                }
                            }
                        }
                    },
                    title = {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text("Threadbear")
                            val n = sections.needsYouCount
                            if (n > 0) {
                                Spacer(Modifier.width(10.dp))
                                CountBadge(n)
                            }
                        }
                    },
                    actions = {
                        if (!connected && inbox != null) {
                            IconButton(onClick = { model.note("Not connected. Showing what came last; it catches up by itself.") }) {
                                Icon(Line.CloudOff, "Not connected", Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                        if (showsPocket) PocketButton(cards.size, shell.pocketHasNew(cards, seen), pocket)
                        IconButton(onClick = newThread) { Icon(Line.SquarePen, "New thread", Modifier.size(21.dp)) }
                    },
                    colors = TopAppBarDefaults.topAppBarColors(
                        containerColor = MaterialTheme.colorScheme.background,
                        scrolledContainerColor = MaterialTheme.colorScheme.background,
                    ),
                    scrollBehavior = scroll,
                )
                // Always showing, rather than tucked away until asked for: on the
                // phone it's the quickest way to an old thread.
                SearchField(query) { query = it }
            }
        },
    ) { inner ->
        if (inbox == null && !searching) {
            Column(Modifier.padding(inner).fillMaxWidth()) {
                LinearProgressIndicator(Modifier.fillMaxWidth().height(1.dp), color = MaterialTheme.colorScheme.outline)
            }
            return@Scaffold
        }
        val padding = PaddingValues(top = inner.calculateTopPadding(), bottom = inner.calculateBottomPadding() + 24.dp)
        if (searching) {
            SearchResults(query, sections, wakeMap, rows, padding, back)
            return@Scaffold
        }
        val offline = RunnerOrder.offlineText(runners, now)
        LazyColumn(Modifier.fillMaxSize(), contentPadding = padding) {
            if (offline != null) item(key = "offline") { OfflineNote(offline) }
            items(starting, key = { "s:${it.id}" }) { row ->
                StartingRow(row, runners, now, Modifier.animateItem()) {
                    shell.retryStarting(row)
                    newThread()
                }
            }
            fun heading(section: PhoneSection, count: Int?) {
                stickyHeader(key = "h:${section.name}") {
                    Heading(section, count ?: 0, folds.isFolded(section)) { model.toggle(section) }
                }
            }
            // Empty sections are left out, Inbox aside, which has the empty view
            // instead. A folded one with no count yet (an older backend) still
            // shows its heading, so there's a way to open it.
            if (sections.starred.isNotEmpty()) {
                heading(PhoneSection.STARRED, sections.starred.size)
                if (!folds.isFolded(PhoneSection.STARRED)) threadRows(sections.starred, PhoneSection.STARRED, rows)
            }
            if (sections.inbox.isNotEmpty()) {
                heading(PhoneSection.INBOX, null)
                threadRows(sections.inbox, PhoneSection.INBOX, rows)
            } else if (sections.isInboxZero) {
                // In the Inbox's place, so anything still running sits under it.
                item(key = "zero") { InboxZero(Modifier.animateItem()) }
            }
            if (sections.running.isNotEmpty()) {
                heading(PhoneSection.RUNNING, sections.running.size)
                if (!folds.isFolded(PhoneSection.RUNNING)) threadRows(sections.running, PhoneSection.RUNNING, rows)
            }
            // The scheduled ones are in the inbox list already; the snoozed ones
            // load once it's opened. Folded, phone:counts says how many, plus
            // any hibernated from here it hasn't counted yet.
            val asleep = InboxSections.hibernating(sections.scheduled, (snoozed ?: emptyList()).filter { it.threadId !in back }, wakeMap)
            val asleepCount = (if (snoozed != null) asleep.size else counts?.hibernating?.plus(sections.scheduled.size))?.plus(tucks.uncounted)
            if (asleepCount != 0) {
                heading(PhoneSection.HIBERNATING, asleepCount)
                if (!folds.isFolded(PhoneSection.HIBERNATING)) {
                    hibernatingRows(asleep, rows)
                    if (snoozed == null) item(key = "l:h") { LoadingRow() }
                }
            }
            // The den list is capped at 100; the count isn't.
            val denCount = counts?.den ?: den?.size
            if (denCount != 0) {
                heading(PhoneSection.ARCHIVED, denCount)
                if (!folds.isFolded(PhoneSection.ARCHIVED)) {
                    val list = den
                    if (list == null) item(key = "l:a") { LoadingRow() }
                    else putAwayRows(list.filter { it.threadId !in back }, PhoneSection.ARCHIVED, rows)
                }
            }
        }
    }

    hibernating?.let { t -> HibernateDialog(t) { hibernating = null } }
}

/** What the search found, under the same headings as the inbox, the put-away threads too. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun SearchResults(
    query: String, sections: InboxSections, wakes: Map<String, ScheduleWake>, rows: RowContext,
    padding: PaddingValues, back: Set<String>,
) {
    val shell = LocalModel.current.shell
    // Only while a search is on: it looks in the put-away lists too.
    val asleep by shell.searchHibernating.collectAsStateWithLifecycle()
    val archived by shell.searchArchived.collectAsStateWithLifecycle()
    val text = query.trim()
    // The phone has only the newest 100 archived threads, so Convex looks
    // through the rest, once typing pauses.
    var older by remember { mutableStateOf<List<PhoneThread>>(emptyList()) }
    LaunchedEffect(text) {
        older = emptyList()
        delay(300)
        older = shell.searchOlderArchived(text)
    }
    val search = remember(text, sections, asleep, archived, older, wakes, back) {
        ThreadSearch.of(text, sections, asleep.filter { it.threadId !in back }, archived.filter { it.threadId !in back }, older, wakes)
    }
    LazyColumn(Modifier.fillMaxSize(), contentPadding = padding) {
        if (search.isEmpty) {
            item(key = "none") {
                Column(Modifier.fillMaxWidth().padding(vertical = 48.dp, horizontal = 32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Icon(Line.Search, null, Modifier.size(28.dp), tint = LocalSignals.current.faint)
                    Spacer(Modifier.height(10.dp))
                    Text("No results for “$text”", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(4.dp))
                    Text("Check the spelling or try a new search.", style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        fun heading(section: PhoneSection) {
            stickyHeader(key = "h:${section.name}") { Heading(section, 0, folded = false, foldable = false) {} }
        }
        if (search.starred.isNotEmpty()) { heading(PhoneSection.STARRED); threadRows(search.starred, PhoneSection.STARRED, rows) }
        if (search.inbox.isNotEmpty()) { heading(PhoneSection.INBOX); threadRows(search.inbox, PhoneSection.INBOX, rows) }
        if (search.running.isNotEmpty()) { heading(PhoneSection.RUNNING); threadRows(search.running, PhoneSection.RUNNING, rows) }
        if (search.hibernating.isNotEmpty()) { heading(PhoneSection.HIBERNATING); hibernatingRows(search.hibernating, rows) }
        if (search.archived.isNotEmpty()) { heading(PhoneSection.ARCHIVED); putAwayRows(search.archived, PhoneSection.ARCHIVED, rows) }
    }
}

/**
 * Each row's key says its heading too, so a row changing heading is one
 * going and another coming, as on the iPhone.
 */
private fun LazyListScope.threadRows(threads: List<PhoneThread>, under: PhoneSection, rows: RowContext) {
    items(threads, key = { "${under.name}:${it.threadId}" }) { t -> InboxRow(t, under, rows, animated(rows.glide)) }
}

/**
 * Under Hibernating, snoozed threads wake with a swipe. One only waiting on
 * a schedule isn't snoozed, so it gets an inbox row that says when the
 * schedule runs, and can be hibernated for real.
 */
private fun LazyListScope.hibernatingRows(threads: List<PhoneThread>, rows: RowContext) {
    items(threads, key = { "${PhoneSection.HIBERNATING.name}:${it.threadId}" }) { t ->
        if (InboxSections.isSnoozed(t, rows.now)) PutAwayRow(t, PhoneSection.HIBERNATING, rows, animated(rows.glide))
        else InboxRow(t, PhoneSection.HIBERNATING, rows, animated(rows.glide))
    }
}

private fun LazyListScope.putAwayRows(threads: List<PhoneThread>, under: PhoneSection, rows: RowContext) {
    items(threads, key = { "${under.name}:${it.threadId}" }) { t -> PutAwayRow(t, under, rows, animated(rows.glide)) }
}

/** Rows fade in and out; they slide to new places only when a thread changed heading. */
private fun LazyItemScope.animated(glide: Boolean): Modifier =
    Modifier.animateItem(placementSpec = if (glide) spring(stiffness = Spring.StiffnessMediumLow, visibilityThreshold = IntOffset.VisibilityThreshold) else null)

@Composable
private fun SearchField(query: String, change: (String) -> Unit) {
    val signals = LocalSignals.current
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, bottom = 8.dp)
            .clip(RoundedCornerShape(12.dp)).background(MaterialTheme.colorScheme.surfaceContainerHigh)
            .padding(horizontal = 12.dp).height(40.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Line.Search, null, Modifier.size(17.dp), tint = signals.faint)
        Spacer(Modifier.width(8.dp))
        Box(Modifier.weight(1f), contentAlignment = Alignment.CenterStart) {
            if (query.isEmpty()) Text("Search", style = MaterialTheme.typography.bodyLarge, color = signals.faint)
            BasicTextField(
                query, change, singleLine = true,
                textStyle = MaterialTheme.typography.bodyLarge.copy(color = MaterialTheme.colorScheme.onSurface),
                cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        if (query.isNotEmpty()) {
            Icon(Line.Close, "Clear", Modifier.size(18.dp).clip(CircleShape).clickable { change("") }, tint = signals.faint)
        }
    }
}

/**
 * The Pocket's button, with how many cards are in it: grey once they've all
 * been seen, the accent when one's been filed or changed since.
 */
@Composable
private fun PocketButton(count: Int, hasNew: Boolean, open: () -> Unit) {
    val description = when {
        count == 0 -> "Pocket, empty"
        else -> "Pocket, $count ${if (count == 1) "card" else "cards"}${if (hasNew) ", new since you last looked" else ""}"
    }
    IconButton(onClick = open) {
        Box {
            Icon(Line.Wallet, description, Modifier.size(21.dp))
            if (count > 0) {
                Box(
                    Modifier.align(Alignment.TopEnd).offset(x = 8.dp, y = (-6).dp)
                        .defaultMinSize(minWidth = 15.dp, minHeight = 15.dp)
                        .clip(RoundedCornerShape(50))
                        .background(if (hasNew) LocalSignals.current.accent else Color(0xFF8E8E93))
                        .padding(horizontal = 4.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text("$count", color = Color.White, fontSize = 10.sp, fontWeight = FontWeight.Bold, lineHeight = 15.sp)
                }
            }
        }
    }
}

@Composable
private fun CountBadge(n: Int) {
    Box(
        Modifier.clip(CircleShape).background(LocalSignals.current.needsYou.copy(alpha = 0.18f)).padding(horizontal = 8.dp, vertical = 1.dp),
    ) {
        Text("$n", style = MaterialTheme.typography.labelMedium, color = LocalSignals.current.needsYou)
    }
}

@Composable
private fun Heading(section: PhoneSection, count: Int, folded: Boolean, foldable: Boolean = section != PhoneSection.INBOX, toggle: () -> Unit) {
    val turn by animateFloatAsState(if (folded) -90f else 0f, label = "chevron")
    Row(
        Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.background)
            .clickable(enabled = foldable, onClick = toggle)
            .padding(start = 20.dp, end = 16.dp, top = 14.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(section.title, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
        if (count > 0 && foldable) {
            Spacer(Modifier.width(6.dp))
            Text("$count", style = MaterialTheme.typography.labelLarge, color = LocalSignals.current.faint)
        }
        Spacer(Modifier.weight(1f))
        if (foldable) {
            Icon(Line.ChevronDown, if (folded) "Open" else "Fold", Modifier.size(16.dp).rotate(turn), tint = LocalSignals.current.faint)
        }
    }
}

/** "Mac offline since 10:42": anything sent meanwhile waits until it's back. */
@Composable
private fun OfflineNote(text: String) {
    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 16.dp, top = 8.dp, bottom = 4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(Line.Moon, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.width(8.dp))
            Text(text, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        Text("Anything you send waits until it's back.", style = MaterialTheme.typography.bodySmall, color = LocalSignals.current.faint,
            modifier = Modifier.padding(start = 24.dp, top = 2.dp))
    }
}

/**
 * "Nothing in the inbox". Having just taken the last row away, the tick
 * pops in, as the iPhone's bear stands and waves: a reward for clearing it,
 * not a launch animation.
 */
@Composable
private fun InboxZero(modifier: Modifier = Modifier) {
    val shell = LocalModel.current.shell
    // Asked once, as it appears: only a clearing just now celebrates.
    val celebrating = remember { shell.takeInboxCleared() }
    val pop = remember { Animatable(if (celebrating) 0.4f else 1f) }
    LaunchedEffect(Unit) {
        if (celebrating) {
            delay(300)
            pop.animateTo(1f, spring(dampingRatio = 0.45f, stiffness = 400f))
        }
    }
    Column(modifier.fillMaxWidth().padding(vertical = 40.dp, horizontal = 32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Icon(Line.Check, null, Modifier.size(28.dp).scale(pop.value), tint = LocalSignals.current.faint)
        Spacer(Modifier.height(8.dp))
        Text("Nothing in the inbox", style = MaterialTheme.typography.titleMedium)
        Spacer(Modifier.height(4.dp))
        Text("Threads that finish or need you turn up here.", style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** A grey stand-in while a list waits for its first answer, so it doesn't look empty. */
@Composable
private fun LoadingRow() {
    val pulse = androidx.compose.animation.core.rememberInfiniteTransition(label = "loading")
    val alpha by pulse.animateFloat(
        0.45f, 1f,
        androidx.compose.animation.core.infiniteRepeatable(androidx.compose.animation.core.tween(900), androidx.compose.animation.core.RepeatMode.Reverse),
        label = "alpha",
    )
    val fill = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = alpha)
    Column(Modifier.fillMaxWidth().padding(start = 20.dp, end = 16.dp, top = 6.dp, bottom = 6.dp)) {
        repeat(3) { i ->
            Row(Modifier.padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(38.dp).clip(RoundedCornerShape(10.dp)).background(fill))
                Spacer(Modifier.width(12.dp))
                Column(verticalArrangement = Arrangement.spacedBy(7.dp)) {
                    Box(Modifier.width(listOf(190, 150, 220)[i].dp).height(11.dp).clip(CircleShape).background(fill))
                    Box(Modifier.width(listOf(110, 90, 130)[i].dp).height(9.dp).clip(CircleShape).background(fill))
                }
            }
        }
    }
}

/**
 * A row in Starred, Inbox, Running, or waiting on a schedule: swipe left to
 * archive, right to hibernate (or wake, a starred one that's asleep), and
 * hold for the rest, as the iPhone's swipe actions and context menu.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalFoundationApi::class)
@Composable
private fun InboxRow(t: PhoneThread, under: PhoneSection, rows: RowContext, modifier: Modifier = Modifier) {
    val model = LocalModel.current
    // Only under Starred can a row be asleep: elsewhere it's in Hibernating.
    val asleep = under == PhoneSection.STARRED && InboxSections.isSnoozed(t, rows.now)
    var menu by remember { mutableStateOf(false) }
    val state = rememberSwipeToDismissBoxState()
    LaunchedEffect(state.currentValue) {
        when (state.currentValue) {
            SwipeToDismissBoxValue.EndToStart -> {
                model.change(t.threadId, ThreadChange.Archive)
                // Gone from the list at once; back in place if it couldn't go.
                if (t.threadId !in model.shell.archiving.value) state.reset()
            }
            SwipeToDismissBoxValue.StartToEnd -> {
                if (asleep) model.change(t.threadId, ThreadChange.Wake) else rows.hibernate(t)
                state.reset()
            }
            SwipeToDismissBoxValue.Settled -> {}
        }
    }
    Box(modifier) {
        SwipeToDismissBox(
            state = state,
            backgroundContent = {
                val leading = state.dismissDirection == SwipeToDismissBoxValue.StartToEnd
                Box(
                    Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainerHigh).padding(horizontal = 24.dp),
                    contentAlignment = if (leading) Alignment.CenterStart else Alignment.CenterEnd,
                ) {
                    val icon = when {
                        !leading -> Line.Archive
                        asleep -> Line.Sun
                        else -> Line.Moon
                    }
                    Icon(icon, null, Modifier.size(22.dp), tint = MaterialTheme.colorScheme.onSurface)
                }
            },
        ) {
            ThreadRow(
                t, rows.now, rows.projects,
                Modifier.background(MaterialTheme.colorScheme.background)
                    .combinedClickable(onClick = { rows.open(t.threadId) }, onLongClick = { menu = true }),
                underRunning = under == PhoneSection.RUNNING,
                wake = if (under == PhoneSection.HIBERNATING) rows.wakes[t.threadId] else null,
                asleep = asleep,
            )
        }
        RowMenu(t, asleep, menu, dismiss = { menu = false }, hibernate = { rows.hibernate(t) })
    }
}

/** A hibernating or archived row: open on a tap, and swipe left to bring it back. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun PutAwayRow(t: PhoneThread, under: PhoneSection, rows: RowContext, modifier: Modifier = Modifier) {
    val model = LocalModel.current
    val change = if (under == PhoneSection.ARCHIVED) ThreadChange.Unarchive else ThreadChange.Wake
    val state = rememberSwipeToDismissBoxState()
    LaunchedEffect(state.currentValue) {
        if (state.currentValue == SwipeToDismissBoxValue.EndToStart) {
            model.shell.bringBack(t.threadId, change)
            if (t.threadId !in model.shell.bringingBack.value) state.reset()
        }
    }
    SwipeToDismissBox(
        state = state,
        modifier = modifier,
        enableDismissFromStartToEnd = false,
        backgroundContent = {
            Box(
                Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainerHigh).padding(end = 24.dp),
                contentAlignment = Alignment.CenterEnd,
            ) {
                Icon(
                    if (under == PhoneSection.ARCHIVED) Line.InboxUp else Line.Sun,
                    if (under == PhoneSection.ARCHIVED) "Back to inbox" else "Wake",
                    Modifier.size(22.dp), tint = MaterialTheme.colorScheme.onSurface,
                )
            }
        },
    ) {
        ThreadRow(t, rows.now, rows.projects, Modifier.background(MaterialTheme.colorScheme.background).clickable { rows.open(t.threadId) })
    }
}

/**
 * A new thread the runner hasn't made yet: what was asked, and "Starting"
 * with a spinner where a working row's time goes, until the runner has made
 * it and its own row takes over. A failed one opens New thread again with
 * what was typed, or swipes away.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun StartingRow(row: StartingThread, runners: List<com.mikerosoft.threadbear.kit.PhoneRunner>, now: Instant, modifier: Modifier, retry: () -> Unit) {
    val shell = LocalModel.current.shell
    val signals = LocalSignals.current
    val failed = row.failed != null
    val state = rememberSwipeToDismissBoxState()
    LaunchedEffect(state.currentValue) {
        if (state.currentValue == SwipeToDismissBoxValue.EndToStart) shell.dropStarting(row.id)
    }
    SwipeToDismissBox(
        state = state,
        modifier = modifier,
        enableDismissFromStartToEnd = false,
        gesturesEnabled = failed,
        backgroundContent = {
            Box(
                Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainerHigh).padding(end = 24.dp),
                contentAlignment = Alignment.CenterEnd,
            ) { Icon(Line.Close, "Remove", Modifier.size(22.dp), tint = MaterialTheme.colorScheme.onSurface) }
        },
    ) {
        Row(
            Modifier.fillMaxWidth().background(MaterialTheme.colorScheme.background)
                .clickable(enabled = failed, onClick = retry)
                .padding(start = 20.dp, end = 16.dp, top = 10.dp, bottom = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            ProjectTile(null, "", 38.dp)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    row.title, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis,
                    color = if (failed) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(row.line(runners, now), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                    Spacer(Modifier.width(8.dp))
                    if (failed) {
                        Text("Failed", style = MaterialTheme.typography.labelMedium, color = signals.failed)
                    } else {
                        CircularProgressIndicator(Modifier.size(10.dp), color = signals.accent, strokeWidth = 1.5.dp)
                        Spacer(Modifier.width(6.dp))
                        Text("Starting", style = MaterialTheme.typography.labelMedium, color = signals.accent)
                    }
                }
            }
        }
    }
}

@Composable
fun ThreadRow(
    t: PhoneThread, now: Instant, projects: Map<String, PhoneProject>, modifier: Modifier = Modifier,
    /** Under the Running heading the time alone; elsewhere "Working 38s", as the iPhone. */
    underRunning: Boolean = false,
    /** The schedule that messages it next: the row says when, where the time would be. */
    wake: ScheduleWake? = null,
    /** Under Starred and hibernating: a moon says so, as there's no heading to. */
    asleep: Boolean = false,
) {
    val signals = LocalSignals.current
    val kind = RowStatus.kind(t)
    Row(
        modifier.fillMaxWidth().animateContentSize().padding(start = 6.dp, end = 16.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // The gutter, as the iPhone's row (and the Mac's ThreadRowLook): a
        // dot when it needs Mike, a tick when it finished unread with
        // nothing needed. Not for unread alone: a running thread is always
        // unread, and its working mark already says so.
        val done = kind == RowStatus.Kind.TIME && t.unread && !t.needsYou
        val needs = kind == RowStatus.Kind.NEEDS_YOU || (t.needsYou && kind != RowStatus.Kind.WORKING)
        Box(Modifier.width(14.dp), contentAlignment = Alignment.Center) {
            when {
                done -> Icon(Line.Check, "Done, unread", Modifier.size(10.dp), tint = signals.accent)
                needs -> Box(Modifier.size(7.dp).clip(CircleShape).background(signals.accent))
            }
        }
        ProjectTile(t.projects?.firstOrNull()?.let { projects[it.id] }, t.projects?.firstOrNull()?.title ?: t.folderName, 38.dp)
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    t.title.ifEmpty { "Untitled" },
                    style = MaterialTheme.typography.bodyLarge,
                    fontWeight = if (t.needsYou || kind == RowStatus.Kind.NEEDS_YOU || done) FontWeight.SemiBold else FontWeight.Normal,
                    maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false),
                )
                if (t.starredAt != null) {
                    Spacer(Modifier.width(4.dp))
                    Icon(Line.StarFilled, "Starred", Modifier.size(12.dp), tint = signals.faint)
                }
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                t.startedBy?.let {
                    Icon(Line.startedBy(it.kind), null, Modifier.size(12.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.width(4.dp))
                }
                val model = LocalModel.current
                val runners by model.runners.collectAsStateWithLifecycle()
                val runner = if (runners.size > 1) runners.firstOrNull { it.runnerId == t.runnerId }?.name else null
                val doing = t.working?.takeIf { t.isBusy || (t.pendingApproval == null && !t.waitingOnMike) }
                val line = listOfNotNull(InboxSections.wakeLine(t, now), t.placeText, runner, doing).joinToString(" · ")
                Text(line, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1,
                    overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                Spacer(Modifier.width(8.dp))
                // The trailing word on the second line, as the iPhone's row has it
                // (InboxView's trailing), so long titles get the whole width.
                val faint = signals.faint
                val (text, color) = when (kind) {
                    // The gutter dot says it, as on the Mac; the time stays.
                    RowStatus.Kind.NEEDS_YOU -> RelativeTime.short(t.lastActivityAt, now) to faint
                    RowStatus.Kind.WORKING -> RowStatus.workingLabel(t.lastActivityAt, now).let { if (underRunning) it else "Working $it" } to signals.accent
                    RowStatus.Kind.FAILED -> "Failed" to signals.failed
                    // Its turn lost the internet; it carries on by itself.
                    RowStatus.Kind.WAITING_FOR_CONNECTION -> RowStatus.text(t, now) to faint
                    RowStatus.Kind.STOPPED -> "Stopped" to MaterialTheme.colorScheme.onSurfaceVariant
                    // Back from hibernating by itself: "Woke up" until it's opened.
                    RowStatus.Kind.TIME -> when {
                        t.woke != null -> RowStatus.text(t, now) to signals.accent
                        wake?.nextRunAt != null -> RelativeTime.clock(wake.nextRunAt!!, now) to faint
                        else -> RowStatus.text(t, now) to faint
                    }
                }
                if (asleep) {
                    Icon(Line.Moon, "Hibernating", Modifier.size(12.dp), tint = faint)
                    Spacer(Modifier.width(6.dp))
                }
                if (kind == RowStatus.Kind.WORKING) {
                    WorkingDot()
                    Spacer(Modifier.width(6.dp))
                }
                Text(text, style = MaterialTheme.typography.labelMedium, color = color, maxLines = 1)
            }
        }
    }
}

/**
 * A project's tile (the iPhone's ProjectIconView): its picture, else the
 * title's first letter on a quiet square; a folder with no project at all.
 * The Mac's SF Symbol icons have no Android match, so they fall back to
 * the letter.
 */
@Composable
fun ProjectTile(project: PhoneProject?, title: String, size: androidx.compose.ui.unit.Dp) {
    val shape = RoundedRectangle(size)
    val picture = project?.icon?.url?.let { ProjectPictures.image(it) }
    if (picture != null) {
        androidx.compose.foundation.Image(
            picture, contentDescription = null,
            modifier = Modifier.size(size).clip(shape),
            contentScale = androidx.compose.ui.layout.ContentScale.Fit,
        )
        return
    }
    Box(
        Modifier.size(size).clip(shape).background(MaterialTheme.colorScheme.surfaceContainerHighest),
        contentAlignment = Alignment.Center,
    ) {
        if (project == null && title.isEmpty()) {
            Icon(Line.Folder, null, Modifier.size(size * 0.5f), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        } else {
            Text(
                ProjectMark.letter(project?.title ?: title),
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.85f),
            )
        }
    }
}

private fun RoundedRectangle(size: androidx.compose.ui.unit.Dp) = RoundedCornerShape(size * 0.26f)

/** A soft pulse in the accent, for a thread that's working. */
@Composable
fun WorkingDot(size: androidx.compose.ui.unit.Dp = 6.dp) {
    val pulse = androidx.compose.animation.core.rememberInfiniteTransition(label = "pulse")
    val alpha by pulse.animateFloat(
        initialValue = 0.35f, targetValue = 1f,
        animationSpec = androidx.compose.animation.core.infiniteRepeatable(
            androidx.compose.animation.core.tween(900), androidx.compose.animation.core.RepeatMode.Reverse,
        ),
        label = "alpha",
    )
    Box(Modifier.size(size).clip(CircleShape).background(LocalSignals.current.accent.copy(alpha = alpha)))
}

