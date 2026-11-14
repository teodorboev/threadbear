// A stand-in BrowserBear for the Browser pane's scenarios (browser-pane.feature):
// a folder with agent/tabs.json, as BrowserBear's agent host writes it, and its
// private socket answering the commands the apps send (docs/protocol.md, "The
// Browser panel"). The app is pointed at it with BROWSERBEAR_HOME, so a
// scenario never reaches the real BrowserBear on the machine running it.
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';

export type Tab = {
  tabId: number;
  url: string;
  title: string;
  thread?: string;
  state?: 'working' | 'yours' | 'idle';
  action?: string;
  finish?: boolean;
  handoffs?: number;
  openedAt?: number;
};

export type Heard = { cmd: string; args: Record<string, unknown>; thread?: string };

// A 1×1 grey JPEG, for panelStill.
const STILL = '/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/wAALCAABAAEBAREA/8QAFAABAAAAAAAAAAAAAAAAAAAACf/EABQQAQAAAAAAAAAAAAAAAAAAAAD/2gAIAQEAAD8AKp//2Q==';

export class StandInBrowserBear {
  readonly home: string;
  readonly heard: Heard[] = [];
  private tabs: Tab[] = [];
  private server: net.Server;
  private sockets = new Set<net.Socket>();

  private constructor(home: string, server: net.Server) {
    this.home = home;
    this.server = server;
  }

  /** A folder of its own, short enough for a socket's path. */
  static async start(): Promise<StandInBrowserBear> {
    const home = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'tb-bear-'));
    fs.mkdirSync(path.join(home, 'agent'), { recursive: true });
    const server = net.createServer();
    const bear = new StandInBrowserBear(home, server);
    bear.write();
    server.on('connection', (socket) => bear.serve(socket));
    await new Promise<void>((resolve, reject) => server.listen(path.join(home, 'browserbear.sock'), resolve).once('error', reject));
    return bear;
  }

  /** A page an agent has open, written to tabs.json as the host does. */
  open(tab: Tab): void {
    this.tabs = this.tabs.filter((t) => t.tabId !== tab.tabId).concat(tab);
    this.write();
  }

  /** The agent hands a page to Mike: `browserbear handoff <tab> --wait`. */
  handOver(tabId: number): void {
    const tab = this.tabs.find((t) => t.tabId === tabId);
    if (!tab) throw new Error(`no tab ${tabId}`);
    tab.state = 'yours';
    tab.finish = true;
    tab.handoffs = (tab.handoffs ?? 0) + 1;
    this.write();
  }

  async stop(): Promise<void> {
    for (const s of this.sockets) s.destroy();
    await new Promise<void>((resolve) => this.server.close(() => resolve()));
    fs.rmSync(this.home, { recursive: true, force: true });
  }

  private write(): void {
    const file = path.join(this.home, 'agent/tabs.json');
    fs.writeFileSync(`${file}.tmp`, JSON.stringify({ port: 9333, socket: path.join(this.home, 'browserbear.sock'), browser: 'Helium', tabs: this.tabs }));
    fs.renameSync(`${file}.tmp`, file);
  }

  private serve(socket: net.Socket): void {
    this.sockets.add(socket);
    socket.on('close', () => this.sockets.delete(socket));
    socket.on('error', () => {});
    let buffer = '';
    socket.on('data', (chunk) => {
      buffer += chunk.toString('utf8');
      for (let at = buffer.indexOf('\n'); at >= 0; at = buffer.indexOf('\n')) {
        const line = buffer.slice(0, at);
        buffer = buffer.slice(at + 1);
        const message = JSON.parse(line) as { id: number; cmd: string; args?: Record<string, unknown>; thread?: string };
        socket.write(`${JSON.stringify({ id: message.id, ...this.answer(message.cmd, message.args ?? {}, message.thread) })}\n`);
      }
    });
  }

  private answer(cmd: string, args: Record<string, unknown>, thread?: string): Record<string, unknown> {
    // The pane's own polling isn't worth keeping.
    if (cmd !== 'panelState' && cmd !== 'panelStill') this.heard.push({ cmd, args, ...(thread ? { thread } : {}) });
    switch (cmd) {
      case 'panelState':
        return { ok: true, result: { profiles: [], tabs: [] } };
      case 'panelStill':
        return { ok: true, result: { data: STILL, at: Date.now() } };
      case 'panelShow':
        return { ok: true, result: {} };
      case 'markDone': {
        const tab = this.tabs.find((t) => t.tabId === args.tab);
        if (!tab) return { ok: false, error: 'no such tab' };
        tab.state = 'idle';
        tab.finish = false;
        this.write();
        return { ok: true, result: {} };
      }
      case 'handoff': {
        // A link Mike clicked: handed over only when an agent's tab is on it.
        const tab = this.tabs.find((t) => t.url === args.tab);
        return tab ? { ok: true, result: { tab: tab.tabId } } : { ok: false, error: 'no tab is on that page' };
      }
      default:
        return { ok: false, error: `unknown command ${cmd}` };
    }
  }
}
