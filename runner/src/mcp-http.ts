// The threadbear tools over MCP's Streamable HTTP transport, for a CLI that
// can't take Claude's in-process SDK server or Codex's dynamic tools: Grok
// Build (docs/grok-plan.md). One small server on 127.0.0.1, started on
// first use. Each live session gets a path with a random secret in it, so
// only the CLI it was handed to can reach its thread's tools, and the path
// goes when the session ends.
//
// Only what a tool client needs: initialize, tools/list, tools/call and
// ping, each answered as one JSON response (the transport allows that
// instead of a stream). No sessions, no server-sent events.
import http from 'node:http';
import crypto from 'node:crypto';
import type { AddressInfo } from 'node:net';
import { z } from 'zod';
import type { Logger } from './log.ts';
import { RUNNER_VERSION } from './config.ts';
import type { ToolDefinition } from './adapters/codex/tools.ts';

export type McpToolCall = { name: string; arguments: Record<string, unknown>; signal: AbortSignal };
export type McpToolResult = { text: string; isError: boolean };

export type McpEndpoint = {
  name: string; // the server's name, as the CLI shows it
  tools: ToolDefinition[];
  instructions?: string;
  call(c: McpToolCall): Promise<McpToolResult>;
};

const MAX_BODY = 8 * 1024 * 1024;

export class McpHttpServer {
  private server?: http.Server;
  private starting?: Promise<number>;
  private endpoints = new Map<string, McpEndpoint>();
  private log: Logger;

  constructor(log: Logger) {
    this.log = log;
  }

  // Registers an endpoint and returns its URL, starting the server if it
  // isn't yet. `remove` takes the path away.
  async add(ep: McpEndpoint): Promise<{ url: string; remove: () => void }> {
    const port = await this.listen();
    const secret = crypto.randomBytes(24).toString('base64url');
    this.endpoints.set(secret, ep);
    return { url: `http://127.0.0.1:${port}/mcp/${secret}`, remove: () => this.endpoints.delete(secret) };
  }

  get size(): number {
    return this.endpoints.size;
  }

  private listen(): Promise<number> {
    if (this.starting) return this.starting;
    this.starting = new Promise<number>((resolve, reject) => {
      const server = http.createServer((req, res) => void this.handle(req, res));
      server.on('error', (e) => {
        this.starting = undefined;
        reject(e);
      });
      server.listen(0, '127.0.0.1', () => {
        this.server = server;
        server.unref();
        resolve((server.address() as AddressInfo).port);
      });
    });
    return this.starting;
  }

  async close(): Promise<void> {
    this.endpoints.clear();
    const s = this.server;
    this.server = undefined;
    this.starting = undefined;
    if (s) await new Promise<void>((r) => s.close(() => r()));
  }

  private async handle(req: http.IncomingMessage, res: http.ServerResponse): Promise<void> {
    const m = /^\/mcp\/([A-Za-z0-9_-]+)\/?$/.exec(req.url ?? '');
    const ep = m ? this.endpoints.get(m[1]) : undefined;
    if (!ep) return end(res, 404);
    if (req.method === 'DELETE') return end(res, 200);
    if (req.method !== 'POST') return end(res, 405, undefined, { Allow: 'POST, DELETE' });
    let body: unknown;
    try {
      body = JSON.parse(await readBody(req));
    } catch {
      return json(res, 400, rpcError(null, -32700, 'Parse error'));
    }
    const abort = new AbortController();
    res.on('close', () => {
      if (!res.writableEnded) abort.abort();
    });
    const batch = Array.isArray(body) ? body : [body];
    const answers = (await Promise.all(batch.map((msg) => this.one(ep, msg, abort.signal)))).filter((a) => a !== null);
    if (!answers.length) return end(res, 202);
    json(res, 200, Array.isArray(body) ? answers : answers[0]);
  }

  // One JSON-RPC message: its answer, or null for a notification.
  private async one(ep: McpEndpoint, msg: unknown, signal: AbortSignal): Promise<unknown> {
    if (!msg || typeof msg !== 'object') return rpcError(null, -32600, 'Invalid request');
    const { id, method, params } = msg as { id?: unknown; method?: unknown; params?: Record<string, unknown> };
    if (id === undefined || id === null) return null; // notifications/initialized and the like
    switch (method) {
      case 'initialize':
        return rpcResult(id, {
          protocolVersion: typeof params?.protocolVersion === 'string' ? params.protocolVersion : '2025-06-18',
          capabilities: { tools: { listChanged: false } },
          serverInfo: { name: ep.name, version: RUNNER_VERSION },
          ...(ep.instructions ? { instructions: ep.instructions } : {}),
        });
      case 'ping':
        return rpcResult(id, {});
      case 'tools/list':
        return rpcResult(id, { tools: ep.tools.map(toolSpec) });
      case 'tools/call': {
        const name = typeof params?.name === 'string' ? params.name : '';
        const args = params?.arguments && typeof params.arguments === 'object' && !Array.isArray(params.arguments) ? (params.arguments as Record<string, unknown>) : {};
        try {
          const r = await ep.call({ name, arguments: args, signal });
          return rpcResult(id, { content: [{ type: 'text', text: r.text }], isError: r.isError });
        } catch (e) {
          this.log.warn('an MCP tool call failed', { tool: name, error: String(e) });
          return rpcResult(id, { content: [{ type: 'text', text: (e as Error)?.message ?? String(e) }], isError: true });
        }
      }
      default:
        return rpcError(id, -32601, `Method not found: ${String(method)}`);
    }
  }
}

export function toolSpec(t: ToolDefinition): Record<string, unknown> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const schema = z.toJSONSchema(z.object(t.inputSchema as any)) as Record<string, unknown>;
  delete schema.$schema;
  return { name: t.name, description: t.description, inputSchema: schema, ...(t.annotations ? { annotations: t.annotations } : {}) };
}

function rpcResult(id: unknown, result: unknown) {
  return { jsonrpc: '2.0', id, result };
}

function rpcError(id: unknown, code: number, message: string) {
  return { jsonrpc: '2.0', id, error: { code, message } };
}

function readBody(req: http.IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    const chunks: Buffer[] = [];
    let size = 0;
    req.on('data', (c: Buffer) => {
      size += c.length;
      if (size > MAX_BODY) {
        reject(new Error('too large'));
        req.destroy();
        return;
      }
      chunks.push(c);
    });
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}

function json(res: http.ServerResponse, status: number, body: unknown): void {
  const text = JSON.stringify(body);
  res.writeHead(status, { 'Content-Type': 'application/json', 'Content-Length': Buffer.byteLength(text) });
  res.end(text);
}

function end(res: http.ServerResponse, status: number, _body?: undefined, headers: Record<string, string> = {}): void {
  res.writeHead(status, headers);
  res.end();
}
