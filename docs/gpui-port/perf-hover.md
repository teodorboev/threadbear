# Hover and press latency in the GPUI app

Mike, 13 Nov 2026, with a screen recording: "when I scroll up and down, mouse
up and down, there's a noticeable delay on the highlighting in the projects
area. There's just this constant kind of little bit of delay everywhere." The
Swift app doesn't lag like this.

This file keeps what was measured, how, and what each change did. Scrolling
and the transcript (perf 2) and idle work and priority (perf 3) have their own
threads.

## What a hover did

A `sample` of Mike's installed app (1,406 threads, 13 Nov) had the main
thread drawing frames 93% of the time. Half of each frame was the Threads
panel working out its sections: `shown_sections` copied and sorted every
thread, and the list, the Threads button's badge and the arrow keys each
asked for it, three or four times a frame. The side pane copied the whole
thread map twice more as it drew (`schedule_rows`, `started_rows`). And a
row's hover notified the whole app, so it redrew the transcript and the side
pane as well as the list.

## Measuring

- `desktop/core/tests/sidebar_cost.rs`: the sorting and copying on 1,400
  summaries shaped like Mike's (about 3.5 KB each: patches, panels, actions,
  hooks). `cargo test --release -p threadbear-core --test sidebar_cost --
  --ignored --nocapture`.
- `features/tools/hover-perf.ts`: real pointer moves and presses on Tinker
  Desk's screen (`pointer-probe.swift`) down the thread list and the Projects
  list, idle and with every core but one busy. The app's `input-probe`
  (`desktop/gpui/src/input_probe.rs`) gives when each hover was handled,
  every presented frame from GPUI's own journal, and the main thread's CPU
  time. `hover-ab.ts` runs two builds turn about. Wall-clock latency on
  Tinker swings two to three times between runs of the same build while
  other threads build there, so compare main-thread CPU per hover first.
- The mock's big set (`TB_MOCK_PERF_PROJECTS=1400`) now carries real-sized
  summaries: with 0.5 KB ones the copying didn't show.

## Results

### Sorting and copying (sidebar_cost.rs, M5 at background priority, so efficiency cores)

| What | Before | After |
|---|---|---|
| Inbox sections, once | 17.4 ms | same, but only when a thread, the search, the order or a schedule changed |
| Inbox sections, per frame (3 to 4 asks) | 52 to 70 ms | 0 on a hover |
| Side pane's copies of every thread, per draw | 2 x 15.3 ms | 0 |
| Sorting every thread for the Projects list | 1.23 ms | 0.12 ms |

### Real pointer on Tinker Desk

