package com.mikerosoft.threadbear.kit

import java.security.SecureRandom
import java.util.Base64
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class PushTest {
    private val random = SecureRandom()
    private fun key() = ByteArray(32).also(random::nextBytes)

    /** As convex/pushSeal.ts seal(): nonce, ciphertext, tag, base64. */
    private fun seal(key: ByteArray, plain: String): String {
        val nonce = ByteArray(12).also(random::nextBytes)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(128, nonce))
        return Base64.getEncoder().encodeToString(nonce + cipher.doFinal(plain.toByteArray()))
    }

    @Test fun opensWhatTheBackendSealed() {
        val k = key()
        assertEquals(PushSeal.Opened("Fix the dock badge", "Done ✓ it clears"), PushSeal.open(seal(k, """{"title":"Fix the dock badge","body":"Done ✓ it clears"}"""), k))
    }

    @Test fun anotherPhonesKeyOpensNothing() {
        assertNull(PushSeal.open(seal(key(), """{"title":"a","body":"b"}"""), key()))
    }

    @Test fun nonsenseOpensNothing() {
        val k = key()
        assertNull(PushSeal.open("not base64!", k))
        assertNull(PushSeal.open(Base64.getEncoder().encodeToString(ByteArray(10)), k))
        assertNull(PushSeal.open(seal(k, """{"title":"no body"}"""), k))
        assertNull(PushSeal.open(seal(k, """["not","an","object"]"""), k))
        assertNull(PushSeal.open(seal(k, """{"title":1,"body":2}"""), k))
        assertNull(PushSeal.open(seal(k, """{"title":"a","body":"b"}"""), ByteArray(16)))
    }
}
