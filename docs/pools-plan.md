# Pools: Threadbear picks the login

Mike, 27 Oct 2026, after looking at the concepts in `docs/account-picker-design/` (thread 8af8d0d2): "pools is the right way to go. A pool can be a collection of providers. It could just be one provider, but it could also be multiple providers."

## Why

Mike has four Claude logins and one Codex login. Before every thread he looks at their usage and picks one, and when a login runs dry partway through a turn the thread fails and he moves it by hand. Theo's CLI proxy hides all of that behind one endpoint. Pools do the same thing inside Threadbear: Mike picks a pool and a model, and the runner decides which login runs each turn.

## What it's like when it's done

- **Composer.** The account chip is gone. The model chip reads "Claude · Opus 5.5 · High". If a thread is pinned to one login, it reads "gmail · Opus 5.5 · High".
- **Picker.** The rail down the left lists pools, not providers. Each one shows its title and a small "3 · 61%" (how many logins, and the best room among them). Picking a pool lists its models. A mixed pool lists them under a heading per provider. Under Effort, one quiet line describes the pool, for example "org 61% · gmail 16% · studio resting until 5 am". Below that, "Pin to a login ›" opens the list of logins with their usage, for the rare times Mike wants one.
- **Folders choose the pool.** A new thread in a folder named in a pool's `folders` starts on that pool, so anything under `~/dev/convex` starts on Convex. Everywhere else, it starts on the pool Mike last picked.
- **Moving mid-thread.** When a pool thread's login hits its limit, the runner moves the thread to the next login with room and carries on. The transcript gets one line: "Moved to mike@example.org: mike@example.net hit its 5-hour limit, back at 6:10 pm." If nothing in the pool has room, the thread waits: "Every login in Claude is out of room. Carrying on at 5:00 am when mike@studio.example is back." It carries on by itself at that time.
- **Settings › Pools** lists the pools. Mike can add, rename and delete pools, tick which logins are in each and drag them into order, and set the folders. The built-in ones are there to start with.

## Decisions

1. **A pool is a named, ordered list of logins, of any provider.** `{ id, title, accountIds, folders? }`. The order of `accountIds` only decides which provider comes first. Within one provider, the runner picks by room.
2. **Built-in pools** live in the code default, like `accountNotes`, so nobody has to edit Mike's `runner.json`. A `pools` key in runner.json replaces the whole list, and saving from Settings writes it. The default is worked out from the logins it finds:
   - one pool per provider (`claude`, `codex`) holding every login of that provider that has no note;
   - one pool per noted login, titled from its note's first word, or the login's title if there isn't one. Mike gets `convex` → [claude-2], with `folders: ["~/dev/convex"]` from a code default (`DEFAULT_POOL_FOLDERS = { 'claude-2': ['~/dev/convex'] }`).
   So Mike's list is Claude [claude-main, claude-1, claude-3], Convex [claude-2], Codex [codex-main].
