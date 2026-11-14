# Scrolling and drawing: measurements

What a scroll costs the GPUI app, from `features/tools/render-perf.ts`: real
trackpad flings through the window server on Tinker Desk's M2 (a 60 Hz
screen, so 16.7 ms a frame; Mike's M5 is a 120 Hz screen, 8.3 ms a frame),
release builds with frame-stats, at the size of Mike's install: 1,186
threads over 80 projects, nearly all archived (TB_MOCK_PERF_PROJECTS), the
decisions experiment's folded Projects rows, and a 1,500-turn thread open.
Builds are compared with `features/tools/render-ab.mjs`, which takes them in
turns so the host's other work falls on each alike.

The number to trust most is the main thread's CPU time per frame shown
(`mainCpuMs` from frame-stats): Tinker Desk is a shared check host, often at
a load of 30 to 200, and frame times and late frames move with that much
more than the main thread's own CPU time does (it leaves out waiting for a
core, though a loaded Mac may still move the thread to an efficiency core,
so loaded and idle numbers aren't comparable with each other). "Loaded" adds one spinning
process per core on top.

## 13 Nov: sidebar lists

Mike's recording (13 Nov) was the Projects list scrolling with a delay.
A scroll in a plain scrolling div makes GPUI draw the view it's in again, so
each frame of a sidebar scroll rebuilt the whole sidebar: every row of the
list, and its sections worked out afresh from every thread.

First measured against main at 23767da5c, medians of 3 rounds each: the
sections kept between frames and sorted by keys read once (B), and with rows
far out of view drawn as gaps too (C):

| Scroll | CPU a frame, ms (main, B, C) | Late frames (main, B, C) |
|---|---|---|
| Inbox | 19.9, 15.9, 9.3 | 66%, 37%, 1.7% |
| Projects | 14.7, 12.1, 9.4 | 30%, 20%, 2.9% |
| Inbox, loaded | 14.5, 11.6, 7.8 | 33%, 16%, 8% |
| Projects, loaded | 13.4, 9.3, 8.1 | 67%, 11%, 7% |

A profile of C showed a row's hover redrawing the whole window as rows passed
under the pointer, and perf 1's change (795dd29cb) took that, the kept
sections and the sort. Measured again on main after it (14 Nov), the gaps
alone:

| Scroll | CPU a frame, ms (main, gaps) | Late frames | Slowest 1 in 100, ms |
|---|---|---|---|
| Inbox | 9.67, 9.06 | 3.4%, 2.3% | 33.7, 22.7 |
| Projects | 9.23, 9.07 | 3.7%, 4.0% | 24.4, 21.2 |
| Inbox, loaded | 14.0, 10.9 | 11.7%, 0.6% | 19.5, 16.0 |
| Projects, loaded | 11.2, 9.5 | 2.1%, 1.3% | 17.6, 17.0 |

## 14 Nov: a long real thread

A copy of a real 13,700-event thread with pictures and thread chips
(private, never in git), scrolled: 9.2 ms of main-thread CPU a frame on the
M2, 5% of frames late. Keeping each reply's thread chips and pictures worked
out between frames (they parse a reply's markdown whole) made no measurable
difference (9.20 to 9.05 ms), so it was left out. Where a frame goes
(`sample`): drawing it is 25% of the main thread's time, of which layout
14%, prepaint 15% and building elements 7%; the composer, drawn again on
every frame of the transcript's scroll though nothing in it changed, is
about a fifth of that.

## 14 Nov: the transcript's rows and the box as cached views (dropped)

GPUI draws again every view above one that changed, and a cached view that
draws again makes every cached view inside it draw again too (`refreshing`
in gpui-pre's view.rs). So with the main panel cached, nothing inside it
could be kept from the frame before: a transcript scroll built the box
again, and the caret's blink (every 500 ms) built the transcript's rows
again. Tried: the rows (`TranscriptList` as a view) and the box each a
cached view of their own, the main panel cached only when it isn't showing
a thread, the pane passing its own and the app's changes on to both. Codex
found four ways that went stale, all fixed, and a profile showed the box no
longer built during a scroll. Against main, 3 rounds each, with an
accessibility client present (the tool's hit test turns GPUI's
accessibility on, for both builds alike):

| | CPU a frame, ms (main, cached) | Late frames |
|---|---|---|
| Real thread, scrolled | 8.76, 8.93 | 2.3%, 3.1% |
| Made-up thread, scrolled | 8.55, 8.32 | 0%, 0% |
| A reply streaming into the open thread | 8.08, 11.47 | 0%, 12% |
| A reply streaming into one kept loaded | 7.78, 15.67 | 0%, 39% |

No gain scrolling, and streaming's frames got heavier, so it was left out
(the branch perf2-cached-panes has it). The measurement worth keeping from
it: on main, a reply streaming into a thread kept loaded behind the open one
costs the main thread 438 ms a second, nearly half a core, for a thread
nobody's looking at, as every event draws the whole app again.

## 14 Nov: a reply streaming in no longer draws the whole app again

Every `thread.event` (each few words of a reply coming in, for the open
thread and any kept loaded) ended in a notify of the whole app, which drew
the sidebar, the main panel and the side pane again, as often as the words
came. The thread's own pane already draws its events again every 33 ms. Now
the app isn't told; what else an event changes (a title, a status) comes as
`thread.updated`, which still tells it. Against main, 3 rounds each, a
reply at 30 chunks a second for 6 seconds on the real thread:

| | Main-thread CPU a second (main, change) | Frames drawn |
|---|---|---|
| Into a thread kept loaded behind the open one | 437, 370 ms | 330, 142 |
| Into the open thread | 444, 429 ms | 346, 306 |

Per frame the change looks worse (16 ms a frame, a third of them late, for
the kept thread), but that's the same work spread over fewer frames: what's
left drawing is the streaming thread's spinner in the sidebar, which costs
the same on main with nothing streaming (perf 3 measured 22 frames a second
at about 17 ms each). Main's extra 190 frames were the whole app drawn again
per chunk. `sample` attached to the app halves what it gets through, so
profiles here are for where time goes, not how much.
