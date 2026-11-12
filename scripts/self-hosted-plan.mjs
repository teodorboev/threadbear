// What setup does for a self-hosted backend (scripts/setup.mjs, "a
// self-hosted backend"; docs/self-hosting.md), worked out apart from doing
// it so a unit test can check it (runner/test/unit/self-hosted-plan.test.ts).
// Node built-ins only, like setup (and secure-url.ts, which has none).
import { isLoopbackHost } from '../runner/src/secure-url.ts';
//
// Two ways to reach it:
//   tailnet  `tailscale serve` puts it on this Mac's tailnet name, for the
//            phone and other Macs (setup --self-hosted)
//   local    loopback only, no Tailscale: this Mac alone, no phone
//            (setup --self-hosted --local). For someone whose work keeps
//            them on its own tailnet, as Tailscale has one on at a time.

// Tailnet port, then this Mac's loopback port. Not 3210 on loopback: that's
// where `npx convex dev` runs a local deployment.
export const PORTS = { api: [3210, 13210], site: [3211, 13211], dashboard: [6791, 16791] };
export const ADMIN_URL = `http://127.0.0.1:${PORTS.api[1]}`;
export const DATA_VOLUME = 'threadbear-convex-data';

// A URL on this Mac's loopback: a local backend's sync.url.
export function isLoopbackUrl(raw) {
  let u;
  try {
    u = new URL(raw);
  } catch {
    return false;
  }
  return isLoopbackHost(u.hostname);
}

// Whether setup self-hosts, and which way. The flags win; with none, an
// earlier self-hosted setup carries on as it was (its admin key file says
// self-hosted, runner.json's loopback sync.url says local).
//
// Plain --self-hosted on a Mac that's already local stays local: moving to
// the tailnet takes --tailnet, so an agent running setup again from the
// docs never puts the backend on a tailnet it wasn't meant for (the one
// this is for is a work tailnet).
//
//   flags    { selfHosted, local, tailnet } from the command line
//   joining  this Mac joins another's Threadbear (--existing, or a runner key)
//   adminKeyFile  secrets/convex-self-hosted.env is here
//   syncUrl  runner.json's sync.url, if any
//
// Returns { selfHosted: false } or { selfHosted: true, mode, from? } (from:
// the mode it had, when this moves it), or { fail } with why.
export function selfHostedPlan({ flags = {}, joining = false, adminKeyFile = false, syncUrl }) {
  const { local = false, tailnet = false } = flags;
  if (local && tailnet) return { fail: '--local and --tailnet pick opposite ways to reach the backend; use one.' };
  const asked = Boolean(flags.selfHosted || local || tailnet);
  if (joining) {
    if (asked) return { fail: 'A Mac joining another Mac\'s Threadbear (--existing) has no backend of its own, so --self-hosted, --local and --tailnet don\'t apply here.' };
    return { selfHosted: false };
  }
  if (!asked && !adminKeyFile) return { selfHosted: false };
  // Only a Mac that's self-hosted already has a way it was reached.
  const had = adminKeyFile && syncUrl ? (isLoopbackUrl(syncUrl) ? 'local' : 'tailnet') : undefined;
  // An admin key but no sync.url: a first setup stopped partway, and which
  // way it was going isn't written anywhere. Guessing the tailnet could put
  // it on a work one, so it asks (cross-lab review, 10 Nov).
  if (adminKeyFile && !syncUrl && !local && !tailnet) return { fail: 'An earlier self-hosted setup stopped before it finished, so setup can\'t tell which way it was going. Run it again with --self-hosted --local (this Mac only) or --self-hosted --tailnet.' };
  const mode = local ? 'local' : tailnet ? 'tailnet' : (had ?? 'tailnet');
  return { selfHosted: true, mode, ...(had && had !== mode ? { from: had } : {}) };
}

// The backend container's settings for each way. Every port is published on
// 127.0.0.1 only, never the LAN: the tailnet way puts them on the tailnet
// with `tailscale serve`, and the local way leaves them on this Mac.
//
// The backend fetches its own stored files (the website, from the
// static-hosting component) through CONVEX_CLOUD_ORIGIN, from inside the
// container, and hands the same origin to browsers in file URLs. So that
// origin has to work in both places:
//   tailnet  https://<mac>.<tailnet>.ts.net:3210, with --add-host so the
//            container reaches that name through the Mac's tailscale serve
//   local    http://127.0.0.1:13210, which inside the container is the
//            container itself. The image binds 3210 and 3211 inside, fixed
//            in its run_backend.sh (a second --port is refused), so the
//            container runs a copy of that script with 13210 and 13211, and
//            publishes them on the same numbers. It stops, saying why, if a
//            new image's script no longer has the lines it changes.
//            Pointing it at host.docker.internal instead was no good: file
//            URLs would name a host this Mac's browser can't find.
export function backendContainer(mode, tailnet = {}) {
  const volume = ['-v', `${DATA_VOLUME}:/convex/data`];
  if (mode === 'local') {
    const [, api] = PORTS.api;
    const [, site] = PORTS.site;
    const script = [
      'set -e',
      `sed -e 's/--port 3210 /--port ${api} /' -e 's/--site-proxy-port 3211 /--site-proxy-port ${site} /' ./run_backend.sh > /tmp/run_backend.sh`,
      `grep -q -- '--port ${api} ' /tmp/run_backend.sh && grep -q -- '--site-proxy-port ${site} ' /tmp/run_backend.sh || { echo "threadbear: this image's run_backend.sh no longer sets --port 3210 and --site-proxy-port 3211, so setup can't move them for a local backend" >&2; exit 1; }`,
      'exec bash /tmp/run_backend.sh',
    ].join('\n');
    return {
      env: { CONVEX_CLOUD_ORIGIN: `http://127.0.0.1:${api}`, CONVEX_SITE_ORIGIN: `http://127.0.0.1:${site}`, DISABLE_BEACON: 'true' },
      runArgs: ['-p', `127.0.0.1:${api}:${api}`, '-p', `127.0.0.1:${site}:${site}`, ...volume, '--entrypoint', '/bin/bash'],
      cmd: ['-c', script],
    };
  }
  const { name, ip } = tailnet;
  return {
    env: { CONVEX_CLOUD_ORIGIN: `https://${name}:${PORTS.api[0]}`, CONVEX_SITE_ORIGIN: `https://${name}:${PORTS.site[0]}`, DISABLE_BEACON: 'true' },
    runArgs: ['-p', `127.0.0.1:${PORTS.api[1]}:3210`, '-p', `127.0.0.1:${PORTS.site[1]}:3211`, ...volume, '--add-host', `${name}:${ip}`],
    cmd: [],
  };
}

