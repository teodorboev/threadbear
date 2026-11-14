// `threadbear setup`: sets Threadbear up on this Mac from a clone of the repo
// (docs/setup-and-updates.md section 2.2). Every step looks at what's already
// there first, so running it again only does what's missing, and --dry-run
// says what it would do without doing any of it.
//
//   ./threadbear setup [--dry-run] [--yes] [--team SLUG] [--project NAME]
//                      [--region REGION] [--cwd DIR] [--name NAME]
//                      [--home DIR] [--skip-install] [--existing]
//                      [--self-hosted [--local | --tailnet |
//                                      --url URL --site-url URL]]
//
// Convex cloud is the default, and the one we recommend: it keeps working
// while your Mac sleeps, sends phone notifications, and needs nothing else
// installed. --self-hosted runs Convex's open source backend in Docker on
// this Mac instead, reached over Tailscale (docs/self-hosting.md). With it
// you lose phone notifications, the phone needs Tailscale on, nothing works
// while this Mac sleeps, and share links only open on your tailnet.
//
// --self-hosted --local is the same backend on this Mac's loopback only,
// with no Tailscale at all: for when you won't use the phone, or your work
// keeps you on its own tailnet. It's this Mac alone: no phone, no second
// Mac, and the website and share links only open here. Run setup again with
// --tailnet later to put it on your tailnet (or --local to come back); the
// data stays.
//
// --self-hosted --url https://… --site-url https://… is the same backend on
// loopback, for when you already reach your Mac your own way (Caddy, nginx,
// cloudflared, your own Tailscale or VPN): point your proxy's two https
// addresses at 127.0.0.1:13210 (--url) and 127.0.0.1:13211 (--site-url), and
// the phone and website use those. Setup checks both work, from this Mac and
// from inside Docker, and doesn't touch Tailscale.
//
//   1. this Mac: macOS, Xcode's command line tools, git, Node 24, Claude Code
//      or Codex, and Tailscale (optional)
//   2. your copy: a fork of Mike's repo, with his as `upstream`, and the
//      key his releases are signed with pinned in the data folder
//      (docs/setup-and-updates.md section 2.3)
//   3. the repo's packages
//   4. a Convex login, then a Convex project of your own with a production
//      deployment (a new one, or the one this clone already uses). With
//      --self-hosted: Convex's backend and dashboard in Docker, served on
//      this Mac's tailnet name with `tailscale serve` (--local: on
//      127.0.0.1 only; --url: on 127.0.0.1, for your own proxy)
//   5. its keys: a deploy key the runner uses to deploy the backend when it
//      updates (self-hosted: the backend's admin key), and the main key,
//      set as RUNNER_KEY
//   With --existing (a second Mac joining a Threadbear another Mac set up),
//   and on a Mac that already has a runner key, steps 4, 5 and 7 are
//   skipped: no Convex login, deploy key or backend deploy here, since each
//   can do anything to the deployment. Its key comes from the first Mac:
//   tb runner add <name> --ssh <this Mac> (docs/protocol.md, "Runner keys").
//   6. runner.json: where the runner mirrors to, and the default folder for
//      new threads (~/ThreadbearThreads, made here, unless --cwd; Settings changes it).
//      One this makes also starts new threads on Auto (defaults.permissionMode)
//   7. the backend and the website, deployed
//   8. the runner (its LaunchAgent, with Threadbear Runner.app as the
//      program, so macOS's privacy prompts name it) and the Mac app, built
//      and installed
//   9. where to pair your iPhone and sign a browser in (the app's Settings)
//
// Node built-ins only, so it runs before the repo's packages are installed.
import { spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { parseArgs } from 'node:util';
import { isJoining, joinSteps, runnerKeyPlan } from './runner-key-plan.mjs';
import { ADMIN_URL, backendContainer, DATA_VOLUME, mustSeeTailnetOff, offTailnetStep, origins, PORTS, selfHostedPlan, servedOn, tailnetPendingFiles } from './self-hosted-plan.mjs';
import { ConvexAdminError, convexEnv, readConvexAdmin, SELF_HOSTED_ENV_FILE } from '../runner/src/convex-admin.ts';

const repo = path.resolve(import.meta.dirname, '..');
// Where new threads start: a folder of their own, not the home folder, and
// not ~/Threadbear, which on macOS is the same folder as a ~/threadbear clone.
const THREADS_FOLDER = '~/ThreadbearThreads';

// Whether a folder is this repo or inside it, as macOS sees it: names
// compared without case, through links where the folder exists.
function insideRepo(folder) {
  const abs = path.resolve(folder.replace(/^~(?=$|\/)/, os.homedir()));
  const real = (p) => { try { return fs.realpathSync(p); } catch { return p; } };
  const a = real(abs).toLowerCase();
  const r = real(repo).toLowerCase();
  return a === r || a.startsWith(r + path.sep);
}
const DEFAULT_HOME = path.join(os.homedir(), 'Library', 'Application Support', 'Threadbear');

const { values: v } = parseArgs({
  options: {
    'dry-run': { type: 'boolean' },
    yes: { type: 'boolean', short: 'y' },
    team: { type: 'string' },
    project: { type: 'string', default: 'threadbear' },
    region: { type: 'string' },
    cwd: { type: 'string' },
    name: { type: 'string' },
    home: { type: 'string' },
    'skip-install': { type: 'boolean' },
    existing: { type: 'boolean' },
    'self-hosted': { type: 'boolean' },
    local: { type: 'boolean' },
    tailnet: { type: 'boolean' },
    url: { type: 'string' },
    'site-url': { type: 'string' },
    help: { type: 'boolean', short: 'h' },
  },
});
if (v.help) {
  const lines = fs.readFileSync(new URL(import.meta.url), 'utf8').split('\n');
  console.log(lines.slice(0, lines.findIndex((l) => l.startsWith('import'))).map((l) => l.replace(/^\/\/ ?/, '')).join('\n'));
  process.exit(0);
}

const dry = Boolean(v['dry-run']);
const home = path.resolve((v.home ?? process.env.THREADBEAR_HOME ?? DEFAULT_HOME).replace(/^~(?=$|\/)/, os.homedir()));
const secrets = path.join(home, 'secrets');
const runnerJsonPath = path.join(home, 'runner.json');
const deployKeyFile = path.join(secrets, 'convex-deploy.env');
const selfHostedFile = path.join(secrets, SELF_HOSTED_ENV_FILE);
const runnerKeyFile = path.join(secrets, 'convex-key');
const npx = path.join(path.dirname(process.execPath), 'npx');
const localKey = (() => {
  try {
    return fs.readFileSync(runnerKeyFile, 'utf8').trim();
  } catch {
    return '';
  }
})();
// A Mac joining another's Threadbear, by the flag or by the runner key it
// already has (setup run again there without the flag).
const joining = isJoining({ existing: v.existing, local: localKey });
// Self-hosted by the flag, or by an earlier self-hosted setup (its admin key
// is here), so running setup again without the flag carries on the same way,
// local, on the tailnet or at your own address (scripts/self-hosted-plan.mjs). A Mac joining
// another's Threadbear has no backend of its own either way. Worked out
// once runner.json has been read, inside the try below.
let plan;
const selfHostedFlags = { selfHosted: Boolean(v['self-hosted']), local: Boolean(v.local), tailnet: Boolean(v.tailnet), url: v.url, siteUrl: v['site-url'] };
const npm = path.join(path.dirname(process.execPath), 'npm');

class Fail extends Error {}
const say = (s = '') => console.log(s);
const ok = (s) => say(`  ✓ ${s}`);
const todo = (s) => say(`  → ${s}`);
const note = (s) => say(`  · ${s}`);

function run(cmd, args, opts = {}) {
  return spawnSync(cmd, args, { cwd: repo, encoding: 'utf8', ...opts });
}
function runOrFail(what, cmd, args, opts = {}) {
  const r = run(cmd, args, { stdio: 'inherit', ...opts });
  if (r.status !== 0) throw new Fail(`${what} failed (${cmd} ${args.join(' ')}).`);
  return r;
}
function has(cmd) {
  return run('/bin/zsh', ['-lc', `command -v ${cmd}`]).status === 0;
}
function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch {
    return {};
  }
}
// runner.json, or {} when there's none yet. One that's there but broken
// stops setup, which writes back what it reads: reading it as {} rewrote
// it with almost nothing in it, as the runner did before 9 Nov
// (runner/src/config.ts, RunnerJsonError).
function readRunnerJson() {
  let text;
  try {
    text = fs.readFileSync(runnerJsonPath, 'utf8');
  } catch (e) {
    if (e.code === 'ENOENT') {
      // A link to a file that has gone is a runner.json that's there.
      let link = false;
      try {
        link = fs.lstatSync(runnerJsonPath).isSymbolicLink();
      } catch {
        // no file and no link: a first run
      }
      if (link) throw new Fail(`${runnerJsonPath} is a link to a file that isn't there. Put the file back (or remove the link), then run setup again.`);
      return {};
    }
    throw new Fail(`Couldn't read ${runnerJsonPath} (${e.code ?? e}). Setup won't change it until it can.`);
  }
  let json;
  try {
    json = JSON.parse(text);
  } catch (e) {
    throw new Fail(`${runnerJsonPath} isn't valid JSON: ${text.trim() ? e.message : 'it is empty'}. Fix it by hand (or move it aside), then run setup again; it won't change it until then.`);
  }
  if (!json || typeof json !== 'object' || Array.isArray(json)) throw new Fail(`${runnerJsonPath} holds JSON, but not an object in { }. Fix it by hand (or move it aside), then run setup again.`);
  return json;
}
function writeJson(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(`${file}.tmp`, JSON.stringify(value, null, 2) + '\n');
  fs.renameSync(`${file}.tmp`, file);
}
function writeSecret(file, text) {
  fs.mkdirSync(path.dirname(file), { recursive: true, mode: 0o700 });
  fs.writeFileSync(file, text, { mode: 0o600 });
  fs.chmodSync(file, 0o600);
}
// The deploy key, read and never printed; its "prod:<name>|" prefix names the deployment.
function deployKey() {
  try {
    const key = fs.readFileSync(deployKeyFile, 'utf8').match(/^CONVEX_DEPLOY_KEY=["']?([^"'\n]+)/m)?.[1];
    return key ? { key, name: key.match(/^[a-z]+:([a-z0-9-]+)\|/)?.[1] } : undefined;
  } catch {
    return undefined;
  }
}
// This Mac's admin credential for the backend (runner/src/convex-admin.ts):
// the deploy key, or a self-hosted backend's admin key.
function admin() {
  try {
    return readConvexAdmin(home);
  } catch (e) {
    if (e instanceof ConvexAdminError) throw new Fail(e.message);
    throw e;
  }
}
// Convex commands against the production deployment (or the self-hosted
// backend), picked by this Mac's admin credential alone so a stray
// CONVEX_DEPLOYMENT or .env.local can't point them anywhere else.
function adminEnv() {
  return convexEnv(admin());
}


// ------------------------------------------------------------ the steps

function checkMac() {
  say('1. This Mac');
  if (process.platform !== 'darwin') throw new Fail('Threadbear runs on macOS.');
  // os.release() is the Darwin version (25.x on macOS 26), so ask sw_vers for the one people know.
  const macos = run('sw_vers', ['-productVersion']).stdout?.trim() || `Darwin ${os.release()}`;
  ok(`macOS ${macos} on ${os.arch()}`);
  if (run('xcode-select', ['-p']).status !== 0) throw new Fail("Xcode's command line tools aren't installed. Run: xcode-select --install");
  ok("Xcode's command line tools");
  if (!has('git')) throw new Fail('git is missing.');
  ok('git');
  const major = Number(process.versions.node.split('.')[0]);
  if (major < 24) throw new Fail(`Node ${process.versions.node} is too old; Threadbear needs 24 or newer.`);
  ok(`Node ${process.versions.node} (${process.execPath})`);
  const claude = fs.existsSync(path.join(os.homedir(), '.local', 'bin', 'claude')) || has('claude');
  const codex = has('codex');
  if (!claude && !codex) throw new Fail('Neither Claude Code nor Codex is installed. Install one and log in first: https://claude.com/claude-code or https://developers.openai.com/codex');
  ok([claude && 'Claude Code', codex && 'Codex'].filter(Boolean).join(' and '));
  // The runner is a login agent, which macOS only loads for someone logged in
  // to the desktop: over SSH to a Mac nobody's logged in to, step 8 can't
  // finish (seen on Tinker Desk, 28 Oct).
  if (desktopSession()) ok('logged in to the desktop');
  else note("Nobody is logged in to this Mac's desktop. Everything up to installing the runner works, then setup stops: log in (in person or with Screen Sharing) and run it again.");
  if (fs.existsSync('/Applications/Tailscale.app') || has('tailscale')) ok('Tailscale');
  else note("Tailscale isn't installed. Threadbear works without it, and so does a second Mac; it only adds a direct link between your machines (https://tailscale.com/download).");
}

function desktopSession() {
  return run('launchctl', ['print', `gui/${process.getuid()}`]).status === 0;
}

// Mike's repo, which everyone else's copy follows for his releases.
const MIKES_REPO = 'https://github.com/mikecann/threadbear.git';
const repoKey = (url) => url.trim().replace(/^git@([^:]+):/, '$1/').replace(/^[a-z+]+:\/\/(?:[^@/]+@)?/, '').replace(/\.git\/?$/, '').replace(/\/$/, '').toLowerCase();

function remoteUrl(name) {
  const r = run('git', ['remote', 'get-url', name]);
  return r.status === 0 ? r.stdout.trim() : undefined;
}

// A fork, not a clone (Mike, 29 Oct): their changes need somewhere of their
// own to live, and `upstream` is where Mike's releases come from. With no
// GitHub login it still works: origin becomes upstream, and their changes
// stay on this Mac until they fork.
function yourCopy() {
  say('2. Your copy');
  const origin = remoteUrl('origin');
  const upstream = remoteUrl('upstream');
  const mikes = repoKey(MIKES_REPO);
  if (upstream) ok(`Mike's releases come from upstream (${upstream})`);
  else if (origin && repoKey(origin) === mikes) {
    // Only a zero exit is a login: with an expired token gh prints the 401's JSON, which once
    // became the "username" and then failed the fork (Tinker Desk, 5 Nov 2026).
    const ghUser = has('gh') ? run('gh', ['api', 'user', '--jq', '.login']) : undefined;
    const login = ghUser?.status === 0 ? ghUser.stdout.trim() : '';
    if (login === 'mikecann') ok("this is Mike's own repo");
    else if (dry) todo(login ? `gh repo fork --remote: fork it to ${login}'s GitHub, origin becomes the fork and upstream Mike's` : "git remote rename origin upstream (no GitHub login, so no fork)");
    else if (login) {
      runOrFail('Forking the repo', 'gh', ['repo', 'fork', '--remote', '--remote-name', 'origin']);
      ok(`forked to ${login}'s GitHub as origin; Mike's repo is upstream`);
    } else {
      runOrFail('Renaming origin to upstream', 'git', ['remote', 'rename', 'origin', 'upstream']);
      ok("Mike's repo is upstream");
      note('Not logged in to GitHub, so this copy has no fork of its own. To keep your changes on GitHub later: gh auth login --web, then gh repo fork --remote --remote-name origin');
    }
  } else if (origin) {
    if (dry) todo(`git remote add upstream ${MIKES_REPO}`);
    else {
      runOrFail('Adding upstream', 'git', ['remote', 'add', 'upstream', MIKES_REPO]);
      ok(`upstream is Mike's repo; origin (${origin}) is yours`);
    }
  } else note("This copy has no git remote, so it won't hear about Mike's releases. Add his repo with: git remote add upstream " + MIKES_REPO);

  // The pinned copy is the one the runner trusts; a later change to the
  // repo's copy never adds a key to it (runner/src/upstream.ts). A new key
  // comes only through a signed release (runner/src/release-keys.ts). A
  // revoked one (scripts/release-keys-revoked) is never pinned, and comes
  // out of a copy pinned before: taking a key away only ever narrows trust.
  const pinned = path.join(home, 'upstream-signers');
  const revokedHome = path.join(home, 'upstream-revoked');
  const shipped = path.join(repo, 'scripts', 'release-signers');
  const shippedRevoked = path.join(repo, 'scripts', 'release-keys-revoked');
  if (!fs.existsSync(shipped)) return note("this copy has no scripts/release-signers, so Mike's releases can't be checked");
  const keyOf = (line) => {
    const w = line.trim().split(/\s+/);
    if (!w[0] || w[0].startsWith('#')) return undefined;
    for (let i = 0; i + 1 < w.length; i++) if (/^(ssh-|ecdsa-sha2-|sk-)/.test(w[i]) && /^[A-Za-z0-9+/]+={0,2}$/.test(w[i + 1])) return `${w[i]} ${w[i + 1]}`;
    return undefined;
  };
  const keysOf = (file) => (fs.existsSync(file) ? fs.readFileSync(file, 'utf8').split('\n').map(keyOf).filter(Boolean) : []);
  const revoked = new Set([...keysOf(revokedHome), ...keysOf(shippedRevoked)]);
  const without = (text) => text.split('\n').filter((l) => l.trim() && !revoked.has(keyOf(l))).join('\n') + '\n';
  // Kept whenever the repo lists one this install hadn't, pinned or not:
  // one still waiting its week (runner/src/release-keys.ts) must never
  // count either (Codex's review, 9 Nov 2026).
  const keepRevoked = () => {
    if (revoked.size > keysOf(revokedHome).length && !dry) fs.writeFileSync(revokedHome, [...revoked].join('\n') + '\n');
  };
  if (fs.existsSync(pinned)) {
    const was = fs.readFileSync(pinned, 'utf8');
    const now = without(was);
    keepRevoked();
    if (now !== was.split('\n').filter((l) => l.trim()).join('\n') + '\n') {
      if (dry) return todo(`take revoked release keys out of ${pinned}`);
      fs.writeFileSync(pinned, now);
      note(`took a revoked release key out of ${pinned} (scripts/release-keys-revoked)`);
    }
    if (now !== without(fs.readFileSync(shipped, 'utf8'))) note(`the release keys in scripts/release-signers differ from the ones pinned at ${pinned}; keeping the pinned ones (a new key comes through a signed release)`);
    return ok("Mike's release key is pinned");
  }
  if (dry) return todo(`pin Mike's release key at ${pinned}`);
  fs.mkdirSync(home, { recursive: true });
  fs.writeFileSync(pinned, without(fs.readFileSync(shipped, 'utf8')));
  keepRevoked();
  ok(`Mike's release key pinned at ${pinned}`);
}

function packages() {
  say('3. Packages');
  if (fs.existsSync(path.join(repo, 'node_modules', 'convex'))) return ok('installed');
  if (dry) return todo('npm ci');
  runOrFail('npm ci', npm, ['ci', '--no-audit', '--no-fund']);
  ok('installed');
}

function convexLogin() {
  say('4. Convex');
  const config = readJson(path.join(os.homedir(), '.convex', 'config.json'));
  if (config.accessToken) ok('logged in to Convex');
  else if (dry) todo('npx convex login (opens the browser)');
  else {
    // Opens the browser and waits for the approval there, with no questions
    // in the terminal, so an agent can run setup too (INSTALL.md).
    runOrFail('Logging in to Convex', npx, ['convex', 'login', '--device-name', os.hostname().split('.')[0], '--login-flow', 'poll']);
    ok('logged in to Convex');
  }
}

// The production deployment's URL. Read from runner.json when setup has
// already run; otherwise made here.
async function project() {
  const json = readRunnerJson();
  const key = deployKey();
  if (json.sync?.url && key) {
    ok(`using ${json.sync.url}`);
    return json.sync.url;
  }
  const selected = fs.existsSync(path.join(repo, '.env.local')) && /CONVEX_DEPLOYMENT=/.test(fs.readFileSync(path.join(repo, '.env.local'), 'utf8'));
  if (!selected) {
    const verb = v.existing ? 'use the existing' : 'create a';
    if (dry) {
      todo(`${verb} Convex project "${v.project}"${v.team ? ` on team ${v.team}` : ''} with a dev deployment`);
      const teams = v.team ? [] : convexTeams();
      if (teams.length > 1) note(`your Convex login has ${teams.length} teams, so the real run needs --team and one of: ${teams.map((t) => `${t.slug} (${t.name})`).join(', ')}`);
      return undefined;
    }
    const args = ['convex', 'dev', '--once', '--configure', v.existing ? 'existing' : 'new', '--project', v.project, '--dev-deployment', 'cloud'];
    const team = v.team ?? onlyTeam();
    if (team) args.push('--team', team);
    runOrFail(v.existing ? 'Selecting the Convex project' : 'Creating the Convex project', npx, args);
    ok(`project ${v.project}`);
  } else ok('this clone already has a Convex project (.env.local)');
  if (dry) return todo(`create its production deployment${(v.region ?? nearestRegion()) ? ` in ${v.region ?? nearestRegion()}` : ''}`), undefined;
  const args = ['convex', 'deployment', 'create', 'production', '--type', 'prod', '--default'];
  const region = v.region ?? nearestRegion();
  if (region) args.push('--region', region);
  const made = run(npx, args);
  const text = `${made.stdout}\n${made.stderr}`;
  let url = text.match(/https:\/\/[a-z0-9-]+(?:\.[a-z0-9-]+)?\.convex\.cloud/)?.[0];
  if (!url) {
    // It may exist already (a second run after a failure): find its URL from the key.
    note('the production deployment may already exist; looking for it');
  }
  return url;
}

async function keys(urlFromProject) {
  say('5. Keys');
  if (plan.selfHosted) ok(`the self-hosted backend's admin key, in ${selfHostedFile}`);
  else if (!deployKey()) {
    if (dry) todo(`a deploy key in ${deployKeyFile}`);
    else {
      fs.mkdirSync(secrets, { recursive: true, mode: 0o700 });
      const keyName = `threadbear-runner-${os.hostname().split('.')[0].toLowerCase()}`;
      runOrFail('Making a deploy key', npx, ['convex', 'deployment', 'token', 'create', keyName, '--prod', '--save-env', deployKeyFile]);
      fs.chmodSync(deployKeyFile, 0o600);
      // By name, so tb runner rotate-owner knows this one is this Mac's own.
      fs.appendFileSync(path.join(secrets, 'made-deploy-keys'), keyName + '\n', { mode: 0o600 });
      ok(`deploy key saved in ${deployKeyFile}`);
    }
  } else ok('deploy key');
  const key = plan.selfHosted ? (dry ? undefined : admin()) : deployKey();
  let url = urlFromProject ?? readRunnerJson().sync?.url;
  if (!url && key?.name) url = await findUrl(key.name);
  if (!dry && !url) throw new Fail("Couldn't work out the production deployment's URL. Put it in runner.json as sync.url and run setup again.");
  // The deployment's RUNNER_KEY is the main key, which can do everything:
  // one that's already set wins, never replaced, or the Macs using it would
  // be locked out. Only a deployment without one gets a new key. A Mac
  // joining with --existing gets a runner key of its own instead (since
  // 5 Nov 2026), so it can't write the first Mac's threads or pair phones.
  if (dry) {
    if (fs.existsSync(runnerKeyFile)) ok('runner key');
    else todo("the runner key: the deployment's own if it has one, else a new one set as RUNNER_KEY");
  }
  if (!dry && key) {
    const got = run(npx, ['convex', 'env', 'get', 'RUNNER_KEY'], { env: adminEnv() });
    const existing = (got.stdout ?? '').trim();
    const local = fs.existsSync(runnerKeyFile) ? fs.readFileSync(runnerKeyFile, 'utf8').trim() : '';
    const nextGet = run(npx, ['convex', 'env', 'get', 'RUNNER_KEY_NEXT'], { env: adminEnv() });
    const plan = runnerKeyPlan({ envGet: got, nextGet, local, joining });
    if (plan.action === 'fail') throw new Fail(plan.why);
    if (plan.action === 'joined') ok("runner key: this Mac's own, left as it is");
    else if (plan.action === 'keep') ok('runner key');
    else if (plan.action === 'copy') {
      writeSecret(runnerKeyFile, existing);
      ok(local ? "runner key: using the deployment's (it already had one)" : 'runner key');
    } else {
      const fresh = local || crypto.randomBytes(32).toString('hex');
      if (!local) writeSecret(runnerKeyFile, fresh);
      const r = spawnSync(npx, ['convex', 'env', 'set', 'RUNNER_KEY'], { cwd: repo, env: adminEnv(), input: fresh, encoding: 'utf8' });
      if (r.status !== 0) throw new Fail('Setting RUNNER_KEY on the deployment failed.');
      ok('runner key set as RUNNER_KEY');
    }
  }
  return url;
}

// The Convex region nearest this Mac, from its time zone, so the phone and
// the website aren't a round trip across the world away (an agent-run install
// on Tinker Desk landed in the US default, 28 Oct). Undefined leaves Convex's
// default (the US). --region overrides it.
function nearestRegion() {
  const zone = Intl.DateTimeFormat().resolvedOptions().timeZone ?? '';
  if (/^(Australia|Pacific\/(Auckland|Fiji|Guam|Port_Moresby)|Asia\/(Singapore|Jakarta|Makassar|Jayapura|Manila|Tokyo|Seoul|Hong_Kong|Shanghai|Taipei|Kuala_Lumpur|Bangkok|Ho_Chi_Minh))/.test(zone)) return 'aws-ap-southeast-2';
  if (/^(Europe|Africa|Atlantic\/(Reykjavik|Canary|Madeira|Azores))/.test(zone)) return 'aws-eu-west-1';
  return undefined;
}

// The team to use when none was given: the only one there is. With several,
// Convex asks in the terminal; with nobody there to answer (an agent), setup
// stops and lists them instead.
// The Convex login's teams, from `convex login status` (which prints the list on stderr). None
// when it isn't logged in yet.
function convexTeams() {
  const r = run(npx, ['convex', 'login', 'status']);
  const out = `${r.stdout ?? ''}\n${r.stderr ?? ''}`;
  return [...out.matchAll(/^\s+- (.+) \(([^)]+)\)$/gm)].map((m) => ({ name: m[1], slug: m[2] }));
}

function onlyTeam() {
  const teams = convexTeams();
  if (teams.length === 1) return teams[0].slug;
  if (process.stdin.isTTY && !v.yes) return undefined;
  throw new Fail(`Your Convex login has ${teams.length} teams. Run setup again with --team and one of: ${teams.map((t) => `${t.slug} (${t.name})`).join(', ')}.`);
}

// Probes the regions Convex runs in for a deployment's URL.
async function findUrl(name) {
  for (const region of ['', 'ap-southeast-2.', 'eu-west-1.', 'us-east-1.', 'us-west-2.']) {
    const url = `https://${name}.${region}convex.cloud`;
    try {
      const res = await fetch(`${url}/version`, { signal: AbortSignal.timeout(5000) });
      if (res.ok) return url;
    } catch {
      // not this one
    }
  }
  return undefined;
}

// ------------------------------------------------- a self-hosted backend
//
// Convex's open source backend and dashboard in Docker (docs/self-hosting.md),
// each published on this Mac's loopback only, so nothing listens on the LAN.
// On the tailnet way, `tailscale serve` then puts them on the tailnet on ports
// of their own, leaving 443 free for whatever already uses it; the local way
// stops at loopback, and the url way leaves loopback to your own proxy. How
// the container reaches itself through its own origin differs between them:
// scripts/self-hosted-plan.mjs, backendContainer.

// Pinned, as an upgrade can need an export and import (Convex's
// self-hosted/advanced/upgrading.md); move it on deliberately.
const CONVEX_IMAGE_TAG = 'd1fcbeaf939502a23a0f4b7e9095a12e84866294';
const BACKEND_IMAGE = `ghcr.io/convex/convex-backend:${CONVEX_IMAGE_TAG}`;
const DASHBOARD_IMAGE = `ghcr.io/convex/convex-dashboard:${CONVEX_IMAGE_TAG}`;
const BACKEND_CONTAINER = 'threadbear-convex';
const DASHBOARD_CONTAINER = 'threadbear-convex-dashboard';

function docker(args, opts = {}) {
  return run('docker', args, opts);
}

function tailscaleCli() {
  if (has('tailscale')) return 'tailscale';
  const app = '/Applications/Tailscale.app/Contents/MacOS/Tailscale';
  return fs.existsSync(app) ? app : undefined;
}

// This Mac on the tailnet: its MagicDNS name and IPv4 address.
function tailnetSelf() {
  const ts = tailscaleCli();
  if (!ts) throw new Fail('A self-hosted backend is reached over Tailscale, which isn\'t installed. Install it (https://tailscale.com/download), log in, and run setup again. Or run it with --self-hosted --local to keep the backend on this Mac alone (no phone), or with --self-hosted --url and --site-url if you reach this Mac your own way, or leave out --self-hosted and use Convex cloud, which we recommend.');
  const r = run(ts, ['status', '--json']);
  let st;
  try {
    st = JSON.parse(r.stdout);
  } catch {
    st = undefined;
  }
  const name = st?.Self?.DNSName?.replace(/\.$/, '');
  const ip = st?.Self?.TailscaleIPs?.find((a) => /^\d+\.\d+\.\d+\.\d+$/.test(a));
  if (r.status !== 0 || st?.BackendState !== 'Running' || !name || !ip) throw new Fail("Tailscale isn't logged in and running on this Mac. Open Tailscale, log in, and run setup again. Or run it with --self-hosted --local to keep the backend on this Mac alone (no phone), or with --self-hosted --url and --site-url if you reach this Mac your own way.");
  if (!st?.CurrentTailnet?.MagicDNSSuffix && !name.endsWith('.ts.net')) throw new Fail('Turn on MagicDNS for your tailnet (Tailscale admin console › DNS), then run setup again: the phone finds the backend by this Mac\'s tailnet name.');
  return { ts, name, ip };
}

function checkDocker() {
  if (!has('docker')) throw new Fail('A self-hosted backend runs in Docker, which isn\'t installed. Install Docker Desktop or OrbStack, start it, and run setup again. Or leave out --self-hosted and use Convex cloud, which we recommend.');
  if (docker(['info', '--format', '{{.ServerVersion}}']).status !== 0) throw new Fail("Docker is installed but not running. Start Docker Desktop (or OrbStack), and set it to start when you log in, as the backend stops with it. Then run setup again.");
}

// The label setup puts on each container: a hash of everything it was run
// with (image, env, ports, volume, --add-host, the command), so a container
// someone changed or made another way, or one from before the Mac was renamed,
// got a new tailnet address or moved between local and the tailnet, is never
// taken for this one.
const CONFIG_LABEL = 'app.threadbear.config';

function containerState(name) {
  // Guarded, as indexing a container's null Labels fails the whole template.
  const r = docker(['inspect', '--format', `{{.State.Running}}|{{if .Config.Labels}}{{index .Config.Labels "${CONFIG_LABEL}"}}{{end}}`, name]);
  if (r.status !== 0) return undefined;
  const [running, label] = r.stdout.trim().split('|');
  return { running: running === 'true', label };
}

// A container with exactly these settings, running. One left from an earlier
// run with other settings (the Mac renamed, a new image) is made again; the
// data is in the volume, so nothing's lost.
function ensureContainer(name, image, want, runArgs, cmd = []) {
  const env = Object.entries(want).flatMap(([k, val]) => ['-e', `${k}=${val}`]);
  // The command only joins the hash when there is one, so a tailnet
  // backend made before it (7 Nov) isn't made again for nothing.
  const config = crypto.createHash('sha256').update(JSON.stringify(cmd.length ? [image, runArgs, env, cmd] : [image, runArgs, env])).digest('hex').slice(0, 16);
  const have = containerState(name);
  const same = have?.label === config;
  if (same && have.running) return ok(`${name} is running`);
  if (dry) return todo(same ? `docker start ${name}` : `docker run ${name} (${image})`);
  // One that won't start is made again too: after Docker stopped uncleanly,
  // its old container can fail with "failed to start shim ... file exists"
  // (Tinker Desk, 7 Nov 2026), and a new one on the same volume is fine. So
  // running setup again is the fix for a backend that won't come up.
  if (same) {
    if (docker(['start', name]).status === 0) return ok(`${name} started`);
    note(`${name} wouldn't start; making it again${name === BACKEND_CONTAINER ? ` (your data is kept, in the ${DATA_VOLUME} volume)` : ''}`);
    docker(['rm', '-f', name]);
  } else if (have) {
    note(`${name} has older settings; making it again${name === BACKEND_CONTAINER ? ` (your data is kept, in the ${DATA_VOLUME} volume)` : ''}`);
    docker(['rm', '-f', name]);
  }
  const r = docker(['run', '-d', '--name', name, '--restart', 'unless-stopped', '--label', `${CONFIG_LABEL}=${config}`, ...runArgs, ...env, image, ...cmd], { stdio: ['ignore', 'ignore', 'inherit'] });
  if (r.status !== 0) throw new Fail(`Starting the ${name} container failed.`);
  ok(`${name} running`);
}

// What `tailscale serve` has on one of this Mac's tailnet ports, or
// undefined when Tailscale couldn't say (its daemon not answering), which
// isn't the same as nothing.
function served(ts, tailnetPort) {
  return servedOn(run(ts, ['serve', 'status', '--json']), tailnetPort);
}

// `tailscale serve` for one port, unless it's already serving that port:
// the same target is fine, anything else is someone else's and is left alone.
function serve(ts, tailnetPort, localPort) {
  const target = `http://127.0.0.1:${localPort}`;
  const handlers = served(ts, tailnetPort) ?? [];
  if (handlers.length) {
    if (handlers.every((h) => h.Proxy?.replace(/\/$/, '') === target)) return ok(`tailnet port ${tailnetPort} serves ${target}`);
    throw new Fail(`Tailscale already serves something else on port ${tailnetPort} of this Mac (tailscale serve status). Take it off (tailscale serve --https=${tailnetPort} off) and run setup again.`);
  }
  if (dry) return todo(`tailscale serve --bg --https=${tailnetPort} ${target}`);
  if (run(ts, ['serve', '--bg', `--https=${tailnetPort}`, target]).status !== 0) throw new Fail(`tailscale serve on port ${tailnetPort} failed. If it asks you to enable HTTPS for your tailnet, do that in the Tailscale admin console (DNS › HTTPS certificates) and run setup again.`);
  ok(`tailnet port ${tailnetPort} serves ${target}`);
}

// A local backend: take its own `tailscale serve` ports off, so it's not on
// any tailnet. Every local run checks, not just a move: a move to the tailnet
// that failed partway can have left some on (cross-lab review, 10 Nov).
// Anything else on those ports is someone else's and stays; a port with this
// backend and something else both on it stops setup, as taking only part of
// it off isn't something `tailscale serve` does by port. A move off the
// tailnet has to see each port off before it saves the local URL: a Tailscale
// it can't find or that can't answer stops it (Daybreak, 11 Nov 2026), and
// as the URL stays the tailnet one, the next run is a move and tries again.
// The decisions are offTailnetStep's (scripts/self-hosted-plan.mjs).
function unserve(ts, tailnetPort, localPort, moving) {
  const step = offTailnetStep({ cli: Boolean(ts), handlers: ts ? served(ts, tailnetPort) : undefined, tailnetPort, localPort, moving });
  if (step.do === 'fail') throw new Fail(step.message);
  if (step.do === 'note') return note(step.message);
  if (step.do === 'nothing') return;
  if (dry) return todo(`tailscale serve --https=${tailnetPort} off`);
  // Tailscale keeps what it serves while it's stopped, and won't take it off then.
  if (run(ts, ['serve', `--https=${tailnetPort}`, 'off']).status !== 0) throw new Fail(`Tailscale still has this backend on tailnet port ${tailnetPort}, saved for when it's next on, and taking it off failed. Start Tailscale (it can't change this while stopped), then run setup again, or run tailscale serve --https=${tailnetPort} off yourself.`);
  // Taken off is what it says now, not what the command's exit said.
  const after = offTailnetStep({ cli: true, handlers: served(ts, tailnetPort), tailnetPort, localPort, moving: true });
  if (after.do !== 'nothing') throw new Fail(`Tailscale still serves this backend on tailnet port ${tailnetPort} after setup took it off (tailscale serve status). Run tailscale serve --https=${tailnetPort} off yourself, then run setup again.`);
  ok(`tailnet port ${tailnetPort} taken off the tailnet`);
}

// Whether the backend answers, trying for up to `seconds`.
async function answers(base, seconds = 2) {
  const until = Date.now() + seconds * 1000;
  do {
    try {
      if ((await fetch(`${base}/version`, { signal: AbortSignal.timeout(2000) })).ok) return true;
    } catch {
      // not up (yet)
    }
    await new Promise((r) => setTimeout(r, 1000));
  } while (Date.now() < until);
  return false;
}

async function waitForBackend() {
  for (let i = 0; i < 60; i++) {
    try {
      const res = await fetch(`${ADMIN_URL}/version`, { signal: AbortSignal.timeout(2000) });
      if (res.ok) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 1000));
  }
  throw new Fail(`The backend didn't answer at ${ADMIN_URL} within a minute. docker logs ${BACKEND_CONTAINER} says why.`);
}

