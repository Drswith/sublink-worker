// Generates tests/fixtures/fetch_cases.json: how Node's fetch (undici) ends for
// raw upstream responses, replayed against HttpFetcher by tests/fetch_compat.rs.
// Requests are made the way fetchSubscriptionWithFormat makes them.
import net from 'node:net';
import zlib from 'node:zlib';

const LIST = 'ss://YWVzLTEyOC1nY206dGVzdA@a.example.com:443#A\nss://YWVzLTEyOC1nY206dGVzdA@b.example.com:443#B\n';
const B = (s) => Buffer.from(s, 'latin1');
const gz = (b) => zlib.gzipSync(b);
const zl = (b) => zlib.deflateSync(b);
const raw = (b) => zlib.deflateRawSync(b);
const br = (b) => zlib.brotliCompressSync(b);
const list = Buffer.from(LIST);

// {{BASE}} stands for http://127.0.0.1:<port> and is substituted at run time.
const resp = (status, headers, body = Buffer.alloc(0)) => {
  let head = `HTTP/1.1 ${status} X\r\n`;
  for (const [k, v] of headers) head += `${k}: ${v}\r\n`;
  if (!headers.some(([k]) => k.toLowerCase() === 'content-length')) head += `Content-Length: ${body.length}\r\n`;
  return Buffer.concat([B(head + 'Connection: close\r\n\r\n'), body]);
};
const UI = ['subscription-userinfo', 'upload=1; download=2'];
const ce = (value, body) => resp(200, [['Content-Encoding', value]], body);

const ROUTES = {
  '/plain': resp(200, [UI], list),
  '/gz': ce('gzip', gz(list)),
  '/xgz': ce('x-gzip', gz(list)),
  '/GZ': ce('GZIP', gz(list)),
  '/gz-sp': ce(' gzip ', gz(list)),
  '/gzgz': ce('gzip, gzip', gz(gz(list))),
  '/gzgz-two-headers': resp(200, [['Content-Encoding', 'gzip'], ['Content-Encoding', 'gzip']], gz(gz(list))),
  '/defl-zlib': ce('deflate', zl(list)),
  '/defl-raw': ce('deflate', raw(list)),
  '/br': ce('br', br(list)),
  '/ident': ce('identity', list),
  '/unknown': ce('foo', gz(list)),
  '/gz-unknown': ce('gzip, foo', gz(list)),
  '/unknown-gz': ce('foo, gzip', gz(list)),
  '/ident-gz': ce('identity, gzip', gz(list)),
  '/gz-comma': ce('gzip,', gz(list)),
  '/gz-nbsp': ce('\xa0gzip', gz(list)),
  '/zstd': ce('zstd', list),
  '/gz-br': ce('gzip, br', br(gz(list))),
  '/br-gz': ce('br, gzip', gz(br(list))),
  '/defl-gz': ce('deflate, gzip', gz(raw(list))),
  '/gz-trailing': ce('gzip', Buffer.concat([gz(list), B('garbage')])),
  '/gz-two': ce('gzip', Buffer.concat([gz(list.subarray(0, 48)), gz(list.subarray(48))])),
  '/gz-multi-trail': ce('gzip', Buffer.concat([gz(list.subarray(0, 48)), gz(list.subarray(48)), B('x')])),
  '/gz-zeros': ce('gzip', Buffer.concat([gz(list), Buffer.alloc(4)])),
  '/gz-zeros-garbage': ce('gzip', Buffer.concat([gz(list), Buffer.alloc(2), B('garbage')])),
  '/gz-trunc': ce('gzip', gz(list).subarray(0, -6)),
  '/gz-trunc-mid': ce('gzip', gz(list).subarray(0, 40)),
  '/gz-hdr-trunc': ce('gzip', gz(list).subarray(0, 5)),
  '/gz-crc-bad': ce('gzip', Buffer.concat([gz(list).subarray(0, -8), Buffer.alloc(4), gz(list).subarray(-4)])),
  '/gz-size-bad': ce('gzip', Buffer.concat([gz(list).subarray(0, -4), Buffer.from([1, 0, 0, 0])])),
  '/gz-badflags': ce('gzip', Buffer.concat([Buffer.from([0x1f, 0x8b, 8, 0xe0]), gz(list).subarray(4)])),
  '/gz-badmethod': ce('gzip', Buffer.concat([Buffer.from([0x1f, 0x8b, 7]), gz(list).subarray(3)])),
  '/gz-fname': ce('gzip', Buffer.concat([gz(list).subarray(0, 3), Buffer.from([8]), gz(list).subarray(4, 10), B('name.txt\0'), gz(list).subarray(10)])),
  '/gz-bad': resp(200, [UI, ['Content-Encoding', 'gzip']], list),
  '/gz-empty': ce('gzip', Buffer.alloc(0)),
  '/gz-204': resp(204, [['Content-Encoding', 'gzip']]),
  '/defl-empty': ce('deflate', Buffer.alloc(0)),
  '/defl-zlib-trail': ce('deflate', Buffer.concat([zl(list), B('garbage')])),
  '/defl-raw-trail': ce('deflate', Buffer.concat([raw(list), B('garbage')])),
  '/defl-zlib-trunc': ce('deflate', zl(list).subarray(0, -3)),
  '/defl-zlib-trunc-mid': ce('deflate', zl(list).subarray(0, 30)),
  '/defl-raw-trunc': ce('deflate', raw(list).subarray(0, 30)),
  '/defl-zlib-badsum': ce('deflate', Buffer.concat([zl(list).subarray(0, -4), Buffer.alloc(4)])),
  '/defl-bad': ce('deflate', B('\x78\x9cnot deflate data at all')),
  '/defl-plain': ce('deflate', list),
  '/br-trunc': ce('br', br(list).subarray(0, -2)),
  '/br-trail': ce('br', Buffer.concat([br(list), B('garbage')])),
  '/br-bad': ce('br', list),
  '/r-plain': resp(302, [['Location', '/plain']]),
  '/r-tab': resp(302, [['Location', '\t/plain']]),
  '/r-multi': resp(302, [['Location', '/plain'], ['Location', '/gz']]),
  '/r-data': resp(302, [UI, ['Location', 'data:,hello']], B('redirect-body')),
  '/r-ftp': resp(302, [['Location', 'ftp://example.com/x']], B('redirect-body')),
  '/r-creds': resp(302, [['Location', '{{CREDS}}/plain']]),
  '/r-bad': resp(302, [['Location', 'http://[::1']], B('redirect-body')),
  '/r-noloc': resp(302, [UI], B('no-location-body')),
  '/r-empty': resp(302, [['Location', '']], B('empty-location-body')),
  '/r-schemeless': resp(302, [['Location', '//{{HOST}}/plain']]),
  '/r-loop': resp(302, [['Location', '/r-loop']]),
  '/r-301': resp(301, [['Location', '/plain']]),
  '/r-303': resp(303, [['Location', '/plain']]),
  '/r-307': resp(307, [['Location', '/plain']]),
  '/r-308': resp(308, [['Location', '/plain']]),
  '/r-300': resp(300, [['Location', '/plain']], B('multiple-choices')),
  '/r-304': resp(304, [['Location', '/plain']]),
  '/r-unicode': resp(302, [['Location', Buffer.from('/pläin').toString('latin1')]]),
  '/r-latin1': resp(302, [['Location', '/pl\xe4in']]),
  '/pl%C3%A4in': resp(200, [], list),
  '/pl%E4in': resp(200, [], B('ss://YWVzLTEyOC1nY206dGVzdA@latin.example.com:443#LATIN1\n')),
  '/trunc-ui': resp(200, [['subscription-userinfo', 'upload=5; total=10'], ['Content-Length', '500']], list),
  '/s500': resp(500, [UI], list),
  '/s404': resp(404, [], B('nope')),
};
// A chain of exactly 20 redirects is followed; one more is too many.
for (let i = 0; i < 21; i++) ROUTES[`/chain${i}`] = resp(302, [['Location', i === 0 ? '/plain' : `/chain${i - 1}`]]);

