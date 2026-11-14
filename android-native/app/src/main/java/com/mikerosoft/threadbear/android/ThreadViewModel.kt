package com.mikerosoft.threadbear.android

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.mikerosoft.threadbear.kit.Answers
import com.mikerosoft.threadbear.kit.ApprovalOption
import com.mikerosoft.threadbear.kit.AskAnswer
import com.mikerosoft.threadbear.kit.BackendApi
import com.mikerosoft.threadbear.kit.PendingApproval
import com.mikerosoft.threadbear.kit.PhoneBackend
import com.mikerosoft.threadbear.kit.PhoneThread
import com.mikerosoft.threadbear.kit.Question
import com.mikerosoft.threadbear.kit.RunnerKeyPins
import com.mikerosoft.threadbear.kit.SecretAnswer
import com.mikerosoft.threadbear.kit.SecretSeal
import com.mikerosoft.threadbear.kit.CommandState
import com.mikerosoft.threadbear.kit.InboxSections
import com.mikerosoft.threadbear.kit.MessageHistory
import com.mikerosoft.threadbear.kit.MessageImage
import com.mikerosoft.threadbear.kit.MessagePage
import com.mikerosoft.threadbear.kit.PhoneMessage
import com.mikerosoft.threadbear.kit.ThreadChange
import com.mikerosoft.threadbear.kit.ThreadDetail
import com.mikerosoft.threadbear.kit.TransportError
import com.mikerosoft.threadbear.kit.WaitingSend
import com.mikerosoft.threadbear.kit.WaitingSends
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.retryWhen
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the thread screen shows: still loading, the thread, or gone. */
sealed class Loaded {
    data object Loading : Loaded()
    data object Gone : Loaded()
    data class Here(val detail: ThreadDetail) : Loaded()
}

/** A message sent from here that the backend hasn't listed as waiting yet: shown at once. */
data class LocalSend(val key: Long, val text: String, val commandId: String? = null, val failed: String? = null)

