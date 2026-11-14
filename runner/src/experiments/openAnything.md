---
id: openAnything
name: Open Anything (⌘K)
title: Open Anything: ⌘K or ⌘P opens any thread, panel, project or page by name
added: 2026-11-13
on: false
reminder: 7febfd2c
---

## What it does

In the new desktop app, ⌘K (or ⌘P, or Go > Open Anything…) puts a box over the window, with the rest dimmed. With nothing typed it shows Recent (what you've opened lately) and Go to (the app's pages, with their keys). Type part of a name and it finds panels, threads, projects and the app's pages and Settings sections, in that order of groups; letters in order are enough ("ytcm" finds yt-channel-manager). Threads are found as the sidebar's search finds them, archived and hibernating ones too. Return opens the first one named for what you typed, so "usage" opens Usage rather than a thread that only mentions it. ↑ and ↓ move, ⌘Return opens beside (a thread in a panel of its own, or a panel next to the main one), and Esc puts it away. Off, the two keys do nothing and the menu item and their lines in Settings > Keyboard go. The Mac app hasn't got it.

## Lives in

`desktop/core/src/open_anything.rs` (what's listed, the ranking and the pick), `desktop/gpui/src/open_anything.rs` (the box, and `open_anything_on`, which reads the experiment), `OpenAnything` and `OpenAnythingAlso` in `desktop/core/src/shortcuts.rs` (`openAnything` and `openAnythingAlso` in shortcuts.json, ⌘K and ⌘P) and `desktop/gpui/src/shell.rs` (the keys, the Go menu item, the command), Recent in `open_anything_recent` in the desktop app's settings (`desktop/gpui/src/settings.rs`), and `features/open-anything.feature`. The app learns it's on from `hello`'s `experiments`, or Settings' switch. The runner keeps nothing of its own for it.

## Taking it out

Delete both `open_anything.rs` files and the feature, the two shortcut commands with their actions and the Go menu item, `anything`, `anything_recent` and `anything_panels_shown` in `desktop/gpui/src/app.rs` with what records them in `shell.rs`, `open_anything_recent` in settings.rs, `bound_open_anything`, and the `anything` automation command and dump. Nothing in the runner or the backend changes.
