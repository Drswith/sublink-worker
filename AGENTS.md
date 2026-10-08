# AGENTS.md

## 协作偏好

- 用中文回复；代码注释用英文，注释写 why 不写 how。
- 简洁直接，不要多余总结和解释；除非存在高风险或信息缺口，直接写代码。
- 函数式优先，TS/JS 中避免 OOP；新功能优先复用或重构现有代码。
- 遵循 KISS、DRY 原则；从第一性原理解构问题，警惕 XY 问题。
- 发现不合理的需求或方向要立即指出，不奉承。

## 项目概览

Sublink Worker 是代理订阅转换器：将 ShadowSocks/VMess/VLESS/Hysteria2/Trojan/TUIC/AnyTLS 节点或订阅转为 Sing-Box/Clash/Xray/Surge 配置。单个 Rust 二进制：hyper + tokio（HTTP）、reqwest + rustls（抓取订阅）、redb（内嵌 KV）、askama（首页 SSR），以 `scratch` Docker 镜像发布。

行为与重写前的 Node.js（Hono）实现逐字节对齐，差异清单见 `docs/rust-rewrite.md`。

## 常用命令

- `cargo run` — 本地启动（默认端口 38471，数据写入 `data/sublink.redb`；`DB_PATH=:memory:` 不落盘）
- `cargo test` — 全部测试；`cargo test --test unit <过滤词>` 跑单个模块
- `GOLDEN_FILTER=<用例名片段> cargo test --test golden` — 只比对部分 golden 用例
- `cargo clippy --all-targets -- -D warnings`、`cargo fmt`（`rustfmt.toml`，行宽 120）
- `docker build -t sublink-worker:local .`

环境变量：`PORT`、`DB_PATH`、`CONFIG_TTL_SECONDS`（`0` 永不过期）、`SHORT_LINK_TTL_SECONDS`；订阅抓取遵循 `HTTPS_PROXY`/`NO_PROXY`。

## 代码结构

- `src/main.rs` 启动与信号处理；`src/settings.rs` 解析环境变量；`src/server.rs` 把 hyper 请求转成原 Node 入口看到的 URL/请求头
- `src/app.rs` 全部路由；`src/hono.rs` 复刻 Hono 的查询参数解析、路径解码、默认 Content-Type
- `src/storage.rs` redb KV（TTL 惰性过期 + 定期 sweep，可注入时钟）；`src/services.rs` 短链与基础配置存储
- `src/js/` JS 语义层：`Value`（Arc 写时复制，模拟引用身份以复现 YAML 锚点）、V8 风格 JSON、数字/URI/Base64
- `src/yaml/` js-yaml 4 兼容的 `load`/`dump`
- `src/parsers/` 协议解析（`protocols.rs`）、订阅抓取与格式识别（`subscription.rs`、`content.rs`）、Clash/Surge 代理转换
- `src/builders/` `ConfigBuilder` trait + `Core`（原 `BaseConfigBuilder` 字段与模板方法）；`singbox.rs`/`clash.rs`/`surge.rs` 各自实现
- `src/config/` 规则定义、规则生成、subconverter；基础配置在 `assets/base/*.json`，翻译在 `assets/i18n.json`
- `src/pages.rs` + `templates/` 首页 SSR；`assets/web/` 前端脚本与 favicon

## 关键约定

- 行为对齐优先：改动必须通过 golden/pages 对照测试；有意改变输出时同步更新 fixture，并在 `docs/rust-rewrite.md` 记录
- 业务逻辑统一使用 `js::Value` 并保留 JS 语义（真值、ToString、属性访问报错文本）——这些文本会直接出现在 500 响应里
- 新增协议：在 `src/parsers/protocols.rs` 加 parser，并在 `src/parsers/mod.rs` 的 scheme 分发中注册
- 模板缩进只为可读，渲染后按 esbuild 的 JSX 规则折叠空白；插值使用 Hono 的转义（`&quot;`、`&#39;`）
- 测试中的网络请求一律走 `tests/common` 的 `MockFetcher`，KV 用 `Store::in_memory()`
- i18n：zh-CN / en-US / fa / ru，在 `assets/i18n.json`

## 测试

- `tests/unit/`：原 vitest 用例逐文件移植（一个模块对应一个原测试文件）
- `tests/golden.rs`、`tests/pages.rs`、`tests/js_core.rs`、`tests/yaml_compat.rs`：与 Node 实现录制结果逐字节对照；fixture 生成方法见 `tests/fixtures/reference/README.md`

## 本地工作流

- 本地分层规划放 `.roadmap/roadmap.md`（已 `.gitignore`）；更新前先同步 GitHub open issues
- 个人偏好、沙盒、临时上下文写入 `AGENTS.local.md`（不提交）
- Claude 共享规则放 `CLAUDE.md`，个人覆盖放 `CLAUDE.local.md`

## 文档参考

代理工具配置问题查官方文档：
- sing-box: https://sing-box.sagernet.org/
- clash/mihomo: https://wiki.metacubex.one/
- surge: https://blankwonder.gitbooks.io/surge-manual/content/
- xray: https://xtls.github.io/config/
