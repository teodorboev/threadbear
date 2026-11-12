import groovy.json.JsonSlurper

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

/**
 * Push (FCM): the Firebase project's app config, read here into BuildConfig
 * rather than through Google's google-services plugin, so a build without
 * it (the Hetzner box, someone's fork) still builds, with push off. It's
 * kept out of git (the repo is public): android-native/app/google-services.json
 * if there's one, else ~/.config/threadbear-android/google-services.json,
 * which is where Mike's M5 has his project's (threadbear-android).
 */
val firebase: Map<String, String> = run {
    val file = listOf(file("google-services.json"), File(System.getProperty("user.home"), ".config/threadbear-android/google-services.json"))
        .firstOrNull { it.isFile } ?: return@run emptyMap()
    @Suppress("UNCHECKED_CAST")
    val json = JsonSlurper().parse(file) as Map<String, Any?>
    val project = json["project_info"] as Map<String, Any?>
    val client = (json["client"] as List<Map<String, Any?>>).firstOrNull {
        ((it["client_info"] as Map<String, Any?>)["android_client_info"] as Map<String, Any?>)["package_name"] == "com.mikerosoft.threadbear.android"
    } ?: return@run emptyMap()
    mapOf(
        "projectId" to project["project_id"] as String,
        "senderId" to project["project_number"] as String,
        "appId" to (client["client_info"] as Map<String, Any?>)["mobilesdk_app_id"] as String,
        "apiKey" to ((client["api_key"] as List<Map<String, Any?>>).first()["current_key"] as String),
    )
}
fun quoted(s: String?) = "\"" + (s ?: "") + "\""

/**
 * Releases (docs/android-native.md, "Releases"): each release/YYYY-MM-DD tag
 * gets a signed APK on its GitHub release, built by .github/workflows/android.yml.
 * THREADBEAR_RELEASE is that tag's date; its digits make the versionCode, so
 * every release installs over the last. Without it (a local build) the
 * version is 0, and the app never offers itself an update.
 */
val releaseDate: String? = System.getenv("THREADBEAR_RELEASE")?.takeIf { Regex("^\\d{4}-\\d{2}-\\d{2}$").matches(it) }

/**
 * The release key: the workflow writes it from the repo's secrets and says
 * where; on Mike's M5 it's the backup in ~/.config/threadbear-android/.
 * One key for good: Android only installs an update signed like the app
 * already there, so a lost key means everyone reinstalling.
 */
val releaseKeystore: File? = (System.getenv("THREADBEAR_KEYSTORE")?.let(::File)
    ?: File(System.getProperty("user.home"), ".config/threadbear-android/release.jks")).takeIf { it.isFile }
val releaseKeyPassword: String? = System.getenv("THREADBEAR_KEYSTORE_PASSWORD")
    ?: File(System.getProperty("user.home"), ".config/threadbear-android/release-password").takeIf { it.isFile }?.readText()?.trim()

/** Where the app looks for a newer release: this repo, or a fork's own (GITHUB_REPOSITORY in Actions). */
val updateRepo: String = System.getenv("GITHUB_REPOSITORY") ?: "mikecann/threadbear"

android {
    namespace = "com.mikerosoft.threadbear.android"
    compileSdk = 37

    defaultConfig {
        // Mike's own, clearly apart from Keith's wrapper (com.tinkerware.threadbear).
        applicationId = "com.mikerosoft.threadbear.android"
        minSdk = 29
        targetSdk = 36
        // 20261112 for release/2026-11-12; 1 for a local build.
        versionCode = releaseDate?.replace("-", "")?.toInt() ?: 1
        versionName = releaseDate ?: "dev"
        buildConfigField("String", "RELEASE_DATE", quoted(releaseDate))
        buildConfigField("String", "UPDATE_REPO", quoted(updateRepo))
        // Phones only: keeps the APK (and this box's disk) small.
        ndk { abiFilters += "arm64-v8a" }
        // Empty when there's no google-services.json: push is off (Push.kt).
        buildConfigField("String", "FIREBASE_PROJECT_ID", quoted(firebase["projectId"]))
        buildConfigField("String", "FIREBASE_SENDER_ID", quoted(firebase["senderId"]))
        buildConfigField("String", "FIREBASE_APP_ID", quoted(firebase["appId"]))
        buildConfigField("String", "FIREBASE_API_KEY", quoted(firebase["apiKey"]))
    }

    signingConfigs {
        if (releaseKeystore != null && releaseKeyPassword != null) {
            create("release") {
                storeFile = releaseKeystore
                storePassword = releaseKeyPassword
                keyAlias = "threadbear"
                keyPassword = releaseKeyPassword
            }
        }
    }

    buildTypes {
        debug {
            buildConfigField("boolean", "ALLOW_LOCAL", "true")
        }
        release {
            isMinifyEnabled = false
            // Unsigned (and so not installable) where there's no key, as in a fork.
            signingConfig = signingConfigs.findByName("release")
            buildConfigField("boolean", "ALLOW_LOCAL", "false")
        }
    }

    buildFeatures {
        compose = true
        buildConfig = true
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

}

kotlin { jvmToolchain(17) }

dependencies {
    implementation(project(":kit"))
    implementation("dev.convex:android-convexmobile:0.8.0@aar") { isTransitive = true }
    // 5.19.1, not 5.14: older builds' libjnidispatch.so leaves writable data
    // on the page its read-only (RELRO) part ends in, so it would crash on a
    // 16 KB-page phone, and Android 17 warns about it on every debug launch.
    implementation("net.java.dev.jna:jna:5.19.1@aar")

    implementation(platform("androidx.compose:compose-bom:2026.09.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui-tooling-preview")
    debugImplementation("androidx.compose.ui:ui-tooling")
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.11.0")
    implementation("androidx.lifecycle:lifecycle-process:2.11.0")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.11.0")
    implementation("androidx.navigation:navigation-compose:2.10.2")
    implementation("androidx.core:core-ktx:1.19.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.10.2")
    implementation("com.google.android.gms:play-services-code-scanner:16.1.0")
    implementation(platform("com.google.firebase:firebase-bom:35.1.0"))
    implementation("com.google.firebase:firebase-messaging")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-play-services:1.10.2")
}
