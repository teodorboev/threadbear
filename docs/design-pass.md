# Design pass 1 (2026-10-26)

Mike asked for Threadbear to look as good as the agent apps he uses every
day. This compares it with T3 Code, the Codex app and Claude's desktop app
(window captures taken on his Mac, kept out of git because they show his
threads) and lists what to change. Before screenshots of Threadbear are the
`Tools/screenshots.sh` set taken from commit 6404ea0.

## Mike's direct asks

1. **Row actions, not toolbar buttons.** Remove the Snooze and Archive
   buttons from the window toolbar. Like T3, a thread row shows small icon
   buttons on hover: Snooze (a menu with the presets) and Archive. In the
   Archive view the hover button is Unarchive. Keep ⌘E, the Thread menu and
   the right-click menu. The toolbar keeps only the ... menu.
2. **No thread search.** Remove the "Search threads" field at the top of the
   sidebar, its ⌘K and ⌘F menu items and the app-side plumbing for it. The
   Archive view keeps its own search field (that's how settled threads are
   found again). The runner's `threads.search` stays in the protocol.

## What the others do better

The gap is mostly hierarchy and density. Threadbear shows everything at
the same visual weight, so a finished turn reads as a wall of grey lines
and the sidebar's titles are dimmer than their previews.

### Transcript

- **Work is grouped.** T3 folds a run of tool calls into one line ("Used
  connectors, read 4 files, ran 3 commands and performed 1 other action").
  Codex folds a whole turn's work under "Worked for 1m 45s >" with a
  hairline under it, then the answer. Claude ends a turn with an "Edited 9
  files" card (file name, folder dimmed, +N −M per file, "See all").
  Threadbear gives every thinking block and every tool call its own row
  with a chevron, an icon and a long grey title, and shows every diff in
  full in the flow.
- **Live status.** T3 shows "Working for 53s" while a turn runs; Codex shows
  the current action. Threadbear shows a small spinner at the end of a tool
  row.
- **Diffs are quiet.** The references use faint tints. Threadbear fills
  added and removed lines with saturated green and red blocks, which is
  the loudest thing on screen.
- **Expanded tools show content, not JSON.** Threadbear's expanded Read
  shows the raw input object (`{"file_path": ...}`) in bold before the file.
- **Cards have a clear primary action.** Threadbear's approval card has
  three equal grey buttons inside a bright blue outline.
- **Images sit in rounded frames** with no grey box around them.
- **Reading rhythm.** The references use more line height (about 1.45 to
  1.55) and more space between blocks. Threadbear is at 1.22.

### Sidebar

- **Title first.** In T3, Codex and Claude the title is the brightest text
  in a row. In Threadbear the title is grey and the preview is white.
- **Status in words.** T3 shows "Working 53s" in the accent colour with a
  spinner, or "Waiting 5h 43m", where the time would be. Threadbear has a
  small coloured dot whose meaning you have to learn.
- **Quiet extras.** Threadbear's account capsule ("mike+3") is heavier than
  anything else on the row.
- **Counts at the bottom.** T3 has "Snoozed (7)" and "Settled (150)" as
  clear footer rows. Threadbear's footer buttons are cramped together.
- **A "New thread" entry** at the top (Codex "New chat", Claude "+ New").

### Header and composer

- **Header:** T3 uses one line, "misc / Design a Personal Agent Workspace".
  Threadbear stacks a grey title over a full path (`/Users/mike/dev/...`).
- **Composer:** the references float a rounded card (about 20 pt corners)
  on a slightly lighter fill, with a round send button (accent or white)
  and a row of small, quiet controls. Threadbear's is a square-cornered
  outline with a grey square send button. Its chips show the full folder
  path and the full email address.
- **New thread:** Codex and Claude put the composer in the middle of the
  empty pane under a short heading. Threadbear leaves the pane empty with
  centred grey text and the composer at the bottom.

## Spec

Numbers are points. Keep both light and dark mode right (check both with
`Tools/screenshots.sh --appearance light`). Keep the copy rules in
AGENTS.md: plain words, no eyebrow or kicker labels, no em dashes.

Text sizes are one step above the first draft of this brief. Review round
1 put Threadbear beside Codex and Claude and its 14 pt body read about 2
pt smaller than theirs, so every size moved up together. The sizes below
are the ones in `TypeScale` (Theme.swift), which everything takes its
sizes from. They're the 100% sizes: ⌘= and ⌘- in the View menu (⌘0 puts
it back) scale every one of them through `TextZoom`, and with them every
layout size that goes through `zoomed` (paddings, icons, row heights, the
column width), so the window zooms like a browser or VS Code. Text-only
zoom left the icons and the room around them small beside big text.

