# Threadbear for Android (native)

A native Android app, Kotlin and Jetpack Compose, ported from the iPhone app
(`ios/`). It lives in `android-native/`, not `android/`: Keith's fork
(platima/threadbear) has a Tauri wrapper in `android/` and merges our main,
so that name is his.

Like the iPhone app it talks only to the Convex mirror (`convex/phone.ts`,
with a phone token), never to a runner, so it works with the Mac asleep.
Same deliberately small feature set: no search, no sort, no settings beyond
what the iPhone has.

- applicationId `com.mikerosoft.threadbear.android` (Keith's wrapper is
  `com.tinkerware.threadbear`), debug-signed for now.
- Convex: `dev.convex:android-convexmobile`, used at its FFI layer
  (`MobileConvexClient`) so answers stay raw JSON, as the Swift app does.
- Built with `android-native/build.sh` (capped memory, one Gradle worker
  pair, no Kotlin daemon), on the Hetzner box or the M5. On a Mac it takes
  the SDK from ~/Library/Android/sdk and its lock through lockf, and goes
  through `scripts/one-build.sh` like any other build there. Installs and
  screenshots go over `adb connect pixel.tail9ef766.ts.net:5555`. A box
  whose adb key the Pixel hasn't allowed keeps putting "Allow USB
  debugging?" over the app until it's disconnected (`adb disconnect`).

## Layout

- `kit/`: plain Kotlin (JVM), the port of `ios/ThreadbearKit`: the rows
  (lenient JSON reading, one bad row is skipped, not the list), the inbox
  sections and urgency, row status words and relative times, turn folding,
  waiting sends, asks and answers, pairing links, the `Backend` interface
  over a `ConvexTransport`, and a fake transport for tests. `./gradlew
  :kit:test` runs its tests on the JVM, no device.
- `app/`: the Compose app: the Convex FFI transport, the token store
  (EncryptedSharedPreferences), screens, and the view model.

## What's ported from ThreadbearKit

| iPhone (ThreadbearKit) | Android (kit) | Slice |
| --- | --- | --- |
| Models.swift (rows, statuses, approvals, ThreadChange) | `Models.kt` | 1 |
| FlexibleDate.swift | `Dates.kt` | 1 |
| Backend.swift, ConvexTransport.swift (protocol) | `Backend.kt`, `Transport.kt` | 1 |
| Inbox.swift, InboxUrgency.swift (sections, RowStatus, RelativeTime, PreviewText) | `Inbox.kt` | 1 |
| Turns.swift, TurnGap.swift (folding, worked lines, dividers) | `Turns.kt` | 1 |
| Pairing.swift (link parsing) | `Pairing.kt` | 1 |
| WaitingSends.swift | `WaitingSends.kt` | 1 |
| MarkdownBlocks.swift | `Markdown.kt` | 1 (basic), 4 |
| AskAnswer/Answers (Turns.swift, Backend.swift) | `Asks.kt` | 2 |
| Secrets.swift, PairingRequest's key pins | `Secrets.kt` | 2 |
| PushKey.swift, PushTaps.swift | `Push.kt` | 3 |
| MessageHistory.swift (older chunks, caps) | `MessageHistory.kt` | 4 |
| MarkdownBlocks.swift (all of it: rich, drafts, nested quotes, pictures) | `Markdown.kt` | 4 |
| DraftBlock.swift | `DraftBlock.kt` | 4 |
| MarkdownBlocksView's inline marks (bold, code, links) | `InlineMarks.kt`, one linear scan, links never inside links | 4 |
| InlinePictures.swift, ThreadPictures (Models.swift) | `InlinePictures.kt`, `ThreadPictures.kt` | 4 |
| PictureDownload.swift, PictureMemory.swift, PictureFraming.swift | `PictureDownload.kt`, `PictureMemory.kt`, `PictureFraming.kt` | 4 |
| ReplyButtons.swift, ThreadLinks.swift, LinkPolicy.swift | `ReplyButtons.kt`, `ThreadLinks.kt`, `LinkPolicy.kt` | 4 |
| SlashCommands.swift (the palette's logic) | `SlashCommands.kt` | 4 |
| AgentHeader.swift, CardMentions.swift, ReviewAnswer.swift | same names, `.kt` | 4 |
| PageSwipes.swift, Plans.swift | `PageSwipes.kt`, `Plans.kt` | 4 |
| PhotoUpload | | 4 |
| Pocket.swift, PocketIcon.swift | `Pocket.kt` | 5 |
| InboxMoves.swift, Tucks.swift | `InboxMoves.kt`, `Tucks.kt` | 5 |
| ThreadSearch (Inbox.swift) | `Search.kt` | 5 |
| NotifyMode, AppModel's runner order and offline line | `Settings.kt` | 5 |
| AppModel's StartingThread (new threads on their way) | `Starting.kt` | 5 |

Dictation uses Android's own SpeechRecognizer (on-device where the phone
has it), not Whisper; there's one recognizer, so no "Dictate with" choice.

Not ported: Quick Ask (Mike, 11 Nov: not needed), Whisper, the bear and pull-peek, the
Pocket's "here" mark (it needs location and a map search), the rows flying
between headings (RowFlight), the demo's sample data beyond what tests need.

