# What's left before the GPUI app can replace the Swift one

The GPUI app's gaps against the Swift app, in plain words, for going through
one by one. It comes from a read of both apps' code on 11 Nov 2026 (the
catalogue's rows are in `features.md`, with the ids in brackets here), less
what's landed since; `PARITY.md` has what's been done, slice by slice, with
how each was checked. Tick a line off, or strike it if it doesn't matter, as
you go.

## Being built now

- [x] The design review window: answering an agent's design options, stepping
  through every shot, zoom, minimap, the header's "Review design options (N)"
  and `review:` links (`review.*`, `transcript.review`, `header.pendingreviews`)
- [x] The review window plays clips in place, as the Swift one does, rather
  than stills that open in their apps (`review.media`). In since 12 Nov on a
  Mac
- [ ] The review window shows pages in place, rather than stills that open in
  the browser (`review.media`)
- [x] Projects: the Projects page and sidebar view, a project's page, its
  decisions, Projects ▸ in the row and ... menus, the header's project tiles
  opening the page, New Thread in a project (`projects.*`, `header.projectchip`)
- [x] The Terminal: ⌘J with a shell per thread and the play icon on a
  reply's shell block (`terminal.*`)
- [x] Slack, email and draft boxes in replies with Copy, and chips in your
  bubbles: pictures, thread ids and card mentions (`rich.draft`,
  `rich.*chips`, `transcript.user`, `transcript.agentmessage`)

## You'd notice these in a day of use

