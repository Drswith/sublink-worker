import { afterEach, describe, expect, it, vi } from 'vitest';
import yaml from 'js-yaml';
import { createApp } from '../src/app/createApp.jsx';
import { MemoryKVAdapter } from '../src/adapters/kv/memoryKv.js';

const app = () => createApp({ kv: new MemoryKVAdapter(), logger: { error: vi.fn() } });
const source = 'https://example.com/sub/private-token';
const request = (endpoint = 'clash') => `http://localhost/${endpoint}?config=${encodeURIComponent(source)}`;

describe('Upstream subscription failures', () => {
    afterEach(() => vi.unstubAllGlobals());

    it.each(['clash', 'singbox', 'surge'])('returns 502 for a failed fetch instead of an empty %s config', async (endpoint) => {
        vi.stubGlobal('fetch', vi.fn().mockRejectedValue(new TypeError('fetch failed')));
        const response = await app().request(request(endpoint));
        expect(response.status).toBe(502);
        const body = await response.text();
        expect(body).toContain('upstream subscription');
        expect(body).not.toContain('private-token');
    });

    it('does not treat an upstream HTTP error as a successful subscription', async () => {
        vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: false, status: 403 }));
        expect((await app().request(request())).status).toBe(502);
    });

    it('imports all nodes from a fetched base64 subscription and preserves the DNS switch', async () => {
        const nodes = [30000, 30001, 30002, 30003].map(port =>
            `vless://00000000-0000-4000-8000-000000000001@example.com:${port}?security=none&type=tcp#Node-${port}`);
        vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
            ok: true, status: 200, text: async () => btoa(nodes.join('\n')),
            headers: new Headers()
        }));
        const response = await app().request(request() + '&include_clash_dns=false');
        expect(response.status).toBe(200);
        const config = yaml.load(await response.text());
        expect(config.proxies).toHaveLength(4);
        expect(config).not.toHaveProperty('dns');
        const group = config['proxy-groups'].find(g => g.name === '🚀 节点选择');
        for (const node of config.proxies) expect(group.proxies).toContain(node.name);
    });
});
