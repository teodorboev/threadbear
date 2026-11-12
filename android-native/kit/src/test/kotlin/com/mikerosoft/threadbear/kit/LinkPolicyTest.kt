package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

/** The phone's copy of the Mac's LinkPolicy: a reply's link to another app waits for a yes, a web page opens (Daybreak, 7 Nov). */
class LinkPolicyTest {
    @Test fun onlyWebPagesOpenWithoutAsking() {
        for (web in listOf("https://example.com/", "HTTP://localhost:5173/")) assertIs<LinkPolicy.Kind.Web>(LinkPolicy.kind(web), web)
        for (other in listOf("shortcuts://run-shortcut?name=x", "tel:123", "JaVaScRiPt:alert(1)", "itms-services://?action=download-manifest",
            "prefs:root=General", "threadbear://pair?url=x", "intent://x#Intent;scheme=y;end")) {
            assertIs<LinkPolicy.Kind.Ask>(LinkPolicy.kind(other), other)
        }
        assertIs<LinkPolicy.Kind.File>(LinkPolicy.kind("file:///etc/hosts"))
    }

    @Test fun noSchemeOrNoHostIsRefused() {
        assertEquals(LinkPolicy.Kind.Refuse, LinkPolicy.kind("/a/b.md"))
        assertEquals(LinkPolicy.Kind.Refuse, LinkPolicy.kind("https:///path"))
        assertEquals(LinkPolicy.Kind.Refuse, LinkPolicy.kind("https://"))
    }

    @Test fun aLongLinkIsCutInTheMiddle() {
        val long = "https://x.dev/" + "a".repeat(400) + "END"
        val shown = LinkPolicy.shown(long)
        assertEquals(200 + 1 + 100, shown.length)
        assert(shown.startsWith("https://x.dev/") && shown.endsWith("END") && "…" in shown)
        assertEquals("https://x.dev", LinkPolicy.shown("https://x.dev"))
    }
}