### Web pages in the app
- [x] Mermaid diagrams and ```html blocks in replies show as code: there's no
  web runtime yet (`rich.mermaid`, `rich.html`, `rich.chrome`, `rich.fullsize`;
  drawn through the web runtime on a Mac since 11 Nov)
- [x] Web panels (the Convex dashboard and the like) show a placeholder with
  Open in browser (`panel.web`, `panel.convex`, `panel.bar`; pages through the
  web runtime on a Mac since 11 Nov, with Open in browser until it's installed)
- [x] No Browser pane, so BrowserBear's handoffs don't show in the app
  (`browser.*`, `rich.browserbear`, `rail.right`; since 13 Nov, with the
  sign-in link's handoff and show_panel for it)
- [x] A reply's links all go to the system browser: no "in a panel", Chrome
  profiles or Copy Link on right-click, no hover peek, ⌘-click isn't read
  (`rich.webmenu`, `rich.filemenu`, `rich.linkpeek`, `rich.cmdclick`).
  A page "in the panel" still opens in the browser, until the pane draws
  pages through the web runtime.

### Reading and copying
- [x] Selecting text in a reply has no Explain, Quote or Copy bar, and no
  Explain pane (`rich.selectionbar`, `rich.explain`)
- [x] ⌘C copies plain text only, without the links (`rich.copy`)
- [x] Tables stop at their content width, fitting or scrolling inside the
  available space, and list items have Swift's six-point gap (`rich.table`, `rich.blocks`)
- [x] File names in replies don't find the files the thread's tools touched,
  and this Mac's documents and pictures open in their app, not the pane
  (`rich.filerefs`, `file.host`)

### Files and folders in the pane
- [x] No Preview/Markdown switch on a Markdown file (`file.markdown`)
- [x] No find bar (`file.find`)
- [x] Files are read-only: no editing, autosave or ⌘S (`file.editor`,
  `file.conflict`)
- [x] PDFs and videos open in their own app rather than the pane
  (`file.quicklook`, `file.video`). In since 12 Nov on a Mac; Windows and
  Linux still open them in their app
- [ ] .html files open in their own app rather than the pane
  (`file.htmlpreview`)
- [x] A folder doesn't update while it's open, takes one row at a time, has no
  type-to-select, no Space, ⌘↑ or ⌘⇧. keys, no icon view, and the path bar's
  "…" isn't a menu (`folder.*`)

### The composer and the card
- [x] Copying or cutting picture chips loses the pictures; a pasted thread id
  stays plain text; no card mentions (`composer.chipcopy`, `composer.threadid`,
  `composer.cardmention`). In since 11 Nov, but for a click on a card on the
  Highlights' board, which waits for the board. A thread dragged from the
  sidebar goes in as its chip (`drag.threadout`)
- [x] The login picker is one flat list: no room left per login, no provider
  groups (`composer.account`). In since 11 Nov
- [x] No "Pinned to … · Unpin" line in the model picker, and no Claude
  breakdown in the context ring (`composer.modelpicker`,
  `composer.contextring`). In since 11 Nov
- [x] No right-click on Send to schedule the message
  (`composer.schedulefromsend`). In since 11 Nov
- [x] The slash palette can't be resized (`composer.slash.resize`). In since
  11 Nov
- [x] In a narrow panel the chips under the box shrink their labels to fit
  (`composer.modelpicker`, `composer.permission`, `ChipTrigger`).
- [x] The card: no lab-review findings on it (since 12 Nov they're a row in
  the transcript instead, in both apps), no slide between pages, picks
  don't show as chips while it's collapsed, no pictures in a Something else
  box, no beep when Return can't go on (`card.*`). In since 11 Nov; Reduce
  Motion's crossfade in place of the slide waits for the app to read it
  (`foundation.reducemotion`), and the beep is the Mac's only

### Threads and the sidebar
- [x] No trackpad swipe or drag-left to archive a row (`sidebar.swipe`,
  `sidebar.mousedrag`). In since 12 Nov, and rows glide between sections and
  counts roll since 11 Nov (`sidebar.rowmotion`). There's no tick under the
  fingers at the line (GPUI has no haptics), and a row archived from a button
  or ⌘E goes at once rather than sliding off
- [x] No "Open in New Panel" (a thread in a panel of its own), or panels that
  close with their thread (`dock.threadpanel`, `dock.openelsewhere`,
  `dock.contexttabs`)
- [x] No View > Collapse Threads (⌘/) (`menu.view`)
- [x] No trackpad swipe back and forward (`nav.swipe`). In since 12 Nov, and
  the keys show while ⌘ is held since 11 Nov (`nav.hints`). Three-finger
  swipes don't reach GPUI, so it's two fingers only, and over the card's row
  of answer chips a sideways swipe goes back rather than scrolling them
- [x] `threadbear://` links (BrowserBear's pill on an agent's tab) don't open
  this app (`nav.deeplink`). They open the GPUI app once install-app.sh has
  run; `desktop/scripts/thread-links.sh swift` gives them back
- [x] Threads, files and pictures can't be dragged out of the app, and the
  card's Something else box takes no drops (`drag.*`, `highlights.dragout`,
  `file.picture`, `transcript.images`). In since 12 Nov: pictures in the
  transcript, Highlights' files, a folder's rows and the picture in the pane
  drag out as the file, and a row as its thread's chip. A file link in a
  reply's words, a web link, and a thread's id dropped in another app wait
  for GPUI to hand the system more than files
- [x] Another Mac's pictures in a transcript stay blank (`transcript.images`).
  In since 12 Nov they're asked of the runner and say "Couldn't get it from
  bruce" (why, on hover), never showing a file of this Mac's at that path.
  Through Convex they stay on their Mac (the direct link that showed them
  in the Swift app was taken out on 13 Nov)

### Smaller things
- [x] Messages sent from the phone have their phone mark beside the words or
  pictures, with its note on hover and click (`transcript.user`). Row menus
  have their copy icons, and copying a transcript disables its menu item
  until the older pages arrive (`transcript.rowmenu`).
- [x] The offer to continue a refused turn is above the composer
  (`banner.refusal`, `composer.rs`), as in Swift.
- [x] The milestone capsule shows at 100 threads and each thousand, once,
  when you are there to see it (`toast.milestone`). Mike chose a static paw
  with no entrance motion or wave.
