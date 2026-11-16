package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue
import kotlin.test.assertFailsWith
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.JsonPrimitive

class PairingTest {
    @Test fun parsesAPairLink() {
        val p = assertNotNull(Pairing.parse("threadbear://pair?url=https://happy-bear-123.convex.cloud&token=tb_abc"))
        assertEquals("https://happy-bear-123.convex.cloud", p.deploymentUrl)
        assertEquals("tb_abc", p.token)
        assertEquals(p, Pairing.parse(p.link))
        assertTrue("tb_abc" !in p.toString())
    }

    @Test fun pairsWithATailnetBackend() {
        val p = assertNotNull(Pairing.parse("threadbear://pair?url=https%3A%2F%2FMac.tail1234.ts.net%3A3210&token=tb_abc"))
        assertEquals("https://mac.tail1234.ts.net:3210", p.deploymentUrl)
        assertEquals("mac.tail1234.ts.net:3210", p.serverName)
        assertEquals(p, Pairing.parse(p.link))
        for (bad in listOf("http://mac.tail1234.ts.net:3210", "https://user@mac.tail1234.ts.net:3210", "https://mac.tail1234.ts.net:3210/api",
            "https://mac.tail1234.ts.net:3210?x=1")) {
            assertNull(Pairing.parse("threadbear://pair?url=${java.net.URLEncoder.encode(bad, "UTF-8")}&token=t"), bad)
        }
    }

    @Test fun pairsWithABackendAtAnyHttpsAddress() {
        for ((raw, want) in listOf("https://Threadbear.Example.com" to "https://threadbear.example.com", "https://threadbear.example.com/" to "https://threadbear.example.com",
            "https://192.168.1.20:8443" to "https://192.168.1.20:8443")) {
            val p = assertNotNull(Pairing.parse("threadbear://pair?url=${java.net.URLEncoder.encode(raw, "UTF-8")}&token=t"), raw)
            assertEquals(want, p.deploymentUrl)
            assertEquals(p, Pairing.parse(p.link))
        }
        for (bad in listOf("http://threadbear.example.com", "https://user:pw@threadbear.example.com", "https://threadbear.example.com/convex",
            "https://127.0.0.1:13210", "https://localhost:3210", "https://[::1]:3210")) {
            assertNull(Pairing.parse("threadbear://pair?url=${java.net.URLEncoder.encode(bad, "UTF-8")}&token=t"), bad)
        }
        assertEquals("threadbear.example.com:8443", Pairing.serverName("https://threadbear.example.com:8443"))
        assertEquals("threadbear.example.com", Pairing.serverName("https://threadbear.example.com"))
        assertEquals("happy-bear-123.convex.cloud", Pairing.serverName("https://happy-bear-123.convex.cloud:443"))
        assertEquals("127.0.0.1", Pairing.serverName("http://127.0.0.1:3210"))
    }

    @Test fun siteUrlBecomesCloud() {
        assertEquals("https://happy-bear-123.convex.cloud", Pairing.parse("threadbear://pair?url=https://happy-bear-123.convex.site&token=t")?.deploymentUrl)
    }

    @Test fun carriesRunnerKeys() {
        val key = "pOCSkrZRwni5dyxWn1+puxPZBrRqtoyd+dwrRAn4ogk="
        val p = assertNotNull(Pairing.parse("threadbear://pair?url=https%3A%2F%2Fx.convex.cloud&token=tb_1&key=0a1b2c%3ApOCSkrZRwni5dyxWn1%2BpuxPZBrRqtoyd%2BdwrRAn4ogk%3D&key=bad%3AAAAA&key=nocolon"))
        assertEquals(mapOf("0a1b2c" to key), p.runnerKeys)
        assertEquals(p, Pairing.parse(p.link))
        assertEquals(mapOf("r1" to key), Pairing.parse("threadbear://pair?url=https://x.convex.cloud&token=t&key=r1:$key")?.runnerKeys)
        assertEquals(mapOf("r1" to key), Pairing.parse("threadbear://pair?url=https://x.convex.cloud&token=t&key=r1:${key.replace('+', ' ')}")?.runnerKeys)
    }

    @Test fun refusesPlainHttpAndBadLinks() {
        assertNull(Pairing.parse("threadbear://pair?url=http://evil.example.com&token=t"))
        assertNull(Pairing.parse("threadbear://pair?url=http://a.convex.cloud&token=t"))
        assertNull(Pairing.parse("threadbear://pair?url=https://a.convex.cloud"))
        assertNull(Pairing.parse("https://pair?url=https://a.convex.cloud&token=t"))
        assertNull(Pairing.parse("not a link at all"))
    }

    @Test fun localBackendOnlyWhenAllowed() {
        val link = "threadbear://pair?url=http://127.0.0.1:3210&token=t"
        assertNull(Pairing.parse(link))
        assertEquals("http://127.0.0.1:3210", Pairing.parse(link, allowLocal = true)?.deploymentUrl)
    }