// Your own address (--url): your proxy has to reach this backend at both
// addresses, from this Mac and from inside the container, or setup stops
// and says which. Inside matters as much as outside: the backend fetches
// its own stored files (the website) through CONVEX_CLOUD_ORIGIN, so an
// address only your phone can look up gives a website of blank pages.
// Each answer is compared with the same request on loopback, so a proxy
// pointed at something else (a default page, the dashboard) doesn't pass.
// It can't tell this backend from another Threadbear backend on the same
// image, as nothing they answer without a key differs (/instance_name is
// "convex-self-hosted" on every one, and changing it would end their admin
// keys): pointing your proxy at another backend of your own is on you.
// The API's sync WebSocket is opened too, as a proxy that doesn't pass on
// the upgrade (nginx without its Upgrade headers) answers every plain
// request and still leaves the runner, the phone and the website with
// nothing live (cross-lab review, 14 Nov 2026).
async function checkOwnAddress(url, site, { quiet = false, fresh = false } = {}) {
  // Quiet: the second time in a run, saying only that it still holds.
  const pass = (line) => { if (!quiet) ok(line); };
  const homeArg = path.resolve(home) === DEFAULT_HOME ? '' : ` --home ${JSON.stringify(home)}`;
  // The command to run again, flags and all: plain `setup` or --self-hosted
  // alone would pick cloud or the tailnet on a Mac that has no admin key yet.
  const again = `./threadbear setup --self-hosted --url ${url} --site-url ${site}${homeArg}`;
  const get = async (u) => {
    try {
      const res = await fetch(u, { signal: AbortSignal.timeout(10_000), redirect: 'manual' });
      return { status: res.status, body: await res.text() };
    } catch (e) {
      return { error: e?.cause?.code ?? e?.cause?.message ?? e?.message ?? String(e) };
    }
  };
  const proxyTo = (port) => `http://127.0.0.1:${port}`;
  const want = await get(`${ADMIN_URL}/version`);
  const got = await get(`${url}/version`);
  if (got.error) throw new Fail(`This Mac can't reach ${url}/version (${got.error}). Point ${url} at ${proxyTo(PORTS.api[1])} in your proxy, check it works in a browser here, then run ${again}`);
  if (got.status !== want.status || got.body !== want.body) {
    throw new Fail(`${url} answers, but not as this backend does (${url}/version gave ${got.status}${got.body ? `: ${got.body.slice(0, 80)}` : ''}), so your proxy points it at something else. Point it at ${proxyTo(PORTS.api[1])}, then run ${again}`);
  }
  const wsVersion = convexClientVersion();
  const wsUrl = `${url.replace(/^https:/, 'wss:')}/api/${wsVersion ? `${wsVersion}/` : ''}sync`;
  const ws = await new Promise((resolve) => {
    let sock;
    const done = (r) => {
      clearTimeout(timer);
      try { sock?.close(); } catch { /* closing anyway */ }
      resolve(r);
    };
    const timer = setTimeout(() => done('it timed out'), 10_000);
    try {
      sock = new WebSocket(wsUrl);
      sock.onopen = () => done('open');
      sock.onerror = (e) => done(e?.message || 'the upgrade was refused');
    } catch (e) {
      done(e?.message ?? String(e));
    }
  });
  if (ws !== 'open') throw new Fail(`${url} answers plain requests but not the backend's live connection (a WebSocket to ${wsUrl}: ${ws}), which the runner, your phone and the website all need. Your proxy has to pass WebSocket upgrades on: Caddy does by itself, and nginx needs proxy_http_version 1.1 plus the Upgrade and Connection headers. Then run ${again}`);
  pass(`${url} reaches the backend from this Mac, live connection included`);
  // A path nothing serves, so it's the same answer whether the website is up
  // yet or not. Not a dotfile path: proxies often refuse those.
  const probe = `/threadbear-setup-check-${crypto.randomBytes(6).toString('hex')}`;
  const siteWant = await get(`${proxyTo(PORTS.site[1])}${probe}`);
  const siteGot = await get(`${site}${probe}`);
  if (siteGot.error) throw new Fail(`This Mac can't reach ${site} (${siteGot.error}). Point ${site} at ${proxyTo(PORTS.site[1])} in your proxy, then run ${again}`);
  if (siteGot.status !== siteWant.status || siteGot.body !== siteWant.body) {
    throw new Fail(`${site} answers, but not as this backend's website does (it gave ${siteGot.status} where ${proxyTo(PORTS.site[1])} gives ${siteWant.status}), so your proxy points it at something else. Point it at ${proxyTo(PORTS.site[1])}, then run ${again}`);
  }
  pass(`${site} reaches the website from this Mac`);
  // curl is in the backend's image (Ubuntu); its output is the version line.
  // `fresh`: from a throwaway container of the same image on Docker's
  // default network, as the url way's container will be, not the one
  // running now, whose settings (a tailnet one's --add-host) can reach names
  // the new one won't (cross-lab review, 14 Nov 2026).
  const curl = ['curl', '-sS', '--max-time', '10', `${url}/version`];
  const inside = fresh ? docker(['run', '--rm', '--entrypoint', curl[0], BACKEND_IMAGE, ...curl.slice(1)]) : docker(['exec', BACKEND_CONTAINER, ...curl]);
  if (inside.status !== 0 || inside.stdout !== want.body) {
    const why = (inside.stderr || '').trim().split('\n').at(-1) || `got ${inside.stdout.slice(0, 80)}`;
    throw new Fail(`The backend can't reach itself at ${url} from inside Docker (${why}), and it needs to, to serve your website. That usually means the name only resolves on your phone's network or VPN. Use a name this Mac can look up too (docker exec ${BACKEND_CONTAINER} curl ${url}/version should print the version), then run ${again}`);
  }
  pass(`${url} reaches the backend from inside Docker`);
  if (quiet) ok(`${url} and ${site} still reach it`);
}