- [x] Highlights shows each project's Notion board (`highlights.board`),
  with card mentions and links to the thread working on a card. Done folds
  away, long columns have More, and the board scrolls sideways.
- [ ] An image panel has no Open Simulator button; `.app`, `.icns` and PDF
  icons fall back to a symbol (`panel.image`, `patches.icons`,
  `rail.originbadge`)
- [x] Settings > Runners, Devices and Keyboard are placeholders (the Keyboard
  file is picked up live if edited by hand) (`settings.runners`,
  `settings.devices`, `settings.keyboard`). In since 11 Nov
- [x] Help's two items, the tour and Finish Setting Up (`menu.help`,
  `tour.firstrun`, `tour.intros`, `tour.setup`). In since 13 Nov. About's window, the
  Edit menu, File > Close, the Window menu's Zoom, Bring All to Front and
  window list, the sidebar's and Settings' arrow keys and the Updates
  icon's popover are in since 12 Nov (`tour.about`, `menu.edit`,
  `menu.window`, `sidebar.keys`, `updates.popover`)
- [x] The Mac's spelling/grammar checks, Substitutions and Speech, with
  Look Up, Translate, Writing Tools and Services on selected words. The shared
  selection getter serves both spelling/Speech and the native responder bridge;
  returned text is checked against the original editable selection. Other
  systems omit these Mac services (`composer.spelling`, `rich.textservices`).
- [x] The archive and hibernate flights: the open thread's page drops into
  the Archived heading or sinks into the moon, and the heading's box bounces
  (`juice.archiveflight`, `juice.hibernateflight`). A drawn card with a
  transcript's shape, not a picture of the real one, and level, as GPUI can't
  picture or turn a view
- [x] The little animations Mike picked from Swift-beside-GPUI clips on 13
  Nov: the sidebar's selection sliding, a row archived from its button
  sliding off, a sent message rising, the send arrow's hop, an arriving card
  settling in with the done tick drawing on, the activity bar's bounce,
  sliding square and view settling in, and the Librarian's wobble (a sway, as
  GPUI can't turn a picture). He passed over the bears (inbox zero, the pane
  bear, the working, waking and sitting bears, the milestone toast's wave),
  a finished row's wash and drawn-on tick, and the unread dot's pop
  (`transcript.sendlift`, `librarian.picture`, `rail.activity`, `card.done`)
- [x] The working line's paw moving, in place of the bear: every 2.4 s it
  flips, spins, kneads, waves or leaves a few prints, one at random each
  time, as Mike asked on 13 Nov (`transcript.working`)
- [ ] A row pressing in while the mouse is down: GPUI can't scale a row
- [x] The Mac's accent colour is followed, and Reduce Motion is read by the
  kit at launch and again when the app comes back to the front
  (`foundation.theme`, `foundation.reducemotion`).
- [x] Settings, the Librarian and Usage use the Mac-sized switches
  (`mac_switch.rs`); the larger switches outside forms use `outside_form`.

## Before other people move over

- [x] The app updating itself (`system.selfupdate`)
- [x] A seamless restart onto a new build (`system.handover`): the incoming
  app restores the windows and threads before the outgoing app leaves
  (`handover.rs`).
- [x] Crash reports to Sentry, and the hang watch (`system.crash`,
  `system.hangwatch`)
- [x] The activity log (`system.activitylog`, `foundation.tooltips`)
- [x] Bringing the Swift app's settings and drafts across on first launch
  (`system.swiftimport`)
- [ ] Windows and Linux: notifications, the badge, a menu bar and window
  buttons (`dock.titlebar`, `menu.*`)
- [ ] Accessibility below the window (`foundation.accessibility`)
- [x] Idle redraws: the sidebar's spinners keep the main thread busy in debug
  builds, and panels drawn inline with the shell redo their layout every
  frame (the Usage helper's note): the spinners ask 30 times a second and
  the main panel is cached