    @Test fun everyLinkIsAskedAboutAndSaysWhatItChanges() {
        val key = "pOCSkrZRwni5dyxWn1+puxPZBrRqtoyd+dwrRAn4ogk="
        val link = Pairing.TEXT_PREFIX + java.util.Base64.getUrlEncoder().withoutPadding().encodeToString("url=https%3A%2F%2Fb.convex.cloud&code=tbp_ab&key=r1:${java.net.URLEncoder.encode(key, "UTF-8")}".toByteArray())
        val first = assertNotNull(PairingRequest.of(link, PairingSource.SCANNED, false, null) { null })
        assertEquals("Pair with b.convex.cloud?", first.title)
        assertTrue(first.message.startsWith("This link pairs the phone with the server at b.convex.cloud. It adds 1 Mac"))
        val moving = assertNotNull(PairingRequest.of(link, PairingSource.SCANNED, false, Pairing("https://a.convex.cloud", "old")) { key })
        assertTrue(moving.message.startsWith("This link moves the phone from a.convex.cloud to b.convex.cloud"))
        assertTrue("stay the same" in moving.message)
        val changed = assertNotNull(PairingRequest.of(link, PairingSource.SCANNED, false, Pairing("https://b.convex.cloud", "old")) { "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=" })
        assertEquals(1, changed.changedKeys)
        assertTrue(changed.samePairedServer)
        assertNull(PairingRequest.of("threadbear://other", PairingSource.SCANNED, false, null) { null })
    }

    // Daybreak 127 (15 Nov 2026): the Mac's QR code is no longer a link.
    private val code = "tbp_" + "ab".repeat(24)
    private fun text(host: String = "happy-bear-123.convex.cloud", code: String = this.code, key: String? = null, extra: String = ""): String {
        var q = "url=" + java.net.URLEncoder.encode("https://$host", "UTF-8") + "&code=" + code + extra
        if (key != null) q += "&key=" + java.net.URLEncoder.encode("mac:$key", "UTF-8")
        return Pairing.TEXT_PREFIX + java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(q.toByteArray())
    }

    @Test fun theMacsPairingTextCarriesACodeNotAToken() {
        val key = "pOCSkrZRwni5dyxWn1+puxPZBrRqtoyd+dwrRAn4ogk="
        val p = assertNotNull(Pairing.parse(text(key = key)))
        assertEquals("https://happy-bear-123.convex.cloud", p.deploymentUrl)
        assertEquals(code, p.code)
        assertEquals("", p.token)
        assertEquals(mapOf("mac" to key), p.runnerKeys)
        assertTrue(!p.toString().contains(code))
        // Not a link: no scheme for an app to claim.
        assertNull(runCatching { java.net.URI(text()).scheme }.getOrNull())
    }

    @Test fun everyLengthOfUnpaddedTextParses() {
        for (n in 1..4) {
            val host = "a".repeat(n) + ".convex.cloud"
            assertEquals("https://$host", Pairing.parse(text(host = host))?.deploymentUrl, host)
        }
    }

    @Test fun aBadPairingTextIsNotAPairing() {
        for (bad in listOf(text(code = "tb_abc"), text(code = "tbp_"), text(code = "tbp_zz"), text(extra = "&token=tb_x"),
            Pairing.TEXT_PREFIX, Pairing.TEXT_PREFIX + "!!", Pairing.TEXT_PREFIX + "%%%", Pairing.TEXT_PREFIX + "A".repeat(5000),
            Pairing.TEXT_PREFIX + java.util.Base64.getUrlEncoder().withoutPadding().encodeToString("url=http%3A%2F%2Fevil.example.com&code=$code".toByteArray()))) {
            assertNull(Pairing.parse(bad), bad)
        }
    }

    @Test fun aPairingLinkAnotherAppOpenedIsRefusedWhateverItCarries() {
        val legacy = "threadbear://pair?url=https://b.convex.cloud&token=tb_t"
        for (t in listOf(legacy, text())) {
            assertTrue(PairingSource.OPENED.refuses(t, false))
            assertNull(PairingRequest.of(t, PairingSource.OPENED, false, null) { null })
        }
        assertTrue(!PairingSource.OPENED.refuses("threadbear://other", false))
        // An older Mac's token link isn't taken from the scanner or the
        // clipboard either: the token may have reached another app already,
        // and a clipboard app could swap one in for the Mac's code.
        assertTrue(PairingSource.isOldLink(legacy, false))
        assertTrue(!PairingSource.isOldLink(text(), false))
        for (source in listOf(PairingSource.SCANNED, PairingSource.PASTED)) {
            assertNull(PairingRequest.of(legacy, source, false, null) { null })
            assertNotNull(PairingRequest.of(text(), source, false, null) { null })
        }
    }

    @Test fun aCodeIsTradedForItsTokenOnce() = runTest {
        val offered = assertNotNull(Pairing.parse(text()))
        val transport = FakeTransport().apply { mutations = { _, _ -> """{"token":"tb_real","expiresAt":"2027-02-13T00:00:00.000Z"}""" } }
        val paired = offered.claimed(transport)
        assertEquals("tb_real", paired.token)
        assertNull(paired.code)
        assertEquals(listOf("tokens:claim"), transport.calls.map { it.first })
        assertEquals(JsonPrimitive(code), transport.calls.single().second["code"])
        paired.claimed(transport)
        assertEquals(1, transport.calls.size)
        val refused = FakeTransport().apply { mutations = { _, _ -> throw TransportError.Server("That pairing code has expired or was already used.") } }
        assertFailsWith<TransportError> { offered.claimed(refused) }
        val nonsense = FakeTransport().apply { mutations = { _, _ -> """{"token":"nope"}""" } }
        assertFailsWith<TransportError> { offered.claimed(nonsense) }
    }
}
