// Drives the original Node app and the Rust app through the same UI flows in
// Chromium and diffs everything a user could observe.
import { chromium } from 'playwright-core';
import fs from 'node:fs';
import { PNG } from 'pngjs';

const NM = new URL('./node_modules/', import.meta.url).pathname;
const read = (p) => fs.readFileSync(NM + p);

const STATIC = [
    ['https://cdn.jsdelivr.net/npm/alpinejs@3.13.10/dist/cdn.min.js', 'application/javascript', read('alpinejs/dist/cdn.min.js')],
    ['https://cdn.jsdelivr.net/npm/js-yaml@4.1.0/dist/js-yaml.min.js', 'application/javascript', read('js-yaml/dist/js-yaml.min.js')],
    ['https://cdn.jsdelivr.net/npm/qrcode-generator@1.4.4/qrcode.min.js', 'application/javascript', read('qrcode-generator/qrcode.js')],
    ['https://cdnjs.cloudflare.com/ajax/libs/font-awesome/6.4.0/css/all.min.css', 'text/css', read('@fortawesome/fontawesome-free/css/all.min.css')],
    // The Tailwind play CDN is not on npm; both apps get the same stub.
    ['https://cdn.tailwindcss.com', 'application/javascript', 'window.tailwind = {};'],
    ['https://fonts.googleapis.com/', 'text/css', ''],
    ['https://api.github.com/repos/7Sageer/sublink-worker/releases/latest', 'application/json', '{"tag_name":"v9.9.9"}'],
];

const PROXIES = [
    'ss://YWVzLTEyOC1nY206dGVzdA@hk1.example.com:443#HK-Node-1',
    'trojan://pass@jp.example.com:443?sni=jp.example.com#JP-Trojan',
    'vless://8b3f5e7c-0000-4000-8000-000000000001@us.example.com:443?security=tls&sni=us.example.com&type=ws&path=%2Fws#US-VLESS',
].join('\n');

const CLASH_BASE = 'port: 7890\nmode: rule\nproxy-groups:\n  - name: Custom\n    type: select\n    proxies: [DIRECT]\n';
const SURGE_INI = '[General]\nloglevel = notify\n\n[Proxy]\nHK = ss, hk.example.com, 443, encrypt-method=aes-128-gcm, password=x\n\n[Rule]\nFINAL,DIRECT\n';

const maskIds = (s) => s.replace(/\b(clash|singbox|surge)_[A-Za-z0-9]{8}\b/g, '$1_<ID>');

