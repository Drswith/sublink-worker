# Reference fixtures

The JSON fixtures next to this directory were captured from the original
Node.js implementation (last commit before the Rust rewrite: `c08b647`).
The Rust tests replay them to prove byte-level parity.

| Fixture | Generator | Checked by |
| --- | --- | --- |
| `js_core.json` | `gen_jscore.mjs` | `tests/js_core.rs` (Number/URI/Base64/trim semantics) |
| `yaml_cases.json` | `gen_yaml.mjs` | `tests/yaml_compat.rs` (js-yaml 4 load/dump) |
| `golden.json.gz` | `gen_golden.mjs` | `tests/golden.rs` (full HTTP responses) |
| `pages.json.gz` | `gen_pages.mjs` | `tests/pages.rs` (server-rendered home page) |

To regenerate (or to extend the corpus) against the original code:

```sh
git worktree add /tmp/sublink-ref c08b647
cd /tmp/sublink-ref && npm ci
cp <repo>/tests/fixtures/reference/* .
node_modules/.bin/esbuild golden-entry.js --bundle --platform=node --format=esm --outfile=golden-bundle.mjs
node gen_golden.mjs <repo>/tests/fixtures/golden.json.gz
node gen_pages.mjs  <repo>/tests/fixtures/pages.json.gz
node gen_jscore.mjs > <repo>/tests/fixtures/js_core.json
node gen_yaml.mjs   > <repo>/tests/fixtures/yaml_cases.json
```
