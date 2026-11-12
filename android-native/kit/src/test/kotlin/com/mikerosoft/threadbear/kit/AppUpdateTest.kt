package com.mikerosoft.threadbear.kit

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class AppUpdateTest {
    private val REPO = "mikecann/threadbear"

    private fun latest(tag: String, assets: String = """[{"name":"threadbear-android.apk","browser_download_url":"https://github.com/mikecann/threadbear/releases/download/$tag/threadbear-android.apk"}]""") =
        """{"tag_name":"$tag","html_url":"https://github.com/mikecann/threadbear/releases/tag/$tag","assets":$assets}"""

    @Test fun aNewerReleaseIsOffered() {
        val a = AppUpdate.newer(latest("release/2026-11-12"), "2026-11-11", REPO)!!
        assertEquals("2026-11-12", a.date)
        assertEquals("https://github.com/mikecann/threadbear/releases/download/release/2026-11-12/threadbear-android.apk", a.apkUrl)
    }

    @Test fun theSameOrAnOlderOneIsNot() {
        assertNull(AppUpdate.newer(latest("release/2026-11-11"), "2026-11-11", REPO))
        assertNull(AppUpdate.newer(latest("release/2026-11-10"), "2026-11-11", REPO))
    }

    @Test fun aBuildThatIsntAReleaseNeverOffersOne() {
        assertNull(AppUpdate.newer(latest("release/2026-11-12"), "", REPO))
        assertNull(AppUpdate.newer(latest("release/2026-11-12"), null, REPO))
    }

    @Test fun aReleaseWithoutTheApkYetIsNotOffered() {
        // The Mac's daily tag can come before the workflow has attached the APK.
        assertNull(AppUpdate.newer(latest("release/2026-11-12", "[]"), "2026-11-11", REPO))
    }

    @Test fun oddAnswersAreNothing() {
        assertNull(AppUpdate.newer("not json", "2026-11-11", REPO))
        assertNull(AppUpdate.newer(latest("v1.2"), "2026-11-11", REPO))
        // Wrong shapes are nothing, not a crash.
        assertNull(AppUpdate.newer("""{"tag_name":{}}""", "2026-11-11", REPO))
        assertNull(AppUpdate.newer("""{"tag_name":"release/2026-11-12","assets":[{"name":{}}]}""", "2026-11-11", REPO))
        assertNull(AppUpdate.newer("[]", "2026-11-11", REPO))
    }

    @Test fun theDownloadIsAlwaysThisRepos() {
        // Whatever address the answer gives, the phone opens this repo's file for that tag.
        val a = AppUpdate.newer(latest("release/2026-11-12", """[{"name":"threadbear-android.apk","browser_download_url":"https://github.com/attacker/repo/releases/download/x/payload.apk"}]"""), "2026-11-11", REPO)!!
        assertEquals("https://github.com/mikecann/threadbear/releases/download/release/2026-11-12/threadbear-android.apk", a.apkUrl)
        assertEquals("https://github.com/mikecann/threadbear/releases/tag/release/2026-11-12", a.pageUrl)
        assertNull(AppUpdate.newer(latest("release/2026-11-12"), "2026-11-11", "../evil"))
    }

    @Test fun theUrlIsOnlyForARepoName() {
        assertEquals("https://api.github.com/repos/mikecann/threadbear/releases/latest", AppUpdate.latestUrl("mikecann/threadbear"))
        assertNull(AppUpdate.latestUrl("../../evil"))
    }
}
