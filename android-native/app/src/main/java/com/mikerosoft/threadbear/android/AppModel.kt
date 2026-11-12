package com.mikerosoft.threadbear.android

import android.content.Context
import com.mikerosoft.threadbear.kit.BackendApi
import com.mikerosoft.threadbear.kit.InboxSections
import com.mikerosoft.threadbear.kit.Pairing
import com.mikerosoft.threadbear.kit.PairingRequest
import com.mikerosoft.threadbear.kit.PhoneAccount
import com.mikerosoft.threadbear.kit.PhoneBackend
import com.mikerosoft.threadbear.kit.PhoneCounts
import com.mikerosoft.threadbear.kit.PhoneIdentity
import com.mikerosoft.threadbear.kit.PhoneProject
import com.mikerosoft.threadbear.kit.PhoneRunner
import com.mikerosoft.threadbear.kit.PhoneSection
import com.mikerosoft.threadbear.kit.PhoneThread
import com.mikerosoft.threadbear.kit.ScheduleWake
import com.mikerosoft.threadbear.kit.SectionFolds
import com.mikerosoft.threadbear.kit.ThreadChange
import com.mikerosoft.threadbear.kit.ThreadsView
import com.mikerosoft.threadbear.kit.TransportError
import com.mikerosoft.threadbear.kit.WaitingSends
import java.time.Instant
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.flatMapLatest
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.retryWhen
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.withLock

