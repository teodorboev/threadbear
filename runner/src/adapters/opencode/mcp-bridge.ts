// The threadbear tools for OpenCode threads, as an MCP server OpenCode
// connects to over HTTP. Claude gets them in-process and Codex as dynamic
// tools; OpenCode only takes MCP servers (ACP's mcpServers, http), so the
// runner serves one on 127.0.0.1, on a port the OS picks, while any
// OpenCode thread is live.
//
// Each session gets its own random bearer token, which says whose thread a
// call is for; anything without a live token is refused. It answers the
// few MCP requests a tools-only server needs (initialize, tools/list,
// tools/call, ping) as plain JSON responses, which MCP's Streamable HTTP
// transport allows, and runs the calls through CodexTools so approvals and
// "Always allow" work as on Codex.
import http from 'node:http';
import crypto from 'node:crypto';
import { z } from 'zod';
import type { Logger } from '../../log.ts';
import { CodexTools, NAMESPACE, type ToolDefinition } from '../codex/tools.ts';

export type BridgeSession = { tools: ToolDefinition[]; runner: CodexTools; instructions?: string };

const PROTOCOL_VERSIONS = ['2025-11-25', '2025-06-18', '2025-03-26', '2024-11-05'];
const MAX_BODY = 16 * 1024 * 1024;

export class McpBridge {
  private log: Logger;
  private server?: http.Server;
  private listening?: Promise<number>;
  private sessions = new Map<string, BridgeSession>();

  constructor(log: Logger) {
    this.log = log;
  }

  // Registers a session's tools and returns what ACP's session/new takes
  // for it. Starts the server on first use.
  async add(s: BridgeSession): Promise<{ token: string; server: { type: 'http'; name: string; url: string; headers: { name: string; value: string }[]; codemode: false } }> {
    const port = await this.listen();
    const token = crypto.randomBytes(24).toString('base64url');
    this.sessions.set(token, s);
    return {
      token,
      // codemode: false asks for the tools as tools of their own rather than
      // behind OpenCode 2's code-mode `execute`. OpenCode 2.1's ACP ignores
      // it, so normalize.ts's codeModeCall names such calls too.
      server: { type: 'http', name: NAMESPACE, url: `http://127.0.0.1:${port}/mcp`, headers: [{ name: 'Authorization', value: `Bearer ${token}` }], codemode: false },
    };
  }

  remove(token: string): void {
    this.sessions.delete(token);
    if (!this.sessions.size) this.close();
  }

  close(): void {
    const s = this.server;
    this.server = undefined;
    this.listening = undefined;
    s?.close();
  }

  private listen(): Promise<number> {
    if (this.listening) return this.listening;
    const server = http.createServer((req, res) => void this.handle(req, res));
    this.server = server;
    this.listening = new Promise<number>((resolve, reject) => {
      server.once('error', reject);
      server.listen(0, '127.0.0.1', () => {
        const addr = server.address();
        resolve(typeof addr === 'object' && addr ? addr.port : 0);
      });
    });
    return this.listening;
  }

  private async handle(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
    const auth = req.headers.authorization ?? '';
    const token = auth.startsWith('Bearer ') ? auth.slice(7) : '';
    const s = token ? this.sessions.get(token) : undefined;
    if (!s) return end(res, 401, { error: 'unknown session' });
    if (req.method === 'DELETE') return end(res, 200, {});
    // No server-to-client stream: everything is answered in the response.
    if (req.method !== 'POST') return end(res, 405, { error: 'POST only' });
    let body: unknown;
    try {
      body = JSON.parse(await readBody(req));
    } catch {
      return end(res, 400, rpcError(null, -32700, 'Parse error'));
    }
    const abort = new AbortController();
    res.on('close', () => {
      if (!res.writableFinished) abort.abort();
    });
    const msgs = Array.isArray(body) ? body : [body];
    const replies: unknown[] = [];
    for (const m of msgs) {
      const r = await this.one(s, m as Record<string, unknown>, abort.signal);
      if (r) replies.push(r);
    }
    if (!replies.length) {
      res.writeHead(202).end();
      return;
    }
    end(res, 200, Array.isArray(body) ? replies : replies[0]);
  }

  private async one(s: BridgeSession, m: Record<string, unknown>, signal: AbortSignal): Promise<unknown> {
    const id = m?.id ?? null;
    const method = typeof m?.method === 'string' ? m.method : '';
    // A notification (initialized, cancelled) or a response: no reply.
    if (id === null || id === undefined) return undefined;
    const params = (m.params && typeof m.params === 'object' ? m.params : {}) as Record<string, unknown>;
    try {
      switch (method) {
        case 'initialize': {
          const asked = typeof params.protocolVersion === 'string' ? params.protocolVersion : '';
          return rpcResult(id, {
            protocolVersion: PROTOCOL_VERSIONS.includes(asked) ? asked : PROTOCOL_VERSIONS[0],
            capabilities: { tools: { listChanged: false } },
            serverInfo: { name: NAMESPACE, version: '1' },
            ...(s.instructions ? { instructions: s.instructions } : {}),
          });
        }
        case 'ping':
          return rpcResult(id, {});
        case 'tools/list':
          return rpcResult(id, {
            tools: s.tools.map((t) => ({ name: t.name, description: t.description, inputSchema: jsonSchema(t), ...(t.annotations ? { annotations: t.annotations } : {}) })),
          });
        case 'tools/call': {
          const name = typeof params.name === 'string' ? params.name : '';
          const r = await s.runner.call({ namespace: NAMESPACE, tool: name, arguments: params.arguments ?? {}, callId: '' } as never, { fromChild: true, signal });
          const text = (r.contentItems ?? []).map((c) => ('text' in c ? String(c.text ?? '') : '')).join('\n');
          return rpcResult(id, { content: [{ type: 'text', text }], isError: !r.success });
        }
        default:
          return rpcError(id, -32601, `Method not found: ${method}`);
      }
    } catch (e) {
      this.log.warn('threadbear tool call from opencode failed', { method, error: String(e) });
      return rpcError(id, -32603, (e as Error)?.message ?? String(e));
    }
  }
}

function jsonSchema(t: ToolDefinition): unknown {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const schema = z.toJSONSchema(z.object(t.inputSchema as any)) as Record<string, unknown>;
  delete schema.$schema;
  return schema;
}

function rpcResult(id: unknown, result: unknown) {
  return { jsonrpc: '2.0', id, result };
}

function rpcError(id: unknown, code: number, message: string) {
  return { jsonrpc: '2.0', id, error: { code, message } };
}

function end(res: http.ServerResponse, status: number, body: unknown): void {
  res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(body));
}

function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    let size = 0;
    const chunks: Buffer[] = [];
    req.on('data', (c: Buffer) => {
      size += c.length;
      if (size > MAX_BODY) {
        reject(new Error('body too large'));
        req.destroy();
        return;
      }
      chunks.push(c);
    });
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}
