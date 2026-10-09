// Captures GET / for several languages; tests/pages.rs compares against it.
// The page masks the inlined form script (esbuild reprints it), so its source
// file is recorded separately.
import { readFileSync, writeFileSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { createApp, MemoryKVAdapter } from './golden-bundle.mjs';
const app = createApp({ kv: new MemoryKVAdapter(), logger: console, config: {} });
const year = String(new Date().getFullYear());
const cases = [
  { query: '?lang=zh-CN' }, { query: '?lang=en-US' }, { query: '?lang=fa' }, { query: '?lang=ru' },
  { query: '?lang=en' }, { query: '?lang=xx' }, { query: '?lang=fa-IR' }, { query: '' },
  { query: '', headers: { 'Accept-Language': 'ru-RU,ru;q=0.9' } },
];
const out = [];
for (const c of cases) {
  const res = await app.request('http://localhost/' + c.query, { headers: c.headers || {} });
  let html = await res.text();
  const start = html.indexOf('\n    ((t) => {') + 6;
  const end = html.indexOf(')();\n  </script>', start);
  html = html.slice(0, start) + '{{form_logic}}' + html.slice(end);
  html = html.split('© ' + year + ' ').join('© {{year}} ');
  out.push({ ...c, status: res.status, contentType: res.headers.get('content-type'), html });
}
const formLogic = readFileSync('src/components/formLogic.js', 'utf8');
writeFileSync(process.argv[2], gzipSync(JSON.stringify({ pages: out, formLogic }), { level: 9 }));
process.exit(0);
