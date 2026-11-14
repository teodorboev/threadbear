# Self-hosting Threadbear's backend

Use Convex cloud if you can. It's setup's default and the one we recommend:
it keeps working while your Mac sleeps, sends phone notifications, and needs
nothing else installed. Self-hosting runs Convex's open source backend in
Docker on your Mac instead, and you lose:

- **Phone notifications.** The push relay only calls back to Convex's own
  hosts to check a backend is real (`relay/convex/verify.ts`), so setup turns
  pushes off (`PUSH_RELAY=off`). The phone's Settings, the Mac's Phone &
  browser and `tb status` say so.
- **The phone without Tailscale.** The backend is only on your tailnet.
- **Anything while the Mac sleeps** or Docker is stopped.
- **Share links for people off your tailnet.**

Or, with `--local`, it stays on this Mac's loopback with no Tailscale at
all (below, "On this Mac only"), and you lose the phone and other Macs
entirely too.

Or, with `--url` and `--site-url`, it's at an address of your own: it stays
on loopback, your own proxy (Caddy, nginx, cloudflared, your own Tailscale
or VPN) puts it where your phone can reach it, and Threadbear doesn't touch
Tailscale (below, "At your own address").

The user guide's "Self-hosting your backend" section is the short version
for people using it. This is how it's put together. Built 7 Nov 2026, after
someone asked on X; tested end to end on Tinker Desk (Docker Desktop,
Docker 27.4, Tailscale 1.104).

## Setting it up

You need Docker Desktop or OrbStack (set to start at login, as the backend
stops with it), and Tailscale logged in with MagicDNS and HTTPS
certificates on for the tailnet. Then:

```sh
./threadbear setup --self-hosted --yes
```

Partway through, macOS may ask whether Terminal can "access data from other
apps". Setup doesn't need it, and Don't Allow is fine: that's what the 7 Nov
recording did.

Running setup again later doesn't need the flag: the admin key file says
this Mac is self-hosted, and runner.json's `sync.url` says which way
(`scripts/self-hosted-plan.mjs`). A Mac already set up with Convex cloud is
refused, as setup doesn't move a Threadbear between backends.

## On this Mac only

```sh
./threadbear setup --self-hosted --local --yes
```

The same containers, volume and admin key, on 127.0.0.1 only, and no
Tailscale anywhere: setup doesn't look for it. Built 10 Nov 2026 after a
viewer on the launch video asked for it: their work keeps them on its
tailnet, and Tailscale has one tailnet on at a time, so the tailnet way
was no good to them. Tested end to end on Tinker Desk with Tailscale
stopped (Docker Desktop 4.37, Docker 27.4).

What it leaves out, and where each is said:

| | |
|---|---|
| The iPhone app | `devices.pair` refuses a phone (a link to 127.0.0.1 would send it to itself), Settings › Phone & browser says why in place of Show the code…, and Finish setting up leaves the phone out (`RunnerStatus.localOnly` in the app) |
| Phone notifications | `PUSH_RELAY=off`, as on the tailnet |
| A second Mac | `tb runner add` refuses, saying to move to the tailnet first |
| The website and share links | open on this Mac only, at `http://127.0.0.1:13211`; the share popover says so |
| A sleeping Mac | nothing works, as on the tailnet |

`tb status` adds an "On this Mac only" line under the mirror. All of them
go by `sync.url` being on loopback (`localOnly` in
`runner/src/convex-admin.ts`), so nothing else needs to know which way it
was set up.

