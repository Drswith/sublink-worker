# Rust 重写说明

Sublink Worker 由 Node.js（Hono）实现改为单个 Rust 二进制。对照基线始终是 Node.js 版 `dev`
分支的最新提交，当前为 `2d90c0f`（含 sing-box 1.14 规则集下载修复、Clash DNS 开关、上游订阅失败返回 502）。
`git fetch origin dev && git log --oneline 2d90c0f..origin/dev` 有输出时说明基线已落后，按
`tests/fixtures/reference/README.md` 重新录制并移植。
本文记录部署层面的变化、对齐原实现的方式，以及刻意保留或无法保留的差异。

## 部署变化

| 项目 | Node.js 版本 | Rust 版本 |
| --- | --- | --- |
| 运行方式 | Cloudflare Workers / Vercel / Node.js / Docker | 单二进制 / Docker（`scratch` 镜像，约 7 MB；amd64/arm64 均在构建机上交叉编译） |
| 存储 | Cloudflare KV / Redis / Upstash / 进程内存 | 进程内 HashMap + 追加日志文件（`data/sublink.aof`） |
| 静态资源 | `STATIC_DIR` 目录 | favicon 编译进二进制 |
| 常驻内存（同等负载实测 RSS） | 约 75 MB | 约 7 MB |

### 环境变量

保留：`PORT`、`CONFIG_TTL_SECONDS`、`SHORT_LINK_TTL_SECONDS`，解析规则与原
`createNodeRuntime()` 一致（按 JS `Number()` 解析；`CONFIG_TTL_SECONDS=0` 表示永不过期）。

数据文件固定为工作目录下的 `data/sublink.aof`，Docker 镜像中即 `/data/sublink.aof`（挂载 `/data` 卷即可持久化），无需配置。

移除：`REDIS_URL`、`REDIS_HOST`、`REDIS_PORT`、`REDIS_USERNAME`、`REDIS_PASSWORD`、
`REDIS_TLS`、`REDIS_KEY_PREFIX`、`KV_REST_API_URL`、`KV_REST_API_TOKEN`、
`DISABLE_MEMORY_KV`、`STATIC_DIR`。

### 数据迁移

不迁移。旧 Redis / KV 中的短链和已保存的基础配置不会被读取，升级后需要重新生成。

### TTL

键过期遵循原 Redis 适配器语义：TTL 向下取整，结果不大于 0 时视为永不过期。
过期键读取时立即不可见，后台每 10 分钟清理一次。

### 持久化

全部数据常驻内存，写操作以 JSON 行追加到 `data/sublink.aof` 并 fsync 后才生效，启动时重放日志：

- 崩溃导致的末尾半行记录会被丢弃并截断，不影响之前的数据；
- 日志中失效记录（覆盖、删除、过期）超过半数且总数超过 1024 条时，后台清理会把日志原子地重写为只含有效记录；
- 同一日志文件同时只能被一个进程打开（`data/sublink.aof.lock` 文件锁）。

## 如何保证行为一致

- 原 38 个 vitest 测试文件全部移植，断言保持等价：37 个在 `tests/unit/`，formLogic 的断言在 `tests/pages.rs`。
- `tests/golden.rs`：815 组请求序列在原 Node 实现上录制响应（状态码、
  Content-Type、`subscription-userinfo`、`Location` 和完整响应体），Rust 实现必须逐字节一致。
  覆盖全部协议、各类订阅格式、规则预设与自定义规则、国家分组、Clash UI、Clash DNS 开关、
  sing-box 版本分档（含 1.14 各类基础配置）、上游订阅失败、短链、配置保存及各种错误路径，
  以及差分审计发现的 JS 语义边界（URLSearchParams 解码、`__proto__` 赋值、`typeof null`、
  类数组 `length`、展开非可迭代值、去重遇到 null 条目、V8 报错文本的渲染方式等）。
- `tests/fetch_compat.rs`：86 组原始上游响应（各种 `Content-Encoding` 组合、截断或损坏的压缩流、
  重定向、URL 中的凭据、各类 User-Agent 取值）由 Node fetch 录制结局，Rust 的抓取层必须一致。
- 差分审计：对两边运行中的服务按功能区发送数千个边界请求逐一比对，发现的差异均已修复或列入下表。
- `tests/pages.rs`：首页 HTML 在 4 种语言及多种 `lang`/`Accept-Language` 组合下与原
  JSX 渲染结果逐字节一致。
- 浏览器端：在 Chromium 中对原版和 Rust 版执行同一组 UI 操作（4 种语言、转换、短链、基础配置、自定义规则、粘贴回填、清空、深色模式、更新提示），46 项观测（DOM、可见文本、整页截图逐像素、弹窗、localStorage、生成链接的响应）全部一致。
- `tests/js_core.rs`、`tests/yaml_compat.rs`：JS 数字/URI/Base64 语义与 js-yaml 4
  的读写结果与 Node 逐项对照。