// The Convex client's version, as the sync WebSocket's path carries it
// (convex/browser opens /api/<version>/sync). The packages step installed
// it; /api/sync, which the backend takes too, if not.
function convexClientVersion() {
  return readJson(path.join(repo, 'node_modules', 'convex', 'package.json')).version ?? '';
}

// The Convex CLI against the self-hosted backend, from an empty folder so a
// checkout's .env.local (which names a dev deployment) can't take part.
function selfHostedConvex(args, input) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'tb-convex-'));
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'tb-convex-env', private: true, dependencies: { convex: '*' } }));
  try {
    return spawnSync(process.execPath, [path.join(repo, 'node_modules', 'convex', 'bin', 'main.js'), ...args], { cwd: dir, env: adminEnv(), input, encoding: 'utf8' });
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

// What a move says it's doing, from where to where.
const WAY = { local: 'this Mac only', tailnet: 'your tailnet', url: 'your own address' };

async function selfHostedBackend() {
  const local = plan.mode === 'local';
  const own = plan.mode === 'url';
  say(`4. Self-hosted backend${local ? ', on this Mac only' : own ? ', at your own address' : ''} (Convex cloud is the one we recommend: docs/self-hosting.md)`);
  if (fs.existsSync(deployKeyFile)) throw new Fail(`This Mac is already set up with Convex cloud (${deployKeyFile}). Setup doesn't move a Threadbear between backends; leave out --self-hosted to carry on with cloud.`);
  if (plan.from) note(`moving it from ${WAY[plan.from]} to ${WAY[plan.mode]}; your threads stay`);
  if (plan.readdressed) note(`moving it to ${plan.url} and ${plan.site}; your threads stay, and a paired phone needs pairing again`);
  checkDocker();
  ok('Docker');
  let tailnet;
  if (local) ok('no Tailscale: the backend, its website and dashboard listen on 127.0.0.1 only');
  else if (own) ok(`your own address: the backend listens on 127.0.0.1 for your proxy (${plan.url} → 127.0.0.1:${PORTS.api[1]}, ${plan.site} → 127.0.0.1:${PORTS.site[1]})`);
  else {
    tailnet = tailnetSelf();
    ok(`Tailscale: this Mac is ${tailnet.name}`);
  }
  const { url, site, dashboard, dashboardBackend } = origins(plan.mode, own ? plan : tailnet?.name);
  if (!dry) {
    for (const image of [BACKEND_IMAGE, DASHBOARD_IMAGE]) {
      if (docker(['image', 'inspect', image]).status === 0) continue;
      say(`  · pulling ${image}`);
      if (docker(['pull', '-q', image], { stdio: ['ignore', 'ignore', 'inherit'] }).status !== 0) throw new Fail(`docker pull ${image} failed.`);
    }
  }
  // A Mac with a backend already (a move, or new addresses) checks your
  // addresses against it before anything changes, so a proxy that isn't
  // right yet stops setup with the old way still working: the containers and
  // any tailnet ports are only touched once it passes (cross-lab review,
  // 14 Nov 2026). One that's stopped is started, and one just starting is
  // waited for, so a slow start doesn't skip the check. Only a backend that
  // won't come up at all goes on unchecked: there's no working way to keep,
  // and making it again is setup's fix for that. Checked again below, on the
  // container as it ends up.
  let prechecked = false;
  const had = own && !dry ? containerState(BACKEND_CONTAINER) : undefined;
  if (had) {
    if (!had.running) docker(['start', BACKEND_CONTAINER]);
    if (await answers(ADMIN_URL, 60)) {
      await checkOwnAddress(url, site, { fresh: true });
      prechecked = true;
    } else note(`the backend there now doesn't answer, so there's nothing working to keep; checking your addresses once it's made again`);
  }
  const b = backendContainer(plan.mode, own ? plan : tailnet);
  ensureContainer(BACKEND_CONTAINER, BACKEND_IMAGE, b.env, b.runArgs, b.cmd);
  ensureContainer(DASHBOARD_CONTAINER, DASHBOARD_IMAGE, { NEXT_PUBLIC_DEPLOYMENT_URL: dashboardBackend }, ['-p', `127.0.0.1:${PORTS.dashboard[1]}:6791`]);
  // In the default data folder whatever --home says: the containers, their
  // volume and the ports served are this Mac's, one set for every data
  // folder, so a run for one has to see what a run for another served
  // (cross-lab review, 11 Nov 2026).
  // A note an older setup left in the --home folder counts too
  // (tailnetPendingFiles), so a local run there still checks every port.
  const [pendingFile, ...legacyPending] = tailnetPendingFiles(DEFAULT_HOME, home, path);
  if (tailnet) {
    // Said before anything is served, so a run that stops after serving
    // but before saving its URL still has a local run check every port.
    if (!dry && !fs.existsSync(pendingFile)) {
      fs.mkdirSync(DEFAULT_HOME, { recursive: true });
      fs.writeFileSync(pendingFile, 'This Mac\'s self-hosted Convex backend has been served on the tailnet (tailscale serve). Setup with --local removes this once it has seen every port taken off.\n');
    }
    for (const [tailnetPort, localPort] of Object.values(PORTS)) serve(tailnet.ts, tailnetPort, localPort);
  } else {
    const ts = tailscaleCli();
    const pendingFiles = [pendingFile, ...legacyPending];
    const moving = mustSeeTailnetOff({ from: plan.from, pending: pendingFiles.some((f) => fs.existsSync(f)) });
    for (const [tailnetPort, localPort] of Object.values(PORTS)) unserve(ts, tailnetPort, localPort, moving);
    // Each port was seen off, or setup stopped above.
    if (!dry && moving) for (const f of pendingFiles) fs.rmSync(f, { force: true });
  }
  if (dry) {
    if (!fs.existsSync(selfHostedFile)) todo(`the backend's admin key, in ${selfHostedFile}`);
    todo('PUSH_RELAY=off on the backend (the push relay only reaches Convex cloud)');
    if (own) todo(`check ${url} and ${site} reach the backend, from this Mac and from inside Docker`);
    return { url, site, dashboard, local, own };
  }
  await waitForBackend();
  if (!fs.existsSync(selfHostedFile)) {
    // generate_admin_key.sh prints "Admin key:" and the key; only the key is kept, never printed.
    const r = docker(['exec', BACKEND_CONTAINER, './generate_admin_key.sh']);
    const key = r.stdout.split('\n').map((l) => l.trim()).filter(Boolean).at(-1);
    if (r.status !== 0 || !key?.includes('|')) throw new Fail(`Couldn't make the backend's admin key (docker exec ${BACKEND_CONTAINER} ./generate_admin_key.sh).`);
    // Quoted, as the key has a "|" in it: a shell can source the file as it is.
    writeSecret(selfHostedFile, `CONVEX_SELF_HOSTED_URL='${ADMIN_URL}'\nCONVEX_SELF_HOSTED_ADMIN_KEY='${key}'\n`);
    ok(`admin key saved in ${selfHostedFile}`);
  } else ok('admin key');
  // After the admin key, which marks this Mac as self-hosted: a first run
  // that stops here is then one plain `setup` asks about ("can't tell which
  // way it was going") rather than taking for cloud (cross-lab review).
  if (own) await checkOwnAddress(url, site, { quiet: prechecked });
  // Notifications would only fail: the relay checks a backend by calling it
  // back on a Convex host (relay/convex/verify.ts). Off, the phone and the
  // Mac say there are none, rather than going quiet (convex/pushHealth.ts).
  const pushSet = selfHostedConvex(['env', 'set', 'PUSH_RELAY', 'off']);
  if (pushSet.status !== 0) throw new Fail(`Setting PUSH_RELAY=off on the backend failed: ${(pushSet.stderr || '').trim().slice(0, 300)}`);
  ok('phone notifications off (the push relay only reaches Convex cloud)');
  return { url, site, dashboard, local, own };
}

async function runnerJson(url, site) {
  say('6. runner.json');
  // Only a runner.json this makes: one already there is an install from
  // before, whose threads have started on Full access all along (unset
  // means full), and it keeps that.
  const fresh = !fs.existsSync(runnerJsonPath);
  const json = readRunnerJson();
  const name = v.name ?? json.sync?.name ?? os.hostname().split('.')[0];
  // Not asked: it's in the Mac app's Settings, and each new thread can pick
  // its own folder anyway. --cwd sets it here. The default is a folder of its
  // own rather than the home folder: an agent searching home reaches Dropbox,
  // Desktop and Documents, and macOS stops everything to ask about each
  // (Mike's first thread on Tinker Desk, 28 Oct).
  let cwd = v.cwd ?? json.defaults?.cwd ?? THREADS_FOLDER;
  // Not inside this repo: a thread there gets the repo's patches and a
  // worktree each. macOS ignores case, so the ~/Threadbear this used to
  // make was the ~/threadbear clone itself (Tinker Desk, 30 Oct). One
  // saved by an older setup moves; one given with --cwd is said.
  if (insideRepo(cwd)) {
    if (v.cwd) note(`${cwd} is inside this repo, so threads there get its patches and a worktree each.`);
    else cwd = THREADS_FOLDER;
  }
  const cwdPath = cwd.replace(/^~(?=$|\/)/, os.homedir());
  if (!dry && !fs.existsSync(cwdPath)) fs.mkdirSync(cwdPath, { recursive: true });
  // A new install's threads start on Auto: a safety check approves or
  // blocks each action (Claude only; on Codex it's Ask before edits).
  // Picking Full access in the composer, or "full" here, changes it.
  const mode = fresh ? { permissionMode: 'auto' } : {};
  // Full sync starts off (docs/full-convex.md step 6): setup leaves sync.full
  // unset, and the runner offers it once a phone, the website or another
  // machine would show what it sends (runner/src/convex-use.ts).
  const next = { ...json, sync: { ...json.sync, ...(url ? { url } : {}), ...(site ? { site } : {}), name }, defaults: { ...mode, ...json.defaults, cwd } };
  if (JSON.stringify(next) === JSON.stringify(json)) return ok(`${runnerJsonPath} as it was`);
  if (dry) return todo(`write ${runnerJsonPath}: sync.url ${url ?? '(the new deployment)'}${site ? `, sync.site ${site}` : ''}, sync.name ${name}, defaults.cwd ${cwd}${fresh ? ', defaults.permissionMode auto' : ''}`);
  writeJson(runnerJsonPath, next);
  ok(`mirrors to ${url} as ${name}; new threads start in ${cwd}${fresh ? ', on Auto' : ''}`);
  if (fresh) note("Full sync is off: turned on, your phone, the website and your other machines can show each thread's tool output and diffs, even when this Mac's asleep. Threadbear offers it once you pair a phone or add a machine, or turn it on with tb settings full on.");
}

function backend() {
  say('7. Backend and website');
  if (dry) return todo('node runner/scripts/deploy-backend.mjs (deploys convex/ and web/ if they changed)');
  // A self-hosted move changes the address built into the website. A deploy
  // from before that address was recorded can't see the change, so a move
  // always deploys again (cross-lab review, 10 Nov).
  // backend.json records the address the website was last built with; a
  // move that stopped partway has already written the new one to
  // runner.json, so this goes by what was built, not by plan.from.
  const built = readJson(path.join(home, 'backend.json')).site?.url;
  const force = plan?.from || plan?.readdressed || (plan?.selfHosted && built !== readRunnerJson().sync?.url) ? ['--force'] : [];
  runOrFail('Deploying the backend', process.execPath, [path.join(repo, 'runner', 'scripts', 'deploy-backend.mjs'), '--home', home, '--repo', repo, ...force]);
}

function install() {
  say('8. Runner and Mac app');
  if (v['skip-install']) {
    note('skipped (--skip-install)');
    // The runner reads sync.url when it starts, and a move changed it.
    if (plan?.from || plan?.readdressed) note('The backend moved, so restart the runner to pick up its new address: tb restart, or run setup again without --skip-install.');
    return;
  }
  if (dry) return todo('runner/scripts/install-runner.sh, then app/install.sh');
  if (!desktopSession()) throw new Fail("Nobody is logged in to this Mac's desktop, and macOS only starts the runner (a login agent) for someone who is. Log in, in person or with Screen Sharing, then run setup again.");
  const env = { ...process.env, THREADBEAR_NODE: process.execPath };
  const homeArgs = path.resolve(home) === DEFAULT_HOME ? [] : ['--home', home];
  runOrFail('Installing the runner', 'bash', [path.join(repo, 'runner', 'scripts', 'install-runner.sh'), ...homeArgs], { env });
  runOrFail('Installing the Mac app', 'bash', [path.join(repo, 'app', 'install.sh')], { env });
  // A Mac joining a Threadbear you already use: you know your way round,
  // so the app's first-run tour stays away (Help > Take the Tour still has it).
  if (joining) run('defaults', ['write', 'com.mikerosoft.threadbear', 'tourSeen', '-bool', 'YES']);
  ok('runner and app installed');
}

function joined() {
  say('9. Join your first Mac');
  const localHost = run('scutil', ['--get', 'LocalHostName']).stdout?.trim() ?? '';
  // Remote Login on: something answers on port 22. nc needs no admin, where
  // systemsetup -getremotelogin does.
  const sshOn = run('nc', ['-z', '-G', '2', 'localhost', '22']).status === 0;
  for (const line of joinSteps({ localHost, hostname: os.hostname(), user: os.userInfo().username, name: v.name, sshOn })) note(line);
}

function finish(url, sh) {
  say('9. Finish setting up');
  if (sh?.local) {
    note(`Your backend runs in Docker on this Mac, at ${url}, on this Mac only; its dashboard is ${sh.dashboard} (log in with the admin key in ${selfHostedFile}).`);
    note('On this Mac only means: no iPhone app (it has nothing it can reach), so no phone notifications either; no second Mac; the website and share links only open on this Mac; and nothing works while this Mac sleeps or Docker is stopped. To use the phone later, run setup again with --self-hosted --tailnet (with Tailscale), or with --self-hosted --url and --site-url if you reach this Mac your own way. Convex cloud has none of those limits (docs/self-hosting.md).');
  } else if (sh?.own) {
    note(`Your backend runs in Docker on this Mac, reached through your own proxy at ${url} (its website at ${sh.site}); its dashboard is ${sh.dashboard}, on this Mac only (log in with the admin key in ${selfHostedFile}).`);
    note('Self-hosted means: no phone notifications, nothing works while this Mac sleeps, Docker is stopped or your proxy is down, and share links open for whoever can reach your address. Convex cloud has none of those limits (docs/self-hosting.md).');
  } else if (sh) {
    note(`Your backend runs in Docker on this Mac, at ${url} on your tailnet; its dashboard is ${sh.dashboard} (log in with the admin key in ${selfHostedFile}).`);
    note('Self-hosted means: no phone notifications, the phone needs Tailscale on, nothing works while this Mac sleeps or Docker is stopped, and share links only open on your tailnet. Convex cloud has none of those limits (docs/self-hosting.md).');
  }
  // "From anywhere" isn't true of a self-hosted backend, which is only on the
  // tailnet, or only on this Mac.
  if (sh?.local) {
    note('Open Threadbear for a short tour that ends with your first thread. While it runs, Finish setting up has the rest: open your website, to check on your threads from a browser on this Mac.');
    note('The website is in Settings (⌘,) › Phone & browser too.');
  } else {
    note(`Open Threadbear for a short tour that ends with your first thread. While it runs, Finish setting up has the rest: pair your iPhone and open your website, to check on your threads ${sh?.own ? 'from anything that reaches your address' : sh ? 'from anything on your tailnet' : 'from anywhere'}.`);
    note('Pairing and the website are in Settings (⌘,) › Phone & browser too.');
  }
  // The terminal scripts still work, for a Mac with no app window to hand.
  const at = url ?? '<your deployment URL>';
  // These scripts only know Convex cloud addresses; a self-hosted backend pairs from Settings.
  if (!sh) note(`From a terminal instead: swift ios/scripts/pair-qr.swift --prod --url ${at}  and  npm run web:sign-in`);
  // docs/on-your-mac.md, "Who it talks to", has the longer version.
  note('Your threads\' text goes to your own Convex deployment with known secret shapes and values taken out first. That catches what it knows, not everything, and it can\'t read pictures. If a key does leak into a thread, rotate it. `tb status` says what this runner does.');
}

try {
  say(dry ? 'Threadbear setup (dry run: nothing changes)\n' : 'Threadbear setup\n');
  // Before anything else, so flags that can't go together stop it straight away.
  plan = selfHostedPlan({ flags: selfHostedFlags, joining, adminKeyFile: fs.existsSync(selfHostedFile), syncUrl: readRunnerJson().sync?.url, syncSite: readRunnerJson().sync?.site });
  if (plan.fail) throw new Fail(plan.fail);
  checkMac();
  say();
  yourCopy();
  say();
  packages();
  say();
  let url = readRunnerJson().sync?.url;
  let sh;
  if (joining) {
    say('4. Convex: skipped (this Mac joins a Threadbear another Mac set up)');
    note('No Convex login and no deploy key here: either can do anything to the deployment. The first Mac deploys the backend, and gives this one its own key.');
    say();
    say('5. Keys: skipped (its key comes from the first Mac)');
  } else if (plan.selfHosted) {
    sh = await selfHostedBackend();
    say();
    url = await keys(sh.url);
  } else {
    convexLogin();
    const made = await project();
    say();
    url = await keys(made);
  }
  say();
  await runnerJson(url, sh?.site);
  say();
  if (joining) say('7. Backend and website: skipped (the first Mac deploys them)');
  else backend();
  say();
  install();
  say();
  if (joining) joined();
  else finish(url, sh);
  say(dry ? '\nDry run done.' : '\nThreadbear is set up.');
} catch (e) {
  if (!(e instanceof Fail)) throw e;
  console.error(`\n✗ ${e.message}\nFix that and run setup again; it carries on from where it stopped.`);
  process.exit(1);
}