async function setupRoutes(context, base) {
    await context.route('**/*', async (route) => {
        const url = route.request().url();
        if (url.startsWith(base)) return route.continue();
        const hit = STATIC.find(([prefix]) => url.startsWith(prefix));
        if (hit) return route.fulfill({ status: 200, contentType: hit[1], body: hit[2] });
        const font = url.match(/font-awesome\/6\.4\.0\/webfonts\/([^?#]+)/);
        if (font) return route.fulfill({ status: 200, body: read('@fortawesome/fontawesome-free/webfonts/' + font[1]) });
        return route.abort();
    });
}

async function run(base) {
    const norm = (s) => maskIds(String(s).split(base).join('ORIGIN'));
    const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH });
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 }, locale: 'zh-CN', timezoneId: 'UTC' });
    await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: base });
    await setupRoutes(context, base);
    const page = await context.newPage();
    const events = [];
    page.on('console', (m) => { if (['error', 'warning'].includes(m.type())) events.push(`console.${m.type()}: ${norm(m.text())}`); });
    page.on('pageerror', (e) => events.push('pageerror: ' + norm(e.message)));
    page.on('dialog', async (d) => { events.push(`dialog.${d.type()}: ${norm(d.message())}`); await d.accept(); });

    const obs = {};
    const ready = async () => { await page.waitForFunction(() => window.__alpineLoaded === true); await page.waitForTimeout(400); };
    const dom = () => page.evaluate(() => {
        const clone = document.documentElement.cloneNode(true);
        for (const s of clone.querySelectorAll('script')) {
            // The form bootstrap script is the one place the source text legitimately differs.
            if (s.textContent.includes('window.APP_TRANSLATIONS')) s.textContent = '[form script]';
        }
        return clone.outerHTML;
    });
    const state = () => page.evaluate(() => {
        const data = Alpine.$data(document.querySelector('[x-data="formData()"]'));
        const rules = Alpine.$data(document.querySelector('[x-data="customRulesData()"]'));
        const pick = (o, keys) => Object.fromEntries(keys.map((k) => [k, o[k]]));
        return JSON.stringify({
            form: pick(data, ['input', 'showAdvanced', 'selectedRules', 'selectedPredefinedRule', 'groupByCountry', 'includeAutoSelect', 'includeClashDns',
                'enableClashUI', 'externalController', 'externalUiDownloadUrl', 'customUA', 'configType', 'configEditor',
                'configValidationState', 'configValidationMessage', 'currentConfigId', 'generatedLinks', 'shortenedLinks', 'customShortCode']),
            rules: pick(rules, ['mode', 'rules', 'jsonContent', 'jsonError', 'jsonValid']),
            url: location.pathname + location.search,
        });
    });
    const shot = () => page.screenshot({ fullPage: true });
    const linkValues = () => page.$$eval('input[readonly]', (els) => els.map((e) => e.value));
    const click = (sel) => page.locator(sel).first().click();
    const at = (label, value) => { obs[label] = typeof value === 'string' ? norm(value) : value; };

    // Static rendering in every language, including the update toast after its 3 s delay.
    for (const lang of ['zh-CN', 'en-US', 'fa', 'ru']) {
        await page.goto(`${base}/?lang=${lang}`);
        await ready();
        await page.waitForTimeout(3500);
        at(`${lang}:dom`, await dom());
        at(`${lang}:text`, await page.evaluate(() => document.body.innerText));
        at(`${lang}:screenshot`, await shot());
    }

    // Full conversion flow.
    await page.goto(`${base}/?lang=zh-CN`);
    await ready();
    await page.fill('#input', PROXIES);
    await click('[x-on\\:click="showAdvanced = !showAdvanced"]');
    await page.selectOption('select[x-model="selectedPredefinedRule"]', 'comprehensive');
    await page.locator('input[type=checkbox][value="Bilibili"]').click();
    await page.locator('button[x-on\\:click="addRule()"]').last().click();
    await page.fill('input[x-model="rule.name"]', 'MyRule');
    await page.fill('input[x-model="rule.domain_suffix"]', 'example.org,example.net');
    await page.fill('input[x-model="rule.src_ip_cidr"]', '192.168.1.13/32');
    await page.locator('input[x-model="groupByCountry"]').evaluate((el) => el.click());
    await page.locator('input[x-model="includeClashDns"]').evaluate((el) => el.click());
    await page.locator('input[x-model="enableClashUI"]').evaluate((el) => el.click());
    await page.fill('input[x-model="externalController"]', '0.0.0.0:9090');
    await page.fill('input[x-model="customUA"]', 'clash.meta');
    await page.waitForTimeout(300);
    at('advanced:screenshot', await shot());
    at('subconverter-url', await page.locator('p[x-text="getSubconverterUrl()"]').innerText());
    await click('button[type=submit]');
    await page.waitForTimeout(800);
    const links = await linkValues();
    at('links', JSON.stringify(links));
    at('after-convert:state', await state());
    at('after-convert:screenshot', await shot());
    for (const [i, link] of links.entries()) {
        const res = await fetch(link);
        at(`link${i}:response`, `${res.status} ${res.headers.get('content-type')}\n${await res.text()}`);
    }

    await page.fill('input[x-model="customShortCode"]', 'e2e-code');
    await click('button[x-on\\:click="shortenedLinks ? shortenedLinks = null : shortenLinks()"]');
    await page.waitForTimeout(800);
    const shortLinks = await linkValues();
    at('short-links', JSON.stringify(shortLinks));
    for (const [i, link] of shortLinks.entries()) {
        const res = await fetch(link, { redirect: 'manual' });
        at(`short${i}:redirect`, `${res.status} ${res.headers.get('location')}`);
    }
    await click('button[x-on\\:click="shortenedLinks ? shortenedLinks = null : shortenLinks()"]');
    await page.waitForTimeout(300);
    at('after-show-full:state', await state());

    // Base config editor: validation, saving and regenerating with the stored config.
    await page.selectOption('select[x-model="configType"]', 'clash');
    await page.fill('#configEditor', CLASH_BASE);
    await click('button[x-on\\:click="validateBaseConfig()"]');
    await page.waitForTimeout(200);
    at('clash-validate:state', await state());
    await click('button[x-on\\:click="saveBaseConfig()"]');
    await page.waitForTimeout(800);
    at('clash-save:state', await state());
    await click('button[type=submit]');
    await page.waitForTimeout(500);
    const withConfig = await linkValues();
    at('links-with-config', JSON.stringify(withConfig));
    const clashRes = await fetch(withConfig[2]);
    at('clash-with-config:response', `${clashRes.status}\n${await clashRes.text()}`);

    await page.selectOption('select[x-model="configType"]', 'surge');
    await page.fill('#configEditor', SURGE_INI);
    await click('button[x-on\\:click="validateBaseConfig()"]');
    await page.waitForTimeout(200);
    at('surge-validate:state', await state());
    await click('button[x-on\\:click="saveBaseConfig()"]');
    await page.waitForTimeout(800);
    at('surge-save:state', await state());

    await page.selectOption('select[x-model="configType"]', 'singbox');
    await page.fill('#configEditor', '{"log": {"level": "info",}}');
    await click('button[x-on\\:click="validateBaseConfig()"]');
    await page.waitForTimeout(200);
    at('singbox-invalid:state', await state());
    await click('button[x-on\\:click="saveBaseConfig()"]');
    await page.waitForTimeout(500);
    at('base-config:screenshot', await shot());

    // Custom rules JSON mode.
    await click('button[x-on\\:click="mode = \'json\'"]');
    await page.fill('#customRulesJson', '{"not": "an array"}');
    await click('button[x-on\\:click="validateJson()"]');
    await page.waitForTimeout(200);
    at('rules-json-invalid:state', await state());
    await page.fill('#customRulesJson', '[{"name": "Other", "ip_cidr": "10.0.0.0/8"}]');
    await click('button[x-on\\:click="validateJson()"]');
    await page.waitForTimeout(200);
    at('rules-json-valid:state', await state());
    at('rules-json:screenshot', await shot());

    // Pasting a generated link and a short link restores the form.
    await page.goto(`${base}/?lang=en-US`);
    await ready();
    await page.fill('#input', links[2]);
    await page.waitForTimeout(1200);
    at('paste-full-link:state', await state());
    await page.goto(`${base}/?lang=en-US`);
    await ready();
    await page.fill('#input', shortLinks[1]);
    await page.waitForTimeout(1500);
    at('paste-short-link:state', await state());
    at('paste-short-link:screenshot', await shot());

    // Clearing everything, dark mode, and what persists in localStorage.
    await click('button[x-on\\:click="clearAll()"]:not([x-show])');
    await page.waitForTimeout(300);
    at('clear-all:state', await state());
    await click('button[x-on\\:click="toggleDarkMode()"]');
    await page.waitForTimeout(300);
    at('dark:html-class', await page.evaluate(() => document.documentElement.className));
    at('dark:screenshot', await shot());
    at('localStorage', await page.evaluate(() => JSON.stringify(Object.entries(localStorage).sort()))
        .then((s) => s.replace(/"sublink_last_version_check","\d+"/, '"sublink_last_version_check","<now>"')));

    // Stack positions point into the inline script, whose source text differs by design.
    at('events', events.join('\n').replace(/\?lang=[\w-]+:\d+:\d+\)/g, '?lang=…:<line>:<col>)'));
    await browser.close();
    return obs;
}

