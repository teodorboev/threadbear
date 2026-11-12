package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class PairingTest {
    @Test fun parsesAPairLink() {
        val p = assertNotNull(Pairing.parse("threadbear://pair?url=https://happy-bear-123.convex.cloud&token=tb_abc"))
        assertEquals("https://happy-bear-123.convex.cloud", p.deploymentUrl)
        assertEquals("tb_abc", p.token)
        assertEquals(p, Pairing.parse(p.link))
        assertTrue("tb_abc" !in p.toString())
    }

    @Test fun pairsWithATailnetBackend() {
        val p = assertNotNull(Pairing.parse("threadbear://pair?url=https%3A%2F%2FMac.tail9ef766.ts.net%3A3210&token=tb_abc"))
        assertEquals("https://mac.tail9ef766.ts.net:3210", p.deploymentUrl)
        assertEquals("mac.tail9ef766.ts.net:3210", p.serverName)
        assertEquals(p, Pairing.parse(p.link))
        for (bad in listOf("http://mac.tail9ef766.ts.net:3210", "https://user@mac.tail9ef766.ts.net:3210", "https://mac.tail9ef766.ts.net:3210/api",
            "https://mac.tail9ef766.ts.net:3210?x=1", "https://ts.net", "https://evil.ts.net.example.com", "https://tail9ef766.ts.net")) {
            assertNull(Pairing.parse("threadbear://pair?url=${java.net.URLEncoder.encode(bad, "UTF-8")}&token=t"), bad)
        }
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

    @Test fun refusesOtherHosts() {
        assertNull(Pairing.parse("threadbear://pair?url=https://evil.example.com&token=t"))
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
        val link = "threadbear://pair?url=https://b.convex.cloud&token=t&key=r1:$key"
        val first = assertNotNull(PairingRequest.of(link, false, null) { null })
        assertEquals("Pair with b.convex.cloud?", first.title)
        assertTrue(first.message.startsWith("This link pairs the phone with the server at b.convex.cloud. It adds 1 Mac"))
        val moving = assertNotNull(PairingRequest.of(link, false, Pairing("https://a.convex.cloud", "old")) { key })
        assertTrue(moving.message.startsWith("This link moves the phone from a.convex.cloud to b.convex.cloud"))
        assertTrue("stay the same" in moving.message)
        val changed = assertNotNull(PairingRequest.of(link, false, Pairing("https://b.convex.cloud", "old")) { "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=" })
        assertEquals(1, changed.changedKeys)
        assertTrue(changed.samePairedServer)
        assertNull(PairingRequest.of("threadbear://other", false, null) { null })
    }
}
