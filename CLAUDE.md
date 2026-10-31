# CLAUDE.md

本文件面向在此仓库工作的编码代理，只记录从代码不易直接看出、但会改变做法的约定。

## 平台与命令

纯 Windows 二进制 crate，依赖 `windows` 0.62 的 GDI+/GDI、DPAPI 和 Win32 消息循环；只能在 Windows 上编译和运行。Rust 1.88+（edition 2024）。

```bash
cargo run --release
cargo test --locked
cargo test --locked --bin magpie-panel model::   # 只跑一个模块
cargo clippy --locked --all-targets -- -D warnings
cargo fmt -- --check
```

- 运行中的面板会锁住 `target/release/magpie-panel.exe`；构建 release 前先正常关闭它。
- `[profile.release]` 追求体积：`opt-level="z"`、`lto`、`panic="abort"`、`strip`。改 profile 前先确认体积影响。
- 测试在 `src/tests/`，源文件用 `#[cfg(test)] #[path = ...] mod tests;` 挂载；测试仍是模块子项，可访问私有函数。只覆盖纯逻辑（解析、布局、格式、配置）；`Worker::run` 走真实网络，不在测试内。
- CI 在 `.github/workflows/ci.yml` 跑 fmt、clippy、test。

## 数据与配置

- 配置：`%APPDATA%\magpie-panel\config.json`，首次运行生成。
- 网关密钥：`%APPDATA%\magpie-panel\credentials.bin`，DPAPI 加密并绑定当前用户。`accounts` 只存 id/名称/地址。
- `MAGPIE_PANEL_HOME` 覆盖整个存储目录（`config::base_dir`）；`AH_PANEL_HOME` 仍兼容。新目录没有配置/密钥时沿用 `%APPDATA%\ah-panel`，不搬移、不覆盖。
- `accounts` 是实际轮询的连接；顶层 `endpoint` 是首次安装和新增连接表单的默认地址。环境变量 `MAGPIE_GATEWAY_KEY`（名字由 `gatewayKeyEnvVar` 改）只覆盖第一条连接，并优先于该连接保存的密钥。
- `pollSeconds` / `activePollSeconds`（默认 5 / 2）只能改配置后重启，设置窗口没有。`period`（默认 `30d`）同样只改配置，决定 usage 窗口。
- `rowLimit` 随窗口高度重算。

## 线程与状态

- UI 线程跑 Win32 消息循环（`main.rs`）；`magpie-panel-poll` 线程跑网络轮询（`worker.rs::run`）；`magpie-panel-detail` 线程用容量 1 的队列串行加载详情。三者用 mpsc 解耦。
- 主 → worker：`Worker::send(Command)`；worker → 主：`Worker::drain()` 非阻塞拉取自上次的全部 `Update`。
- UI 的 `TIMER_POLL` 每 250ms 调 `pump_worker`：drain → `State::with` 应用 → 必要时 `Action::Redraw`。网络轮询间隔由 worker 自己按 config + 退避计算（失败 2s → 30s），与这个 UI 节拍无关。`TIMER_CLOCK` 每秒重绘相对时间。
- 全部状态在 UI 线程的 `thread_local! RefCell<Option<State>>`（`app`、`worker`、`fonts`、`detail`、`settings`、`scale` 等），经 `State::with(|s| ...)` 修改。`State` 只属于 UI 线程。
- 窗口过程不直接调用 `SetWindowPos` / `DestroyWindow` / 模态弹窗等会泵消息的 Win32 API：它们可能在持有 `RefCell` 借用时重入并触发 `RefCell already borrowed`。把意图压进 `Vec<Action>`，释放借用后由 `run_actions` 执行；Action 可继续压 Action。
- `Action::Redraw` 只作用于发起它的 `hwnd`；`pump_worker` 在详情弹窗打开时要额外 `invalidate(popup_hwnd)`。detail / settings 是独立窗口，但共用同一个 `State`。
- 详情回包用 `DetailTicket` 的 nonce、`targets_revision`、endpoint 和原始行 `DetailIdentity` 一起校验；连接或凭据编辑会推进 revision，迟到回包必须丢弃。

## 绘制

- `WM_PAINT` 经 `with_double_buffer` 画到离屏位图后一次 `BitBlt`，避免闪烁。
- 布局集中在 `ui/layout.rs`（纯函数、可测），按显示器物理像素密度缩放，而不是系统缩放；`config.window` 存 96 DPI 逻辑单位（`logicalWindow`）。`WM_DPICHANGED` 只处理系统 DPI 变化，跨屏密度变化由 `WM_MOVE` 侦测。
- 中英文混排由 `theme.rs::split_runs` 按字形来源分段，分别用拉丁/CJK 字体测量和绘制。
- 表格没有表头；第一列是状态圆点。列宽取当前页每列最长文本的实测宽度，空列自动隐藏，窗口宽度跟随列宽。
- 详情弹窗最大宽度 1080 物理像素，日志超长时在弹窗内换行。

## 数据流与 Magpie 语义

- `model.rs` 把 usage/concurrency 响应转成 `Row`，表格、筛选和详情共用。`app.rs` 保留原始行，在 `rebuild()` 里统一应用显示名缩写、状态筛选、关键词搜索和 `rowLimit`；筛选与搜索纯本地。worker 每次拉 100–500 条供本地过滤。
- 显示名缩写只影响展示；详情匹配用原始字段构成的 `DetailIdentity`，不要用映射后的名字。
- `client.rs` 轮询 `/v1/magpie/usage/requests` 与 `/v1/magpie/concurrency`。详情在显式回环地址优先查 `/v1/magpie/trace`；不可用时用 `usage/requests?period=all&route=...` 补查历史摘要（最多 4 页 × 500 条），并明确提示可能不完整。
- 详情只显示状态码、供应商、模型、类型、错误日志五列，按尝试顺序保留失败与最终结果；不读取请求/响应正文，不探测独立控制台端口。主表格错误列只显示状态码，长日志只在详情换行。
- 认证：回环地址免密钥；远程只接受可选的 `Authorization: Bearer <gateway key>`。受限密钥拒绝 `/concurrency` 时视为「没有进行中请求」，不是连接失败。没有 JWT、refresh token、账号密码或 API key 换 JWT 的机制。
- Magpie wire 字段是 snake_case（`cache_read`、`first_text_ms`、`handed_on`、`route_id`），少数显示名是 camelCase（`agentName`、`providerName`、`providerAccount`、`providerKeyName`），逐个显式 `#[serde(rename = ...)]`；`in` 是 Rust 关键字，也要 rename。新增字段时对照原始 JSON，不要依赖 `rename_all`。
- token：`in` 不含缓存读写，`cache_read` / `cache_write` 单列；`out` 已含 `reasoning`，不要再加一次。`cost` 只在 `priced: true` 时展示。
- 没有全网关唯一请求 id：行 id 由 `t`、`agent`、`model`、`rid`、`session` 合成（进行中为 `live:<route>`），不要假设跨轮询稳定；详情按 `DetailIdentity` 匹配。