A few rules changed in the design reviews after the first build, each for
a reason given where it applies: the sidebar doesn't repeat its section
heading on a row, an allowed approval folds into the work, and a live
group leaves the turn's one spinner to the working line.

### Sidebar and header (owner: sidebar)

- Row: title 14 pt, `.primary`, semibold when the thread needs Mike or is
  unread, regular otherwise. Preview 13 pt `.secondary`, one line. Right
  side of the title line: the relative time in 11 pt with monospaced
  digits (a readable faint grey; the system tertiary was about 2:1), or the
  status instead of it:
  - running: an accent spinner and how long the turn has run ("37s"), in
    the accent colour, counting once a second,
  - waiting on an approval or question: how long it has waited ("9m"), in
    orange,
  - failed: "Failed" in red; cut off by a runner restart: "Stopped",
  - finished and unread: a 7 pt accent dot before the title (Mail style).
- A row says its status in words ("Working 37s", "Needs you") only when
  it isn't under the heading that already says it. (Since 27 Oct there's
  no Needs you heading: it merged into Inbox, so waiting rows say "Needs
  you" again.) Under "Running" and "Needs you" the word took about 60 pt of the title line, so the rows
  that mattered most had the most cut-off titles; the heading, the accent
  spinner and the orange time say it there. The tooltip still says
  "Working" or "Waiting for you". Rows in the Snoozed and Archive lists
  show when they come back or were archived, not a status.
- Account: only when it isn't the default account, as plain 11 pt
  `.tertiary` text (the part of the email before the @), not a capsule.
- Hover: the time or status is replaced by two 22 pt borderless icon
  buttons, Snooze (`moon.zzz`, menu of presets) and Archive
  (`archivebox`). In the Archive view: Unarchive
  (`arrow.uturn.backward`). Clicking them must not select the row.