First A/B (13 Nov, two pairs, mock threads still light): too noisy to read
(the same build's median moved from 19 to 68 ms between runs while other
threads' builds ran). Since then the harness reports main-thread CPU, and
the mock's threads are real-sized.

The activity bar and right rail as views of their own (13 Nov evening,
3 interleaved pairs, main with the fix above against main plus it). The
idea: GPUI's hover styles and `request_animation_frame` notify the view that
drew them, and the root view's observers redraw everything. Measured, it made
no difference, so it didn't land:

| Round | CPU per frame, without | with |
|---|---|---|
| Threads list hover, idle | 15.2 ms | 15.2 ms |
| Activity bar hover, idle | 15.3 ms | 15.1 ms |
| Projects list hover, idle | 12.6 ms | 12.6 ms |
| Activity bar hover, busy | 11.7 ms | 10.7 ms |
| Projects list hover, busy | 12.3 ms | 13.8 ms |

Hover to frame on screen, median, idle: 22 to 35 ms on the threads list and
27 to 37 ms on Projects, both builds. A frame still costs 12 to 15 ms of
main-thread CPU on Tinker Desk after the fix, which is where to look next.

### Where a hover frame goes now (main after the fix, Tinker, `sample` during the sweeps)

- Drawing is about 15% of the main thread while the pointer sweeps the list.
  Of each draw: Taffy laying out the window about half, the Threads panel's
  render about a sixth, painting about a fifth.
- Pointer to frame handed over is about 37 ms median at 60 Hz, two refreshes,
  while a frame's main-thread CPU is about 7 ms. So the rest is frame pacing,
  not work. The Mac build draws through gpui-pre (not gpui-fast, which is
  resolved but never compiled), and its `CAMetalDisplayLink`
  (gpui-pre-apple's `display_link.rs`) leaves `preferredFrameLatency` at
  Apple's default of 2. 1 would show each frame a refresh sooner, if every
  frame fits in one refresh (8.3 ms at 120 Hz). Trying it means vendoring
  gpui-pre-apple, and it changes pacing for scrolling too.

### Frame latency 1 against 2 (14 Nov, one build both ways, 3 interleaved pairs, Tinker at 60 Hz)

`preferredFrameLatency` set through a vendored copy of gpui-pre-apple, with
each display link update's `targetPresentationTimestamp` kept so the probe
could time pointer to glass. Neither landed (no gain, and not worth a
vendored crate): the build is on the branch `latency-wip2` of thread
df197695 if it's wanted again. Medians over the runs:

| Round | Pointer to glass, 2 | 1 | Frames late, 2 | 1 |
|---|---|---|---|---|
| Threads list hover, idle | 66.3 ms | 65.5 ms | 29 | 22 |
| Projects list hover, idle | 63.5 ms | 60.7 ms | 9 | 8 |
| Thread press, idle | 76.0 ms | 67.5 ms | 17 | 15 |
| Threads list hover, busy | 64.0 ms | 63.4 ms | 3 | 4 |
| Projects list hover, busy | 60.7 ms | 61.2 ms | 65 | 58 |
| Thread press, busy | 61.3 ms | 66.1 ms | 9 | 9 |

Scrolling (scroll-perf.ts, two runs each): 99th percentile frame work 18.5 to
19.9 ms either way, 8 to 15 refreshes missed in the second round either way.

No difference: with 1, a frame was still due on the glass about two
refreshes (31 ms) after it was handed over, as with 2, so the setting didn't
take as built. What the numbers do give is main's real latency at 60 Hz:
about 64 ms from the pointer moving to the highlight on the glass, made of
about 5 ms before the app handles the move, about 28 ms to the frame being
handed over (waiting for the display link's next update, then about 7 to 15
ms of work), and about 31 ms from there to the glass.

### Swift app against GPUI (14 Nov, Tinker at 60 Hz)

`features/tools/hover-glass.ts` times both apps the same way from outside: a
real pointer move onto a thread list row, and ScreenCaptureKit for when the
row's pixels change, to within a refresh. Same mock data (1,400 real-sized
threads), same 1180 x 800 window, GPUI and Swift turn about, two runs each.
Load was 7.6 at the start and 3.2 at the end.

| App | Moves timed | Median | 90% | Fastest | Slowest |
|---|---|---|---|---|---|
| GPUI, release, run 1 | 24 | 64.1 ms | 69.8 ms | 47.1 ms | 81.5 ms |
| Swift, debug, run 1 | 30 | 130.0 ms | 147.2 ms | 112.7 ms | 165.6 ms |
| GPUI, release, run 2 | 24 | 65.8 ms | 77.0 ms | 53.0 ms | 81.2 ms |
| Swift, debug, run 2 | 30 | 133.8 ms | 144.3 ms | 119.1 ms | 161.3 ms |

GPUI's 64 to 66 ms from outside agrees with the 63 to 66 ms its own probe
gave above, so the two ways of timing agree. The Swift reference took about
twice as long, but it isn't the app Mike runs: it's the debug build the
specs start (features/support/swift.ts), launched directly rather than by
Launch Services, which leaves it at a lower scheduling priority (37 against
47). Read it as "GPUI isn't behind the Swift app here", not as a number
for Swift's release. Two more things to label:

- Whether the Swift app was the active app wasn't checked. The in-front
  check only proves a click there reaches its window, not that macOS made
  it the frontmost app, and perf 3 found the direct launch can leave the
  window on top with the app inactive, where AppKit may handle hovers
  differently. The Swift numbers are activation-uncontrolled, and a
  UserNotificationCenter alert was up on Tinker Desk during the runs.
  Mike tried 795dd29cb and found the hovers fine (14 Nov), so the frame
  pipeline patch below wasn't started.
- Both GPUI runs had accessibility on: the check that the window is in
  front (real-screen.ts, an AX hit test) turns it on, and GPUI then
  builds its accessibility update every frame. Any accessibility tool on
  the Mac does the same. Without it GPUI would likely be a little faster.
- 7 of GPUI's 31 moves a run showed no change in the strip watched (its
  hover fill is fainter than Swift's); those are left out, not counted
  as slow.

So at 60 Hz GPUI is at about four refreshes from pointer to glass, and most
of that is frame pacing (above), not the app's work. Mike's lag was the
whole window drawing 93% of the time with 1,400 threads, which 795dd29cb
fixed. If GPUI ever needs to get closer to one or two refreshes, the
lever is the frame pipeline in gpui-pre's Mac platform (drawing as soon
as an input dirties the window instead of waiting for the display link's
next update, and a frame latency that really takes), which means patching
gpui-pre-macos and gpui-pre-apple and keeping that patch across GPUI
bumps. Taffy's layout is about half of each frame's work, but at 7 ms a
frame it isn't what the user waits on.
