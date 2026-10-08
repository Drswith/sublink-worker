import yaml from './node_modules/js-yaml/index.js';
function describe(v) {
  if (v === null) return 'null';
  if (v === undefined) return 'undefined';
  if (typeof v === 'boolean') return String(v);
  if (typeof v === 'number') return 'n:' + (Object.is(v, -0) ? '-0' : String(v));
  if (typeof v === 'string') return JSON.stringify(v);
  if (v instanceof Date) return 'd:' + (isNaN(v.getTime()) ? 'Invalid' : v.toISOString());
  if (v instanceof Uint8Array) return '{' + Array.from(v).map((b, i) => JSON.stringify(String(i)) + ':n:' + b).join(',') + '}';
  if (Array.isArray(v)) return '[' + v.map(describe).join(',') + ']';
  return '{' + Object.keys(v).map(k => JSON.stringify(k) + ':' + describe(v[k])).join(',') + '}';
}
function decodeInput(v) {
  if (Array.isArray(v)) return v.map(decodeInput);
  if (v && typeof v === 'object') {
    if ('$nan' in v) return NaN;
    if ('$inf' in v) return Infinity;
    if ('$ninf' in v) return -Infinity;
    if ('$negzero' in v) return -0;
    if ('$undef' in v) return undefined;
    if ('$date' in v) return new Date(v.$date);
    const o = {};
    for (const k of Object.keys(v)) o[k] = decodeInput(v[k]);
    return o;
  }
  return v;
}
const loadCases = [
`proxies:
  - name: HK-01
    type: ss
    server: hk.example.com
    port: 443
    cipher: aes-128-gcm
    password: "p@ss:word"
    udp: true
  - {name: JP, type: vmess, server: jp.example.com, port: 8443, uuid: abc, alterId: 0, cipher: auto, tls: true, network: ws, ws-opts: {path: /ws, headers: {Host: jp.example.com}}}
proxy-groups:
  - name: Proxy
    type: select
    proxies: [HK-01, JP, DIRECT]
`,
`base: &base
  type: ss
  cipher: aes-128-gcm
proxies:
  - <<: *base
    name: a
    server: 1.1.1.1
    port: 1
  - <<: [*base]
    name: b
    type: trojan
`,
`a: 1
a: 2
`,
`a:\n\t- b\n`,
`key: value\n  bad: indent\n`,
`---\na: 1\n---\nb: 2\n`,
`--- # comment\nfoo: bar\n...\n`,
`%YAML 1.2\n---\nfoo: bar\n`,
`ints: [0, -0, +5, 012, 0x1F, 0o17, 0b101, 1_000, 99999999999999999999]
floats: [1.5, .5, 1., 1e3, -1E-7, .inf, -.Inf, .NaN, +.INF, 1.5e400]
bools: [true, True, TRUE, false, yes, no, on, off, y, n]
nulls: [~, null, Null, NULL, , ]
dates: [2024-01-02, 2024-1-2 3:04:05, 2024-01-02T03:04:05.123456Z, 2024-01-02 03:04:05 +08:00, 2024-13-45]
strs: ['single ''q''', "double \\"q\\" \\t \\u00e9 \\U0001F600 \\x41", plain text, "\\ud83d\\ude00"]
merge: <<
`,
`literal: |
  line1
  line2

keep: |+
  a

strip: |-
  b
folded: >
  folded
  text

  para
indent: |2
    two
`,
`emoji: 🇭🇰香港
"quoted key": v
? explicit key
: explicit value
? [a, b]
: list key
1: one
0: zero
-1: neg
`,
`tagged:\n  s: !!str 123\n  i: !!int "42"\n  f: !!float "1.5"\n  b: !!bool "true"\n  n: !!null ""\n  set: !!set {a, b}\n  omap: !!omap [{a: 1}, {b: 2}]\n  pairs: !!pairs [{a: 1}, {a: 2}]\n  bin: !!binary aGVsbG8=\n`,
`custom: !foo bar\n`,
`a: *undefined_alias\n`,
`- - nested\n  - seq\n- - x\n`,
`# only comment\n`,
``,
`plain`,
`ss://YWVz@a:1#x\nvmess://e30=`,
`[General]\nloglevel = notify\n`,
`proxies: []`,
`{"outbounds": [{"type": "direct"}]}`,
`a: b: c\n`,
`a: 'unterminated\n`,
`a: "bad \\q escape"\n`,
`key: "multi\n  line\n\n  quoted"\n`,
`key: 'multi\n  line'\n`,
`key: plain\n  continued\n  lines\n`,
`k: [a, b,\n  c]\n`,
`k: {a: 1, b: [x, y], c: {d: e}}\n`,
`- a\n-\n- c\n`,
`anchors:\n  - &x {a: 1}\n  - *x\n  - &s scalar\n  - *s\n`,
`list:\n- a\n- b\nmap:\n  x: 1\n`,
`"a\tb": tab\n`,
`windows: line\r\nendings: here\r\n`,
`\ufeffbom: yes\n`,
`k: v # comment\nk2: v#notcomment\n`,
`url: http://example.com:8080/path?x=1#frag\nnum_str: "443"\n`,
`? |\n  block key\n: v\n`,
`a:\n  - b\n  -   c: 1\n      d: 2\n`,
`password: 2024-01-01\nport: 0443\n`,
`k: &a !!str 1\nk2: *a\n`,
`!!map\n  !!str key: value\n`,
`&anchor\nkey: value\n`,
`items:\n  - name: x\n    sub:\n      - 1\n      - 2\n  - name: y\n`,
`deep: [[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[1]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]\n`,
`long: ` + 'word '.repeat(40) + `\n`,
`bad:\n- a\n  - b\n`,
`a: [1, 2\n`,
`a: {b: 1,, c: 2}\n`,
`x: !<tag:yaml.org,2002:str> 5\n`,
`x: !!str\n`,
`x: !!seq\ny: !!map\n`,
`? a\n? b\n`,
`- ? a\n  : b\n`,
`a: >-\n  x\n   y\n  z\n`,
`a: |\n  \ttab\n`,
`"\\u0041": 1\n`,
`a: "\\x0"`,
`'unterminated`,
`a:\n  b:\n    c: [1, {d: [2, {e: 3}]}]\n`,
];
const out = [];
for (const src of loadCases) {
  let r;
  try { r = { ok: describe(yaml.load(src)) }; } catch (e) { r = { err: e.message }; }
  out.push({ kind: 'load', input: src, ...r });
}
const dumpCases = [
  {a: 'true', b: 'yes', c: '1:2', d: '', e: ' lead', f: 'trail ', g: 'a: b', h: 'a #b', i: '#x', j: '-x', k: '- x', l: '? x', m: '@x', n: '`x', o: 'x\ny', p: 'multi\nline\n', q: 'keep\n\n', r: 'tab\there', s: 'nbsp\u00a0here', t: 'emoji 🇭🇰', u: "it's", v: 'say "hi"', w: 'back\\slash', x: 'ctrl\u0001', y: 'ls\u2028ps', z: 'plain value'},
  {long: 'word '.repeat(30).trim(), longnospace: 'x'.repeat(120), url: 'https://gh-proxy.com/https://github.com/MetaCubeX/meta-rules-dat/raw/refs/heads/meta/geo/geosite/category-ads-all.mrs', multi_long: ('aaa bbb ccc '.repeat(10) + '\n').repeat(3), nested: {deep: {deeper: 'word '.repeat(25)}}},
  {nums: [0, 1, -1, 1.5, 1e21, 1e-7, 1.5e-7, 123456789012, {$negzero: 1}, {$nan: 1}, {$inf: 1}, {$ninf: 1}, 0.1], bools: [true, false], nil: null, undef: {$undef: 1}, arr_undef: [1, {$undef: 1}, 3], date: {$date: '2024-01-02T03:04:05.000Z'}},
  {empty_arr: [], empty_obj: {}, nested_arr: [[1, 2], [], [[3]]], arr_of_obj: [{a: 1, b: 2}, {}, {c: [1]}], obj_arr: {x: [{y: [{z: 1}]}]}},
  {'1': 'one', 'b': 'bee', '0': 'zero', 'true': 't', 'a:b': 1, 'key with space': 2, '-k': 3, '': 4, 'null': 5, '#': 6, '1.5': 7, 'émoji🇭🇰': 8},
  {'port': 7890, 'mode': 'rule', 'proxy-groups': [{name: '🚀 节点选择', type: 'select', proxies: ['DIRECT', 'REJECT']}], 'rules': ['DOMAIN-SUFFIX,google.com,🚀 节点选择', 'MATCH,🐟 漏网之鱼', 'RULE-SET,geolocation-!cn,🌐 非中国', 'IP-CIDR,1.1.1.1/32,DIRECT,no-resolve']},
  ['top', 'level', ['seq']],
  'just a string',
  42,
  {k: '0x1F', k2: '012', k3: '1e5', k4: '.inf', k5: '~', k6: '2024-01-01', k7: '<<', k8: 'NULL', k9: 'False', k10: '+1', k11: '1_000', k12: '-.5'},
  {s1: 'a,b', s2: '[x]', s3: '{y}', s4: 'x: ', s5: 'x:y', s6: 'x :y', s7: 'a#', s8: 'a #', s9: '%x', s10: '!x', s11: '&x', s12: '*x', s13: '|x', s14: '>x', s15: "'x", s16: '"x', s17: 'x\'', s18: '?', s19: ':', s20: '-', s21: 'x-', s22: 'x?', s23: '.', s24: '~x', s25: 'a\u0085b', s26: '\ufeffx', s27: 'x\r\ny'},
  {indented: '  leading spaces\nnext', newline_first: '\nstarts with newline', only_newlines: '\n\n', single_nl: '\n', trailing_spaces_lines: 'a  \nb  '},
  {key_long: {['k'.repeat(1100)]: 'v'}},
  {obs: {'obfs-password': 'pass', 'password': 'p@ss/w+rd=', 'path': '/path?ed=2048', 'headers': {Host: 'a.b'}, 'short-id': 'a1b2', 'public-key': 'xyz-_ABC', 'sni': ''}},
];
for (const c of dumpCases) {
  let r;
  try { r = { ok: yaml.dump(decodeInput(c)) }; } catch (e) { r = { err: e.message }; }
  out.push({ kind: 'dump', input: c, ...r });
}
const roundtrip = [
`a: &x {k: [1, 2]}\nb: *x\nc: [*x, *x]\n`,
`base: &b\n  alpn: &al [h2, http/1.1]\n  opts: {path: /x}\nproxies:\n  - <<: *b\n    name: a\n  - <<: *b\n    name: b\n    extra: *al\n`,
`l: &l []\nm: &m {}\nx: [*l, *m, *l]\n`,
`s: &s [[1], [2]]\nt: *s\n`,
];
for (const src of roundtrip) {
  let r;
  try { r = { ok: yaml.dump(yaml.load(src)) }; } catch (e) { r = { err: e.message }; }
  out.push({ kind: 'roundtrip', input: src, ...r });
}
console.log(JSON.stringify(out));