// Where the runner, the website and the dashboard reach it.
export function origins(mode, name) {
  if (mode === 'local') {
    const at = (port) => `http://127.0.0.1:${port}`;
    return { url: at(PORTS.api[1]), site: at(PORTS.site[1]), dashboard: at(PORTS.dashboard[1]) };
  }
  const at = (port) => `https://${name}:${port}`;
  return { url: at(PORTS.api[0]), site: at(PORTS.site[0]), dashboard: at(PORTS.dashboard[0]) };
}

// What `tailscale serve status --json` (its spawnSync result) says is served
// on one tailnet port: the handlers, [] for nothing, or undefined when it
// couldn't say (it failed, didn't run, or printed something that isn't its
// JSON). Undefined is never "nothing": a stopped Tailscale keeps what it
// serves and puts it back when it starts (Daybreak, 11 Nov 2026).
export function servedOn(status, tailnetPort) {
  if (!status || status.error || status.status !== 0) return undefined;
  let cfg;
  try {
    cfg = JSON.parse(status.stdout?.trim() || '{}');
  } catch {
    return undefined;
  }
  if (!cfg || typeof cfg !== 'object' || Array.isArray(cfg)) return undefined;
  const web = cfg.Web ?? {};
  if (!web || typeof web !== 'object') return undefined;
  const out = [];
  for (const [hostPort, w] of Object.entries(web)) {
    if (!hostPort.endsWith(`:${tailnetPort}`)) continue;
    if (!w || typeof w !== 'object') return undefined;
    out.push(...Object.values(w.Handlers ?? {}));
  }
  return out;
}

// What a local backend's setup does about one of its tailnet ports:
//   { do: 'off' }      take it off (`tailscale serve --https=<port> off`)
//   { do: 'nothing' }  nothing of this backend's is served there
//   { do: 'note', message }  couldn't tell, said, and carries on
//   { do: 'fail', message }  stops setup, keeping the tailnet URL in
//                            runner.json, so the next run is a move again
//                            and tries again
// `moving` is a move off the tailnet (plan.from === 'tailnet'): there it has
// to know the port is off before it says the backend is on this Mac only, so
// no Tailscale found, or one that can't say, stops it. Otherwise only an
// earlier failed move could have left anything (and that one stopped), so it
// says so and goes on. `handlers` is servedOn's answer; `cli` whether a
// tailscale command was found at all.
export function offTailnetStep({ cli, handlers, tailnetPort, localPort, moving }) {
  const target = `http://127.0.0.1:${localPort}`;
  if (!cli) {
    // Tailscale keeps what it serves until it's told otherwise, so there's
    // no telling it's gone without it: setup has to see the ports off once.
    if (moving) return { do: 'fail', message: `Setup can't find Tailscale (no tailscale command, and no /Applications/Tailscale.app), so it can't check that this backend is off your tailnet, where an earlier setup served it on port ${tailnetPort}. If it's installed, put the tailscale command on your PATH; if you removed it, install it again (https://tailscale.com/download) and open it. Then run setup again: it takes the backend's ports off, and you can remove Tailscale after.` };
    return { do: 'nothing' };
  }
  if (!handlers) {
    if (moving) return { do: 'fail', message: `Couldn't ask Tailscale what it serves (tailscale serve status failed), so setup can't take this backend off your tailnet. Open Tailscale and run setup again.` };
    return { do: 'note', message: `couldn't ask Tailscale what it serves on port ${tailnetPort}; if an earlier setup put this backend on your tailnet, check tailscale serve status` };
  }
  const ours = handlers.filter((h) => typeof h?.Proxy === 'string' && h.Proxy.replace(/\/$/, '') === target);
  if (!ours.length) return { do: 'nothing' };
  if (ours.length !== handlers.length) return { do: 'fail', message: `Tailscale serves this backend on tailnet port ${tailnetPort} alongside something else (tailscale serve status), so it's still on your tailnet. Take the backend's part off by hand, then run setup again.` };
  return { do: 'off' };
}

// Whether a local run has to see every tailnet port off before it carries
// on: a move off the tailnet (the saved URL is a tailnet one), or any run
// after one that put the backend on the tailnet without the local one since
// seeing it off. That's the note setup leaves before it first serves
// (TAILNET_PENDING_FILE in the default data folder, as the backend is the Mac's), as a tailnet run that stopped
// before saving its URL left a local URL behind, which alone said nothing
// was served (cross-lab review of the Daybreak fix, 11 Nov 2026).
export const TAILNET_PENDING_FILE = 'self-hosted-on-tailnet';
export function mustSeeTailnetOff({ from, pending }) {
  return from === 'tailnet' || Boolean(pending);
}