**One origin for everything.** The tailnet way needs `--add-host` so the
container can reach its own `https://<mac>.ts.net` origin (below). Here the
origin is `http://127.0.0.1:13210`, for the browser, the runner and the
container alike: inside the container, 127.0.0.1 is the container itself.
That only works if the backend listens on 13210 inside too, and the image
fixes `--port 3210 --site-proxy-port 3211` in its `run_backend.sh` (a second
`--port` is refused). So the container runs a copy of that script with the
two ports changed, and publishes `127.0.0.1:13210:13210` and
`127.0.0.1:13211:13211`. The copy is checked: if a new image's script no
longer has those lines, the container stops with a message saying so, and
setup's wait for the backend fails, rather than it coming up on ports
nothing reaches. Pointing the origin at `host.docker.internal` instead
was ruled out: file URLs (`ctx.storage.getUrl`, a thread's pictures) carry
the origin, and this Mac's browser can't resolve that name.

Plain `http` is fine here: it never leaves the Mac, and browsers count
127.0.0.1 as a secure context. The runner already takes plain http to
loopback for `sync.url` and the admin URL, and the Mac app takes a
loopback `http` site (`RunnerStatus.websiteURL`).

**Moving between the two.** Run setup with `--self-hosted --tailnet` on a
local Mac (Tailscale installed and logged in), or `--local` on a tailnet
one. The containers' settings change, so setup makes them again on the
same volume; the admin key still works, as it comes from the instance
secret in the volume; runner.json gets the new `sync.url` and `sync.site`;
and the website is rebuilt, as its built-in backend address changed.
Moving to local takes this backend's own `tailscale serve` ports off, and
checks they're off before it saves the local address: if Tailscale isn't
running or can't be found, setup stops and says so (a stopped Tailscale
keeps what it serves and puts it back when it starts), and the next run
tries again. Every later local run checks those ports again.
Phones paired on the tailnet need pairing again after a move back. Plain
`--self-hosted` on a local Mac keeps it local, so an agent running the
usual command again can't put it on a tailnet (a work one, for the person
this is for) by accident.

## At your own address

```sh
./threadbear setup --self-hosted --yes \
  --url https://threadbear.example.com \
  --site-url https://threadbear-site.example.com
```

For when your phone already reaches your Mac some way of your own, and
Threadbear has no business caring which. Built 14 Nov 2026 after the same
launch-video viewer as "On this Mac only" pointed out that their phone
already reaches their homelab over their own Tailscale, so the app
shouldn't insist on running it.

The same containers, volume and admin key, published on 127.0.0.1 only, as
with `--local`. Your proxy points `--url` at `127.0.0.1:13210` (the API)
and `--site-url` at `127.0.0.1:13211` (your website and share links): two
names, or one name on two ports, as the backend serves them on two ports.
Both have to be https with no path: the phone's token and your threads go
there, and iOS only talks https. A proxy on another machine can't reach
loopback, so run one on the Mac (Caddy is two lines and gets its own
certificate) and point the other machine at that.

The two addresses are the backend's origins (`CONVEX_CLOUD_ORIGIN`,
`CONVEX_SITE_ORIGIN`), so file URLs and share links carry them, and the
runner, the phone and the website all use them; runner.json's `sync.url`
and `sync.site` are set to them. The dashboard stays on this Mac at
`http://127.0.0.1:16791`, talking to the backend on loopback.

Your proxy has to pass WebSocket upgrades on, as the runner, the phone and
the website all hold a live connection to `--url`. Caddy does that by
itself; this is all it needs, on the Mac:

```
threadbear.example.com {
	reverse_proxy 127.0.0.1:13210
}
threadbear-site.example.com {
	reverse_proxy 127.0.0.1:13211
}
```

nginx needs `proxy_http_version 1.1` and the `Upgrade` and `Connection`
headers passed on for `--url`.

**Setup checks it works before carrying on** (`checkOwnAddress` in
`scripts/setup.mjs`), and stops saying which part didn't, with the exact
command to run again:

- `--url`'s `/version` from this Mac, compared with the backend's own
  answer on loopback, so a proxy pointed at something else doesn't pass;
- the sync WebSocket through `--url` (`/api/<client version>/sync`), as a
  proxy that drops the upgrade answers everything else and leaves nothing
  live;