/**
 * One open thread (the iPhone's ThreadView state): the thread's row, its
 * newest messages live, what's waiting to send, and sending and stopping.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class ThreadViewModel(private val model: AppModel, val threadId: String) : ViewModel() {
    private var markedSeen = false

    val loaded: StateFlow<Loaded> = model.backend.flatMapLatest { b ->
        if (b == null) flowOf(Loaded.Gone) else flow {
            // A thread made a moment ago can open before its row is written:
            // "not found" means "not yet" for a while before it means gone.
            var seen = false
            var waited = 0
            while (true) {
                var gone = false
                b.thread(threadId).collect { d ->
                    if (d == null) {
                        gone = true
                        if (seen || waited >= 15) emit(Loaded.Gone)
                    } else {
                        seen = true
                        emit(Loaded.Here(d))
                        markSeen(d)
                    }
                }
                if (!gone || seen || waited >= 15) break
                waited++
                delay(1000)
            }
        }.retryWhen { cause, _ -> if (cause is TransportError.Unauthorized) false else { delay(2000); true } }
            .catch { model.report(it) }
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), Loaded.Loading)

    /** API 2: the newest messages and tool lines, live from Convex. */
    val page: StateFlow<MessagePage?> = model.backend.flatMapLatest { b ->
        if (b == null || model.apiVersion in 1 until BackendApi.MESSAGE_PAGES) flowOf(null)
        else b.messages(threadId, FIRST_PAGE).retryWhen { cause, _ -> cause !is TransportError.Unauthorized && run { delay(2000); true } }
            .catch { model.report(it) }
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), null)

    /** API 57: this thread's messages from the phone the runner hasn't taken. */
    val waiting: StateFlow<List<WaitingSend>> = model.backend.flatMapLatest { b ->
        if (b == null || model.apiVersion in 1 until BackendApi.WAITING_SENDS) flowOf(emptyList())
        else b.waiting(threadId).retryWhen { cause, _ -> cause !is TransportError.Unauthorized && run { delay(2000); true } }
            .catch { emit(emptyList()) }
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), emptyList())

    /**
     * From API 2: the thread's messages as far back as it's been scrolled
     * (the kit's MessageHistory): the live page above, taken in by the
     * screen as it comes (takeLive, so the page stays a WhileSubscribed
     * query), and older chunks read once each (loadChunk). Published as a
     * copy after each change; null until the first page.
     */
    private val h = MessageHistory()
    val history = MutableStateFlow<MessageHistory?>(null)
    /** A chunk of older rows has been asked for and hasn't come yet. */
    val loadingChunk = MutableStateFlow(false)
    /** Before API 2: the oldest seq when Load earlier was tapped, until older messages arrive or the runner says it couldn't. */
    val loadingBefore = MutableStateFlow<Int?>(null)

    fun takeLive(page: MessagePage) {
        h.takeLive(page)
        history.value = h.copy()
    }

    /**
     * API 37: pictures of rows read once. A picture can finish uploading
     * after its row went into a chunk or slid off the live page, and neither
     * is read again. This follows only from the first row read once.
     */
    val lateImages: StateFlow<List<MessageImage>> = combine(model.backend, history.map { it?.lateImagesFrom }.distinctUntilChanged()) { b, from -> b to from }
        .flatMapLatest { (b, from) ->
            if (b == null || from == null || model.apiVersion in 1 until BackendApi.IMAGES) flowOf(emptyList())
            else b.images(threadId, from).retryWhen { cause, _ -> cause !is TransportError.Unauthorized && run { delay(2000); true } }
                .catch { emit(emptyList()) }
        }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), emptyList())

    fun takeLateImages(images: List<MessageImage>) {
        if (h.live == null) return
        h.takeLateImages(images)
        history.value = h.copy()
    }

    /** Older rows than the live page holds, read once (MessageHistory). */
    fun loadChunk() {
        val b = model.backend.value ?: return
        if (loadingChunk.value) return
        val before = h.nextBefore ?: return
        loadingChunk.value = true
        viewModelScope.launch {
            try {
                val chunk = b.olderMessages(threadId, before, MORE_PER_PAGE)
                if (h.takeOlder(chunk, before)) history.value = h.copy()
            } catch (e: Exception) {
                model.report(e)
            } finally {
                loadingChunk.value = false
            }
        }
    }

    /**
     * A backend before API 2: the runner pages older messages in, and they
     * turn up in the thread's own query; this only watches for a failure,
     * or the Mac being asleep.
     */
    fun loadEarlier(before: Int?) {
        val b = model.backend.value ?: return
        if (before == null || loadingBefore.value == before) return
        loadingBefore.value = before
        viewModelScope.launch {
            try {
                val id = b.loadEarlier(threadId, before)
                val status = b.command(id).first { it == null || it.state == CommandState.DONE || it.state == CommandState.FAILED }
                if (status?.state == CommandState.FAILED) {
                    model.note(status.note ?: "The Mac couldn't send older messages.")
                    loadingBefore.value = null
                    return@launch
                }
            } catch (e: Exception) {
                model.report(e)
            }
            // Done (or gone): if nothing older arrived, let it be tapped again.
            delay(2000)
            if (loadingBefore.value == before) loadingBefore.value = null
        }
    }

    val locals = MutableStateFlow<List<LocalSend>>(emptyList())
    private var nextKey = 0L

    private fun markSeen(d: ThreadDetail) {
        val t = d.thread
        if ((t.unread || t.needsYou) && !InboxSections.needsAnswer(t) && !markedSeen) {
            // Opening it reads it, as on the Mac. Once per visit.
            markedSeen = true
            viewModelScope.launch { runCatching { model.backend.value?.update(threadId, ThreadChange.MarkSeen) } }
        } else if (!t.unread) {
            markedSeen = false
        }
    }

    /**
     * Sends what's typed. It shows straight away under the transcript, then
     * as the backend's waiting row, then as the message itself.
     */
    fun send(
        text: String,
        answers: List<AskAnswer> = emptyList(),
        photos: List<PickedPhoto> = emptyList(),
        onSent: () -> Unit = {},
        onFailed: (String) -> Unit,
    ) {
        val b = model.backend.value ?: return
        val words = text.trim()
        if (words.isEmpty() && photos.isEmpty()) return
        val key = nextKey++
        // With photos, as the transcript will have it: "[1 picture]" for the ones no marker places.
        val shown = if (photos.isEmpty()) words
            else WaitingSends.mirrored(WaitingSend("", CommandState.PENDING, words, photos.size, 0.0))
        locals.update { it + LocalSend(key, shown) }
        viewModelScope.launch {
            try {
                // Up already, as Send waits for them; one that failed gets a fresh try.
                val images = model.upload(photos)
                val id = b.send(threadId, words, answers, images)
                sent(photos)
                onSent()
                locals.update { list -> list.map { if (it.key == key) it.copy(commandId = id) else it } }
                watch(key, id)
            } catch (e: Exception) {
                locals.update { list -> list.filterNot { it.key == key } }
                model.report(e)
                onFailed(words)
            }
        }
    }

    // Cards. A tapped answer spins until its card goes (the thread's row
    // moves on), and comes back with why if the runner refuses it.

    /** The card being answered and the choice tapped on it. */
    val answering = MutableStateFlow<Pair<String, String>?>(null)
    /** The card of asks (by its asksSeq) and the done card (by the reply's seq) answered from here. */
    val answeredAsks = MutableStateFlow<Double?>(null)
    val answeredDone = MutableStateFlow<Int?>(null)

    private fun answerCard(cardId: String, choice: String, call: suspend (PhoneBackend) -> String) {
        val b = model.backend.value ?: return
        if (answering.value?.first == cardId) return
        answering.value = cardId to choice
        viewModelScope.launch {
            try {
                val status = b.command(call(b)).first { it == null || it.state != CommandState.PENDING && it.state != CommandState.TAKEN }
                if (status?.state == CommandState.FAILED) {
                    model.note("Didn't go through: ${status.note ?: "the runner refused it"}")
                    if (answering.value?.first == cardId) answering.value = null
                }
            } catch (e: Exception) {
                model.report(e)
                if (answering.value?.first == cardId) answering.value = null
            }
        }
    }

    fun respond(approvalId: String, option: ApprovalOption) =
        answerCard(approvalId, option.wire) { it.respond(threadId, approvalId, option) }

    fun answer(approvalId: String, questions: List<Question>, chosen: Map<Int, Set<String>>, typed: String) {
        val answers = Answers.build(questions, chosen, typed)
        if (answers.isEmpty()) return
        answerCard(approvalId, "answer") { it.answer(threadId, approvalId, answers, Answers.summary(questions, answers)) }
    }

    /** Sealed here to the paired Mac's key; refused if the card's key isn't that one. */
    fun secret(card: PendingApproval.Secret, runnerId: String, value: String?) {
        if (value == null) {
            answerCard(card.id, "decline") { it.secret(threadId, card.id, SecretAnswer.Decline) }
            return
        }
        if (RunnerKeyPins.check(card.key, model.runnerKey(runnerId)) != RunnerKeyPins.Verdict.TRUSTED) {
            model.note("This Mac's key isn't one this phone was paired with. Answer it on the Mac.")
            return
        }
        val sealed = try { SecretSeal.seal(value, card.key, card.id) } catch (e: Exception) { model.report(e); return }
        answerCard(card.id, "secret") { it.secret(threadId, card.id, SecretAnswer.Sealed(sealed)) }
    }

    /** The picked answers on the card of asks, with anything typed, as the Mac's one message. */
    fun sendAsks(thread: PhoneThread, picks: Map<Int, Int>, extra: String = "", onFailed: (String) -> Unit = {}) {
        val asks = thread.asks ?: return
        val answers = AskAnswer.answers(asks, picks).map { it.second }
        if (answers.isEmpty() && extra.isBlank()) return
        answeredAsks.value = thread.asksSeq
        send(AskAnswer.message(asks, picks, extra), answers, onFailed = { answeredAsks.value = null; onFailed(extra) })
    }

    fun sendFollowUps(followUps: List<String>, lastSeq: Int?) {
        if (followUps.isEmpty()) return
        answeredDone.value = lastSeq
        send(AskAnswer.followUpMessage(followUps), onFailed = { answeredDone.value = null })
    }

    /** Until the runner has it: a refusal says why and gives the words back. */
    private suspend fun watch(key: Long, commandId: String) {
        val b = model.backend.value ?: return
        val status = try {
            b.command(commandId).first { it == null || it.state != CommandState.PENDING }
        } catch (_: Exception) {
            null
        }
        if (status?.state == CommandState.FAILED) {
            // Refused: a card it answered comes back.
            answeredAsks.value = null
            answeredDone.value = null
        }
        if (status?.state == CommandState.FAILED && status.note != "Cancelled") {
            model.note("Didn't go through: ${status.note ?: "the runner refused it"}")
        } else if (status != null) {
            // The waiting list or the transcript has it by now.
            delay(1500)
        }
        locals.update { list -> list.filterNot { it.key == key } }
    }

    val cancelling = MutableStateFlow<Set<String>>(emptySet())

    /** Takes a waiting message back; its words go back in the box. */
    fun cancel(send: WaitingSend, giveBack: (String) -> Unit) {
        val b = model.backend.value ?: return
        if (send.commandId in cancelling.value) return
        cancelling.update { it + send.commandId }
        viewModelScope.launch {
            try {
                if (b.cancel(send.commandId)) {
                    locals.update { list -> list.filterNot { it.commandId == send.commandId } }
                    giveBack(send.text)
                } else {
                    model.note("Too late to cancel: it's already gone to the Mac.")
                }
            } catch (e: Exception) {
                model.report(e)
            } finally {
                cancelling.update { it - send.commandId }
            }
        }
    }

    companion object {
        const val FIRST_PAGE = 80
        const val MORE_PER_PAGE = 200
    }
}
