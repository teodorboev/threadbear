// One `opencode acp` process: newline-delimited JSON-RPC 2.0 over stdio, the
// Agent Client Protocol (agentclientprotocol.com). Requests out, responses
// and notifications in, and the agent's own requests to us (permission
// cards) answered through onRequest.
//
// `opencode acp` is a client of OpenCode's background service, which runs
// the session itself: killing this process doesn't stop a running turn, so
// the session sends session/cancel first (adapter.ts).
import type { ChildProcess } from 'node:child_process';
import type { Logger } from '../../log.ts';
import type { ProcessRegistry } from '../../proc.ts';
import { groupSpawner } from '../../proc.ts';

export type AcpOptions = {
  binary: string;
  cwd: string;
  env?: Record<string, string>;
  registry: ProcessRegistry;
  log: Logger;
  label: string;
  requestMs?: number;
};

export class AcpError extends Error {
  code: number;
  data: unknown;
  constructor(code: number, message: string, data?: unknown) {
    super(message);
    this.name = 'AcpError';
    this.code = code;
    this.data = data;
  }
}

export class AcpExitedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'AcpExitedError';
  }
}

type Pending = { resolve: (v: unknown) => void; reject: (e: Error) => void; timer?: NodeJS.Timeout };
export type RequestHandler = (method: string, params: Record<string, unknown>) => Promise<unknown>;

// Requests that can take as long as a turn: no timeout.
const UNTIMED = new Set(['session/prompt']);

export class AcpProcess {
  private opts: AcpOptions;
  private child?: ChildProcess;
  private abort?: AbortController;
  private nextId = 0;
  private pending = new Map<number, Pending>();
  private buf = '';
  private stderrTail: string[] = [];
  private exitWaiters: (() => void)[] = [];
  exited = false;
  closing = false;
  version?: string;
  onNotification: (method: string, params: Record<string, unknown>) => void = () => {};
  onRequest: RequestHandler = async (method) => {
    throw new AcpError(-32601, `Threadbear doesn't handle ${method}`);
  };
  onExit: (error?: Error) => void = () => {};

  constructor(opts: AcpOptions) {
    this.opts = opts;
  }

  get pid(): number | undefined {
    return this.child?.pid;
  }

  // Spawns `opencode acp` and runs initialize.
  async start(): Promise<Record<string, unknown>> {
    const { opts } = this;
    this.abort = new AbortController();
    const spawn = groupSpawner(opts.registry, opts.log, (line) => this.stderr(line), opts.label);
    const child = spawn({ command: opts.binary, args: ['acp'], cwd: opts.cwd, env: { ...process.env, ...opts.env }, signal: this.abort.signal } as never) as unknown as ChildProcess;
    this.child = child;
    child.on('error', (e) => {
      this.markExited();
      this.failAll(e);
    });
    child.stdin?.on('error', () => {
      // EPIPE when it died; the exit handler reports it.
    });
    child.stdout?.setEncoding('utf8');
    child.stdout?.on('data', (d: string) => this.onData(d));
    child.once('exit', (code, signal) => {
      const tail = this.stderrTail.slice(-5).join('\n');
      this.markExited();
      const what = `opencode acp exited (${signal ?? `code ${code}`})${tail ? `\n${tail}` : ''}`;
      this.failAll(new AcpExitedError(what));
      this.onExit(this.closing ? undefined : new Error(what));
    });
    const init = (await this.request('initialize', {
      protocolVersion: 1,
      clientCapabilities: { fs: { readTextFile: false, writeTextFile: false }, terminal: false },
      clientInfo: { name: 'threadbear', title: 'Threadbear', version: '1' },
    })) as Record<string, unknown>;
    const info = init?.agentInfo as { version?: unknown } | undefined;
    if (typeof info?.version === 'string') this.version = info.version;
    return init;
  }