3. **Which login a turn runs on.** Each thread has a home provider, the provider of the model it's on. A turn starts on the thread's current login if that login is still OK (not blocked, and at or above `MIN_ROOM_PERCENT`, or its usage isn't known). Otherwise the runner moves the thread first. This keeps a thread on one login as long as it can, because every move costs a cold prompt cache. A new thread starts on `pickAccount` among the pool's logins of its home provider.
4. **Moving.** The next login is the pool login on the home provider with the most room (`pickAccount` over the pool's logins, leaving out the one that ran dry). If none has room, it's the first other provider in pool order that has a login with room. That's a cross-provider move: the model becomes that provider's remembered model (`defaults.models[provider]` or the account's default), and the move is a handoff, as with any provider change today. A thread that spilled over stays where it is afterwards. It doesn't move back by itself.
5. **Claude to Claude keeps everything.** For a Claude-to-Claude login move, the runner copies the session transcript from the old login's `configDir/projects/<cwd>/` to the new one's and resumes it, the way same-account folder moves already do (`copySessionTranscript`, extended to take two config dirs). The handoff note is only the fallback, for when the copy or the resume fails. **This needs checking with a real e2e first:** does the CLI resume another login's transcript, thinking signatures and all? If it doesn't, every move uses the handoff note.
6. **Triggers.**
   - *Before a turn* (a send, a queued message, a schedule): the current login is blocked or under the minimum → move, then start.
   - *During a turn*: the turn ends `failed` with `errorKind: 'usageLimit'` → mark the login blocked in `UsageStore` if the reading doesn't already say so, move, and send a runner-origin message: "Your last login hit its usage limit partway through that turn, so this thread has moved to another one. Carry on from where you left off." The move goes in the transcript as a `system.note`. Only one automatic carry-on per user turn: if the new login fails the same way straight away, it doesn't loop. It waits (see below).
   - *Nothing has room*: the thread gets `waitingForRoom: { until, accountId }`, where `until` is the earliest `resetsAt` among the pool's logins plus one minute (or 30 minutes if no reset time is known). A `system.note` says so, and a timer sends the carry-on message at that time, after re-checking. A message from Mike before then cancels the wait, and his message goes through the same "before a turn" check. The wait survives a runner restart because it's in thread.json.
7. **Pinning.** `poolId` absent with an `accountId` set means pinned: the runner never moves the thread. Old threads stay pinned. (We could put them in the pool that holds their login, but changing what an existing thread does without asking isn't polite.) The app shows the login's short name on the chip for a pinned thread.
8. **Notes still guard.** A login with a note is never picked for a thread whose pool doesn't list it. With pools that already happens by construction, but `pickAccount` keeps the rule for threads with no pool.
9. **Naming a model picks the provider.** `threads.create` (so `create_thread`, `tb new` and schedules too) with a `model` but no `accountId` or `poolId`: if the model's provider isn't in the chosen pool, the runner uses the first pool that has a login of that provider. This fixes the bug where `create_thread` with `model: "claude-opus-5-5"` landed on codex-main and dropped the model.
10. **Last-used memory.** The app remembers the last pool picked (not the last login), and model and effort per provider as it does now. `runner.json` `defaults.poolId` is the runner's equivalent (`rememberAccount` becomes `rememberPool`), and `defaults.accountId` stays for apps that predate pools.

## Protocol (additions only; old apps keep working)

```ts
export type Pool = {
  id: string;            // "claude", "codex", "convex", or a slug Mike made
  title: string;         // "Claude"
  accountIds: string[];  // in order; may mix providers
  folders?: string[];    // ~ allowed; a new thread under one starts here
  builtIn?: boolean;     // from the code default, not runner.json
};

// hello result gains:
pools?: Pool[];
// RunnerDefaults gains:
poolId?: string;         // the pool a new thread with nothing named starts on (already folder-blind; the app applies folders itself via poolForFolder)

// ThreadSummary gains:
poolId?: string;                                  // absent = pinned to accountId
waitingForRoom?: { until: string; accountId?: string };  // accountId: the login it's waiting on

// threads.create params gain poolId?: string. accountId wins if both are given (pinned).
// threads.update params gain poolId?: string | null. null pins the thread to its current login;
//   a string moves it into that pool (and onto one of its logins at once if its current one isn't in it).
//   accountId given on update pins (poolId cleared) as before.

// New requests:
pools.list {}                       -> { pools: Pool[] }
pools.save { pools: Pool[] }        -> { pools: Pool[] }   // replaces the whole list in runner.json; -32602 on a duplicate id, an empty accountIds, or an unknown account id
// New notification:
pools.updated { pools: Pool[] }     // after pools.save, and when the login list changes the defaults
```

`threadbear` MCP tools: `create_thread` gains `pool` (an id or title). `update_thread` gains `pool` (`null`, or `"pin"`, pins the thread). `list_accounts` also prints the pools with their logins and their room, and says which pool a new thread would start on.

`tb`: `tb pools` lists them. `tb new --pool <id>`.

`docs/protocol.md` gets a "Pools" section with the above.

## Runner pieces

- `runner/src/pools.ts`: `defaultPools(accounts, notes, folders)`, `readPools(cfg, accounts)`, `validatePools`, `poolForFolder(pools, cwd)`, `pickInPool(pool, accounts, { provider, exclude, current })` (built on `usage.ts`'s `accountRoom` and `MIN_ROOM_PERCENT`), `nextLogin(pool, accounts, thread)` → `{ accountId, provider } | { waitUntil, accountId? }`. Pure functions with unit tests.
- `config.ts`: `pools?` from runner.json, `DEFAULT_POOL_FOLDERS`.
- `store.ts` `ThreadRecord`: `poolId?`, `waitingForRoom?`, `autoCarryOn?: string` (the turn id that already had its one automatic carry-on; dropped 12 Nov 2026, when a carry-on that runs dry started moving on again).
- `runner.ts`:
  - `threads.create` picks the pool (explicit, else folder, else the model's provider, else `defaults.poolId`) and the login;
  - a `beforeTurn` check where queued sends start;
  - the `usageLimit` handling where turns end;
  - the wait timer, re-armed on start from thread.json;
  - `threads.update` with `poolId`;
  - `pools.*` RPCs, `pools.updated`, and hello.
- `transcripts.ts`: `copySessionTranscript(fromConfigDir, toConfigDir, fromCwd, toCwd, id)`; `carryTranscript` allows a Claude-to-Claude login change.
- Tests: unit tests for pools.ts, and runner tests with fake adapters for the pre-turn move, the mid-turn move and its single carry-on, the wait and its timer (fake clock), a cross-provider spill, the model-picks-provider fix and a pinned thread never moving. A real e2e (claude-haiku-5-5, two Claude logins) checks the transcript carry: send a code word on login 1, move to login 2, and ask for it back.

## App pieces

- `RunnerAPI.swift` types, `RPCClient` (`pools.list`, `pools.save`, `.poolsUpdated`), `AppModel.pools`, `lastPoolId`, and `newThreadPoolId`, which applies the folder rule with the same `poolForFolder` logic and test cases in a shared fixture, like `pick-account-cases.json`.
- `ModelPickerView`: the rail lists pools (title plus "3 · 61%"). The models are those of the pool's providers, with provider headings in a mixed pool. Under Effort go the pool status line and "Pin to a login ›" (an inline list using the current `AccountRow`; a pick pins). A pinned thread's rail highlights no pool and shows a "Pinned to gmail · Unpin" line instead.
- `ComposerControls`: the account chip goes. The model chip's title is "<pool title or login short name> · <model> · <effort>". When every login in the pool is low or blocked, the chip shows the existing orange or red badge.
- Transcript: the move and wait notes are `system.note`s, which already draw. A waiting thread's sidebar row reads "Waiting for room · 5:00 am" in the secondary colour.
- Settings › Pools: list, add, rename, delete; login checkboxes with drag order; a folders field. Saving calls `pools.save`.
- `Automation` and `AppDump` get the new picker parts, so scripts can check them. The mock runner and screenshot scenes get pools, and `Screenshots.swift` gets scenes for the picker, a pinned thread and Settings › Pools.
- The model chip's tick bug: the account menu's tick and the chip disagreed in Mike's screenshot (tick on studio, chip on org). Both read `ThreadSettings.account`, so one of the two views was stale. The account menu goes away, but the pin list must read the live value.

## Order

1. Contracts: protocol.ts, RunnerAPI.swift, docs/protocol.md, and the shared pool fixtures. (On the `pools` branch, before the pieces.)
2. Runner and app in parallel, each in its own worktree off `pools`.
3. Merge runner, then app, into `pools`. Run `./check.sh --e2e`, including the transcript-carry e2e. Then fast-forward main. The runner updates itself from main once it's idle.
