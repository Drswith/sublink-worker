// Generates tests/fixtures/golden.json.gz: request/response pairs captured from
// the original Node implementation, replayed by tests/golden.rs.
import { gzipSync } from 'node:zlib';
import { writeFileSync, readFileSync } from 'node:fs';
import { createApp, MemoryKVAdapter } from './golden-bundle.mjs';

const b64 = (s) => Buffer.from(s, 'utf8').toString('base64');
const b64url = (s) => b64(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
const vmess = (o) => 'vmess://' + b64(JSON.stringify(o));

// ---------------------------------------------------------------------------
// Proxy URIs
// ---------------------------------------------------------------------------
const P = {
    ss_sip002: 'ss://YWVzLTEyOC1nY206dGVzdA@hk1.example.com:443#HK-Node-1',
    ss_sip002_pad: 'ss://YWVzLTI1Ni1nY206dGVzdA==@us1.example.com:8388#US-Node-1',
    ss_plain: 'ss://chacha20-ietf-poly1305:p%40ss%3Aword@sg.example.com:8388#%E6%96%B0%E5%8A%A0%E5%9D%A1%2001',
    ss_legacy: 'ss://' + b64('aes-256-gcm:legacypass@jp.example.com:10086') + '#JP%20Legacy',
    ss_obfs: 'ss://YWVzLTEyOC1nY206dGVzdC1wYXNzd29yZC0xMjM0@test.example.com:8388/?plugin=simple-obfs%3Bobfs%3Dhttp%3Bobfs-host%3Dcdn.example.com#%F0%9F%87%AD%F0%9F%87%B0Test-Node',
    ss_v2ray: 'ss://Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTp0ZXN0LXBhc3N3b3Jk@example.com:443/?plugin=v2ray-plugin%3Bmode%3Dwebsocket%3Bhost%3Dexample.com%3Bpath%3D%2Fv2ray%3Btls#SS-V2Ray-Test',
    ss_ipv6: 'ss://YWVzLTEyOC1nY206dGVzdA@[2001:db8::1]:8443#IPv6-SS',
    ss_noname: 'ss://YWVzLTEyOC1nY206dGVzdA@noname.example.com:443',
    ss_plus: 'ss://YWVzLTI1Ni1nY206cGFzcyt3aXRoK3BsdXM=@server1.com:8388#Node1',
    ss_2022: 'ss://' + b64url('2022-blake3-aes-128-gcm:YWJjZGVmZ2hpamtsbW5vcA==') + '@de.example.com:9000#Germany%20SS2022',
    vmess_ws_tls: vmess({ v: '2', ps: '日本 JP-01', add: 'jp1.example.com', port: '443', id: 'add66666-8888-8888-8888-888888888888', aid: '0', scy: 'auto', net: 'ws', type: 'none', host: 'cdn.example.com', path: '/ws?ed=2048', tls: 'tls', sni: 'sni.example.com', alpn: 'h2,http/1.1', fp: 'chrome' }),
    vmess_tcp: vmess({ v: '2', ps: 'US Plain', add: '1.2.3.4', port: 10086, id: 'b831381d-6324-4d53-ad4f-8cda48b30811', aid: 64, net: 'tcp', type: 'none', host: '', path: '', tls: '' }),
    vmess_grpc: vmess({ v: '2', ps: 'TW gRPC', add: 'tw.example.com', port: '443', id: 'b831381d-6324-4d53-ad4f-8cda48b30811', aid: '0', net: 'grpc', path: 'mygrpc', tls: 'tls', sni: 'tw.example.com' }),
    vmess_h2: vmess({ v: '2', ps: 'KR h2', add: 'kr.example.com', port: '443', id: 'b831381d-6324-4d53-ad4f-8cda48b30811', aid: '0', net: 'h2', host: 'kr.example.com', path: '/h2', tls: 'tls' }),
    vmess_http: vmess({ v: '2', ps: 'UK http', add: 'uk.example.com', port: '80', id: 'b831381d-6324-4d53-ad4f-8cda48b30811', aid: '0', net: 'tcp', type: 'http', host: 'a.com,b.com', path: '/p1,/p2', tls: '' }),
    vmess_httpupgrade: vmess({ v: '2', ps: 'httpupgrade', add: 'hu.example.com', port: '443', id: 'b831381d-6324-4d53-ad4f-8cda48b30811', aid: '0', net: 'httpupgrade', host: 'hu.example.com', path: '/up', tls: 'tls' }),
    vless_reality: 'vless://8b3f5e7c-0000-4000-8000-000000000001@reality.example.com:443?encryption=none&flow=xtls-rprx-vision&security=reality&sni=www.microsoft.com&fp=chrome&pbk=SomePublicKey123&sid=abcd1234&type=tcp&headerType=none#%F0%9F%87%BA%F0%9F%87%B8%20US%20Reality',
    vless_ws: 'vless://8b3f5e7c-0000-4000-8000-000000000002@ws.example.com:443?encryption=none&security=tls&sni=ws.example.com&type=ws&host=ws.example.com&path=%2Fvless%3Fed%3D2048#Singapore%20WS',
    vless_grpc: 'vless://8b3f5e7c-0000-4000-8000-000000000003@grpc.example.com:443?security=tls&type=grpc&serviceName=grpcsvc&mode=gun&sni=grpc.example.com#HK%20gRPC',
    vless_none: 'vless://8b3f5e7c-0000-4000-8000-000000000004@plain.example.com:80?security=none&type=tcp#Plain%20VLESS',
    vless_http: 'vless://8b3f5e7c-0000-4000-8000-000000000005@h2.example.com:443?security=tls&type=http&host=h2.example.com&path=%2Fh2&alpn=h2#JP%20h2',
    vless_udp: 'vless://test-uuid@example.com:443?security=tls&sni=example.com&udp=false&allowInsecure=1#TestVless',
    vless_xhttp: 'vless://8b3f5e7c-0000-4000-8000-000000000006@xh.example.com:443?security=tls&type=xhttp&host=xh.example.com&path=%2Fxh&mode=auto#XHTTP',
    trojan_basic: 'trojan://pass@example.com:443?allowInsecure=1&peer=www.apple.com.cn&sni=www.apple.com.cn&type=tcp#JP-03',
    trojan_ws: 'trojan://p%40ss@tws.example.com:443?security=tls&type=ws&host=tws.example.com&path=%2Ftrojan&sni=tws.example.com&alpn=h2%2Chttp%2F1.1#Korea%20Trojan%20WS',
    trojan_grpc: 'trojan://secret@tg.example.com:443?type=grpc&serviceName=tgrpc&sni=tg.example.com#Trojan-gRPC',
    trojan_none: 'trojan://pass@example.com:443?security=none#no-tls',
    hy2_basic: 'hysteria2://letmein@hy2.example.com:443?sni=hy2.example.com&insecure=1&obfs=salamander&obfs-password=obfspw#%E7%BE%8E%E5%9B%BD%20HY2',
    hy2_short: 'hy2://pw@hy.example.com:8443/?sni=hy.example.com&alpn=h3&up=50&down=100#Canada%20hy2',
    hy2_hop: 'hysteria2://pass@example.com:443?mport=20000-30000&hop_interval=30&sni=example.com#hop',
    hy2_ports: 'hysteria2://pass@example.com:443?ports=20000-30000,40000&hop-interval=15#hop2',
    tuic_basic: 'tuic://8b3f5e7c-0000-4000-8000-000000000007:tuicpass@tuic.example.com:443?congestion_control=bbr&alpn=h3&sni=tuic.example.com&udp_relay_mode=native&allow_insecure=1#France%20TUIC',
    tuic_min: 'tuic://uuid-1:pw@[2001:db8::2]:8443#TUIC-v6',
    anytls_full: 'anytls://p%40ss@example.com:8443/?sni=any.example.com&insecure=1&alpn=h2,http%2F1.1&fp=chrome&udp=true&idle-session-check-interval=30&idle_session_timeout=120&min-idle-session=5#ANYTLS%20main',
    anytls_v6: 'anytls://secret@[2409:8a71:6a00:1953::615]:8964/?insecure=1#IPv6%20node',
};

// ---------------------------------------------------------------------------
// Remote subscriptions served by the mock fetch
// ---------------------------------------------------------------------------
const CLASH_SUB = `
mixed-port: 7890
allow-lan: false
mode: rule
dns:
  enable: true
  nameserver:
    - 223.5.5.5
proxies:
  - name: "HK Clash 01"
    type: ss
    server: hk.clash.example.com
    port: 8388
    cipher: aes-256-gcm
    password: "clashpw"
    udp: true
  - name: US Clash VMess
    type: vmess
    server: us.clash.example.com
    port: 443
    uuid: b831381d-6324-4d53-ad4f-8cda48b30811
    alterId: 0
    cipher: auto
    tls: true
    servername: us.clash.example.com
    network: ws
    ws-opts:
      path: /clash
      headers:
        Host: us.clash.example.com
  - name: JP Trojan
    type: trojan
    server: jp.clash.example.com
    port: 443
    password: trojanpw
    sni: jp.clash.example.com
    skip-cert-verify: true
  - name: SG Hy2
    type: hysteria2
    server: sg.clash.example.com
    port: 443
    password: hy2pw
    sni: sg.clash.example.com
    obfs: salamander
    obfs-password: x
  - name: DE VLESS Reality
    type: vless
    server: de.clash.example.com
    port: 443
    uuid: 8b3f5e7c-0000-4000-8000-000000000010
    network: tcp
    tls: true
    flow: xtls-rprx-vision
    servername: www.apple.com
    client-fingerprint: chrome
    reality-opts:
      public-key: PUBKEY
      short-id: "01"
  - name: TW TUIC
    type: tuic
    server: tw.clash.example.com
    port: 443
    uuid: 8b3f5e7c-0000-4000-8000-000000000011
    password: tuicpw
    alpn: [h3]
    congestion-controller: bbr
proxy-groups:
  - name: My Group
    type: select
    proxies:
      - HK Clash 01
      - US Clash VMess
      - DIRECT
  - name: Auto Test
    type: url-test
    proxies:
      - JP Trojan
      - SG Hy2
    url: http://www.gstatic.com/generate_204
    interval: 300
rules:
  - DOMAIN-SUFFIX,example.org,My Group
  - MATCH,My Group
`;

const SINGBOX_SUB = JSON.stringify({
    log: { level: 'warn' },
    dns: { servers: [{ tag: 'remote', address: 'tls://8.8.8.8' }] },
    outbounds: [
        { type: 'shadowsocks', tag: 'SB HK SS', server: 'hk.sb.example.com', server_port: 8388, method: 'aes-128-gcm', password: 'sbpw' },
        { type: 'vless', tag: 'SB US VLESS', server: 'us.sb.example.com', server_port: 443, uuid: '8b3f5e7c-0000-4000-8000-000000000020', flow: 'xtls-rprx-vision', tls: { enabled: true, server_name: 'us.sb.example.com', utls: { enabled: true, fingerprint: 'chrome' } } },
        { type: 'trojan', tag: 'SB JP Trojan', server: 'jp.sb.example.com', server_port: 443, password: 'pw', tls: { enabled: true }, transport: { type: 'ws', path: '/t' } },
        { type: 'hysteria2', tag: 'SB SG HY2', server: 'sg.sb.example.com', server_port: 443, password: 'pw', tls: { enabled: true, server_name: 'sg.sb.example.com' } },
        { type: 'selector', tag: 'proxy', outbounds: ['SB HK SS', 'SB US VLESS'] },
        { type: 'direct', tag: 'direct' },
        { type: 'block', tag: 'block' },
        { type: 'dns', tag: 'dns-out' },
    ],
    route: { rules: [{ protocol: 'dns', outbound: 'dns-out' }], final: 'proxy' },
}, null, 2);

const SURGE_SUB = `[General]
loglevel = notify
dns-server = 223.5.5.5, 114.114.114.114

[Proxy]
HK Surge SS = ss, hk.surge.example.com, 8388, encrypt-method=aes-128-gcm, password=sspw, udp-relay=true
US Surge VMess = vmess, us.surge.example.com, 443, username=b831381d-6324-4d53-ad4f-8cda48b30811, ws=true, ws-path=/ws, ws-headers=Host:us.surge.example.com, tls=true, sni=us.surge.example.com
JP Surge Trojan = trojan, jp.surge.example.com, 443, password=trojanpw, sni=jp.surge.example.com, skip-cert-verify=true
SG Surge Hy2 = hysteria2, sg.surge.example.com, 443, password=hy2pw, sni=sg.surge.example.com
TW Surge TUIC = tuic, tw.surge.example.com, 443, token=tok, uuid=8b3f5e7c-0000-4000-8000-000000000030, password=pw, alpn=h3
Snell Node = snell, snell.example.com, 443, psk=abc, version=4

[Proxy Group]
Proxy = select, HK Surge SS, US Surge VMess, DIRECT
Auto = url-test, JP Surge Trojan, SG Surge Hy2, url=http://www.gstatic.com/generate_204, interval=300

[Rule]
DOMAIN-SUFFIX,example.org,Proxy
FINAL,Proxy
`;

const PLAIN_LIST = [P.ss_sip002, P.vmess_ws_tls, P.trojan_basic, P.hy2_basic, P.vless_reality].join('\n');

const MOCKS = {
    'https://sub.example.com/base64': { body: b64([P.ss_sip002_pad, P.vless_ws, P.tuic_basic, P.anytls_full].join('\n')), headers: { 'subscription-userinfo': 'upload=1024; download=2048; total=1073741824; expire=1767225600' } },
    'https://sub.example.com/plain': { body: PLAIN_LIST },
    'https://sub.example.com/clash': { body: CLASH_SUB, headers: { 'content-type': 'text/yaml', 'Subscription-Userinfo': 'upload=1; download=2; total=3' } },
    'https://sub.example.com/singbox': { body: SINGBOX_SUB, headers: { 'content-type': 'application/json' } },
    'https://sub.example.com/surge': { body: SURGE_SUB },
    'https://sub.example.com/base64-clash': { body: b64(CLASH_SUB) },
    'https://sub.example.com/urlencoded': { body: encodeURIComponent(PLAIN_LIST) },
    'https://sub.example.com/notfound': { status: 404, body: 'not found' },
    'https://sub.example.com/forbidden': { status: 403, body: 'forbidden' },
    'https://sub.example.com/bad-items': { body: [P.ss_sip002, 'vmess://not-base64-json', 'ss://@:'].join('\n') },
    'https://sub.example.com/nested-bad': { body: ['https://sub.example.com/notfound', P.ss_noname].join('\n') },
    'https://sub.example.com/empty': { body: '' },
    'https://sub.example.com/garbage': { body: 'this is not a subscription at all' },
    'https://sub.example.com/nested': { body: ['https://sub.example.com/plain', P.ss_noname].join('\n') },
    'https://sub.example.com/crlf': { body: [P.ss_plain, P.hy2_short, P.trojan_ws].join('\r\n') + '\r\n' },
};

const fetchLog = [];
globalThis.fetch = async (input, init = {}) => {
    const url = typeof input === 'string' ? input : input.url;
    const headers = new Headers(init.headers || {});
    fetchLog.push({ url, ua: headers.get('user-agent') });
    const mock = MOCKS[url];
    if (!mock) {
        throw new TypeError('fetch failed');
    }
    return new Response(mock.body, { status: mock.status || 200, headers: mock.headers || {} });
};
console.warn = () => {};
console.error = () => {};
console.log = () => {};

// ---------------------------------------------------------------------------
// Inputs
// ---------------------------------------------------------------------------
const INPUTS = {
    ...Object.fromEntries(Object.entries(P).map(([k, v]) => [k, v])),
    multi_mixed: [P.ss_sip002, P.ss_plain, P.vmess_ws_tls, P.vmess_grpc, P.vless_reality, P.vless_ws, P.trojan_ws, P.hy2_basic, P.tuic_basic, P.anytls_full].join('\n'),
    multi_all: Object.values(P).join('\n'),
    multi_dupes: [P.ss_sip002, P.ss_sip002, P.ss_sip002_pad, P.ss_sip002].join('\n'),
    base64_block: b64([P.ss_sip002, P.vmess_tcp, P.trojan_grpc].join('\n')),
    base64_url: b64url([P.vless_grpc, P.hy2_hop].join('\n')),
    sub_base64: 'https://sub.example.com/base64',
    sub_plain: 'https://sub.example.com/plain',
    sub_clash: 'https://sub.example.com/clash',
    sub_singbox: 'https://sub.example.com/singbox',
    sub_surge: 'https://sub.example.com/surge',
    sub_b64clash: 'https://sub.example.com/base64-clash',
    sub_urlenc: 'https://sub.example.com/urlencoded',
    sub_404: 'https://sub.example.com/notfound',
    sub_empty: 'https://sub.example.com/empty',
    sub_garbage: 'https://sub.example.com/garbage',
    sub_nested: 'https://sub.example.com/nested',
    sub_crlf: 'https://sub.example.com/crlf',
    sub_unreachable: 'https://unreachable.example.com/sub',
    sub_403: 'https://sub.example.com/forbidden',
    sub_bad_items: 'https://sub.example.com/bad-items',
    sub_nested_bad: 'https://sub.example.com/nested-bad',
    sub_mix_fail: [P.ss_sip002, 'https://sub.example.com/notfound', 'https://sub.example.com/base64'].join('\n'),
    sub_b64_fail: b64(['https://sub.example.com/plain', 'https://unreachable.example.com/sub'].join('\n')),
    sub_mix: ['https://sub.example.com/base64', 'https://sub.example.com/clash', P.hy2_hop, 'https://sub.example.com/singbox'].join('\n'),
    inline_clash: CLASH_SUB,
    inline_singbox: SINGBOX_SUB,
    inline_surge: SURGE_SUB,
    garbage: 'hello world',
    unknown_scheme: 'socks5://user:pass@1.2.3.4:1080#socks\n' + P.ss_sip002,
    bad_vmess: 'vmess://not-base64-json',
    bad_ss: 'ss://@:',
    whitespace: '  \n  ' + P.trojan_none + '  \n\n',
};

const SELECTED = {
    minimal: 'minimal',
    balanced: 'balanced',
    comprehensive: 'comprehensive',
    json: JSON.stringify(['Ad Block', 'AI Services', 'Google', 'Telegram', 'Non-China', 'Private', 'Location:CN']),
    invalid: '[not json',
    object: '{"a":1}',
};

const CUSTOM_RULES = JSON.stringify([
    { name: 'MyRule', site: 'openai,anthropic', ip: 'private', domain_suffix: 'example.org,example.net', domain_keyword: 'kw', ip_cidr: '10.0.0.0/8', protocol: 'bittorrent' },
    { name: 'Second', domain_suffix: 'second.example', src_ip_cidr: '192.168.1.0/24' },
    { name: 'Direct', outbound: 'DIRECT', domain_suffix: 'direct.example' },
]);

const cases = [];
const add = (name, steps) => cases.push({ name, steps: Array.isArray(steps) ? steps : [steps] });
const get = (path, extra = {}) => ({ method: 'GET', url: 'http://localhost' + path, ...extra });
const q = (params) => '?' + Object.entries(params).map(([k, v]) => `${k}=${encodeURIComponent(v)}`).join('&');

// Every input through every converter.
for (const [key, input] of Object.entries(INPUTS)) {
    for (const route of ['singbox', 'clash', 'surge', 'xray']) {
        add(`${route}/${key}`, get(`/${route}` + q({ config: input })));
    }
}

// Option matrix on a few representative inputs.
const MATRIX_INPUTS = ['multi_mixed', 'sub_clash', 'sub_singbox', 'sub_surge', 'sub_mix', 'inline_clash'];
const OPTIONS = {
    sel_minimal: { selectedRules: SELECTED.minimal },
    sel_comprehensive: { selectedRules: SELECTED.comprehensive },
    sel_json: { selectedRules: SELECTED.json },
    sel_invalid: { selectedRules: SELECTED.invalid },
    sel_object: { selectedRules: SELECTED.object },
    custom: { selectedRules: SELECTED.balanced, customRules: CUSTOM_RULES },
    custom_only: { customRules: CUSTOM_RULES },
    country: { group_by_country: 'true' },
    country_noauto: { group_by_country: 'true', include_auto_select: 'false' },
    noauto: { include_auto_select: 'false' },
    nodns: { include_clash_dns: 'false' },
    nodns_zero: { include_clash_dns: '0' },
    ui: { enable_clash_ui: 'true', external_controller: '0.0.0.0:9090', external_ui_download_url: 'https://example.com/ui.zip' },
    ui_default: { enable_clash_ui: 'true' },
    lang_en: { lang: 'en-US' },
    lang_fa: { lang: 'fa' },
    lang_ru: { lang: 'ru' },
    lang_xx: { lang: 'xx' },
    ua_param: { ua: 'clash.meta' },
    everything: { selectedRules: SELECTED.comprehensive, customRules: CUSTOM_RULES, group_by_country: 'true', enable_clash_ui: 'true', lang: 'en-US' },
};
for (const key of MATRIX_INPUTS) {
    for (const [opt, params] of Object.entries(OPTIONS)) {
        for (const route of ['singbox', 'clash', 'surge']) {
            add(`${route}/${key}/${opt}`, get(`/${route}` + q({ config: INPUTS[key], ...params })));
        }
    }
}

// sing-box version selection.
for (const [name, params, headers] of [
    ['v1.11', { singbox_version: '1.11' }],
    ['legacy', { sb_version: 'legacy' }],
    ['latest', { sb_ver: 'latest' }],
    ['v1.13', { singbox_version: '1.13.2' }],
    ['v1.14', { singbox_version: '1.14' }],
    ['v2', { singbox_version: 'v2.0' }],
    ['auto_ua111', { singbox_version: 'auto' }, { 'User-Agent': 'SFI/1.12.2 (Build 2; sing-box 1.11.4; language zh_CN)' }],
    ['ua112', {}, { 'User-Agent': 'SFA/1.12.12 (587; sing-box 1.12.12; language zh_Hans_CN)' }],
    ['ua114', {}, { 'User-Agent': 'sing-box/1.14.0' }],
    ['junk', { singbox_version: 'junk' }],
]) {
    for (const key of ['multi_mixed', 'sub_mix']) {
        add(`singbox/version/${name}/${key}`, get('/singbox' + q({ config: INPUTS[key], ...params }), { headers }));
    }
    add(`singbox/version/${name}/ui`, get('/singbox' + q({ config: INPUTS.multi_mixed, enable_clash_ui: 'true', ...params }), { headers }));
}

// Headers.
add('clash/accept-language', get('/clash' + q({ config: INPUTS.multi_mixed }), { headers: { 'Accept-Language': 'en-US,en;q=0.9' } }));
add('singbox/accept-language-fa', get('/singbox' + q({ config: INPUTS.multi_mixed }), { headers: { 'Accept-Language': 'fa-IR' } }));
add('surge/user-agent', get('/surge' + q({ config: INPUTS.sub_base64 }), { headers: { 'User-Agent': 'Surge iOS/2920' } }));
add('xray/user-agent', get('/xray' + q({ config: INPUTS.sub_base64 }), { headers: { 'User-Agent': 'v2rayN/6.0' } }));

// Missing / empty config.
for (const route of ['singbox', 'clash', 'surge', 'xray']) {
    add(`${route}/missing`, get(`/${route}`));
    add(`${route}/empty`, get(`/${route}?config=`));
    add(`${route}/head`, get(`/${route}` + q({ config: P.ss_sip002 }), { method: 'HEAD' }));
}

// Query-string corner cases.
add('singbox/plus-space', get('/singbox?config=' + P.ss_sip002.replace('#', '%23') + '&lang=en+US'));
add('clash/raw-hash', get('/clash?config=' + P.ss_sip002));
add('clash/repeated-config', get('/clash' + q({ config: P.ss_sip002 }) + '&config=' + encodeURIComponent(P.trojan_basic)));
add('clash/encoded-key', get('/clash?con%66ig=' + encodeURIComponent(P.ss_sip002)));
add('clash/bad-percent', get('/clash?config=' + encodeURIComponent(P.ss_sip002) + '%ZZ'));
add('surge/flag-no-value', get('/surge' + q({ config: P.ss_sip002 }) + '&group_by_country'));

// Subconverter.
for (const [name, params] of [
    ['default', {}],
    ['minimal', { selectedRules: 'minimal' }],
    ['comprehensive', { selectedRules: 'comprehensive' }],
    ['typo', { selectedRules: 'balancde' }],
    ['json', { selectedRules: SELECTED.json }],
    ['object', { selectedRules: '{"a":1}' }],
    ['country', { selectedRules: 'minimal', group_by_country: 'true' }],
    ['country-noauto', { selectedRules: 'minimal', group_by_country: 'true', include_auto_select: 'false' }],
    ['noauto', { selectedRules: 'minimal', include_auto_select: 'false' }],
    ['en', { selectedRules: 'minimal', lang: 'en' }],
    ['custom', { selectedRules: 'balanced', customRules: CUSTOM_RULES }],
    ['ru', { lang: 'ru', group_by_country: 'true' }],
]) {
    add(`subconverter/${name}`, get('/subconverter' + q(params)));
}

// Short links.
const shortUrl = 'http://localhost/clash' + q({ config: P.ss_sip002, lang: 'en-US' });
add('short/roundtrip', [
    get('/shorten-v2' + q({ url: shortUrl, shortCode: 'abc123' })),
    get('/c/abc123'),
    get('/b/abc123'),
    get('/s/abc123'),
    get('/x/abc123'),
    get('/resolve' + q({ url: 'https://other.example.com/c/abc123' })),
    get('/resolve' + q({ url: 'https://other.example.com/z/abc123' })),
    get('/resolve' + q({ url: 'https://other.example.com/c' })),
    get('/resolve' + q({ url: 'https://other.example.com/c/missing' })),
    get('/resolve' + q({ url: 'not a url' })),
    get('/resolve'),
    get('/resolve' + q({ url: 'https://other.example.com/c/missing', lang: 'en-US' })),
    get('/c/missing'),
    get('/c/'),
    get('/c/abc123/extra'),
]);
add('short/random-code', [
    { ...get('/shorten-v2' + q({ url: 'http://localhost/singbox?config=x' })), capture: true },
    get('/b/{{0}}'),
]);
add('short/no-query', [get('/shorten-v2' + q({ url: 'http://localhost/clash', shortCode: 'empty' })), get('/c/empty')]);
add('short/unicode', [
    get('/shorten-v2' + q({ url: 'http://localhost/clash?config=节点 名称', shortCode: '中文' })),
    get('/c/' + encodeURIComponent('中文')),
    get('/resolve' + q({ url: 'http://h/c/' + encodeURIComponent('中文') })),
]);
add('short/errors', [get('/shorten-v2'), get('/shorten-v2?url=not-a-url'), get('/shorten-v2?url=')]);

// Stored configs.
const CLASH_BASE = `port: 7891\nmode: rule\nproxy-groups:\n  - name: Custom\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,DIRECT\n`;
const post = (body) => ({ method: 'POST', url: 'http://localhost/config', body });
add('config/clash-yaml', [
    { ...post(JSON.stringify({ type: 'clash', content: CLASH_BASE })), capture: true },
    get('/clash?config=' + encodeURIComponent(PLAIN_LIST) + '&configId={{0}}'),
    get('/singbox?config=' + encodeURIComponent(PLAIN_LIST) + '&configId={{0}}'),
]);
add('config/clash-object', [
    { ...post(JSON.stringify({ type: 'clash', content: { 'mixed-port': 1234, dns: { enable: false } } })), capture: true },
    get('/clash?config=' + encodeURIComponent(INPUTS.multi_mixed) + '&configId={{0}}&group_by_country=true'),
]);
add('config/singbox', [
    { ...post(JSON.stringify({ type: 'singbox', content: { log: { level: 'debug' }, dns: { servers: [{ tag: 'x', address: '1.1.1.1' }] }, outbounds: [], route: { rules: [] } } })), capture: true },
    get('/singbox?config=' + encodeURIComponent(PLAIN_LIST) + '&configId={{0}}'),
]);
add('config/singbox-string', [
    { ...post(JSON.stringify({ type: 'singbox', content: JSON.stringify({ log: { level: 'info' } }) })), capture: true },
    get('/singbox?config=' + encodeURIComponent(PLAIN_LIST) + '&configId={{0}}'),
]);
add('config/clash-object-nodns', [
    { ...post(JSON.stringify({ type: 'clash', content: { 'mixed-port': 1234, dns: { enable: false } } })), capture: true },
    get('/clash?config=' + encodeURIComponent(INPUTS.multi_mixed) + '&configId={{0}}&include_clash_dns=false'),
    get('/clash?config=' + encodeURIComponent(INPUTS.sub_clash) + '&configId={{0}}&include_clash_dns=false'),
]);
// sing-box 1.14 keeps the rule-set download detour target non-empty.
const SB114_BASES = {
    plain: { log: { level: 'debug' }, dns: { servers: [{ tag: 'x', address: '1.1.1.1' }] }, route: { rules: [] } },
    no_dns: { route: { rules: [] } },
    no_direct: { outbounds: [], dns: { servers: [{ tag: 'local', type: 'udp' }] }, route: { rules: [] } },
    direct_not_direct: { outbounds: [{ type: 'block', tag: 'DIRECT' }], dns: { servers: [{ tag: 'local', type: 'udp' }] }, route: { rules: [] } },
    outbounds_object: { outbounds: { tag: 'DIRECT' }, route: { rules: [] } },
    mixed_dns: { dns: { servers: [{ tag: 'fake', type: 'fakeip' }, { tag: 'remote', type: 'https', server: '1.1.1.1', detour: 'Proxy' }, { tag: 'doh', type: 'https', server: '223.5.5.5' }, { tag: 'local', type: 'udp', server: '223.5.5.5' }, { type: 'udp', server: '8.8.8.8' }] }, route: { rules: [] } },
    only_fakeip: { dns: { servers: [{ tag: 'fake', type: 'fakeip' }, { tag: 'remote', type: 'udp', detour: 'Proxy' }] }, route: { rules: [] } },
    resolver_string: { dns: { servers: [{ tag: 'local', type: 'udp' }] }, route: { rules: [], default_domain_resolver: 'custom' } },
    resolver_object: { dns: { servers: [{ tag: 'local', type: 'udp' }] }, route: { rules: [], default_domain_resolver: { server: 'local' } } },
    resolver_empty: { dns: { servers: [{ tag: 'local', type: 'udp' }] }, route: { rules: [], default_domain_resolver: '' } },
    own_client: { http_clients: [{ tag: 'mine', detour: 'DIRECT' }], dns: { servers: [{ tag: 'local', type: 'udp' }] }, route: { rules: [] } },
    own_client_nodetour: { http_clients: [{ tag: 'mine' }], dns: { servers: [{ tag: 'local', type: 'udp' }] }, route: { rules: [] } },
    own_default_client: { http_clients: [{ tag: 'a', detour: 'DIRECT' }, { tag: 'b', detour: 'DIRECT' }], dns: { servers: [{ tag: 'local', type: 'udp' }] }, route: { rules: [], default_http_client: 'b' } },
    missing_default_client: { dns: { servers: [{ tag: 'local', type: 'udp' }] }, route: { rules: [], default_http_client: 'ghost' } },
    direct_with_dial: { outbounds: [{ type: 'direct', tag: 'DIRECT', bind_interface: 'eth0' }], dns: { servers: [{ tag: 'local', type: 'udp' }] }, route: { rules: [] } },
    detour_selector: { http_clients: [{ tag: 'via-proxy', detour: '🚀 节点选择' }], dns: { servers: [{ tag: 'local', type: 'udp' }] }, route: { rules: [] } },
    null_client: { http_clients: [null, { tag: 'second', detour: 'DIRECT' }], route: { rules: [] } },
    client_object: { http_clients: { tag: 'x' }, route: { rules: [], default_http_client: 'x' } },
    client_strings: { http_clients: ['x', 5], route: { rules: [], default_http_client: 'x' } },
    numeric_tags: { http_clients: [{ tag: 5, detour: 'DIRECT' }], dns: { servers: [{ tag: 'first', type: 'udp' }, { tag: 7, type: 'https' }] }, route: { rules: [], default_http_client: 5 } },
    servers_object: { dns: { servers: { tag: 'x' } }, route: { rules: [] } },
    servers_string: { dns: { servers: 'x' }, route: { rules: [] } },
};
const DIRECT_OUT = [{ type: 'direct', tag: 'DIRECT' }];
for (const [name, base] of Object.entries(SB114_BASES)) {
    const content = { outbounds: DIRECT_OUT, ...base };
    add(`config/singbox-114/${name}`, [
        { ...post(JSON.stringify({ type: 'singbox', content })), capture: true },
        get('/singbox?config=' + encodeURIComponent(P.ss_sip002) + '&configId={{0}}&singbox_version=1.14'),
        get('/singbox?config=' + encodeURIComponent(P.ss_sip002) + '&configId={{0}}&singbox_version=1.12'),
    ]);
}
add('config/surge', [
    { ...post(JSON.stringify({ type: 'surge', content: { general: { loglevel: 'verbose' } } })), capture: true },
    get('/surge?config=' + encodeURIComponent(PLAIN_LIST) + '&configId={{0}}'),
]);
add('config/surge-null', [
    { ...post(JSON.stringify({ type: 'surge', content: null })), capture: true },
    get('/surge?config=' + encodeURIComponent(PLAIN_LIST) + '&configId={{0}}'),
]);
add('config/clash-null', [
    { ...post(JSON.stringify({ type: 'clash', content: 'null' })), capture: true },
    get('/clash?config=' + encodeURIComponent(PLAIN_LIST) + '&configId={{0}}'),
]);
add('config/errors', [
    post('not json'),
    post(''),
    post('null'),
    post('5'),
    post(JSON.stringify({ content: {} })),
    post(JSON.stringify({ type: 'singbox', content: 5 })),
    post(JSON.stringify({ type: 'singbox', content: 'not json' })),
    post(JSON.stringify({ type: 'singbox' })),
    post(JSON.stringify({ type: 'clash', content: 'key: [unclosed' })),
    post(JSON.stringify({ type: 'clash', content: '# only: comment' })),
    post(JSON.stringify({ type: 'clash', content: 'plain' })),
    post(JSON.stringify({ type: 'clash', content: 42 })),
    get('/clash?config=x&configId=clash_missing'),
    get('/singbox?config=' + encodeURIComponent(P.ss_sip002) + '&configId=singbox_missing'),
]);
add('config/date-yaml', [
    { ...post(JSON.stringify({ type: 'clash', content: 'created: 2024-01-02\nport: 1' })), capture: true },
    get('/clash?config=' + encodeURIComponent(P.ss_sip002) + '&configId={{0}}'),
]);

// Misc routes.
add('misc/404', get('/unknown-path'));
add('misc/trailing-slash', get('/singbox/' + q({ config: P.ss_sip002 })));
add('misc/post-singbox', { method: 'POST', url: 'http://localhost/singbox' + q({ config: P.ss_sip002 }) });
add('misc/get-config', get('/config'));
add('misc/encoded-path', get('/sing%62ox' + q({ config: P.ss_sip002 })));
add('misc/favicon', get('/favicon.ico'));

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------
const KEEP_HEADERS = ['content-type', 'subscription-userinfo', 'location', 'cache-control'];

async function run() {
    const out = [];
    for (const c of cases) {
        const app = createApp({
            kv: new MemoryKVAdapter(),
            assetFetcher: async () => new Response(readFileSync('public/favicon.ico'), { headers: { 'Content-Type': 'image/x-icon', 'Cache-Control': 'public, max-age=86400' } }),
            logger: console,
            config: { configTtlSeconds: 60, shortLinkTtlSeconds: null },
        });
        const vars = [];
        const steps = [];
        for (const step of c.steps) {
            const fill = (s) => s.replace(/\{\{(\d+)\}\}/g, (_, i) => vars[Number(i)]);
            const url = fill(step.url);
            const init = { method: step.method, headers: step.headers || {} };
            if (step.body !== undefined) init.body = step.body;
            const res = await app.request(url, init);
            const buf = Buffer.from(await res.arrayBuffer());
            const isText = !(res.headers.get('content-type') || '').startsWith('image/');
            const body = isText ? buf.toString('utf8') : buf.toString('base64');
            // Generated IDs are random: capture them and refer to them by placeholder.
            const capture = step.capture || (step.method === 'POST' && res.status === 200);
            const mask = (s) => vars.reduce((acc, v, i) => acc.split(v).join(`{{${i}}}`), s);
            const headers = {};
            for (const h of KEEP_HEADERS) {
                const v = res.headers.get(h);
                if (v !== null) headers[h] = mask(v);
            }
            const expectedBody = capture ? null : (isText ? mask(body) : body);
            if (capture) vars.push(body);
            steps.push({
                method: step.method,
                url: step.url,
                headers: step.headers || {},
                body: step.body,
                capture,
                expect: { status: res.status, headers, body: expectedBody, binary: !isText },
            });
        }
        out.push({ name: c.name, steps });
    }
    return out;
}

const result = await run();
const json = JSON.stringify({ mocks: Object.entries(MOCKS).map(([url, m]) => ({ url, status: m.status || 200, headers: m.headers || {}, body: m.body })), cases: result });
writeFileSync(process.argv[2], gzipSync(json, { level: 9 }));
process.stderr.write(`cases=${result.length} raw=${json.length} gz=${gzipSync(json, { level: 9 }).length}\n`);
