package com.mikerosoft.threadbear.kit

import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit

/**
 * What the phone keeps of pictures between looks, held to a byte budget
 * across all of them (PictureMemory.swift). Each picture under PictureLimits
 * could still add up: a thread of a hundred big GIFs kept every file's bytes
 * in its rows (Codex review of Daybreak finding 8). So the decoded,
 * downsampled pictures live here, least recently used out first, and a row
 * only holds its picture while it's on screen. Thread-safe.
 */
class CostLRU<K : Any, V : Any>(val limit: Int) {
    private class Entry<V>(val value: V, val cost: Int, var used: Long)

    private val entries = HashMap<K, Entry<V>>()
    private var tick = 0L
    private var total = 0

    val totalCost: Int get() = synchronized(this) { total }
    val count: Int get() = synchronized(this) { entries.size }

    fun value(key: K): V? = synchronized(this) {
        val entry = entries[key] ?: return null
        entry.used = ++tick
        entry.value
    }

    /** Keeps `value`, dropping the least recently used until the total fits. One costing more than the whole budget isn't kept at all. */
    fun set(value: V, key: K, cost: Int) = synchronized(this) {
        entries.remove(key)?.let { total -= it.cost }
        if (cost > limit) return@synchronized
        entries[key] = Entry(value, cost, ++tick)
        total += cost
        while (total > limit) {
            val oldest = entries.minByOrNull { it.value.used } ?: break
            entries.remove(oldest.key)
            total -= oldest.value.cost
        }
    }
}

/** At most `limit` bodies running at once; the rest wait their turn. A thread of pictures doesn't fetch them all at once. */
class ConcurrencyLimit(val limit: Int) {
    private val permits = Semaphore(limit)

    suspend fun <T> run(body: suspend () -> T): T = permits.withPermit { body() }
}
