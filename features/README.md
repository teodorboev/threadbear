# Behaviour specs

Gherkin `.feature` files for what Threadbear's desktop apps do, run with
[Cucumber.js](https://github.com/cucumber/cucumber-js) against both the Swift
Mac app and the GPUI app (`desktop/`). The Swift app is the definition of right
until it's retired: a scenario is written against it first, then the GPUI app
has to pass the same file. See `docs/gpui-port.md`.

## What a scenario is for

As in Let's Build It, `.feature` files are an **intent contract for designed
behaviour**, not a regression dragnet. A scenario earns its place when it
describes something a person using the app experiences.

Use them for user-visible flows, designed empty and error states, and state
changes you can watch happen. Don't use them for exhaustive option matrices,
implementation details, or how something looks: pixels and styling are for
screenshots and component tests.

**Rendering rule.** A scenario about content asserts on what's drawn (the text
of the reply, the card's command), not that a container appeared.

**Behaviour first.** For anything a user can see, write the scenario before the
code, and make it fail first when you can.

## How it's laid out

- `<name>.feature` is owned by `steps/<name>.steps.ts`. Shared steps go in
  `steps/_common.steps.ts`; files starting with `_` don't own a feature.
- Every scenario carries a tag naming the row it covers in
  `docs/gpui-port/features.md`, such as `@sidebar.search`. `verify-pairing.mjs`
  checks both of these, and `parity.mjs` builds the table in
  `docs/gpui-port/PARITY.md` from them.
- `@gpui-pending` or `@swift-pending` means that app doesn't pass it yet.
  `@mac-pending`, `@windows-pending` and `@linux-pending` mean the GPUI app
  doesn't pass it on that system yet (HTML on Linux's clipboard, say): say why
  in a comment above it. `@manual` means nobody's found a way to automate it (a
  real trackpad gesture, say). The table counts all of them as not done.
- What only differs in its words from system to system (Finder or Folder, ⌘Q or
  Ctrl+Q) isn't pending anywhere: the step says both, as in `on "⌘Q" on a Mac,
  "Ctrl+Q" elsewhere`, and checks the one for the system it's running on.
- `@long-thread` runs a scenario on a made-up long thread and one short demo
  thread (`support/long-thread.ts`), for what only shows on more than a page
  of events. `@remote-threads` turns on the mock's other-Mac threads
  (`bruce_*`), which it leaves out otherwise.
- `@browserbear` gives the app a stand-in BrowserBear (`support/browserbear.ts`):
  its folder through `BROWSERBEAR_HOME`, with `agent/tabs.json` and its socket.
  Without it the GPUI app has none, so a run never reaches the real one.
- `support/` holds the drivers. Both apps take the same JSON commands and give
  the same `dump` (`app/README.md`, "Seeing and driving it from a script"), so
  a step never asks which app it's driving.

## Running them

Each scenario starts its own app on its own mock runner, about 6 seconds apiece.

```bash
npm ci                                  # once, in this folder
npm run check                           # steps line up, no apps needed

