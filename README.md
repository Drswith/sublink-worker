<div align="center">
  <img src="assets/web/favicon.png" alt="Sublink Worker" width="120" height="120"/>

  <h1><b>Sublink Worker</b></h1>
  <h5><i>One Worker, All Subscriptions</i></h5>

  <p><b>A lightweight subscription converter and manager for proxy protocols, shipped as a single Rust binary (or a ~9 MB Docker image).</b></p>

  <a href="https://trendshift.io/repositories/12291" target="_blank">
    <img src="https://trendshift.io/api/badge/repositories/12291" alt="7Sageer%2Fsublink-worker | Trendshift" width="250" height="55"/>
  </a>

  <br>

  <h3>📚 Documentation</h3>
  <p>
    <a href="https://app.sublink.works"><b>⚡ Live Demo</b></a> ·
    <a href="https://sublink.works/en/"><b>Documentation</b></a> 
    <a href="https://sublink.works"><b>中文文档</b></a>·
  </p>
  <p>
    <a href="https://sublink.works/guide/quick-start/">Quick Start</a> ·
    <a href="https://sublink.works/api/">API Reference</a> ·
    <a href="https://sublink.works/guide/faq/">FAQ</a>
  </p>
</div>

## 🚀 Quick Start

### Docker Compose (recommended)

```bash
docker compose up -d
```

Open `http://localhost:38471`. Short links and saved configs live in the
`sublink-data` volume, so they survive restarts and upgrades.

Compose uses this fork's GHCR image by default. Override `SUBLINK_WORKER_IMAGE`
in `.env` to use another image or a specific version. GitHub Actions publishes
multi-arch images (amd64/arm64) on pushes to `main`, `v*` tags, and manual
workflow runs.

### Docker

```bash
docker run -d -p 38471:38471 -v sublink-data:/data ghcr.io/drswith/sublink-worker:latest
```

### From source

```bash
cargo build --release
./target/release/sublink-worker
```

To build and run the current source as an image:

```bash
docker build -t sublink-worker:local .
SUBLINK_WORKER_IMAGE=sublink-worker:local docker compose up -d --pull never
```

### Configuration

| Variable | Default | Description |
| --- | --- | --- |
| `PORT` | `38471` | HTTP listen port |
| `DB_PATH` | `data/sublink.redb` (`/data/sublink.redb` in Docker) | Embedded database file; `:memory:` keeps data in RAM only |
| `CONFIG_TTL_SECONDS` | `2592000` (30 days) | Lifetime of saved base configs; `0` keeps them forever |
| `SHORT_LINK_TTL_SECONDS` | unset (never expire) | Lifetime of short links |

Outbound subscription downloads honor `HTTPS_PROXY` / `HTTP_PROXY` / `NO_PROXY`.

Upgrading from the Node.js version? See [docs/rust-rewrite.md](docs/rust-rewrite.md):
Redis is no longer used and existing short links are not migrated.

## ✨ Features

### Supported Protocols
ShadowSocks • VMess • VLESS • Hysteria2 • Trojan • TUIC

### Client Support
Sing-Box • Clash • Xray/V2Ray • Surge

### Input Support
- Base64 subscriptions
- HTTP/HTTPS subscriptions
- Full configs (Sing-Box JSON, Clash YAML, Surge INI)

### Core Capabilities
- Import subscriptions from multiple sources
- Generate fixed/random short links (stored in an embedded database, no Redis needed)
- Light/Dark theme toggle
- Flexible API for script automation
- Multi-language support (Chinese, English, Persian, Russian)
- Web interface with predefined rule sets and customizable policy groups

## 🤝 Contributing

Issues and Pull Requests are welcome to improve this project.

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## ⚠️ Disclaimer

This project is for learning and exchange purposes only. Please do not use it for illegal purposes. All consequences resulting from the use of this project are solely the responsibility of the user and are not related to the developer.

## ⭐ Star History

Thanks to everyone who has starred this project! 🌟

<a href="https://star-history.com/#7Sageer/sublink-worker&Date">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=7Sageer/sublink-worker&type=Date&theme=dark" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=7Sageer/sublink-worker&type=Date" />
   <img alt="Star History Chart" src="https://api.star-history.com/svg?repos=7Sageer/sublink-worker&type=Date" />
 </picture>
</a>
