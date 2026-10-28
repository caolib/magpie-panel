# CLAUDE.md

This file provides guidance to CodeBuddy Code when working with code in this repository.

## 构建、运行与测试

本仓库是纯 Windows 二进制 crate，依赖 `windows` 0.62（GDI+ / GDI / DPAPI / Win32 消息循环）。**只能在 Windows 上编译和运行**。

```bash
cargo run --release          # 运行（首次启动自动创建本机连接）
cargo build --release        # 仅构建；产物在 target/release/magpie-panel.exe
cargo test                   # 跑全部内置测试
cargo test --bin magpie-panel <过滤>   # 例：cargo test --bin magpie-panel model:: 只跑某模块
cargo clippy --all-targets   # 代码质量检查（需已装 clippy）
cargo fmt                    # 格式化
```

测试集中放在 `src/tests/`：`src/tests/*.rs` 对应根模块，`src/tests/ui/*.rs` 对应 UI 模块，源文件里以 `#[cfg(test)] #[path = ...] mod tests;` 引用。测试仍是各模块的子模块（`use super::*` 照旧），所以能访问私有函数。无 `dev-dependencies`，纯 std，`cargo test` 直接跑。注意 `Worker::run` 涉及真实网络，测试只覆盖纯逻辑（解析、布局、格式、配置）。

`Cargo.toml` 的 `[profile.release]` 设了 `opt-level="z"`、`lto=true`、`panic="abort"`、`strip=true`——发布构建体积敏感，改 profile 需谨慎。

## 配置与数据位置

- 配置：`%APPDATA%\magpie-panel\config.json`，首次运行自动生成（`config.rs`）。
- 网关密钥（DPAPI 加密，绑定当前用户）：`%APPDATA%\magpie-panel\credentials.bin`。配置里只存非敏感的 `accounts`（id/名称/地址）。
- 用环境变量 `MAGPIE_PANEL_HOME` 可整体覆盖该目录（`config::base_dir`），用于指向不同 profile 或本地测试；旧变量 `AH_PANEL_HOME` 仍兼容。新目录无配置/密钥时继续使用 `%APPDATA%\ah-panel` 旧资料，不搬移或覆盖。
- 刷新间隔由 `pollSeconds`（空闲，默认 5）和 `activePollSeconds`（有请求 in flight，默认 2）控制，**设置窗口里没有这一项**，只能改 `config.json` 后重启。
- `period` 决定 `/v1/magpie/usage/requests` 的时间窗口（`today` / `7d` / `30d` / `all` / `last:24h` / `YYYY-MM-DD..YYYY-MM-DD`），同样只能改配置。

## 高层架构

### 1. 线程模型与数据流（双通道解耦）

单条 UI 线程跑 Win32 消息循环（`main.rs`）；另一条 `magpie-panel-poll` 线程跑网络轮询（`worker.rs::run`，由 `Worker::spawn` 起）。两者用两个 `mpsc` 通道解耦，互不阻塞：

- 主 → worker：`Worker::send(Command)`，命令如 `RefreshNow` / `SetTargets` / `FetchDetail` / `CancelDetail` / `SetRowLimit` / `TestAccount` / `Shutdown`。
- worker → 主：`Worker::drain()`，**非阻塞**拉取自上次的全部 `Update`（`Snapshot` / `Accounts` / `Detail` / `Probe`）。
- `magpie-panel-detail` 独立线程通过容量为 1 的队列加载详情，慢请求不阻塞轮询；`DetailTicket` 的 nonce、目标 revision、endpoint 与原始行身份一起拒绝迟到回包。

UI 线程上每 200ms 的 `WM_TIMER`（定时器 id `TIMER_POLL`）触发 `pump_worker`：`drain()` 取出 Update → 经 `State::with` 应用到 `App` → 把 `Action::Redraw` 压入待执行队列。**真正的网络轮询间隔由 worker 自己按 config + backoff 计算**，和这 200ms 的 UI 同步节拍是两回事。

### 2. 全局可变状态 `State`

整个面板状态装在一个 `thread_local! RefCell<Option<State>>`，结构见 `main.rs`：`app`（面板数据）、`worker`、`fonts`、`detail`（详情弹窗，至多一个）、`settings` / `settings_login`（设置窗口往返）、`activatable`、`scale`（绘制缩放比）。所有窗口过程都通过 `State::with(|s| ...)` 借 `&mut` 改状态。因为是单 UI 线程，`RefCell` 是安全的——不要试图把 `State` 搬到别的线程。

### 3. 副作用延迟执行：Action 机制

