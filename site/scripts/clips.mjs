#!/usr/bin/env node
// Records the hero's clips: the demo app (mock runner, made-up threads, dark mode) driven through
// one feature each, recorded from its window and encoded into public/clips/. The results are
// committed, so this only runs when the clips should change. Encodes use two threads, to stay
// light on the Mac they run on.
//
//   MOCK_SCRIPTED_TURNS=$PWD/scripts/clips/turns.json \
//     ../app/Tools/dev-app.sh start --mock --appearance dark --size 1240x820 --id siteclips
//   node scripts/clips.mjs [name...]        (all of them without names)
//   ../app/Tools/dev-app.sh stop --id siteclips
//
// turns.json has the made-up replies the mock streams when a clip sends a message or answers a
// question, and made-up content for a few demo threads (their asks, a review, a panel). The
// pointer is drawn afterwards (clips/cursor.mjs) from where each clip clicked, with the traffic
// lights over the window's corner (a window in the background draws them as a grey blob); both
// pictures are in clips/overlays. Needs Screen Recording permission for the shell
// (scripts/record-window.swift) and ffmpeg. A clip changes the mock's threads (it sends
// messages, answers a question, archives), so record from a fresh instance: stop and start it
// again before running them all.
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { CURSOR, RING, cursorPath, sendcmds } from "./clips/cursor.mjs";

const site = join(dirname(fileURLToPath(import.meta.url)), "..");
const devApp = join(site, "../app/Tools/dev-app.sh");
const id = process.env.CLIPS_APP_ID ?? "siteclips";
const out = join(site, "public/clips");
// A fresh folder of our own (mkdtemp, 0700): the recorder is compiled into it and then run, so a
// fixed name another account could make first would let it swap the program (Codex's review of
// Daybreak 114/116, 13 Nov).
const work = mkdtempSync(join(tmpdir(), "threadbear-site-clips-"));
process.on("exit", () => rmSync(work, { recursive: true, force: true }));
mkdirSync(out, { recursive: true });

function run(...commands) {
  const r = spawnSync(devApp, ["--id", id, "do", ...commands.map((c) => JSON.stringify(c))], { encoding: "utf8" });
  if (r.status !== 0) throw new Error(`${commands.map((c) => c.cmd).join(", ")}: ${r.stdout}${r.stderr}`);
  return r.stdout;
}
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

// What the pointer does during a recording, for clips/cursor.mjs: seconds since the recording
// started, and where (window points).
let events = [];
let recordingStarted = 0;
const now = () => (Date.now() - recordingStarted) / 1000;
/** A real click at a spot in the window (Automation's `point`), with the pointer drawn there. */
function click(x, y) {
  events.push({ t: now(), x, y, click: true });
  return run({ cmd: "point", x, y });
}
/** The pointer goes to a spot (a control the next command presses by script, or a text box). */
function aim(x, y, clicks = true) {
  events.push({ t: now() + 0.05, x, y, click: clicks });
}
/** Types like a person: the composer (or a search box) filled a few letters at a time. */
async function type(text, field = "composer") {
  for (let i = 5; i < text.length + 5; i += 5) {
    const part = text.slice(0, i);
    run(field === "composer" ? { cmd: "type", text: part, replace: true } : { cmd: "search", text: part, projects: true });
  }
}
/** Waits for a thread's turn to finish, as the status command reports it. */
async function idle(thread, ms = 20000) {
  const end = Date.now() + ms;
  await wait(600);
  while (Date.now() < end) {
    const status = JSON.parse(run({ cmd: "status" }).replace(/^status ok /, ""));
    if (status.threads.find((t) => t.id === thread)?.status === "idle") return;
    await wait(250);
  }
  throw new Error(`${thread} still running`);
}

// The mock writes the landing pages thread's mockups into the instance's data folder.
const mockup = (name) => join(tmpdir(), "threadbear-dev-app", id, "data/work/site/mockups", name);