  request<T = unknown>(method: string, params: unknown, timeoutMs?: number): Promise<T> {
    if (this.exited) return Promise.reject(new AcpExitedError('opencode acp has exited'));
    const id = ++this.nextId;
    return new Promise<T>((resolve, reject) => {
      const p: Pending = { resolve: resolve as (v: unknown) => void, reject };
      const ms = timeoutMs ?? (UNTIMED.has(method) ? 0 : (this.opts.requestMs ?? 60_000));
      if (ms > 0) {
        p.timer = setTimeout(() => {
          this.pending.delete(id);
          reject(new Error(`opencode didn't answer ${method} within ${Math.round(ms / 1000)} seconds`));
        }, ms);
      }
      this.pending.set(id, p);
      this.write({ jsonrpc: '2.0', id, method, params });
    });
  }

  notify(method: string, params: unknown): void {
    if (!this.exited) this.write({ jsonrpc: '2.0', method, params });
  }

  stderrLines(): string[] {
    return [...this.stderrTail];
  }

  // Ends the process: stdin closed, then its group stopped if it lingers.
  async stop(graceMs = 3000): Promise<void> {
    this.closing = true;
    if (this.exited) return;
    const gone = new Promise<void>((r) => this.exitWaiters.push(r));
    try {
      this.child?.stdin?.end();
    } catch {
      // already closed
    }
    const timer = setTimeout(() => this.abort?.abort(), graceMs);
    await gone;
    clearTimeout(timer);
  }

  kill(): void {
    this.closing = true;
    this.abort?.abort();
  }

  private write(msg: unknown): void {
    try {
      this.child?.stdin?.write(JSON.stringify(msg) + '\n');
    } catch {
      // the exit handler reports it
    }
  }

  private onData(d: string): void {
    this.buf += d;
    let i;
    while ((i = this.buf.indexOf('\n')) >= 0) {
      const line = this.buf.slice(0, i);
      this.buf = this.buf.slice(i + 1);
      if (line.trim()) this.onLine(line);
    }
  }

  private onLine(line: string): void {
    let msg: Record<string, unknown>;
    try {
      msg = JSON.parse(line);
    } catch {
      this.opts.log.warn('unparseable line from opencode', { threadId: this.opts.label, bytes: line.length });
      return;
    }
    if (!msg || typeof msg !== 'object') return;
    const method = typeof msg.method === 'string' ? msg.method : undefined;
    const params = (msg.params && typeof msg.params === 'object' ? msg.params : {}) as Record<string, unknown>;
    if (method && msg.id !== undefined && msg.id !== null) {
      const id = msg.id;
      this.onRequest(method, params).then(
        (result) => this.write({ jsonrpc: '2.0', id, result: result ?? null }),
        (e: unknown) => {
          const err = e instanceof AcpError ? e : new AcpError(-32603, (e as Error)?.message ?? String(e));
          this.write({ jsonrpc: '2.0', id, error: { code: err.code, message: err.message } });
        },
      );
      return;
    }
    if (method) {
      try {
        this.onNotification(method, params);
      } catch (e) {
        this.opts.log.error('could not handle an opencode notification', { threadId: this.opts.label, method, error: String(e) });
      }
      return;
    }
    const id = typeof msg.id === 'number' ? msg.id : Number(msg.id);
    const p = this.pending.get(id);
    if (!p) return;
    this.pending.delete(id);
    if (p.timer) clearTimeout(p.timer);
    const error = msg.error as { code?: number; message?: string; data?: unknown } | undefined;
    if (error) p.reject(new AcpError(Number(error.code ?? -32000), String(error.message ?? 'opencode error'), error.data));
    else p.resolve(msg.result);
  }

  private stderr(line: string): void {
    if (!line.trim()) return;
    this.stderrTail.push(line);
    if (this.stderrTail.length > 20) this.stderrTail.shift();
    this.opts.log.debug('opencode stderr', { threadId: this.opts.label, line: line.slice(0, 500) });
  }

  private failAll(e: Error): void {
    for (const p of this.pending.values()) {
      if (p.timer) clearTimeout(p.timer);
      p.reject(e);
    }
    this.pending.clear();
  }

  private markExited(): void {
    this.exited = true;
    const ws = this.exitWaiters;
    this.exitWaiters = [];
    ws.forEach((w) => w());
  }
}