- `tests/e2e/clients.py`：用真实客户端验证生成的配置可用——本地起各协议的 sing-box 服务端，
  让 sing-box 1.11–1.14 与 mihomo 加载生成的配置并实际转发流量，只有服务端日志证明请求经过代理才算通过。
  原版与 Rust 版在 72 个用例上的通过/失败结果完全相同（60 个通过），失败项见下节。

录制脚本与重新生成方法见 `tests/fixtures/reference/README.md`。

## 刻意保留的原有行为

以下行为看起来像缺陷，但为保持输出一致而原样保留：

- 自定义规则在每次生成规则时都会被原地反转顺序；Clash 与 Surge 路由在 `build()`
  之后又调用一次 `formatConfig()`，因此两者的自定义规则顺序与 sing-box 不同。
- 协议解析抛出的异常会让整个请求返回 500 `Error: <JS 错误信息>`，错误文本与
  V8 完全一致（例如 `Cannot read properties of undefined (reading 'x')`）。
- 短链与保存的配置共用同一个键空间（`/shorten-v2?shortCode=clash_xxx` 可以覆盖配置）。
- 真实客户端无法运行的几类输出（详见 `tests/e2e/README.md`）：1.11 分档仍输出 anytls 出站；
  Clash 订阅转 sing-box 时带入 Clash 的 `dns` 段；sing-box 订阅转 sing-box 时输出非官方的
  `providers` 字段，转 Clash 时把 `ntp`/`inbounds`/`route` 等 sing-box 专有段原样带入（mihomo 拒绝 `ntp.interval: 30m`）。
- Hono 的查询参数解析细节（`+` 视为空格、首个同名参数生效、编码键名的回退解析）、
  `c.text()` 快速路径的 `text/plain;charset=UTF-8` 与常规路径的
  `text/plain; charset=UTF-8` 差异等，均按原样实现。

## 已知差异

| 场景 | Node.js 版本 | Rust 版本 |
| --- | --- | --- |
| 未配置 KV（`DISABLE_MEMORY_KV=true`） | 短链/配置接口返回 501 | 存储始终可用，不存在该状态 |
| YAML 自引用锚点（如 `a: &x [1, *x]`） | 生成循环对象，后续 `JSON.stringify` 抛错（500） | 别名取锚点在该位置的快照（`[1, []]`），不再报错 |
| JSON 中的孤立代理项（如 `"\ud800"`） | 原样保留在字符串里 | 替换为 U+FFFD |
| 以 `constructor`、`toString`、`__proto__` 等 `Object.prototype` 成员名作为键名的输入（`selectedRules` 预设名、`lang`、自定义规则名、Surge 段名、`__proto__://` 链接等），以及含 `.` 的自定义规则名走到译文字符串的属性（如 `Ad Block.0`、`GLOBAL.length`） | 命中原型成员或字符串属性，得到未翻译的名称、孤立代理项、数字、异常输出或 500 | 按普通键处理（`selectedRules` 按 JSON 解析失败回退到 minimal，`/subconverter` 返回 400）；`__proto__` 键在赋值与深拷贝时同样被丢弃，但不模拟它带来的原型继承 |
| 数组充当配置对象（如基础配置为 `[1,2]`、`route: []`） | 数组上可以挂命名属性，输出只剩数组元素或报其他错误 | 数组不能携带命名属性，返回 500，报错文本不同 |
| YAML `!!binary` 值 | 读成 `Uint8Array`，Clash 输出 `!!binary`，Surge 输出逗号分隔的字节 | 读成以下标为键的对象 |
| 首页内联的表单脚本 | esbuild 重新打印后的代码（部分局部变量被改名、注释被删除） | 原始源码；功能完全相同 |
| 抓取订阅时的代理 | Node `fetch` 忽略 `HTTPS_PROXY` 等环境变量 | 遵循 `HTTPS_PROXY` / `HTTP_PROXY` / `NO_PROXY` |
| 抓取订阅超时 | 转换接口 15 秒；`/xray` 无超时 | 转换接口 15 秒；`/xray` 连接 10 秒、读取空闲 300 秒 |
| 上游返回不规范的 HTTP 报文（重复的 `Content-Length`、首尾带空白的 `Location`、折行的头部等） | 按 undici 的解析器取舍 | 按 hyper 的解析器取舍，个别报文一方接受而另一方拒绝 |
| 客户端请求行里的 `#片段`、原始的 `"` `<` `>` 或非 ASCII 字节 | Node 的 HTTP 解析器保留片段（进入查询参数和 Surge 托管链接），接受 `"<>`，拒绝非 ASCII | hyper 丢弃片段，拒绝 `"<>`（400），接受非 ASCII |
| 大订阅 | 节点去重为平方复杂度：2000 个节点约 3 秒，3 万个要十几分钟且期间阻塞整个进程 | 输出相同，3 万个节点约 1–2 秒 |
| 请求行 + 请求头大小上限 | 16 KB（超出返回 431） | 约 400 KB，长订阅链接更不容易被拒 |
| 响应传输方式 | `Transfer-Encoding: chunked` | `Content-Length` |
| 页脚年份 | 容器本地时区 | UTC |

日志格式也有变化：启动时仍输出 `Sublink worker running on http://0.0.0.0:<port>`，
错误日志写到 stderr。