// One per slide in scripts/build.mjs's SLIDES, in the same order, which follows the launch video's:
// the bits Mike is most excited about. `setup` runs before recording; `window` names a window
// other than the main one to record (alone, so the main one doesn't cover it); `file` is a clip
// made elsewhere, cropped to the others' shape instead of recorded.
const CLIPS = {
  // A card of asks answered one click at a time (each pick moves the card on), then sent, and
  // the thread goes off to work. Spots are where the card's buttons sit in a 1240 x 820 window.
  decisions: {
    setup: () => run({ cmd: "mode", mode: "inbox" }, { cmd: "open", id: "demo_testflight_beta" }),
    play: async () => {
      await wait(1300);
      click(484, 641); // Done
      await wait(1300);
      click(599, 620); // The testers group
      await wait(1300);
      click(494, 642); // Send them
      await wait(1000);
      click(863, 771); // send
      await idle("demo_testflight_beta");
      await wait(1400);
    },
  },
  // The options window: each design in turn, then side by side, one picked and a reply typed.
  // Its controls are pressed by script; the pointer goes where they are.
  options: {
    setup: () => run({ cmd: "mode", mode: "inbox" }, { cmd: "open", id: "demo_design_review" }, { cmd: "review" },
      // 820 less its 33 pt title bar, so the whole window is the main one's 1240 x 820.
      { cmd: "resize", review: "review_account_picker", w: 1240, h: 787 }),
    window: { cmd: "window", review: "review_account_picker" },
    play: async () => {
      await wait(1200);
      aim(852, 790); // next
      await wait(150);
      run({ cmd: "review", show: 1 });
      await wait(1300);
      aim(852, 790);
      await wait(150);
      run({ cmd: "review", show: 2 });
      await wait(1300);
      aim(795, 790); // all of them
      await wait(150);
      run({ cmd: "review", mode: "All" });
      await wait(1400);
      aim(921, 190); // B's tick box
      await wait(150);
      run({ cmd: "review", pick: "B" });
      await wait(900);
      aim(1069, 732); // the reply box
      await wait(300);
      const reply = "Stats first, but give the map a bit more room";
      for (let i = 4; i < reply.length + 4; i += 4) run({ cmd: "review", reply: reply.slice(0, i) });
      await wait(1500);
    },
  },
  // Finished threads archived from their row's archive button (drawn as hovered), each flying
  // off into the archive; the next one slides up into the same spot.
  inbox: {
    setup: () => run({ cmd: "mode", mode: "inbox" }, { cmd: "open", id: "demo_upload_resume" }),
    play: async () => {
      for (const id of ["bruce_media", "demo_tidy_imports", "demo_settings_tabs"]) {
        await wait(900);
        aim(200, 536, false);
        await wait(250);
        run({ cmd: "hover", id });
        await wait(550);
        aim(353, 536);
        await wait(150);
        run({ cmd: "archive", id });
        await wait(700);
      }
      await wait(900);
    },
  },
  // No folder to pick: a new thread just starts. Then the librarian's page, where it files
  // threads into projects.
  librarian: {
    setup: () => run({ cmd: "mode", mode: "inbox" }, { cmd: "open", id: "demo_upload_resume" }),
    play: async () => {
      await wait(1000);
      // New thread by script, the pointer on its button: a real click opens the button's menu,
      // which holds the app until it closes.
      aim(366, 57);
      await wait(150);
      run({ cmd: "new" });
      await wait(900);
      // Its message box: the pointer only, as a real click into a text view starts its own
      // drag-to-select loop and waits there for a mouse up.
      aim(670, 325);
      await wait(300);
      await type("Why does the Walkies map jump when I zoom out?");
      await wait(400);
      click(894, 383); // send
      await wait(2600);
      click(21, 166); // the Librarian, in the left rail
      await wait(2800);
    },
  },
  // The iPhone app, from the launch video's B-roll (Mike's own thread on his phone: the bike
  // pedal picture it made him). No pointer: it's a phone.
  phone: {
    file: process.env.CLIPS_PHONE ?? join(homedir(), "dev/convex/convex-videos/threadbear-launch/broll/bike-pedals/phone-thread.mp4"),
  },
  // Patches: the built-in Convex panel and a project's own stats panel, from the icons down the
  // pane's right edge, then one of its buttons above the message box.
  patches: {
    setup: () => run({ cmd: "mode", mode: "inbox" }, { cmd: "open", id: "demo_dark_mode" }, { cmd: "panel", id: "threadbear.highlights" }),
    play: async () => {
      await wait(1100);
      click(1216, 57); // Convex
      await wait(1900);
      click(1216, 93); // Walkies stats
      await wait(1900);
      click(730, 672); // Deploy to prod
      await wait(2000);
    },
  },
  // Mike's release in the Updates panel, brought into this copy by a thread.
  fork: {
    setup: () => {
      run({ cmd: "mode", mode: "inbox" }, { cmd: "open", id: "demo_upload_resume" });
      run({ cmd: "mock", method: "updates", params: { state: "idle", upstream: { state: "available", checkedAt: new Date().toISOString(),
        release: { tag: "release/2026-11-05", commit: "64b4f2e", date: new Date().toISOString(), changes: 6, subjects: [
          "The hero plays clips of the app", "Setup asks which Convex team in the dry run", "Copying from a thread keeps its links in Slack",
          "A card about pools the first time a pool has two logins", "Editing a message asks before it stops background work",
          "Schedules can change their login"] } } } });
    },
    play: async () => {
      await wait(1000);
      click(21, 720); // Updates, in the left rail
      await wait(2100);
      click(113, 724); // Bring Them In
      await wait(5500);
    },
  },
};