`SetWindowPos` / `DestroyWindow` / 模态弹窗这类 Win32 调用会泵消息、重入窗口过程，若在持有 `RefCell` 借用时调用会触发 `RefCell already borrowed`。所以窗口过程**不直接**执行这些副作用，而是把意图收集进 `Vec<Action>`，在释放状态借用之后由 `run_actions` 统一执行。Action 还能链式再压 Action（drain 用 `while let Some ... pop`）。任何新增会触发窗口/进程级副作用的逻辑，都应走 `Action`，不要就地调用 Win32。

### 4. 绘制管线：双缓冲 + 物理 DPI 缩放

- `WM_PAINT` 经 `with_double_buffer` 先画到离屏位图再一次性 `BitBlt`，避免闪烁。
- 布局计算集中在 `ui/layout.rs`（纯函数、可测），按**显示器物理像素密度**（EDID，非系统缩放）缩放；窗口尺寸以 96 DPI 逻辑单位保存（`config.window` + `logicalWindow`），故跨屏观感大小一致，`WM_DPICHANGED` 只在系统 DPI 变化时触发，跨屏密度不同由 `WM_MOVE` 单独侦测。
- 中英文混排由 `theme.rs` 的 `split_runs` 分段绘制（中文缺字形时回退字体）。
- 表格列宽由 `ui/table.rs` 实测每列最长文本得出；没有表头行，状态列是彩色圆点，`流`/`转`/`透` 标记列由颜色表达。

### 5. 数据模型与显示

- `model.rs` 把 Magpie 的 usage / concurrency 响应转成 `Row`，是表格、筛选和详情弹窗共享的数据结构。
- `app.rs` 持有原始行（wire 模型名），在 `rebuild()` 里统一应用模型缩写、状态筛选、关键词搜索和行数上限。筛选与搜索都是**纯本地**的，不向 worker 发命令。
- `client.rs` 轮询 `/v1/magpie/usage/requests` 与 `/v1/magpie/concurrency`；详情在显式回环地址优先查 `/v1/magpie/trace`，不可用时以 `usage/requests?period=all&route=...` 补查历史摘要。远程用网关密钥（`Authorization: Bearer`），不探测独立控制台端口。
- 详情仅显示状态码、供应商、模型、错误日志四列，按尝试顺序保留失败和最终结果；历史摘要可能不完整，加载/权限/分页限制需明确提示。主表格错误列仅显示状态码，长日志只在详情换行显示。

### 6. 认证约束（易踩坑）

Magpie 是本地网关：默认 `http://127.0.0.1:3425`，回环访问**不需要密钥**。只有网关被共享到网络外时才需要 Magpie gateway key；受限的 key 可能拒绝 `/concurrency`，此时应视为「没有进行中请求」，而不是连接失败。没有 JWT、refresh token、账号密码或 API key 换 JWT 之类的机制——不要重新引入。

## 跨文件陷阱 / 注意点

- **serde 字段命名**：Magpie 的 wire 字段是 snake_case（`cache_read`、`first_text_ms`、`handed_on`、`route_id`），但少数显示名是 camelCase（`agentName`、`providerName`、`providerAccount`、`providerKeyName`），必须显式 `#[serde(rename = ...)]`；`in` 是 Rust 关键字，也要 rename。新增字段时逐个核对原始 JSON，不要依赖 `rename_all`。
- **token 语义**：`in` 不含缓存读写，`cache_read` / `cache_write` 单列；`out` 已含 `reasoning`，聚合时不要再加一次。`cost` 仅在 `priced: true` 时展示。
- **没有全网关唯一的请求 id**：行 id 是合成的（`t` + `rid` / `live:<route>` 等），不要假设它能跨轮询稳定匹配；详情按原始字段构成的 `DetailIdentity` 匹配，不使用映射后的显示名。
- **两层定时器**：200ms 的 `TIMER_POLL` 只负责把 worker 结果同步进 `State` 并触发重绘；网络轮询间隔由 worker 自管。改刷新频率别动 UI 定时器，改 `config` 的 `pollSeconds` / `activePollSeconds`。
- **弹窗共享 State**：detail / settings 是独立窗口但都通过同一个 `State::with` 访问状态。`Action::Redraw` 只作用于发起它的那个 `hwnd`，所以 `pump_worker` 在 `Popup` 打开时会额外 `invalidate(popup_hwnd)`，否则弹窗不会刷新。
- **运行中的 exe 会锁住 release 构建**：改完代码想 `cargo build --release` 前，先把正在跑的 `magpie-panel.exe` 关掉，否则 Windows 文件锁会导致链接失败。