function pixelDiff(a, b) {
    const pa = PNG.sync.read(a);
    const pb = PNG.sync.read(b);
    if (pa.width !== pb.width || pa.height !== pb.height) return `size ${pa.width}x${pa.height} vs ${pb.width}x${pb.height}`;
    let diff = 0;
    for (let i = 0; i < pa.data.length; i += 4) {
        if (pa.data[i] !== pb.data[i] || pa.data[i + 1] !== pb.data[i + 1] || pa.data[i + 2] !== pb.data[i + 2]) diff++;
    }
    return diff ? `${diff} pixels differ` : null;
}

const out = new URL('./out/', import.meta.url).pathname;
fs.mkdirSync(out, { recursive: true });
// Capture each app separately on the same origin, so port numbers shown on the page cannot differ.
if (process.argv[2] === 'capture') {
    const obs = await run(process.argv[3]);
    const encoded = Object.fromEntries(Object.entries(obs).map(([k, v]) => [k, Buffer.isBuffer(v) ? { png: v.toString('base64') } : v]));
    fs.writeFileSync(`${out}${process.argv[4]}.json`, JSON.stringify(encoded));
    process.exit(0);
}
const load = (name) => Object.fromEntries(Object.entries(JSON.parse(fs.readFileSync(`${out}${name}.json`, 'utf8')))
    .map(([k, v]) => [k, v && v.png ? Buffer.from(v.png, 'base64') : v]));
const [a, b] = [load('node'), load('rust')];
let failures = 0;
for (const key of Object.keys(a)) {
    let problem = null;
    if (Buffer.isBuffer(a[key])) {
        problem = pixelDiff(a[key], b[key]);
        fs.writeFileSync(`${out}${key.replace(/:/g, '_')}-node.png`, a[key]);
        fs.writeFileSync(`${out}${key.replace(/:/g, '_')}-rust.png`, b[key]);
    } else if (a[key] !== b[key]) {
        const i = [...a[key]].findIndex((c, n) => c !== b[key][n]);
        problem = `differs at ${i}:\n    node: ${JSON.stringify(a[key].slice(Math.max(0, i - 80), i + 120))}\n    rust: ${JSON.stringify(b[key].slice(Math.max(0, i - 80), i + 120))}`;
    }
    console.log(`${problem ? 'DIFF' : 'same'}  ${key}${problem ? '\n  ' + problem : ''}`);
    if (problem) failures++;
}
console.log(`\n${Object.keys(a).length} observations, ${failures} differ`);
console.log('--- events (node) ---\n' + a.events);