## Screens

1. Pair: Scan the code (camera, ML Kit barcode) and Paste a pairing link,
   both asking "Pair with <server>?" before keeping anything, checked with
   `phone:me`. `threadbear://pair?...` links open the app too.
2. Inbox: Starred, Inbox, Running, Hibernating, Archived headings (the last
   three folded at first), rows like the iPhone's: project tile, title,
   place, status word or time, unread dot. Swipe to archive, long-press for
   Star/Hibernate. Offline/connection line at the top.
3. Thread: the folded transcript, live (`phone:thread` + `phone:messages`),
   the working line while it runs, waiting sends with Cancel, the composer
   with Send turning into Stop while a turn runs, then the cards: approval,
   question, secret, asks, done. Replies draw all of MarkdownBlocks:
   tables, code, nested quotes, ```slack/```email/```draft boxes with Copy,
   ```mermaid and ```html in a locked-down web view (Pele bundled in
   assets, as on the iPhone: no network, no navigation, a page's own
   scripts never run), and pictures (capped download with one deadline,
   50 MP header check, downsampled, one 96 MB cache, tap for full screen).
   Reply buttons, thread chips and links (anything but a web page asks
   first), work: links opening the turn's working, the slash palette,
   plan cards and the plan sheet, older history in chunks, and
   swipe left for Forward / right for Back.
4. New thread (the bar's pencil): text with photos and dictation, runner,
   model and effort as the iPhone offers; it closes once the backend has it,
   a Starting row stands in, and the thread opens if it starts within a minute.
5. Settings (the bar's menu): paired server, phone, token expiry, runners
   and their order (drag by the handles), Notify me once push is on, the
   self-hosted note, Unpair with a confirm.
6. Pocket (the wallet button, with its count): the deck, a card full screen,
   swipe a card away with Undo, open its thread, "Add to my pocket".

The inbox's search is always showing, and after a 300 ms pause asks Convex
for archived threads past the newest 100 (`phone:search`). Rows swipe left
to archive and right to hibernate (wake, a starred one that's asleep), and
a long press has Star, Hibernate, Archive, Mark as read and Copy Thread ID.
Archive and hibernate hide the row at once and wait for the runner
(ShellModel), as the iPhone's do.

## Slices

1. Pairing, inbox, open a thread and watch it stream, send, stop.
2. Cards: approvals, questions, secrets (sealed), asks, done card with
   follow-ups and archive.
3. Push: FCM through the relay, with Mike's own Firebase project
   (`threadbear-android`, his choice on 11 Nov) as the relay's second key
   (docs/push-relay.md); tap opens the thread. The app's
   google-services.json is kept out of git: the build reads it from
   `app/` or `~/.config/threadbear-android/`, and without it push is off.
4. The rest: hibernate/star/archive menus, new thread, photos, older
   history, reply buttons, links, markdown tables and code.

Each slice: kit tests for what it ports, screenshots from Mike's Pixel in
dark mode, pushed to `cloud/android-native`, landed from the M5. All four
plus the inbox shell (5) landed on 11 Nov, after cross-lab reviews.

## Releases

Mike chose GitHub releases over Google Play (11 Nov 2026): no Play account
or review, and how Keith ships his. For each `release/YYYY-MM-DD` tag Mike's
runner pushes, `.github/workflows/android.yml` builds a signed APK on a
standard (free) GitHub runner and attaches it to that tag's GitHub release
as `threadbear-android.apk`, so the newest is always at
https://github.com/mikecann/threadbear/releases/latest/download/threadbear-android.apk.
A release from before `android-native/` existed is skipped, and in a fork
the job doesn't run (no secrets there). The same workflow builds and tests
the app on every push to main and on PRs that touch it, so each commit has
an Android verdict for the daily tag to wait on.

- The version is the tag's date (`versionName` 2026-11-12, `versionCode`
  20261112), so each release installs over the last. A local build is
  `dev`, version code 1, and never offers itself an update.
- The release key (`threadbear`, RSA 4096, made 11 Nov 2026, certificate
  SHA-256 `185d962a…c358`, pinned in the workflow) is a secret of the
  `android-release` environment (`ANDROID_KEYSTORE_BASE64`,
  `ANDROID_KEYSTORE_PASSWORD`, with `ANDROID_GOOGLE_SERVICES_JSON` for push),
  which only hands secrets to jobs on release/* tags; only Mike can write
  tags (the "Only Mike writes tags" ruleset). Mike's choice over keeping it
  on his Mac. A release's APK is never replaced once attached, and only
  the newest release tag is marked latest. The backup is
  `~/.config/threadbear-android/release.jks` and `release-password` on the
  M5, which a local `:app:assembleRelease` signs with. Android installs an
  update only if it's signed like the installed app, so losing the key
  means everyone uninstalling and reinstalling.
- The app asks GitHub for the latest release at most every six hours as
  it comes forward (`Updates.kt`, `AppUpdate` in the kit), release builds
  only. A newer one puts a dot on the inbox's menu, an "Update to 12 Nov"
  item in it, and the same in Settings, App; tapping it opens the APK in the
  browser, and Android asks to install.
- A debug build is signed with the build machine's debug key, so going from
  one to a release build means uninstalling (and pairing again) once.
