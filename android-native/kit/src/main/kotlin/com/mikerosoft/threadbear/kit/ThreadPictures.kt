package com.mikerosoft.threadbear.kit

import java.net.URI

/**
 * The open thread's pictures (ThreadPictures in Models.swift): what the
 * runner uploaded for the paths an agent wrote, the photos Mike sent, and
 * pictures the agent pinned.
 */
data class ThreadPictures(val images: List<MessageImage> = emptyList()) {
    /**
     * The picture for `src` in the message at `seq`. The same address in
     * another message may be a different picture by now, so that message's
     * own comes first; failing that, the latest with that address at or
     * before it, then any.
     */
    fun image(src: String, seq: Int?): MessageImage? {
        val matching = images.filter { it.src == src }
        if (seq == null) return matching.lastOrNull()
        return matching.firstOrNull { it.seq == seq } ?: matching.lastOrNull { it.seq <= seq } ?: matching.lastOrNull()
    }

    /**
     * The photos Mike sent with his message at `seq`, in their order, as the
     * runner put them up (`src` picture:N, share-images.ts
     * pastedImagesInEvent). Empty when they stayed on the Mac.
     */
    fun pasted(seq: Int): List<MessageImage> =
        images.mapNotNull { image -> if (image.seq != seq) null else pictureNumber(image.src)?.let { it to image } }
            .sortedBy { it.first }
            .map { it.second }

    /**
     * His message's words for the bubble, with the runner's "[2 pictures]"
     * line (sync.ts toMessageDoc) taken off once every picture it counts is
     * here to show instead. While any of them is missing the line stays, so
     * a photo kept on the Mac still says it was there.
     */
    fun words(text: String, seq: Int): String {
        val match = PICTURES_LINE.find(text) ?: return text
        val count = match.groupValues[1].toIntOrNull() ?: return text
        val placed = InlinePictures.pieces(text).mapNotNull { (it as? InlinePictures.Piece.Picture)?.number }.toSet()
        val unplaced = pasted(seq).filter { (pictureNumber(it.src) ?: 0) !in placed }
        if (unplaced.size < count) return text
        return text.substring(0, match.range.first)
    }

    /** Pinned pictures by the message they go under, and those with no reply yet. */
    data class Pinned(val under: Map<Int, List<MessageImage>>, val trailing: List<MessageImage>)

    /**
     * Pinned pictures by the message they go under: the turn's reply, the
     * first message from the agent on show at or after the pin, as the pin's
     * own line is folded away with the work. `trailing` have no reply yet.
     */
    fun pinned(items: List<ThreadItem>): Pinned {
        val replies = items.mapNotNull { item ->
            (item as? ThreadItem.Message)?.message?.takeIf { it.role == Role.ASSISTANT }?.seq
        }
        val under = LinkedHashMap<Int, MutableList<MessageImage>>()
        val trailing = mutableListOf<MessageImage>()
        for (image in images) {
            if (image.pinned != true) continue
            val reply = replies.firstOrNull { it >= image.seq }
            if (reply != null) under.getOrPut(reply) { mutableListOf() } += image else trailing += image
        }
        return Pinned(under, trailing)
    }

    companion object {
        private val PICTURES_LINE = Regex("""(?:\n\n)?\[([1-9]\d{0,2}) pictures?]$""")

        internal fun pictureNumber(src: String?): Int? {
            if (src == null || !src.startsWith("picture:")) return null
            return src.substring(8).toIntOrNull()?.takeIf { it > 0 }
        }

        /** A web picture needs nothing uploaded (https only). Anything else unmatched is on the Mac. */
        fun webUrl(src: String): String? {
            if (!src.lowercase().startsWith("https://")) return null
            return runCatching { URI(src) }.getOrNull()?.let { src }
        }
    }
}
