package com.mikerosoft.threadbear.kit

import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNotEquals
import kotlin.test.assertTrue

class SecretsTest {
    private val b64 = Base64.getDecoder()
    // The runner's vector (runner/src/secrets.ts), as SecretsTests.swift has it.
    private val recipientPrivate = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="
    private val recipientPublic = "pOCSkrZRwni5dyxWn1+puxPZBrRqtoyd+dwrRAn4ogk="
    private val ephemeralPrivate = "AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI="

    @Test fun x25519MatchesRfc7748() {
        fun hex(s: String) = s.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
        val out = X25519.scalarMult(
            hex("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4"),
            hex("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c"),
        )
        assertEquals("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552", out.joinToString("") { "%02x".format(it) })
        assertEquals(recipientPublic, Base64.getEncoder().encodeToString(X25519.publicKey(b64.decode(recipientPrivate))))
    }

    @Test fun sealMatchesTheRunnersVector() {
        val sealed = SecretSeal.seal("hunter2 ✓", recipientPublic, "approval-123", b64.decode(ephemeralPrivate), b64.decode("AwMDAwMDAwMDAwMD"))
        assertEquals("zo060cy2M+x7cMF4FKXHbs0CloUFDTRHRboFhw5YfVk=", sealed.epk)
        assertEquals("AwMDAwMDAwMDAwMD", sealed.nonce)
        assertEquals("818iYL0/xbv1amlCprX8NVHr59l6QxFyCAWu", sealed.box)
    }

    @Test fun theRunnersKeyOpensItAndOnlyForItsCard() {
        val sealed = SecretSeal.seal("s3cret", recipientPublic, "a1")
        fun open(approvalId: String): String {
            val epk = b64.decode(sealed.epk)
            val shared = X25519.scalarMult(b64.decode(recipientPrivate), epk)
            val key = SecretSeal.hkdf(shared, epk + b64.decode(recipientPublic), "threadbear secret v1 $approvalId".toByteArray(), 32)
            val c = Cipher.getInstance("AES/GCM/NoPadding")
            c.init(Cipher.DECRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, b64.decode(sealed.nonce)))
            return String(c.doFinal(b64.decode(sealed.box)))
        }
        assertEquals("s3cret", open("a1"))
        assertFailsWith<Exception> { open("a2") }
    }

    @Test fun eachSealIsFresh() {
        val a = SecretSeal.seal("same", recipientPublic, "a1")
        val b = SecretSeal.seal("same", recipientPublic, "a1")
        assertNotEquals(a.epk, b.epk)
        assertNotEquals(a.nonce, b.nonce)
        assertEquals(12, b64.decode(a.nonce).size)
    }

    @Test fun aBadKeyIsRefused() {
        assertFailsWith<BadKey> { SecretSeal.seal("x", "not base64!", "a") }
        assertFailsWith<BadKey> { SecretSeal.seal("x", Base64.getEncoder().encodeToString(ByteArray(16)), "a") }
        assertFalse(SecretSeal.isUsableKey("AAAA"))
        assertTrue(SecretSeal.isUsableKey(recipientPublic))
    }

    @Test fun onlyThePairedKeyIsTrusted() {
        assertEquals(RunnerKeyPins.Verdict.TRUSTED, RunnerKeyPins.check(recipientPublic, recipientPublic))
        assertEquals(RunnerKeyPins.Verdict.CHANGED, RunnerKeyPins.check(Base64.getEncoder().encodeToString(ByteArray(32) { 9 }), recipientPublic))
        assertEquals(RunnerKeyPins.Verdict.UNKNOWN, RunnerKeyPins.check(recipientPublic, null))
    }
}