const CASES = [
  ...Object.keys(ROUTES).filter((p) => !p.startsWith('/chain') && !p.startsWith('/pl%')).map((path) => ({ path })),
  { path: '/chain19' },
  { path: '/chain20' },
  { path: '/plain', credentials: true },
  ...[' spaced ', '\tx\t', 'café', '中文', 'a\x7fb', 'x ', 'a\x01b', '  ', '', 'clash.meta', 'ÿ', 'Ā'].map((ua) => ({ path: '/ua', ua })),
];

const server = net.createServer((sock) => {
  let data = Buffer.alloc(0);
  sock.on('data', (chunk) => {
    data = Buffer.concat([data, chunk]);
    const end = data.indexOf('\r\n\r\n');
    if (end < 0) return;
    const head = data.subarray(0, end).toString('latin1');
    const path = head.split(' ')[1];
    let out;
    if (path === '/ua') {
      const line = head.split('\r\n').find((l) => l.toLowerCase().startsWith('user-agent:'));
      const ua = line === undefined ? '<none>' : Buffer.from(line.slice(11), 'latin1').toString('hex');
      out = resp(200, [], B(`trojan://p@ua.example.com:443#UA[${ua}]\n`));
    } else {
      out = ROUTES[path] ? fill(ROUTES[path]) : resp(404, [], B('nf'));
    }
    sock.end(out);
  });
  sock.on('error', () => {});
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const host = `127.0.0.1:${server.address().port}`;
const fill = (buf) => B(buf.toString('latin1').replaceAll('{{CREDS}}', `http://u:p@${host}`).replaceAll('{{HOST}}', host));

const results = [];
for (const c of CASES) {
  const url = (c.credentials ? `http://u:p@${host}` : `http://${host}`) + c.path;
  let outcome;
  try {
    const headers = new Headers();
    if (c.ua) headers.set('User-Agent', c.ua);
    const res = await fetch(url, { method: 'GET', headers, signal: AbortSignal.timeout(15000) });
    const ui = res.headers.get('subscription-userinfo');
    try {
      outcome = { kind: 'ok', status: res.status, ui, text: await res.text() };
    } catch {
      outcome = { kind: 'body-error', status: res.status, ui };
    }
  } catch {
    outcome = { kind: 'error' };
  }
  results.push({ ...c, outcome });
}
server.close();
const routes = Object.fromEntries(Object.entries(ROUTES).map(([k, v]) => [k, v.toString('base64')]));
console.log(JSON.stringify({ routes, cases: results }, null, 1));
process.exit(0);