# Build the app first, then:
TB_APP=gpui  node_modules/.bin/cucumber-js        # or swift
TB_TAGS='@sidebar.search' ...                     # just these
TB_INCLUDE_PENDING=1 ...                          # to see what's left
TB_REDUCE_MOTION=on ...                           # GPUI with Reduce Motion (off otherwise, whatever the Mac says)
```

Never `npx cucumber-js`: if the package isn't installed it goes and fetches
one with that name, and it isn't this one.

The GPUI app needs `cargo build -p threadbear-gpui --features automation`, and
the Swift app needs `swift build --product Threadbear` in `app/`. Whole runs
go on another machine (AGENTS.md):

```bash
desktop/scripts/remote.sh cloud -- bash -c 'cd features && TB_APP=gpui node_modules/.bin/cucumber-js'
desktop/scripts/remote.sh tinker-desk --desk -- bash -c 'cd features && TB_APP=swift node_modules/.bin/cucumber-js'
```

A failed scenario leaves a screenshot in `reports/failures/`.

`@real-screen` cases post real pointer events and are excluded by default.
Use `TB_REAL_SCREEN=1` only on Tinker Desk through `remote.sh --desk`, with
no recording running there. Never run them on the Mac Mike is using. The
picture-promise case starts its own source window and checks it is in front
before dragging into its own test app.

`tools/idle-perf.ts` measures the app process's CPU time and wakeups on Tinker
Desk, without polling the app during the interval. GPUI builds with the thread
CPU counter also report the main thread's CPU as a percentage of one core.
`TB_IDLE_ONLY=idle` measures a settled transcript with nothing working.
`TB_IDLE_LOADING=on` holds
a transcript read long enough to measure its loader; `TB_IDLE_ONLY=loading`
runs just that phase, with no working sidebar row by default. Add
`TB_IDLE_WORKING=on` for a loader beside a quiet running thread; this requires
loading-only mode and a generated fixture. It checks the working row is
actually visible before and after the interval. `TB_IDLE_ONLY=background`
instead adds a quiet running turn to exercise the working row and its clock.
`TB_IDLE_ONLY=offscreen` measures only a quiet running row below the visible
Inbox. It requires GPUI and a generated fixture with at least eight unarchived
other threads, without `TB_IDLE_LOADING`. Before and after the interval it
checks that the transcript is loaded, the selected Inbox row is visible, and
three later Inbox rows plus the following Running row are laid out outside
the visible list. A tall window may require more threads to put those rows
below it. An external watcher checks every 250 ms that the app stays active,
its display is awake and its window is uncovered throughout every measured
phase, without sending automation or accessibility requests to the app.
The watcher records its sampling rate, display refresh rates, thermal states
and Low Power Mode. Current profiler builds also report GPUI's active-window
state at both counter endpoints; `TB_IDLE_REQUIRE_ACTIVE=on` requires that
field and refuses inactive samples. Older saved binaries leave it unknown.
`TB_REDUCE_MOTION=on` selects static generic loaders; working arcs retain
their existing motion policy. The full run includes seven CPU workers on an eight-core machine;
`TB_IDLE_LOAD=off` skips those. Its command times include the automation
transport and caller scheduling, so they are not event-to-paint latency. `TB_IDLE_SHOT=/path/loader.png`
captures the app window after the interval and command probes, leaving the
measurement unaffected.

Keep an old release binary and use `tools/idle-ab.mjs <old> <new> [rounds=5]`.
`TB_IDLE_REQUIRE_LOADER_PHASE=on` requires a focused GPUI loading run with
Reduce Motion off. It checks angle changes of the transcript loader itself,
not window FPS or other busy controls. `TB_AB_REQUIRE_NEW_LOADER_PHASE=on`
enforces it only for the A/B tool's new arm, allowing older baseline binaries.
Samples report `loaderGateEnforced`; unsupported phase combinations fail.
This probe is for the generated fixture with one loading transcript pane;
it does not distinguish simultaneous loading panes in the same window.
Both tools take the shared performance and check locks automatically; the
A/B parent holds them across every child run. It discards a warm-up pair,
then alternates AB/BA, prints every sample with the ambient load, and reports
medians, ranges and ratios, including frames per second so a frame-rate change
is visible alongside CPU savings. Compare CPU within each load condition:
different CPU cores can take different amounts of CPU time for the same work.
Release builds use the same locks and run at
background priority.
`TB_APP=swift TB_IDLE_ONLY=background` runs the same quiet working fixture with
the already-built Swift debug app. Before measuring, it activates only the
existing private PID and verifies its canonical executable path; the external
watcher still requires foreground activation throughout the interval.
`TB_IDLE_SWIFT_UUIDS` can pin the comma-separated, lowercase Mach-O build UUIDs
from the cached source. The frozen launcher re-signs each private bundle, so
source and private SHA-256 values are recorded separately rather than required
to match. Supplying a source hash also requires the build UUIDs; a hash alone
does not pin the private copy. Key-window state and installed/LaunchServices
equivalence are not established. Swift's dump has no loading-spinner field,
so it cannot run the loading phase. Its command transport differs, so it omits
command times. These counters cover each app process, excluding Chromium's
helpers and Swift's render-server animation work. Swift's debug build and the
different animation paths limit this comparison to app-process work.

The screen guard also checks Accessibility when the Dock's full-screen,
layer-20 window covers a point. It accepts the ordinary window directly
beneath it only when the Accessibility hit-test reaches that same process.
No Accessibility permission, a failed hit-test or a different result keeps
the drag blocked; other covering windows still block it too. The exception
also refuses checked points within 24 points of the display edge, where moving the
pointer could reveal an auto-hidden Dock after the check.

`@picture-loading` gives the mock a cancellable scripted turn while a picture
is delayed. Its script is temporary and no model runs.


## Measuring performance on a shared host

`desktop/scripts/remote.sh` takes `~/.cache/threadbear-perf.lock`, then
`~/.cache/threadbear-check/lock`, for the whole command, including a build
followed by a benchmark. It waits and says what it is waiting for, and who
holds the lock (its pid, how long it has had it, what it's running) and how many
runs are queued ahead, again every five minutes. The holder is in
`~/.cache/threadbear-perf.lock.holder` on the host, the waiters in
`~/.cache/threadbear-perf.lock.waiting`. Read that file, not `lsof`, to see who holds it: every waiter has the
lock file open too, so a waiter at 0% CPU looks like a stuck holder. A thread
killed its own queued run that way on 14 Nov. Other
checkouts and landing checks cannot build over a measurement. Cargo and Swift
build executables run at background priority; the benchmark keeps normal
priority. Cancellation keeps the locks until the remote command has stopped.

A measurement tool that can run directly starts with:

```ts
import { ensurePerfLock } from './perf-lock.ts';
await ensurePerfLock();
```

Put the call after imports, before making fixtures, compiling helpers or
starting an app. Use it in an A/B parent and its children, preserving
`process.env` in child spawns: they share the parent's turn, including between
runs. Keep this in entry points; `real-screen.ts` is also used by running
spec scenarios and must not restart their process to take a lock. Remove manual `lockf` wrappers when using these tools: an outer manual lock cannot
hand its ownership to the helper. Shell tools can use
`perl scripts/perf-lock.pl - <command...>` with an open stdin channel; closing
it cancels the run. `remote.sh` handles that channel itself.


For performance A/B builds, add `--perf` to `remote.sh` (it can go before or
after `--desk`). That uses `~/.cache/threadbear-check/gpui/perf-target`, which
ordinary cache eviction leaves in place. Other runs keep their target per
checkout so an updater copying its binaries after a build still reads its own.
Queued runs mark their source and target in use before waiting for the host
locks and renew those markers until they finish.

The performance wrapper registers the workload's process group and the GPUI
driver's detached app and mock groups before starting them. SwiftDriver uses
`open` and Node adapters under that lease to register its private app and mock
before exec too, including a start or rebuild killed before PID recording.
The adapters stay in the checkout when the registry is retired. Only launch
commands get a separate group; dumps and other commands stay in the worker's
group. The instance path is recorded with the lease for cleanup diagnostics.

The Swift app runs directly from its registered private bundle, preserving
arguments, requested `--env` values and output files. It inherits the worker's
environment; LaunchServices normally supplies launchd's environment. The mock
uses `THREADBEAR_NODE` (or its old alias), else the worker's Node binary.
Every Swift run under a lease uses this path, local or remote, including specs
and screenshots;
ordinary debug launches outside a lease still use LaunchServices. The driver
prints the direct-launch mode. Settle the app before sampling counters, and
check LaunchServices/direct QoS and App Nap before treating timings as equivalent.

After the worker ends, including a crash or KILL, it stops registered groups before
unlocking. The registry is temporary, under
`~/.cache/threadbear-check/perf-children-*`; records contain group IDs and start
times, so a reused PID is left alone. Desk workloads register in that same run,
even if the Desk client finishes early. Keep child spawns' `process.env` intact.


Shared performance builds invalidate Cargo's owner marker before compiling.
When a different checkout last built the target, the next build touches the
workspace and built-in art inputs inside the final lock, so an unchanged
checkout cannot reuse the other checkout's binary. The marker returns only
after Cargo succeeds. A cleanup that cannot yet observe killed work stopping
keeps the host locked and writes `~/.cache/threadbear-check/perf-cleanup.log`
after 30 seconds; it does not let another measurement start over that work.
