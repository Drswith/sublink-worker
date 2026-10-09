# Real-client end-to-end check

`clients.py` proves that the configs a running worker generates are accepted
*and usable* by the real clients, which golden tests cannot show (they only
compare bytes with the original implementation).

For every case it:

1. starts local sing-box servers, one inbound per supported protocol
   (ss, ss2022, vmess tcp / ws+tls, vless ws / reality, trojan, hysteria2,
   tuic, anytls), and builds the matching share links;
2. asks the worker for a client config (`/singbox?sb_ver=…`, `/clash`) from
   those links, or from a subscription URL served locally in Base64, Clash
   YAML or sing-box JSON form;
3. validates it with the official `sing-box check` / `mihomo -t`;
4. runs the client and fetches `http://e2e.example:<port>/<token>` through its
   SOCKS port. `e2e.example` resolves only inside the proxy server, so the
   case passes only if the target answers *and* the server logged the request
   on a proxy inbound — a config that silently routes DIRECT fails.

Matrix: every protocol × sing-box 1.11 / 1.12 / 1.13 / 1.14 and mihomo; all
protocols with every option on (`comprehensive` rules, custom rules, country
groups, Clash UI, no auto-select); Clash without the generated DNS section;
the three subscription formats; Xray round-trip. Surge has no Linux client and is not covered.

## Running

Requires Python 3 and `curl`. Clients (pinned in the script) and geodata are
downloaded on first use; remote rule sets are fetched once and served from a
local mirror, because TUN inbounds are dropped and rule-set URLs rewritten so
the clients run unprivileged.

```sh
cargo run --release &                    # or any deployed worker
python3 tests/e2e/clients.py --worker http://127.0.0.1:38471
python3 tests/e2e/clients.py --only sing-box-1.13   # substring filter on case names
```

Work files (generated configs, client and server logs) stay in
`/tmp/sublink-e2e-*` for inspection.

## Known failures

These come from the generated configs themselves and reproduce identically
on the latest Node.js `dev` (`2d90c0f`), so they are kept for parity; see
`docs/rust-rewrite.md`. None of them affects Clash output from share links or
Base64/Clash subscriptions.

| Cases | Cause |
| --- | --- |
| `sing-box-1.11.*/anytls`, `all-options`, `sub-base64` | anytls outbounds are emitted for the 1.11 tier, which does not know the type |
| `sing-box-*/sub-clash` | the Clash subscription's `dns` section is copied into the sing-box config (`dns.enable: unknown field`) |
| `sing-box-*/sub-singbox` | non-standard `providers` fields on outbounds (1.12+); 1.12-style DNS servers on the 1.11 tier |
| `mihomo/sub-singbox` | the subscription's sing-box-only sections (`ntp`, `inbounds`, `route`, `experimental`) are copied into the Clash config; mihomo rejects `ntp.interval: 30m` |
