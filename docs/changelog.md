# The changelog (threadbear.mikecann.app/changelog)

What's new in each daily release, for people who use Threadbear rather than people reading its
commits. Mike picked the layout on 10 Nov 2026 after seeing Wayne Sutton's bearfooding changelog
(tame-weasel-262.convex.site): a thread down the page with a sewn button at each release, the
release's main changes (as many as can be shown, often 6 to 12) as cards with a short clip or a picture, and everything
else as plain Added / Changed / Fixed lines.

What Mike said after the first version (11 Nov), which this page follows:
- Videos beat before/after sliders, so a card gets a clip wherever something moves or happens in
  steps, a still only when a still really shows it. There's no before/after type any more.
- More than three a day, and then "more videos and screenshots please": every card has one, and anything a user can see that can be filmed gets a card.
- Check something is really new before calling it Added. "Paste a screenshot" went up as new on
  9 Nov when pasting pictures had worked "for ages"; that release only fixed one kind of paste.

This is Mike's copy's page. In a fork there's nothing to do here.

## Where things are

- `site/changelog/<YYYY-MM-DD>.json`: one file per release tag (`release/<YYYY-MM-DD>`). The
  format is at the top of `site/scripts/changelog.mjs`, which builds the page and refuses a bad
  file. A release with nothing a user would notice gets no file.
- Pictures and clips: not in the repo, so a fork doesn't download them. They're made in
  `site/.media/changelog/<release>/` (ignored by git; `npm run serve` serves it at `/media/`),
  then `npm run media -- upload <release>` puts them in the R2 bucket `threadbear-site-media`
  (it refuses a name that is already up with a different file),
  which the site's Worker serves at `/media/changelog/<release>/<name>`. A name never changes
  once it's up (they're cached for good): a new picture gets a new name.
- `npm run media -- check` says whether every file the JSON names is up on the live site.

## Writing a release's entry

The daily changelog thread does this once a new `release/*` tag exists, for each release tag
that has no file yet (`git tag -l 'release/*'` against `ls site/changelog`).

1. Read the release: `git log --no-merges --format='%h %s%n%b---' release/<previous>..release/<this>`,
   and the tag's own plain notes (`git cat-file -p release/<this>`, its "What's new:" lines, a
   good start but check them against the commits).
2. Write `site/changelog/<this>.json`:
   - `highlights`: every change a user can see that can be filmed (often 6 to 12; fewer for a small release),
     each something the debug app can show, with a short title and one or two short sentences on
     what you can now do and where (menu path, setting, shortcut). Mike likes less text.
   - Before calling anything Added, or a big Changed, check it didn't exist before this release:
     `git log --oneline -S '<a distinctive string>' -- app runner ios web | tail -3`, or earlier
     commits about the same feature. If it existed, it's a fix or a change: say exactly what
     changed.
   - Credit everyone a change came from, on cards and plain lines alike: their PR, a fork it
     was ported from, their issue, or a bug they reported that the commit names ("Ville's Auto
     threads asked..."). A card takes `"credit": "Name"`; a line becomes
     `{ "text": "...", "credit": "Name" }`. Find them with
     `git log --format='%h %an | %s' <range> | grep -v 'Mike Cann'`, the commits' non-Claude
     `Co-Authored-By` lines, and "fork", "PR #", "issue" and people's names in the bodies; `gh
     api users/<login> -q .name` gives a real name (else use the login). Mike, 13 Nov:
     "attribute any fixes or whatever to people if it came from other people".
     Each name links from `site/changelog/people.json`: their X account when `gh api
     users/<login> -q .twitter_username` gives one, else their fork
     (`https://github.com/<login>/threadbear`, if it answers), else their GitHub profile. The
     build refuses a credit with nobody from that file in it, so add a new person there first.
   - `added` / `changed` / `fixed`: every other change a user would notice, one plain line each,
     several commits about one thing as one line. Leave out tests, refactors, docs, CI, agent
     instructions, review rounds and security-scan internals; security fixes users should know
     about are one plain `fixed` line.
   - Plain words for someone who's never seen the code: no file or function names, no em or en
     dashes (the build refuses them), no marketing words, Australian spelling. Check it against
     `~/.claude/skills/mike-voice/references/ai-tells.md`.
3. Pictures for the highlights (below), then their `media` fields.
4. `cd site && npm run build`, look at `/changelog` with `npm run serve`, then
   `npm run media -- upload <this>`, land the commit, `npm run deploy` from the main checkout
   and `npm run media -- check`.

## Making the pictures

Use the debug app on the mock runner (`app/Tools/dev-app.sh`; the commands are in
app/README.md, "Seeing and driving it from a script"). Its window stays behind Mike's work and
nothing moves the real pointer, so it's fine on Mike's Mac, one build at a time with
`taskpolicy -b`. Always `--appearance dark --size 1240x820`, and stop every instance when done.

- For the release's own build: borrow a worktree (`./borrow-worktree.sh`, it has build caches),
  `git checkout --detach release/<this>` in it, then `app/Tools/dev-app.sh start --mock
  --appearance dark --size 1240x820 --id cl-after`. Give it back afterwards.
- **clip** is the default: anything that moves or happens in steps (a palette opening as you
  type, a card filling as you swipe, a change to how something works), a few seconds long. For
  a change to something that existed, show the new behaviour in the clip; there's no
  before/after. **image** is only for something that reads standing still. To record, compile
  `site/scripts/record-window.swift` once, get the window's id with
  `dev-app.sh do '{"cmd":"window","sheet":false,"settings":false}'` (the third word), record it
  for a few seconds while sending the commands with short sleeps between, then crop and encode:
  `ffmpeg -ss <start> -i raw.mov -t <secs> -vf "crop=W:H:X:Y,fps=30,format=yuv420p" -c:v libx264
  -preset slow -crf 24 -threads 2 -movflags +faststart -an <name>.mp4`, and a WebP poster from a
  frame that shows the point.
- Shots are at 2x. Crop to the part that shows the change, with a little around it, as WebP
  (`ffmpeg -i shot.png -vf crop=W:H:X:Y -c:v libwebp -quality 88 <name>.webp`); the page caps
  pictures at 520 points high, so very tall crops come out small.
- Nothing private in a picture: the demo threads are made up, but some mock data (the account
  picker design review, for one) shows Mike's real email addresses. Crop those out or use
  another screen.
- Every highlight has a clip or a picture: the build refuses one without (Mike, 11 Nov, "more
  videos and screenshots please"). Something that only draws with the window in front (the
  Terminal panel) is filmed on Tinker Desk, where taking over the screen is fine: a temp clone
  of the release, commands through `ssh tinker-desk '~/.local/bin/desk "<cmd>"'`, then delete
  it all, as Tinker Desk is kept clear of Threadbear. Something the mock can't show at all (the
  Browser panel, a thread waiting for a connection) becomes a plain line, never a faked picture.
- `alt` says what the picture shows, for someone who can't see it.
