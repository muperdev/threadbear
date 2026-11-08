// Whether a provider's API can be reached again, after a turn failed
// because it couldn't be (errorKind network). The runner asks this while
// such threads wait, and carries them on once it says yes
// (Runner.onNetworkLost).
//
// A TCP connection to the API host's port 443 is enough: the turn failed
// before any answer (DNS, no route, a dropped line), and a connection that
// opens means the CLI's own request would get through too. Through a proxy
// (HTTPS_PROXY, which the CLIs use as well) the proxy is asked for a
// tunnel to the host, as the CLI's request would.
import net from 'node:net';
import type { Provider } from './protocol.ts';

export type Reachable = (provider: Provider) => Promise<boolean>;

const HOSTS: Record<Provider, string> = {
  claude: 'api.anthropic.com',
  // Codex logins are ChatGPT ones; its requests go to chatgpt.com.
  codex: 'chatgpt.com',
  // Grok Build under a grok.com login chats through this proxy.
  grok: 'cli-chat-proxy.grok.com',
};

const PROBE_TIMEOUT_MS = 5_000;

export function probeTarget(provider: Provider, env: NodeJS.ProcessEnv = process.env): { host: string; port: number; proxy?: { host: string; port: number } } {
  const host = HOSTS[provider] ?? HOSTS.claude;
  const proxy = env.HTTPS_PROXY || env.https_proxy;
  if (proxy) {
    try {
      const u = new URL(proxy);
      return { host, port: 443, proxy: { host: u.hostname, port: Number(u.port) || (u.protocol === 'https:' ? 443 : 80) } };
    } catch {
      // Not a URL; try the host directly.
    }
  }
  return { host, port: 443 };
}

export const reachable: Reachable = (provider) => {
  const { host, port, proxy } = probeTarget(provider);
  return new Promise((resolve) => {
    const socket = net.connect(proxy ?? { host, port });
    const done = (ok: boolean) => {
      socket.destroy();
      resolve(ok);
    };
    socket.setTimeout(PROBE_TIMEOUT_MS, () => done(false));
    socket.once('error', () => done(false));
    socket.once('connect', () => {
      if (!proxy) return done(true);
      // The proxy's answer to CONNECT: a 2xx means it reached the host.
      socket.write(`CONNECT ${host}:${port} HTTP/1.1\r\nHost: ${host}:${port}\r\n\r\n`);
      let head = '';
      socket.on('data', (b: Buffer) => {
        head += b.toString('latin1');
        if (head.includes('\r\n')) done(/^HTTP\/1\.[01] 2\d\d/.test(head));
      });
      socket.once('end', () => done(false));
    });
  });
};
