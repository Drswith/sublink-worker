# Reference fixtures

The JSON fixtures next to this directory were captured from the Node.js
implementation at the tip of its `dev` branch — the parity baseline, currently
`2d90c0f`. The Rust tests replay them to prove byte-level parity.

| Fixture | Generator | Checked by |
| --- | --- | --- |
| `js_core.json` | `gen_jscore.mjs` | `tests/js_core.rs` (Number/URI/Base64/trim semantics) |
| `yaml_cases.json` | `gen_yaml.mjs` | `tests/yaml_compat.rs` (js-yaml 4 load/dump) |
| `golden.json.gz` | `gen_golden.mjs` | `tests/golden.rs` (full HTTP responses) |
| `pages.json.gz` | `gen_pages.mjs` | `tests/pages.rs` (server-rendered home page, and the form script source the page masks) |
| `fetch_cases.json` | `gen_fetch.mjs` | `tests/fetch_compat.rs` (Node fetch outcomes for raw upstream responses) |

To regenerate (or to extend the corpus) against the original code:

```sh
git fetch origin dev
git worktree add /tmp/sublink-ref origin/dev   # baseline: latest dev, currently 2d90c0f
cd /tmp/sublink-ref && npm ci
cp <repo>/tests/fixtures/reference/* .
node_modules/.bin/esbuild golden-entry.js --bundle --platform=node --format=esm --outfile=golden-bundle.mjs
node gen_golden.mjs <repo>/tests/fixtures/golden.json.gz
node gen_pages.mjs  <repo>/tests/fixtures/pages.json.gz
node gen_jscore.mjs > <repo>/tests/fixtures/js_core.json
node gen_yaml.mjs   > <repo>/tests/fixtures/yaml_cases.json
node gen_fetch.mjs  > <repo>/tests/fixtures/fetch_cases.json   # depends only on the Node version (undici)
```

## Browser UI comparison

`ui_compare.mjs` drives the same UI flows (every language, conversion with
presets and custom rules, short links, base-config validation/saving, custom
rule JSON mode, link pasting, clear-all, dark mode, update toast) in Chromium
against both apps and diffs DOM, visible text, full-page screenshots (pixel by
pixel), dialogs, `localStorage` and the responses of the generated links. CDN
assets are served from the same npm package versions so both runs see
identical inputs.

```sh
npm i playwright-core@1.56 alpinejs@3.13.10 js-yaml@4.1.0 qrcode-generator@1.4.4 @fortawesome/fontawesome-free@6.4.0 pngjs
PORT=38580 node dist/node-server.cjs &          # original, from the dev worktree
node ui_compare.mjs capture http://127.0.0.1:38580 node
# stop it, then on the same port:
(cd "$(mktemp -d)" && PORT=38580 <repo>/target/release/sublink-worker) &   # fresh, empty data dir
node ui_compare.mjs capture http://127.0.0.1:38580 rust
node ui_compare.mjs diff
```

Set `CHROMIUM_PATH` when Playwright cannot locate its own Chromium build.

## When `dev` moves

`git log --oneline 2d90c0f..origin/dev` lists the Node.js commits the Rust
port has not caught up with yet. For each batch:

1. Add golden cases that exercise the new behavior to `gen_golden.mjs`.
2. Regenerate the fixtures from the new `dev` tip as above and review which
   cases changed — only the ones the new commits explain should.
3. Port the code and its vitest cases (`tests/unit/`), then run `cargo test`.
4. Re-run `tests/e2e/clients.py` against both implementations and refresh its
   known-failure table.
5. Update the baseline commit here, in `docs/rust-rewrite.md` and in
   `tests/e2e/README.md`.
