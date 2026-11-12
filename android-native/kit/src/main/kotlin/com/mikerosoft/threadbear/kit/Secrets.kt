package com.mikerosoft.threadbear.kit

import java.math.BigInteger
import java.security.SecureRandom
import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.Mac
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

// The phone's half of a secret card (docs/protocol.md, "Secrets";
// Secrets.swift): the value is sealed here to the runner's X25519 key, so
// the Convex mirror only carries ciphertext. Nothing here logs or keeps it.

/** What phone:secret takes as `sealed`, standard base64 each. */
data class SealedSecret(val epk: String, val nonce: String, val box: String)

sealed class SecretAnswer {
    data class Sealed(val sealed: SealedSecret) : SecretAnswer()
    data object Decline : SecretAnswer()
}

class BadKey : Exception("Not a key the phone can seal to")

/**
 * The seal: an ephemeral X25519 key agrees a secret with the runner's,
 * HKDF-SHA256 (salt epk ‖ key, info "threadbear secret v1 <approvalId>")
 * makes a 32-byte key, and AES-256-GCM seals the UTF-8 value with a random
 * 12-byte nonce. Must match runner/src/secrets.ts byte for byte.
 */
object SecretSeal {
    private val random = SecureRandom()

    fun seal(value: String, runnerKey: String, approvalId: String, ephemeral: ByteArray? = null, nonce: ByteArray? = null): SealedSecret {
        val recipient = decodeKey(runnerKey) ?: throw BadKey()
        val secret = ephemeral ?: ByteArray(32).also(random::nextBytes)
        val epk = X25519.publicKey(secret)
        val shared = X25519.scalarMult(secret, recipient)
        val key = hkdf(shared, salt = epk + recipient, info = "threadbear secret v1 $approvalId".toByteArray(), length = 32)
        val iv = nonce ?: ByteArray(12).also(random::nextBytes)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, iv))
        val box = cipher.doFinal(value.toByteArray(Charsets.UTF_8))
        key.fill(0)
        shared.fill(0)
        if (ephemeral == null) secret.fill(0)
        val b64 = Base64.getEncoder()
        return SealedSecret(b64.encodeToString(epk), b64.encodeToString(iv), b64.encodeToString(box))
    }

    fun isUsableKey(key: String) = decodeKey(key) != null

    private fun decodeKey(key: String): ByteArray? =
        runCatching { Base64.getDecoder().decode(key) }.getOrNull()?.takeIf { it.size == 32 }

    internal fun hkdf(ikm: ByteArray, salt: ByteArray, info: ByteArray, length: Int): ByteArray {
        val mac = Mac.getInstance("HmacSHA256")
        mac.init(SecretKeySpec(salt, "HmacSHA256"))
        val prk = mac.doFinal(ikm)
        mac.init(SecretKeySpec(prk, "HmacSHA256"))
        val out = ByteArray(length)
        var t = ByteArray(0)
        var done = 0
        var i = 1
        while (done < length) {
            mac.update(t)
            mac.update(info)
            mac.update(i.toByte())
            t = mac.doFinal()
            val n = minOf(t.size, length - done)
            System.arraycopy(t, 0, out, done, n)
            done += n
            i++
        }
        return out
    }
}

/**
 * Whether a secret card's key is one to seal to: only the key the Mac's own
 * pairing code gave for that runner (RunnerKeyPins). A key the mirror made up
 * or changed is refused, so the mirror can't ask for a secret sealed to itself.
 */
object RunnerKeyPins {
    enum class Verdict { TRUSTED, CHANGED, UNKNOWN }

    fun check(cardKey: String, kept: String?): Verdict {
        if (kept == null) return Verdict.UNKNOWN
        val a = runCatching { Base64.getDecoder().decode(cardKey) }.getOrNull()
        val b = runCatching { Base64.getDecoder().decode(kept) }.getOrNull()
        return if (a != null && b != null && a.contentEquals(b)) Verdict.TRUSTED else Verdict.CHANGED
    }
}

/**
 * X25519 (RFC 7748) on BigInteger: only ever run once per secret, with a
 * fresh key, so it needn't be constant-time or quick. Android's own X25519
 * is only there from API 31 and its raw-key form differs by provider.
 */
object X25519 {
    private val P = BigInteger.TWO.pow(255).subtract(BigInteger.valueOf(19))
    private val A24 = BigInteger.valueOf(121665)
    private val BASE = ByteArray(32).also { it[0] = 9 }

    fun publicKey(secret: ByteArray) = scalarMult(secret, BASE)

    fun scalarMult(scalar: ByteArray, point: ByteArray): ByteArray {
        require(scalar.size == 32 && point.size == 32)
        val k = scalar.copyOf()
        k[0] = (k[0].toInt() and 248).toByte()
        k[31] = ((k[31].toInt() and 127) or 64).toByte()
        val u = point.copyOf().also { it[31] = (it[31].toInt() and 127).toByte() }
        val x1 = decode(u)
        var x2 = BigInteger.ONE
        var z2 = BigInteger.ZERO
        var x3 = x1
        var z3 = BigInteger.ONE
        var swap = 0
        for (t in 254 downTo 0) {
            val bit = (k[t / 8].toInt() shr (t % 8)) and 1
            swap = swap xor bit
            if (swap == 1) { val a = x2; x2 = x3; x3 = a; val b = z2; z2 = z3; z3 = b }
            swap = bit
            val a = x2.add(z2).mod(P)
            val aa = a.multiply(a).mod(P)
            val b = x2.subtract(z2).mod(P)
            val bb = b.multiply(b).mod(P)
            val e = aa.subtract(bb).mod(P)
            val c = x3.add(z3).mod(P)
            val d = x3.subtract(z3).mod(P)
            val da = d.multiply(a).mod(P)
            val cb = c.multiply(b).mod(P)
            x3 = da.add(cb).pow(2).mod(P)
            z3 = x1.multiply(da.subtract(cb).pow(2)).mod(P)
            x2 = aa.multiply(bb).mod(P)
            z2 = e.multiply(aa.add(A24.multiply(e))).mod(P)
        }
        if (swap == 1) { x2 = x3; z2 = z3 }
        k.fill(0)
        return encode(x2.multiply(z2.modPow(P.subtract(BigInteger.TWO), P)).mod(P))
    }

    private fun decode(le: ByteArray) = BigInteger(1, le.reversedArray())

    private fun encode(n: BigInteger): ByteArray {
        val be = n.toByteArray()
        val out = ByteArray(32)
        for (i in 0 until minOf(32, be.size)) out[i] = be[be.size - 1 - i]
        return out
    }
}
