// What setup does for a self-hosted backend (scripts/setup.mjs, "a
// self-hosted backend"; docs/self-hosting.md), worked out apart from doing
// it so a unit test can check it (runner/test/unit/self-hosted-plan.test.ts).
// Node built-ins only, like setup (and secure-url.ts, which has none).
import { isLoopbackHost } from '../runner/src/secure-url.ts';
//
// Three ways to reach it:
//   tailnet  `tailscale serve` puts it on this Mac's tailnet name, for the
//            phone and other Macs (setup --self-hosted)
//   local    loopback only, no Tailscale: this Mac alone, no phone
//            (setup --self-hosted --local). For someone whose work keeps
//            them on its own tailnet, as Tailscale has one on at a time.
//   url      your own address (setup --self-hosted --url … --site-url …):
//            the backend stays on loopback, and whatever you already reach
//            your Mac with (Caddy, nginx, cloudflared, your own Tailscale or
//            VPN) points at it. Threadbear doesn't need to know how the
//            phone gets there. Asked for by a viewer on the launch video
//            (11 Nov 2026; built 14 Nov) whose phone already reaches their
//            homelab their own way.

// Tailnet port, then this Mac's loopback port. Not 3210 on loopback: that's
// where `npx convex dev` runs a local deployment.
export const PORTS = { api: [3210, 13210], site: [3211, 13211], dashboard: [6791, 16791] };
const TAILNET_PORTS = Object.values(PORTS).map(([tailnet]) => tailnet);
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

// The tailnet way's address: this Mac's MagicDNS name on port 3210. Told
// apart from an address of your own by that, so the way it was set up
// needs nothing saved beyond runner.json's sync.url (and --url refuses one
// that looks like it, below).
function isTailnetUrl(raw) {
  let u;
  try {
    u = new URL(raw);
  } catch {
    return false;
  }
  return u.protocol === 'https:' && u.hostname.endsWith('.ts.net') && u.port === String(PORTS.api[0]);
}

// The way an earlier self-hosted setup went, from the sync.url it saved.
export function reachOf(syncUrl) {
  if (isLoopbackUrl(syncUrl)) return 'local';
  return isTailnetUrl(syncUrl) ? 'tailnet' : 'url';
}