- `--site-url`, the same way as `/version`, on a path nothing serves;
- `--url` from inside the container (`docker exec … curl`), as the backend
  fetches the website's stored files through its own origin. A name only
  your phone's network can look up fails here: with it, every page of the
  website would be blank.

On a Mac with a backend already (a move, or new addresses) it checks
against that one first, starting it if it's stopped and waiting for it,
before the containers or any tailnet ports change, so a proxy that isn't
right yet leaves the old way working. That first check's inside-Docker part
runs in a throwaway container of the same image, as the new one will be
set up, since a tailnet container's `--add-host` can reach names the new
one can't. On a
first run it checks after saving the admin key, so a run that stops there
is one plain `setup` asks about rather than taking for Convex cloud.

What it can't tell is this backend from another Threadbear backend on the
same image: nothing they answer without a key differs (`/instance_name` is
`convex-self-hosted` on every one, and changing it would end their admin
keys). If your proxy points at a different backend of yours, the runner
would mirror there, with its key. That's left as it is: https already
proves the address is the one you gave, and a proxy that meant harm could
pass any check on to this backend and still read everything going through
it, so a check for it would only catch your own proxy pointed at another
backend of your own (cross-lab review, 14 Nov 2026).

What's different from the tailnet way:

| | |
|---|---|
| Tailscale | not looked for, and nothing served with it. This backend's own `tailscale serve` ports come off, as with `--local`, if an earlier setup put them on |
| The phone | pairs with any https address, asking first and naming it (iPhone 1.0.5 and later; the Android app too). `Pairing.swift` says why the host was never what kept a stranger's link out |
| A second Mac | joins as usual, through your address |
| Share links | open for whoever can reach `--site-url` |
| Your proxy | has to be up for anything off this Mac, and for the runner too, which mirrors through `--url` like everything else |

**Run again, or moved.** Plain `./threadbear setup` (or `--self-hosted`)
keeps it at the addresses in runner.json. Giving new ones moves it there.
`--local` or `--tailnet` move it off, and `--url … --site-url …` moves a
local or tailnet Mac onto your addresses, on the same volume each time. The
way it was set up goes by `sync.url` alone: loopback is local, this Mac's
`.ts.net` name on port 3210 is the tailnet (so `--url` refuses exactly
that, saying to use `--tailnet`), and anything else is your own address
(`reachOf` in `scripts/self-hosted-plan.mjs`). A paired phone needs pairing
again after any move.

## What setup makes

`scripts/setup.mjs`, "a self-hosted backend":

| | |
|---|---|
| `threadbear-convex` | the backend container, `ghcr.io/convex/convex-backend` at a pinned tag (`CONVEX_IMAGE_TAG`) |
| `threadbear-convex-dashboard` | the dashboard container, the same tag |
| `threadbear-convex-data` | the Docker volume with everything in it: the SQLite database, stored files, and the instance secret (`/convex/data/credentials`) |
| 127.0.0.1:13210, 13211, 16791 | the API, the site and the dashboard, on loopback only. Not 3210: that's where `npx convex dev` runs a local deployment |
| `tailscale serve` on 3210, 3211, 6791 | the same three on `https://<mac>.<tailnet>.ts.net`, for the tailnet only. Port 443 is left alone, as people often serve something else there. Not with `--local` or `--url` |
| `secrets/convex-self-hosted.env` | `CONVEX_SELF_HOSTED_URL` (the loopback API) and `CONVEX_SELF_HOSTED_ADMIN_KEY`, quoted so a shell can source it |
| runner.json `sync.url`, `sync.site` | the tailnet API and site addresses (`--local`: `http://127.0.0.1:13210` and `:13211`; `--url`: your two addresses) |
| `PUSH_RELAY=off`, `RUNNER_KEY` | the backend's only env vars |

Two things it has to get right, each found by trying it:

- **The origins must reach the backend from inside the container too.**
  The static-hosting component serves the website by fetching its own stored
  files through `CONVEX_CLOUD_ORIGIN`. With the origin left at the compose
  file's `http://127.0.0.1:<port>`, every page was a 500 ("connection
  refused"), as that port is the host's, not the container's. So the origins
  are the tailnet addresses, and the container gets
  `--add-host <mac>.<tailnet>.ts.net:<the Mac's tailnet IP>`; Docker Desktop
  then routes it out through the Mac to `tailscale serve`.
- **No `--prod` on a self-hosted backend.** The Convex CLI refuses it there
  ("cannot be used with a self-hosted deployment"), so the website upload
  leaves it off (`prodArgs` in `runner/src/convex-admin.ts`).

## Keys

`runner/src/convex-admin.ts` reads which admin credential this Mac has (a
cloud deploy key or the self-hosted admin key, never both) and builds the
env for every Convex CLI call: setup, `runner/scripts/deploy-backend.mjs`
and `tb runner rotate-owner`.

Every admin key is made from the backend's instance secret, so none can be
listed or revoked on its own. `tb runner check` counts the admin key file as
an admin credential, like a deploy key, and `rotate-owner` says it can't list
them. If one leaks, the way out is a new instance secret, which ends every
admin key at once.

A second Mac joins as it would with cloud: `setup --existing` there, then
`tb runner add <name> --ssh <host>` on this Mac, which also writes
`sync.site` into its runner.json.

## The phone and the website

- The iPhone app pairs with any https address as well as Convex cloud ones
  (`ios/ThreadbearKit Pairing.swift`), asking first as always and naming the
  server; it keeps a self-hosted one's token and pinned keys under its host
  and port. That needs iPhone 1.0.5 or later (1.0.4 took only Convex cloud).
  The Android app (`android-native/kit Pairing.kt`) does the same.
- It doesn't ask for notification permission on a backend that says
  `pushOff`, as there'd be nothing to send.
- The Mac app takes the website's address from `runner.status`'s
  `sync.site`, falling back to the `.convex.site` twin for cloud.
- `phone:me` and the owner's heartbeat answer `pushOff: true`
  (`convex/pushHealth.ts`).

## When it won't come up

Run setup again (`./threadbear setup`, no flag needed). It starts anything
that's stopped, and makes a container again if it won't start or its
settings have changed, always on the same volume, so your data stays. After
Docker Desktop was stopped uncleanly on Tinker Desk, the old backend
container failed with "failed to start shim ... file exists" until setup
made it again.

## Backing up, upgrading, taking it out

Everything is in the `threadbear-convex-data` volume. To back it up, export
with the Convex CLI using the admin key (`npx convex export`, with the
variables from `secrets/convex-self-hosted.env` set).

The image tag is pinned because an upgrade can need an export and an import
(Convex's `self-hosted/advanced/upgrading.md`). Move `CONVEX_IMAGE_TAG` on
deliberately; setup then makes the containers again on the same volume.

To take it out: `docker rm -f threadbear-convex threadbear-convex-dashboard`,
`docker volume rm threadbear-convex-data` (this deletes your threads'
mirror), `tailscale serve --https=3210 off` (and 3211, 6791; only for a
tailnet one), your own proxy's two addresses for a `--url` one, and delete
`secrets/convex-self-hosted.env` and `sync.url` and `sync.site` from
runner.json.

## The setup video

`scripts/record-self-hosted-install.sh` records an agent doing the whole
install from the website's prompt on a clean Mac's desktop (Mike's is Tinker
Desk, through `desk`), and `scripts/remove-self-hosted-install.sh --yes`
takes it all out again, so it can be recorded again whenever setup changes.

The recording has to start with no prompts in the way, so the script runs the
agent in an empty folder with GitHub logged out and its permissions named, in a
full-screen Terminal. Claude Code hides setup's output, so the video's list of
steps comes from running `./threadbear setup` again afterwards, in that Terminal.

The docs page plays `site/public/video/self-hosting.mp4`: that recording,
narrated by the Threadbear bear, with the tailnet name blurred.