/**
 * The app's state, for as long as the process lives (the iPhone's
 * AppModel): the pairing, the backend over it, the live lists every screen
 * reads, and what's on its way (archives, stops) so taps show at once.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class AppModel(context: Context, private val scope: CoroutineScope) {
    private val appContext = context.applicationContext
    private val store = PairingStore(context)
    private val prefs = context.getSharedPreferences("app", Context.MODE_PRIVATE)
    private var transport: ConvexFfiTransport? = null

    private val _backend = MutableStateFlow<PhoneBackend?>(null)
    val backend: StateFlow<PhoneBackend?> = _backend
    private val _pairing = MutableStateFlow<Pairing?>(null)
    val pairing: StateFlow<Pairing?> = _pairing
    private val _identity = MutableStateFlow<PhoneIdentity?>(null)
    val identity: StateFlow<PhoneIdentity?> = _identity

    /** A pairing link waiting for a yes. */
    val pendingPair = MutableStateFlow<PairingRequest?>(null)
    /** Checking a pairing with phone:me. */
    val pairingBusy = MutableStateFlow(false)

    private val _notes = MutableSharedFlow<String>(extraBufferCapacity = 8)
    /** Short lines for the snackbar: what failed, or what to know. */
    val notes: SharedFlow<String> = _notes

    val folds = MutableStateFlow(SectionFolds.fromSaved(prefs.getStringSet("folds", null)?.toList()))

    /** Archived (or hibernated) from here, hidden until the list agrees. */
    val puttingAway = MutableStateFlow<Set<String>>(emptySet())
    /** Stop pressed, until the turn ends. */
    val stopping = MutableStateFlow<Set<String>>(emptySet())

    /** The inbox's and the shell's own state (ShellModel.kt): Pocket, Settings, row moves. */
    val shell: ShellModel by lazy { ShellModel(this, context, scope) }

    init {
        store.load()?.let { connect(it) }
    }

    val apiVersion: Int get() = _identity.value?.apiVersion ?: 0

    private fun connect(pairing: Pairing) {
        transport?.close()
        val t = ConvexFfiTransport(pairing.deploymentUrl)
        transport = t
        _pairing.value = pairing
        _backend.value = PhoneBackend(t, pairing.token)
        scope.launch {
            try {
                _identity.value = _backend.value?.me()
                registerPush()
            } catch (e: TransportError.Unauthorized) {
                report(e)
            } catch (_: Exception) {
                // Offline: the lists say so; me is asked again on the next launch.
            }
        }
    }

    /** Every list follows the backend, and starts again after a dropped connection. */
    private fun <T> follow(initial: T, read: (PhoneBackend) -> Flow<T>): StateFlow<T> =
        _backend.flatMapLatest { b ->
            if (b == null) flowOf(initial) else read(b).retryWhen { cause, _ ->
                if (cause is TransportError.Unauthorized) { report(cause); false } else { delay(2000); true }
            }.catch { }
        }.stateIn(scope, SharingStarted.WhileSubscribed(5000), initial)

    val inbox: StateFlow<List<PhoneThread>?> = follow(null) { b ->
        if (apiVersion in 1 until BackendApi.THREAD_CHANGES) b.threads(ThreadsView.INBOX) else b.inboxChanges()
    }
    val runners: StateFlow<List<PhoneRunner>> = follow(emptyList()) { it.runners() }
    val projects: StateFlow<List<PhoneProject>> = follow(emptyList()) { it.projects() }
    val counts: StateFlow<PhoneCounts?> = follow(null) { it.counts() }
    val wakes: StateFlow<List<ScheduleWake>> = follow(emptyList()) { it.wakes() }
    /** Every runner's logins and their models (backend API 20), for the new thread pickers. */
    val accounts: StateFlow<List<PhoneAccount>> = follow(emptyList()) { b ->
        if (apiVersion in 1 until BackendApi.MODELS) flowOf(emptyList()) else b.accounts()
    }
    val connected: StateFlow<Boolean> = _backend.flatMapLatest { it?.connected ?: flowOf(false) }
        .stateIn(scope, SharingStarted.Eagerly, false)

    /** A put-away list, only while its heading is open. */
    fun putAway(section: PhoneSection): StateFlow<List<PhoneThread>?> = sectionLists.getOrPut(section) {
        val view = section.view ?: return@getOrPut MutableStateFlow(emptyList())
        folds.flatMapLatest { f ->
            if (f.isFolded(section)) flowOf(null) else _backend.flatMapLatest { b ->
                b?.threads(view)?.retryWhen { _, _ -> delay(2000); true }?.catch { } ?: emptyFlow()
            }
        }.stateIn(scope, SharingStarted.WhileSubscribed(5000), null)
    }
    private val sectionLists = HashMap<PhoneSection, StateFlow<List<PhoneThread>?>>()

    fun toggle(section: PhoneSection) {
        folds.update { it.toggle(section) }
        prefs.edit().putStringSet("folds", folds.value.saved.toSet()).apply()
    }

    /** The key the Mac's pairing code gave for a runner: the only one a secret is sealed to. */
    fun runnerKey(runnerId: String): String? = _pairing.value?.let { store.runnerKey(it.serverName, runnerId) }

    fun project(id: String): PhoneProject? = projects.value.firstOrNull { it.id == id }

    fun offlineRunner(thread: PhoneThread): String? = WaitingSends.offlineRunner(thread.runnerId, runners.value, Instant.now())

    // Pairing

    /** A link from the camera, the clipboard or another app: asked about first. */
    fun ask(link: String): Boolean {
        val server = Pairing.parse(link, BuildConfig.ALLOW_LOCAL)?.serverName ?: return false
        val request = PairingRequest.of(link, BuildConfig.ALLOW_LOCAL, _pairing.value) { store.runnerKey(server, it) } ?: return false
        if (pendingPair.value == null) pendingPair.value = request
        return true
    }

    fun cancelPair() { pendingPair.value = null }

    /** Yes to the request on screen: checked with phone:me before anything is kept. */
    fun acceptPair(shown: PairingRequest) {
        if (pendingPair.value !== shown) return
        pendingPair.value = null
        pairingBusy.value = true
        scope.launch {
            val trial = ConvexFfiTransport(shown.pairing.deploymentUrl)
            try {
                val me = PhoneBackend(trial, shown.pairing.token).me()
                // Another server: the old one stops reaching this phone. The
                // same server with a new token keeps the registration, which
                // registerPush moves to the new token.
                val old = _pairing.value
                pairingChanged(forget = old != null && old.deploymentUrl != shown.pairing.deploymentUrl)
                store.save(shown.pairing)
                trial.close()
                connect(shown.pairing)
                _identity.value = me
                note("Paired with ${shown.host}")
                registerPush()
            } catch (e: Exception) {
                trial.close()
                report(e)
            } finally {
                pairingBusy.value = false
            }
        }
    }

    // Push

    /** The Firebase token the backend has for this phone, once registered (for Notify me). */
    private val _pushToken = MutableStateFlow<String?>(null)
    val pushToken: StateFlow<String?> = _pushToken

    /**
     * Hands the backend this phone's Firebase token and push key, as the
     * iPhone's didRegister: on each connect, after pairing, when the
     * permission's given, and when Firebase makes a new token. Not while
     * notifications aren't allowed (nothing would show), nor for a backend
     * that sends none (self-hosted). Quiet on failure: the next launch tries again.
     */
    fun registerPush(newToken: String? = null) {
        val b = _backend.value ?: return
        if (!Push.available || !Push.permitted(appContext)) return
        // Already registered this run: the resume check costs nothing then.
        if (newToken == null && _pushToken.value != null) return
        if (_identity.value?.sendsNotifications == false) return
        val epoch = pairingEpoch
        scope.launch {
            // One at a time with forgetting an old pairing's token (pairingChanged),
            // so a quick re-pair can't register the token being deleted, and work
            // for a pairing that's since changed is dropped, not finished.
            pushLock.withLock {
                if (epoch != pairingEpoch) return@withLock
                val token = newToken ?: Push.token() ?: return@withLock
                val key = PushKeyStore(appContext).loadOrCreate()?.let { android.util.Base64.encodeToString(it, android.util.Base64.NO_WRAP) }
                try {
                    b.registerDevice(token, "production", "android", key)
                    if (epoch == pairingEpoch) {
                        _pushToken.value = token
                        // Settings' Notify me picker shows once there's a token.
                        shell.pushToken.value = token
                        // As the iPhone: the phone's own choice goes with each
                        // registration (a backend before the setting notifies every time).
                        if (apiVersion >= com.mikerosoft.threadbear.kit.BackendApi.NOTIFICATION_SETTING) {
                            runCatching { b.setNotifications(token, shell.notifyMode.value) }
                        }
                    }
                } catch (_: Exception) {
                }
            }
        }
    }

    private val pushLock = kotlinx.coroutines.sync.Mutex() // fair: queued work runs in order
    @Volatile private var pairingEpoch = 0

    /**
     * The pairing changed (unpaired, or paired with something else): what's
     * queued for the old one is dropped, and with [forget] the old backend
     * loses this phone (Firebase deletes the token, so its next send comes
     * back UNREGISTERED and it drops the device) and the push key is made
     * anew, so the old backend's sealed notifications can't be opened.
     */
    private fun pairingChanged(forget: Boolean) {
        // Another server (or none): its notifications come down and any being
        // opened are dropped, before anything else of the new pairing's.
        Push.pairingChanged(appContext, forget = forget)
        pairingEpoch++
        _pushToken.value = null
        shell.pushToken.value = null
        if (forget) scope.launch { pushLock.withLock { Push.forget(appContext) } }
    }

    fun unpair() {
        store.clear()
        pairingChanged(forget = true)
        shell.clear()
        transport?.close()
        transport = null
        _backend.value = null
        _pairing.value = null
        _identity.value = null
    }

    // Changes, each showing at once

    fun change(threadId: String, change: ThreadChange) {
        // Archive and hibernate wait for the runner to say, as the iPhone's do (ShellModel).
        when (change) {
            ThreadChange.Archive -> return shell.archive(threadId)
            is ThreadChange.Snooze, ThreadChange.SnoozeUntilRestart -> return shell.hibernate(threadId, change)
            else -> {}
        }
        val b = _backend.value ?: return
        val hides = change is ThreadChange.Archive || change is ThreadChange.ArchiveWithVerdict || change is ThreadChange.Snooze
        if (hides) puttingAway.update { it + threadId }
        scope.launch {
            try {
                b.update(threadId, change)
                // Shown gone until the list has caught up, then let go.
                delay(8000)
            } catch (e: Exception) {
                report(e)
            } finally {
                if (hides) puttingAway.update { it - threadId }
            }
        }
    }

    fun stop(threadId: String) {
        val b = _backend.value ?: return
        if (threadId in stopping.value) return
        stopping.update { it + threadId }
        scope.launch {
            try {
                b.interrupt(threadId)
                // Spinning until the turn ends (stopped()), or a while at most.
                delay(30_000)
            } catch (e: Exception) {
                report(e)
            } finally {
                stopping.update { it - threadId }
            }
        }
    }

    fun stopped(threadId: String) = stopping.update { it - threadId }

    fun needsYouCount(threads: List<PhoneThread>) =
        InboxSections.build(threads, Instant.now(), InboxSections.wakes(wakes.value)).needsYouCount

    fun note(text: String) { _notes.tryEmit(text) }

    fun report(e: Throwable) {
        val text = when (e) {
            is TransportError -> e.message ?: "Something went wrong"
            else -> e.message ?: "Something went wrong"
        }
        note(text)
    }
}