// An address of your own for --url or --site-url, as an origin, or { fail }.
// https only: the phone's token and your threads go to it, and iOS refuses
// plain http anyway. Nothing on this Mac's loopback (inside the container
// that's the container itself, and a phone would talk to itself), no login,
// path, query or fragment, and not a Convex cloud host.
export function ownAddress(raw, flag) {
  let u;
  try {
    u = new URL(String(raw ?? '').trim());
  } catch {
    return { fail: `${flag} isn't a URL. Give the https address your phone reaches this Mac's backend at, like ${flag} https://threadbear.example.com` };
  }
  if (u.protocol !== 'https:') return { fail: `${flag} has to be https (${u.protocol}//${u.host}): your phone's token and your threads go to it, and the iPhone app only talks https. Put a proxy with a certificate in front (Caddy makes one for you).` };
  if (u.username || u.password) return { fail: `${flag} has a name or password in it; take them out.` };
  // A trailing dot is the same name to DNS but not to the checks below
  // (mac.tail1.ts.net. got past the tailnet-port check, cross-lab review).
  if (u.hostname.endsWith('.')) return { fail: `${flag} ends its name with a dot (${u.host}); leave the dot off.` };
  if ((u.pathname !== '/' && u.pathname !== '') || u.search || u.hash) return { fail: `${flag} has to be just an address (https://host or https://host:port), with no path: Convex's backend expects to be at the root.` };
  if (isLoopbackHost(u.hostname)) return { fail: `${flag} is on this Mac's loopback (${u.host}), which a phone can't reach. For this Mac alone, use --local instead.` };
  if (u.hostname.endsWith('.convex.cloud') || u.hostname.endsWith('.convex.site')) return { fail: `${flag} is a Convex cloud address. For Convex cloud, leave out --self-hosted and --url.` };
  return { origin: u.origin };
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
// An address of your own (--url) stays one too: plain --self-hosted on such
// a Mac keeps it, with the addresses runner.json has.
//
//   flags    { selfHosted, local, tailnet, url, siteUrl } from the command line
//   joining  this Mac joins another's Threadbear (--existing, or a runner key)
//   adminKeyFile  secrets/convex-self-hosted.env is here
//   syncUrl, syncSite  runner.json's sync.url and sync.site, if any
//
// Returns { selfHosted: false } or { selfHosted: true, mode, from?, url?,
// site?, readdressed? } (from: the mode it had, when this moves it; url and
// site: the addresses, for mode "url"; readdressed: still "url", at new
// addresses), or { fail } with why.
export function selfHostedPlan({ flags = {}, joining = false, adminKeyFile = false, syncUrl, syncSite }) {
  const { local = false, tailnet = false } = flags;
  const own = flags.url !== undefined || flags.siteUrl !== undefined;
  if ([local, tailnet, own].filter(Boolean).length > 1) return { fail: '--local, --tailnet and --url each pick a different way to reach the backend; use one.' };
  const asked = Boolean(flags.selfHosted || local || tailnet || own);
  if (joining) {
    if (asked) return { fail: 'A Mac joining another Mac\'s Threadbear (--existing) has no backend of its own, so --self-hosted, --local, --tailnet and --url don\'t apply here.' };
    return { selfHosted: false };
  }
  if (!asked && !adminKeyFile) return { selfHosted: false };
  let addr;
  if (own) {
    // Two addresses, as the backend serves its API and its website (and
    // share links) on two ports, so a proxy needs two names or two ports.
    if (flags.url === undefined || flags.siteUrl === undefined) return { fail: '--url and --site-url go together: the backend serves its API and your website on two ports (127.0.0.1:13210 and 127.0.0.1:13211), so your proxy needs an address for each, like --url https://threadbear.example.com --site-url https://threadbear-site.example.com.' };
    const url = ownAddress(flags.url, '--url');
    if (url.fail) return { fail: url.fail };
    const site = ownAddress(flags.siteUrl, '--site-url');
    if (site.fail) return { fail: site.fail };
    if (url.origin === site.origin) return { fail: '--url and --site-url are the same address, but they need to reach two different ports (127.0.0.1:13210 for --url, 127.0.0.1:13211 for --site-url).' };
    // The tailnet way's ports on a tailnet name: --url's would be read back
    // as the tailnet way next time, and setup takes this backend's own
    // `tailscale serve` off all three on every run that isn't the tailnet
    // way, which would take your proxy with it.
    for (const [flag, origin] of [['--url', url.origin], ['--site-url', site.origin]]) {
      const u = new URL(origin);
      if (u.hostname.endsWith('.ts.net') && TAILNET_PORTS.includes(Number(u.port))) return { fail: `${flag} ${origin} is on a port --tailnet uses (${TAILNET_PORTS.join(', ')}). Use --tailnet for that, or other ports for your own.` };
    }
    addr = { url: url.origin, site: site.origin };
  }
  // Only a Mac that's self-hosted already has a way it was reached.
  const had = adminKeyFile && syncUrl ? reachOf(syncUrl) : undefined;
  // An admin key but no sync.url: a first setup stopped partway, and which
  // way it was going isn't written anywhere. Guessing the tailnet could put
  // it on a work one, so it asks (cross-lab review, 10 Nov).
  if (adminKeyFile && !syncUrl && !local && !tailnet && !own) return { fail: 'An earlier self-hosted setup stopped before it finished, so setup can\'t tell which way it was going. Run it again with --self-hosted --local (this Mac only), --self-hosted --tailnet, or --self-hosted --url <address> --site-url <address>.' };
  const mode = local ? 'local' : tailnet ? 'tailnet' : own ? 'url' : (had ?? 'tailnet');
  if (mode === 'url' && !addr) {
    // Kept from before: the addresses are the ones it saved.
    if (!syncSite) return { fail: 'This Mac\'s backend is at an address of your own, but runner.json has no sync.site saying where its website is. Run setup again with --url and --site-url.' };
    addr = { url: syncUrl, site: syncSite };
  }
  const moved = had && had !== mode ? { from: had } : {};
  const readdressed = had === 'url' && mode === 'url' && (addr.url !== syncUrl || addr.site !== syncSite) ? { readdressed: true } : {};
  return { selfHosted: true, mode, ...moved, ...(addr ? addr : {}), ...readdressed };
}

// The backend container's settings for each way. Every port is published on
// 127.0.0.1 only, never the LAN: the tailnet way puts them on the tailnet
// with `tailscale serve`, the local way leaves them on this Mac, and the url
// way leaves them for your own proxy to reach.
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
//   url      your addresses, reached from inside the container the way
//            anything on this Mac reaches them (Docker Desktop sends the
//            container's traffic out through the Mac). Setup checks that
//            works before it carries on (checkOwnAddress in setup.mjs).
//
// `at` is { name, ip } for the tailnet, { url, site } for the url way.
export function backendContainer(mode, at = {}) {
  const volume = ['-v', `${DATA_VOLUME}:/convex/data`];
  if (mode === 'url') {
    return {
      env: { CONVEX_CLOUD_ORIGIN: at.url, CONVEX_SITE_ORIGIN: at.site, DISABLE_BEACON: 'true' },
      runArgs: ['-p', `127.0.0.1:${PORTS.api[1]}:3210`, '-p', `127.0.0.1:${PORTS.site[1]}:3211`, ...volume],
      cmd: [],
    };
  }
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
  const { name, ip } = at;
  return {
    env: { CONVEX_CLOUD_ORIGIN: `https://${name}:${PORTS.api[0]}`, CONVEX_SITE_ORIGIN: `https://${name}:${PORTS.site[0]}`, DISABLE_BEACON: 'true' },
    runArgs: ['-p', `127.0.0.1:${PORTS.api[1]}:3210`, '-p', `127.0.0.1:${PORTS.site[1]}:3211`, ...volume, '--add-host', `${name}:${ip}`],
    cmd: [],
  };
}

// Where the runner, the website and the dashboard reach it, and the backend
// address the dashboard's page talks to (dashboardBackend). `at` is the
// tailnet name for the tailnet, { url, site } for the url way.
//
// The url way's dashboard stays on this Mac (it's only for you, and your
// proxy has no address for it), talking to the backend on loopback, so it
// works even when your proxy doesn't.
export function origins(mode, at) {
  if (mode === 'local') {
    const on = (port) => `http://127.0.0.1:${port}`;
    return { url: on(PORTS.api[1]), site: on(PORTS.site[1]), dashboard: on(PORTS.dashboard[1]), dashboardBackend: on(PORTS.api[1]) };
  }
  if (mode === 'url') return { url: at.url, site: at.site, dashboard: `http://127.0.0.1:${PORTS.dashboard[1]}`, dashboardBackend: ADMIN_URL };
  const on = (port) => `https://${at}:${port}`;
  return { url: on(PORTS.api[0]), site: on(PORTS.site[0]), dashboard: on(PORTS.dashboard[0]), dashboardBackend: on(PORTS.api[0]) };
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

// Where to look for that note. Setup before 11 Nov (4d8cdb23) left it in the
// --home data folder, so a run on that same folder still finds a note an
// interrupted --tailnet run left there, rather than reading no note as never
// served (Daybreak, 12 Nov 2026). Both are cleared once every port is off.
export function tailnetPendingFiles(defaultHome, home, path) {
  const files = [path.join(defaultHome, TAILNET_PENDING_FILE)];
  const legacy = path.join(home, TAILNET_PENDING_FILE);
  if (path.resolve(legacy) !== path.resolve(files[0])) files.push(legacy);
  return files;
}