const recorder = join(work, "record-window");
execFileSync("swiftc", ["-O", join(site, "scripts/record-window.swift"), "-o", recorder], { stdio: "inherit" });
const windowId = run({ cmd: "window" }).match(/window ok (\d+)/)[1];
// The traffic lights (from the dark screenshot set), laid over each clip's top left corner; the
// pointer; and a ring for each click.
const overlays = join(site, "scripts/clips/overlays");
const lights = join(overlays, "lights.png");

for (const name of process.argv.slice(2).length ? process.argv.slice(2) : Object.keys(CLIPS)) {
  const clip = CLIPS[name];
  if (!clip) throw new Error(`No clip called ${name}`);
  const mp4 = join(out, `${name}.mp4`);
  if (clip.file) {
    // Cropped from the middle to the windows' 1600 x 1058, sound off.
    execFileSync("ffmpeg", ["-loglevel", "error", "-y", "-i", clip.file, "-vf",
      "crop=ih*1600/1058:ih,fps=30,scale=1600:1058:flags=lanczos,format=yuv420p", "-c:v", "libx264", "-preset", "slow", "-threads", "2",
      "-crf", "26", "-an", "-movflags", "+faststart", mp4]);
  } else {
    console.log(`${name}: setting up`);
    try { run({ cmd: "escape" }); } catch {}
    try { run({ cmd: "updates", show: false }); } catch {}
    clip.setup();
    await wait(1200);
    const raw = join(work, `${name}.mov`);
    // A stop file left from a run that didn't finish would end this recording at once.
    rmSync(raw, { force: true });
    rmSync(`${raw}.stop`, { force: true });
    const target = clip.window ? run(clip.window).match(/window ok (\d+)/)[1] : windowId;
    const recording = spawn(recorder, [target, raw, "60", ...(clip.window ? ["--window-only"] : [])], { stdio: ["ignore", "pipe", "inherit"] });
    const exited = new Promise((r) => recording.on("exit", r));
    // The clock for the pointer starts when the recorder says it's recording.
    await new Promise((r) => recording.stdout.on("data", (d) => String(d).includes("recording") && r()));
    recordingStarted = Date.now();
    events = [];
    await wait(400);
    await clip.play();
    writeFileSync(`${raw}.stop`, "");
    await exited;
    if (!existsSync(raw)) throw new Error(`${name}: nothing recorded`);
    // H.264 for every browser, 1600 wide (the hero shows it at 1100 at most, so sharp on 2x
    // screens up to about 800), 30 fps, no sound, the index up front so it starts at once.
    const seconds = Number(execFileSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", raw], { encoding: "utf8" }));
    const path = cursorPath(events, [...name].reduce((n, c) => n * 31 + c.charCodeAt(0), 7));
    const cmds = join(work, `${name}.cursor.txt`);
    writeFileSync(cmds, sendcmds(path, seconds));
    // Each click's ring, a fifth of a second under the pointer, then the pointer on top.
    const n = path.clicks.length;
    let graph = `[0][1]overlay=0:0,fps=30,scale=1600:-2:flags=lanczos[v0];`;
    if (n) graph += `[3]scale=${RING}:${RING},split=${n}${path.clicks.map((_, i) => `[r${i}]`).join("")};`;
    path.clicks.forEach((c, i) => {
      graph += `[v${i}][r${i}]overlay=x=${Math.round(c.x - RING / 2)}:y=${Math.round(c.y - RING / 2)}:enable='between(t,${c.t.toFixed(2)},${(c.t + 0.22).toFixed(2)})'[v${i + 1}];`;
    });
    graph += `[2]scale=${CURSOR.w}:${CURSOR.h}[cur];[v${n}]sendcmd=f='${cmds}'[vc];[vc][cur]overlay@cur=x=0:y=0,format=yuv420p[out]`;
    execFileSync("ffmpeg", ["-loglevel", "error", "-y", "-i", raw, "-i", lights, "-i", join(overlays, "cursor.png"), "-i", join(overlays, "click.png"),
      "-filter_complex", graph, "-map", "[out]", "-c:v", "libx264", "-preset", "slow", "-threads", "2", "-crf", "26", "-an", "-movflags", "+faststart", mp4]);
  }
  // The first frame, for the poster and for anyone with reduced motion.
  execFileSync("ffmpeg", ["-loglevel", "error", "-y", "-i", mp4, "-frames:v", "1", "-c:v", "libwebp", "-quality", "80", join(out, `${name}.webp`)]);
  const seconds = execFileSync("ffprobe", ["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", mp4], { encoding: "utf8" }).trim();
  console.log(`${name}: ${mp4.replace(site + "/", "")}, ${Number(seconds).toFixed(1)} s`);
}
