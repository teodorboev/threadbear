package com.mikerosoft.threadbear.kit

/**
 * A thread's messages on the phone (backend API 2; MessageHistory.swift), as
 * far back as you've scrolled: the newest ones live from phone:messages, and
 * older chunks asked for once each with beforeSeq.
 *
 * The live page stays the size it opened with. It used to grow by raising
 * its limit as you scrolled back, up to 2,000 rows, and a live query reads
 * its whole page again for every new message: a scrolled-back phone on the
 * launch thread cost 37.6 MB of Convex I/O in a day (8 Nov), more than a
 * free plan's daily share. Older rows come in chunks from where the shown
 * ones begin, read once, since past messages don't change.
 *
 * Mutable, as the Swift struct is mutated in place; [copy] gives a snapshot
 * to publish. Every list it holds is immutable and replaced, never changed,
 * so a copy shares them safely.
 */
class MessageHistory private constructor(
    live: MessagePage?,
    older: List<PhoneMessage>,
    olderImages: List<MessageImage>,
    olderHasMore: Boolean?,
    lateImages: List<MessageImage>,
    private var loadedRows: Int,
    private var loadedPictures: Int,
    private var loadedBytes: Int,
    private var lowest: Int?,
    stuck: Boolean,
    private val sizes: java.util.IdentityHashMap<PhoneMessage, Int>,
) {
    constructor() : this(null, emptyList(), emptyList(), null, emptyList(), 0, 0, 0, null, false, java.util.IdentityHashMap())

    /** The newest messages, live. */
    var live: MessagePage? = live
        private set
    /** Older messages from chunks, oldest first, all before `live`'s. */
    var older: List<PhoneMessage> = older
        private set
    var olderImages: List<MessageImage> = olderImages
        private set
    /** Whether there's more before `older`; null until a chunk has come. */
    var olderHasMore: Boolean? = olderHasMore
        private set
    /**
     * Pictures from phone:images (API 37) from `older`'s first row on: one
     * that finishes uploading after its row was read (into a chunk, or off
     * the live page's top) comes only from there.
     */
    var lateImages: List<MessageImage> = lateImages
        private set
    /**
     * A chunk brought nothing that stayed (it was all let go at once, or
     * empty), so no more are read: the phone loads by itself as Load
     * earlier comes into view, and would fetch the same rows forever.
     */
    var stuck: Boolean = stuck
        private set

    /** Shares the row sizes it has worked out, which only ever say what a row (by identity) costs. */
    fun copy() = MessageHistory(live, older, olderImages, olderHasMore, lateImages, loadedRows, loadedPictures, loadedBytes, lowest, stuck, sizes)

    /**
     * A row's [bytes], worked out once per row. Swift knows a string's UTF-8
     * length without walking it; here it's a walk, and capOlder asks about
     * every kept row on every update: 1,251 updates of 8 KB rows took 19 s
     * uncached. Keyed by identity, as `older`'s rows are the very objects the
     * pages brought, and let go with them.
     */
    private fun size(m: PhoneMessage): Int = synchronized(sizes) {
        sizes[m] ?: bytes(m).also {
            if (sizes.size > 2 * older.size + 1024) {
                val keep = java.util.Collections.newSetFromMap(java.util.IdentityHashMap<PhoneMessage, Boolean>()).apply { addAll(older) }
                sizes.keys.retainAll(keep)
            }
            sizes[m] = it
        }
    }

    val messages: List<PhoneMessage> get() = older + (live?.messages ?: emptyList())

    /** The pictures of every row shown, each once. */
    val images: List<MessageImage>
        get() {
            val seen = HashSet<String>()
            val shown = messages.mapTo(HashSet()) { it.seq }
            return (olderImages + (live?.images ?: emptyList()) + lateImages.filter { it.seq in shown }).filter { seen.add(it.url) }
        }

    val hasMore: Boolean get() = !stuck && (olderHasMore ?: (live?.hasMore ?: false))

    val firstSeq: Int? get() = older.firstOrNull()?.seq ?: live?.messages?.firstOrNull()?.seq

    /** Where phone:images should watch from: the first row read once, or null while the live page is all there is. */
    val lateImagesFrom: Int? get() = older.firstOrNull()?.seq

    /** The seq to ask for an older chunk before, or null when there's no more. */
    val nextBefore: Int? get() = if (hasMore) firstSeq else null

    private fun reset() {
        live = null; older = emptyList(); olderImages = emptyList(); olderHasMore = null; lateImages = emptyList()
        loadedRows = 0; loadedPictures = 0; loadedBytes = 0; lowest = null; stuck = false
    }

    /**
     * A new value of the live query. Rows that slide off its top as new ones
     * come in move to `older`, or there'd be a hole above the live ones.
     */
    fun takeLive(page: MessagePage) {
        val old = live
        val first = page.messages.firstOrNull()?.seq
        if (old != null && first != null) {
            val last = old.messages.lastOrNull()?.seq
            if (last != null && last < first) {
                // Nothing in common with the page before (the phone was away
                // while a whole page of new messages came): what's between
                // them is unknown, so start again from the newest rather
                // than show a hole.
                reset()
            } else {
                // Its top can also move back up (big rows above the byte cap
                // that got smaller): those rows are the live page's again.
                older = older.filter { it.seq < first }
                olderImages = olderImages.filter { it.seq < first }
                val after = older.lastOrNull()?.seq ?: Int.MIN_VALUE
                older = older + old.messages.filter { it.seq < first && it.seq > after }
                olderImages = olderImages + unseen(old.images.filter { it.seq < first && it.seq > after }, olderImages)
                capOlder()
            }
        }
        live = page
        if (first != null) lowest = minOf(lowest ?: first, first)
    }

    /**
     * Keeps one of each picture, then lets the oldest of `older` go until
     * it's within its allowance, with their pictures. Walks from the
     * newest, so the rows nearest the live page are the ones kept. A
     * picture counts with the row it goes after (the nearest at or before
     * its seq), and one before every row (or with no rows kept at all)
     * counts on its own, so no picture goes uncounted. Pictures came again
     * with every live page before (Codex's review): one at a seq below
     * the first row, with no older rows, was added once per update.
     */
    private fun capOlder() {
        val maxRows = OLDER_MAX_ROWS + loadedRows
        val maxPictures = OLDER_MAX_PICTURES + loadedPictures
        val maxBytes = OLDER_MAX_BYTES.toLong() + loadedBytes
        // Indexes into olderImages, newest first, so what's kept is marked
        // without hashing any picture's address.
        val order = olderImages.indices.sortedByDescending { olderImages[it].seq }
        val pictures = order.map { olderImages[it] }
        var kept = 0
        var pictureCount = 0
        var total = 0L
        var keepFrom = older.size
        while (keepFrom > 0) {
            val m = older[keepFrom - 1]
            var size = size(m).toLong()
            var n = 0
            while (kept + n < pictures.size && pictures[kept + n].seq >= m.seq) {
                size += bytes(pictures[kept + n])
                n++
            }
            if (older.size - keepFrom + 1 > maxRows || pictureCount + n > maxPictures || total + size > maxBytes) break
            total += size
            pictureCount += n
            kept += n
            keepFrom--
        }
        if (keepFrom == 0) {
            while (kept < pictures.size && pictureCount < maxPictures && total + bytes(pictures[kept]) <= maxBytes) {
                total += bytes(pictures[kept])
                pictureCount++
                kept++
            }
        }
        if (keepFrom == 0 && kept >= pictures.size) return
        if (keepFrom > 0) {
            older = older.subList(keepFrom, older.size).toList()
            val firstKept = older.firstOrNull()?.seq ?: Int.MAX_VALUE
            lateImages = lateImages.filter { it.seq >= firstKept }
            olderHasMore = true
        }
        val keep = BooleanArray(olderImages.size)
        for (i in order.subList(0, kept)) keep[i] = true
        olderImages = olderImages.filterIndexed { i, _ -> keep[i] }
    }

    fun takeLateImages(images: List<MessageImage>) {
        lateImages = images
    }

    /**
     * A chunk of rows before `before`. False, and nothing changes, if the
     * shown rows no longer start there (it came too late).
     */
    fun takeOlder(chunk: MessagePage, before: Int): Boolean {
        if (before != firstSeq) return false
        val rows = chunk.messages.filter { it.seq < before }
        val pictures = chunk.images.filter { it.seq < before }
        // What's older than anything held before is room on top of the
        // caps, up to LOADED_MAX times them; rows read again earn nothing.
        val floor = lowest ?: before
        val fresh = rows.filter { it.seq < floor }
        val freshPictures = pictures.filter { it.seq < floor }
        loadedRows = minOf(loadedRows + fresh.size, OLDER_MAX_ROWS * LOADED_MAX)
        loadedPictures = minOf(loadedPictures + freshPictures.size, OLDER_MAX_PICTURES * LOADED_MAX)
        val freshBytes = fresh.sumOf { bytes(it).toLong() } + freshPictures.sumOf { bytes(it).toLong() }
        loadedBytes = minOf(loadedBytes + freshBytes, OLDER_MAX_BYTES.toLong() * LOADED_MAX).toInt()
        rows.firstOrNull()?.seq?.let { lowest = minOf(floor, it) }
        older = rows + older
        olderImages = unseen(pictures, olderImages) + olderImages
        olderHasMore = chunk.hasMore
        capOlder()
        // Nothing it brought stayed: stop, rather than read it again.
        if ((firstSeq ?: before) >= before) stuck = true
        return true
    }

    companion object {
        /** The most rows one phone:messages query returns (PAGE_MAX in convex/phone.ts). */
        const val LIVE_MAX = 2000

        /*
         * How much more than you've loaded with "Load earlier" `older` keeps,
         * in rows, pictures and bytes (about: its text and the pictures'
         * addresses, see `bytes`), the same as one page of phone:messages
         * can be at most (PAGE_MAX, PAGE_BYTES). Rows sliding off the live
         * page used to be kept for as long as the thread was open, so another
         * runner writing to its own thread a few rows at a time grew the
         * phone's memory without end, all of it drawn (Daybreak, 10 Nov). Now
         * what slides off by itself fits in this, past which the oldest rows
         * go (and can be loaded again). Each chunk you load raises the
         * allowance by the size of what it brings that's older than anything
         * this view has held, up to LOADED_MAX times the caps, so going back
         * is never cut off short of that, and memory only grows as far back
         * as you go. Rows let go and read again earn nothing: the phone loads
         * a chunk by itself when Load earlier scrolls into view, and a runner
         * keeping that row in view had it fetch the rows the cap let go, over
         * and over, each time adding room (Codex's review).
         */
        const val OLDER_MAX_ROWS = 2000
        const val OLDER_MAX_PICTURES = 2000
        const val OLDER_MAX_BYTES = 8 * 1024 * 1024
        const val LOADED_MAX = 10

        /** About how much a row holds: its text and the other words on it, plus a little for the rest. */
        internal fun bytes(m: PhoneMessage): Int {
            var n = 64 + utf8(m.text) + utf8(m.origin) + utf8(m.toolKind) + utf8(m.turn)
            n += (m.buttons?.size ?: 0) * 256 + (m.wokeBy?.size ?: 0) * 256
            return n
        }

        internal fun bytes(p: MessageImage): Int = 64 + utf8(p.url) + utf8(p.src)

        /** UTF-8 length without making the bytes. */
        private fun utf8(s: String?): Int {
            if (s == null) return 0
            var n = 0
            var i = 0
            while (i < s.length) {
                val c = s[i]
                when {
                    c.code < 0x80 -> n += 1
                    c.code < 0x800 -> n += 2
                    Character.isHighSurrogate(c) && i + 1 < s.length && Character.isLowSurrogate(s[i + 1]) -> { n += 4; i++ }
                    else -> n += 3
                }
                i++
            }
            return n
        }

        /**
         * The pictures of `new` that `existing` doesn't have already. Pictures
         * came again with every live page (Codex's review): one at a seq below
         * the first row, with no older rows, was added once per update.
         * Addresses are only compared between pictures at the same seq.
         */
        internal fun unseen(new: List<MessageImage>, existing: List<MessageImage>): List<MessageImage> {
            if (new.isEmpty()) return emptyList()
            val seqs = new.mapTo(HashSet()) { it.seq }
            val bySeq = HashMap<Int, MutableList<String>>()
            for (p in existing) if (p.seq in seqs) bySeq.getOrPut(p.seq) { mutableListOf() } += p.url
            val out = mutableListOf<MessageImage>()
            for (p in new) {
                val urls = bySeq.getOrPut(p.seq) { mutableListOf() }
                if (p.url in urls) continue
                out += p
                urls += p.url
            }
            return out
        }
    }
}