- Row padding 8 vertical, 18 leading (a gutter for the unread dot, so
  titles line up with or without one) and 10 trailing; selected background
  `Color.primary.opacity(0.08)` (0.06 when the window isn't key),
  8 pt corners. Section headings 13 pt semibold `.secondary` (no weaker
  than the rows they group), 18 above, 6 below.
- Top of the sidebar, where search was: a 14 pt "New thread" row with
  `square.and.pencil` and a ⌘N hint in `.tertiary`.
- Footer: "Snoozed" and "Archive" as two full-width rows with counts
  ("Snoozed 7"), 13 pt, icon + label + count, hover highlight. The list
  fades out over its last 30 pt above the footer rather than cutting a
  row through its text.
- Header: one line, `<folder name> / <title>`, folder in `.secondary`,
  title in `.primary` semibold 15 pt (at 14 the title was smaller than the
  15 pt text under it). The full path goes in the tooltip.
  Toolbar: only the ... menu.
- New thread state: a heading ("What should we work on?") and the
  composer centred in the pane (about 40% down), with "Starts in misc on
  studio" below it in `.tertiary`. The composer moves to the bottom
  once the first message is sent.
- Screenshots: add a shot of a sidebar row in its hover state (a debug
  override for the hovered row is fine).

### Composer (owner: composer)

- Card: 20 pt corners, fill `Color.primary.opacity(0.05)` on dark, white
  with a soft shadow on light, 0.5 pt `separator` border, 14 pt padding,
  12 pt margin from the window's bottom edge, the same width as the
  transcript column (780 max) and centred under it.
- Text 15 pt on the transcript's 23 pt lines, placeholder "Message" in
  `.tertiary`, at least two lines tall, grows to about 10 lines then
  scrolls.
- Controls row: "+" on the left, then quiet menu chips in 13 pt
  `.secondary`: account (the part of the email before the @), model
  ("Default model" becomes the account's model name if known, else
  "Default"), and the permission chip "Full access" (`lock.open`) or "Ask
  before edits" (`lock`). Thin 12 pt vertical dividers between chips.
- **No folder chip** (added after Mike asked "is this directory needed?",
  first for the default folder, then for every folder on 3 Nov). A thread
  still needs a folder, but nearly every one is misc; the header icon's
  tooltip names it, and a new thread says "Starts in …". "+" is a menu:
  "Attach images…" (the file picker) and "Folder"
  (recent folders, "Choose…", and the default marked), which is how you
  change it.
- Send: a 28 pt circle on the right. With text, filled `.primary` with an
  `arrow.up` in the background colour; empty, `Color.primary.opacity(0.1)`
  with a `.tertiary` arrow. While running it becomes Stop (`stop.fill`) in
  the same circle.
- Attached image thumbnails: 56 pt, 8 pt corners, a remove (x) badge on
  hover.

### Transcript (owner: transcript)

- **Work groups.** In ThreadbearCore, fold each maximal run of reasoning,
  tool calls (with their results and diffs) and system notes inside one
  turn into a group row. Assistant text, user messages, approval and
  question cards still waiting or turned down, plan cards, images, errors
  and the turn footer end a group and are never hidden in one. A run of
  one item is shown as that item, not as a group.
  - An approval Mike allowed is a step of the work it let run: one work
    line ("Allowed  Write release.sh") that folds into the run around it.
    As a card it split every edit in an "Ask before edits" thread into a
    group, a card, the edit and a second group. When the tool call it
    allowed has its own row, the line doesn't open: that row opens to the
    same file or command. Denied and expired ones are lines too, between
    runs, and a waiting one is a card.
  - Collapsed (the default): one line, a `hammer` icon, a summary in
    14 pt `.secondary` and `chevron.right` after the words: "Read 3
    files, searched twice, edited 2 files, ran 4 commands" (and "used 2
    tools", "thought" when there's nothing else, "ran an agent", "fetched
    2 pages"). Counts in words up to ten is optional; keep it short.
  - Expanded: the group's rows as they are now, indented 18.
  - Live (the last group of a running turn): the summary plus the current
    action's title after a dot, e.g. "Read 3 files · Running npm test",
    the action the brightest text on the line. Stays collapsed unless Mike
    opens it. It keeps its hammer rather than a spinner: the working line
    right under it is the turn's one spinner, and two stacked busy lines
    read as a stray child row.
  - Ids: `g:<first item id>`, stable as the group grows, so expansion and
    cached heights survive streaming.
  - Unit tests for the grouping rules.
- **Working line.** While a turn runs, the last row is "Working for 12s"
  (counting) in 14 pt `.secondary` with a spinner (at 12 pt tertiary it
  was hard to read). It replaces the spinner on tool rows and groups.
- **Changed files card.** After a turn's last row (before the footer), if the
  turn edited or wrote files: a card "Edited 3 files" listing each file
  (name in `.primary`, folder in `.tertiary`, `+N` green `−M` red at the
  right). Clicking a file expands its diff under it. 10 pt corners,
  0.5 pt separator border, faint fill.
- **Diffs.** Header row with the path and +N −M; lines on faint tints (added
  green at 10 to 12% opacity, removed red at 10 to 12%), the +/- sign and
  line numbers in a gutter in `.tertiary`, 13 pt monospaced, 10 pt
  corners, collapsed after 12 lines with "Show 30 more lines".
- **Expanded tools.** Commands: `$ command` then the output in a
  monospaced block. Read: the file path then the content with line numbers.
  Search: the query then the matches. Others: input as `key: value` lines
  (not JSON braces) then the result. No bold JSON.
- **Markdown.** Body 15 pt on 23 pt lines (about 1.5), 10 between
  paragraphs, 6 between list items. Headings: 18 / 16 / 15 semibold with
  16 above. Inline code 13 pt monospaced on a faint pill. Code blocks: 8 pt
  corners, faint fill, 0.5 pt border, 12 padding, 13 pt (one monospaced
  size for code, tool output and diffs).
- **User messages.** Right-aligned bubble, 18 pt corners, 10/14 padding,
  max 80% of the column, fill a touch tinted towards the accent (about 8%
  accent over 6% white on dark). Attached images above it, right-aligned,
  rounded, no grey box.
- **Thinking.** One quiet line, `Thought` + the first line of the summary
  in `.tertiary`, inside groups only (a lone thinking block is its own
  one-item row).
- **Approval and question cards.** 12 pt corners, faint fill, 0.5 pt border
  with a 3 pt accent bar on the left instead of the blue outline. Title 14
  pt semibold ("Run this command?", "Write .gitignore?"), the payload in a
  monospaced block (the command, or the file path and the start of the
  content). Buttons: Allow as the prominent default (accent fill, ⏎),
  Always allow and Deny as plain bordered buttons, Deny on the right.
- **Images.** 8 pt corners, 0.5 pt separator border, no box behind them,
  max 320 wide in the flow.
- **Turn footer** (duration and cost) stays as is, 11 pt `.tertiary`.

## Not in this pass

Syntax colouring in code blocks, a right-hand panel like Claude's, a
project chooser. Performance bars in plan.md still apply: grouping must
keep row inserts and reloads incremental, not full reloads.
