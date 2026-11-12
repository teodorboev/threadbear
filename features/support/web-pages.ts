// Little web sites for the web panel scenarios (features/web-panels.feature),
// on this machine, made before the app starts as its environment names one:
//
// - the panel's own site: a home page with links to a page of its own and to
//   the other site, plain and in new windows;
// - another site (another port is another site, as WebPanelPool.sameSite
//   has it), where a link that leaves the panel goes;
// - a stand-in for Convex's embedded dashboard, which asks for its
//   credentials as the real one does (docs.convex.dev/platform-apis/embedded-dashboard)
//   and says in its title whose it got, never the key itself. The GPUI app
//   is pointed at it with THREADBEAR_CONVEX_EMBEDDED, which only a scripted
//   build reads.
import http from 'node:http';
import type { AddressInfo } from 'node:net';

export type Sites = { home: string; other: string; dashboard: string };

let servers: http.Server[] = [];

function serve(pages: (base: string) => Record<string, string>): Promise<string> {
  return new Promise((resolve, reject) => {
    let base = '';
    const server = http.createServer((req, res) => {
      const page = pages(base)[(req.url ?? '/').split('?')[0]];
      res.writeHead(page === undefined ? 404 : 200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' });
      res.end(page ?? '<title>Not here</title>');
    });
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
      servers.push(server);
      resolve(base);
    });
  });
}

export async function startSites(): Promise<Sites> {
  const other = await serve(() => ({ '/video': '<!doctype html><title>Video</title><p>A video on another site.</p>', '/docs': '<!doctype html><title>Docs</title>' }));
  const home = await serve((base) => ({
    '/': `<!doctype html><title>Panel home</title><body style="font:14px -apple-system,system-ui;padding:16px">
<p><a id="other" href="${other}/video">A video on another site</a></p>
<p><a id="next" href="${base}/second">Another of this site's pages</a></p>
<p><a id="next-window" target="_blank" href="${base}/second">The same, in a new window</a></p>
<p><a id="other-window" target="_blank" href="${other}/video">The video, in a new window</a></p>
<p><a id="file-window" target="_blank" href="file:///etc/hosts">A file on this Mac, in a new window</a></p>
<p><a id="data-window" target="_blank" href="data:text/html,%3Ctitle%3EMade%20of%20data%3C%2Ftitle%3E">A page made of data, in a new window</a></p>
<p><button id="many">Four ways to the other site, a second apart, on one click</button></p>
<p><button id="many-windows">The same, a new window first</button></p>
<p><button id="late-link">A new window, then a link six seconds late</button></p>
<script>
const link = (n, target) => { const a = document.createElement('a'); a.href = '${other}/video?' + n; a.target = target; document.body.append(a); a.click(); };
const wait = (ms) => { const until = performance.now() + ms; while (performance.now() < until) {} };
// One click, then four ways to the other site a little over a second apart,
// all inside the five seconds Chromium keeps a click's activation: a link
// and a link in a new window from the click's handler, still running, then
// a window.open and a change of location from timers it set. Only the first
// may reach the browser.
document.getElementById('many').onclick = () => {
  link(0, '');
  wait(1200);
  link(1, '_blank');
  // The change of location last: a page that stays in the panel takes
  // this one away.
  setTimeout(() => open('${other}/video?2'), 1200);
  setTimeout(() => { location.href = '${other}/video?3'; }, 2400);
};
// The same with a new window first: Chromium uses a click's activation up
// for a new window only when it makes one, and a panel makes none, so a
// window then another and a link could each still have it (Codex review).
document.getElementById('many-windows').onclick = () => {
  open('${other}/video?0');
  wait(1200);
  link(1, '_blank');
  wait(1200);
  link(2, '');
  document.title = 'Tried them all';
};
// A link held back past the click's five seconds: the page stops in the
// navigation's own event, opens a window there, and waits, and the link
// reaches the app later still carrying the click (Codex review, 12 Nov).
document.getElementById('late-link').onclick = () => {
  navigation.addEventListener('navigate', () => { open('${other}/video?0'); wait(6000); }, { once: true });
  link(1, '');
};
</script>`,
    '/second': '<!doctype html><title>Second page</title><p>The site\'s second page.</p>',
  }));
  const dashboard = await serve(() => ({
    '/': `<!doctype html><title>Waiting for credentials</title><body style="font:14px -apple-system,system-ui;padding:16px">
<p><a id="docs" href="${other}/docs">Convex's docs</a></p>
<script>
addEventListener('message', (e) => {
  if (e.data && e.data.type === 'dashboard-credentials') {
    document.title = 'Credentials for ' + e.data.deploymentName + (e.data.adminKey ? ' with a key' : ' with no key');
  }
});
postMessage({ type: 'dashboard-credentials-request' }, location.origin);
</script>`,
  }));
  return { home: `${home}/`, other, dashboard: `${dashboard}/` };
}

export function stopSites() {
  for (const server of servers.splice(0)) server.close();
}
