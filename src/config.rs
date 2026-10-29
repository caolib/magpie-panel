//! On-disk settings plus a DPAPI-protected store for gateway keys.
//!
//! Magpie is a local gateway: a connection needs an endpoint (`127.0.0.1:3425`
//! by default) and, only when the gateway is shared over the network, a gateway
//! key. Keys are encrypted with DPAPI (current Windows user) before they touch
//! the disk.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::warn;

/// Where a stock Magpie gateway listens.
pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:3425";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub endpoint: String,
    /// Poll cadence while no request is in flight.
    pub poll_seconds: u64,
    /// Faster cadence used while at least one request is still in flight.
    pub active_poll_seconds: u64,
    pub row_limit: i64,
    /// Usage window sent to `/v1/magpie/usage/requests`: `today`, `7d`, `30d`,
    /// `all`, `last:24h`, or `YYYY-MM-DD..YYYY-MM-DD`.
    pub period: String,
    /// Environment variable read for a gateway key instead of the stored blob.
    /// Empty disables it.
    pub gateway_key_env_var: String,
    pub always_on_top: bool,
    pub pin_position: bool,
    pub always_on_bottom: bool,
    /// Whether `window.width`/`height` are logical (96-DPI) units. Configs
    /// written before this existed stored device pixels, so an older file has
    /// this false and its size is converted once, on load.
    pub logical_window: bool,
    /// Body font size in pixels at the 96-DPI baseline. The whole panel scales
    /// in proportion with it, so this is the single knob for text size.
    pub font_size: f32,
    /// Installed font family for Latin, digits and punctuation. Empty uses the
    /// built-in fallback chain.
    pub font_family: String,
    /// Installed font family for Chinese. Empty follows `font_family` when that
    /// face has Chinese glyphs, otherwise the built-in fallback chain.
    pub cjk_font_family: String,
    /// Panel opacity in percent (10–100).
    pub opacity: u8,
    /// Saved Magpie gateways. All of them are polled and their requests merged
    /// into one list; keys live in the encrypted blob under the same ids.
    pub accounts: Vec<Account>,
    /// (connection, provider) pairs whose requests are hidden from the list.
    pub hidden_providers: Vec<HiddenProvider>,
    /// Model-name abbreviations applied before display.
    pub model_aliases: Vec<NameAlias>,
    /// Supplier-name abbreviations applied before display.
    pub provider_aliases: Vec<NameAlias>,
    /// Caller-name (agent) abbreviations applied before display.
    pub agent_aliases: Vec<NameAlias>,
    /// Table columns the user switched off. Empty means everything is shown.
    pub hidden_fields: Vec<DisplayField>,
    pub window: WindowState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowLevel {
    Normal,
    Topmost,
    Bottommost,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Rows the panel shows by default.
pub const DEFAULT_ROWS: usize = 12;

/// Body font size bounds in px at the 96-DPI baseline.
pub const FONT_SIZE_MIN: f32 = 6.0;
pub const FONT_SIZE_MAX: f32 = 50.0;
/// Panel opacity bounds in percent at the 96-DPI baseline.
pub const OPACITY_MIN: u8 = 10;
pub const OPACITY_MAX: u8 = 100;

impl Default for WindowState {
    fn default() -> Self {
        WindowState {
            // Negative coordinates mean "not positioned yet"; the window is
            // parked at the top-right of the work area on first launch.
            x: -1,
            y: -1,
            width: 720,
            height: crate::ui::layout::height_for_rows(DEFAULT_ROWS, 1.0),
        }
    }
}

/// Columns hidden out of the box: a Magpie row carries far more than fits a
/// panel-wide table, so the everyday readings start visible and the rest are
/// one settings click away.
pub const DEFAULT_HIDDEN_FIELDS: &[DisplayField] = &[
    DisplayField::UpstreamModel,
    DisplayField::Protocol,
    DisplayField::CacheWrite,
    DisplayField::Cost,
    DisplayField::Completion,
    DisplayField::Source,
    DisplayField::Account,
];

impl Default for Config {
    fn default() -> Self {
        Config {
            endpoint: DEFAULT_ENDPOINT.into(),
            poll_seconds: 5,
            active_poll_seconds: 2,
            row_limit: DEFAULT_ROWS as i64,
            period: "30d".into(),
            gateway_key_env_var: "MAGPIE_GATEWAY_KEY".into(),
            font_size: 12.5,
            font_family: String::new(),
            cjk_font_family: String::new(),
            opacity: 100,
            always_on_top: true,
            pin_position: false,
            always_on_bottom: false,
            logical_window: true,
            accounts: Vec::new(),
            hidden_providers: Vec::new(),
            model_aliases: Vec::new(),
            provider_aliases: Vec::new(),
            agent_aliases: Vec::new(),
            hidden_fields: DEFAULT_HIDDEN_FIELDS.to_vec(),
            window: WindowState::default(),
        }
    }
}

/// One saved Magpie gateway. Non-secret: the gateway key lives in the encrypted
/// blob under the same `id`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    /// Label the user typed when adding the connection.
    pub name: String,
    pub endpoint: String,
}

/// One provider of one connection whose requests the panel does not show.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct HiddenProvider {
    pub account_id: String,
    pub provider: String,
}

/// One display-name abbreviation, e.g. `deepseek` -> `ds`: every occurrence of
/// `from` in the displayed name (compared case-insensitively) is replaced with
/// `to`. An empty `to` deletes the matches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NameAlias {
    pub from: String,
    pub to: String,
}

/// Which display name an alias list rewrites.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AliasKind {
    /// 模型: requested, served and shortened model names.
    #[default]
    Model,
    /// 供应商: the provider that answered.
    Provider,
    /// 发起方: the application that sent the call.
    Agent,
}

impl AliasKind {
    /// Every kind, in the order the settings window offers them.
    pub const ALL: [AliasKind; 3] = [AliasKind::Model, AliasKind::Provider, AliasKind::Agent];

    pub fn label(self) -> &'static str {
        match self {
            AliasKind::Model => "模型",
            AliasKind::Provider => "供应商",
            AliasKind::Agent => "发起方",
        }
    }
}

/// One informational table column. Listed in the order the settings window
/// offers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DisplayField {
    /// 模型: the model that actually served the request.
    Model,
    /// 声明模型: the model string the provider's reply carried.
    UpstreamModel,
    /// 协议: inbound endpoint, and the upstream one when converted.
    Protocol,
    /// 应用: the agent that sent the call.
    Agent,
    /// 供应商: the provider that answered.
    Provider,
    /// Legacy: the key name used to be its own column. It now rides in the
    /// provider cell as `provider(key)`, so this variant is accepted when an
    /// older config lists it but never becomes a column.
    ProviderKey,
    Tokens,
    /// 缓存: prompt cache read rate.
    Cache,
    /// 写缓存: tokens written into the prompt cache.
    CacheWrite,
    /// 速度: tokens per second.
    Speed,
    /// 重试: attempts beyond the first.
    Retry,
    /// 费用: estimated cost in USD.
    Cost,
    /// 耗时: total latency and time to first token.
    Latency,
    /// 补全: completion tokens, with the reasoning share called out.
    Completion,
    /// 来源: the client endpoint and the host it was sent to.
    Source,
    /// 错误: only the HTTP status code; messages belong in the detail popup.
    /// The cache column already shows this code on failed rows, so the column
    /// only appears when the cache column is switched off.
    Error,
    /// 连接: which saved gateway the request came from.
    Account,
    /// 创建时间: relative timestamp.
    CreatedAt,
}

impl DisplayField {
    /// Every column, in settings order.
    pub const ALL: [DisplayField; 17] = [
        DisplayField::Model,
        DisplayField::UpstreamModel,
        DisplayField::Protocol,
        DisplayField::Agent,
        DisplayField::Provider,
        DisplayField::Tokens,
        DisplayField::Cache,
        DisplayField::CacheWrite,
        DisplayField::Speed,
        DisplayField::Retry,
        DisplayField::Cost,
        DisplayField::Latency,
        DisplayField::Completion,
        DisplayField::Source,
        DisplayField::Error,
        DisplayField::Account,
        DisplayField::CreatedAt,
    ];

    /// Name shown on the settings checkbox.
    pub fn label(self) -> &'static str {
        match self {
            DisplayField::Model => "模型",
            DisplayField::UpstreamModel => "声明模型 · 上游回复",
            DisplayField::Protocol => "协议 · 接口类型",
            DisplayField::Agent => "应用 · 发起方",
            DisplayField::Provider => "供应商",
            DisplayField::ProviderKey => "上游密钥 · 已并入供应商",
            DisplayField::Tokens => "词元 · Token 数",
            DisplayField::Cache => "缓存 · 命中率",
            DisplayField::CacheWrite => "写缓存 · 写入词元",
            DisplayField::Speed => "速度 · tps",
            DisplayField::Retry => "重试次数",
            DisplayField::Cost => "费用 · 估算",
            DisplayField::Latency => "耗时 · [首字·总]",
            DisplayField::Completion => "补全 · 输出词元",
            DisplayField::Source => "来源 · 入口与主机",
            DisplayField::Error => "错误 · HTTP 状态码",
            DisplayField::Account => "连接标签",
            DisplayField::CreatedAt => "创建时间",
        }
    }
}

/// What is written to the encrypted blob: one gateway key per connection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Stored {
    pub accounts: Vec<StoredAccount>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct StoredAccount {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl Stored {
    /// The stored gateway key for one connection, if any.
    pub fn key_for(&self, id: &str) -> Option<String> {
        self.accounts
            .iter()
            .find(|a| a.id == id)
            .and_then(|a| a.key.clone())
            .filter(|key| !key.is_empty())
    }

    /// Store a gateway key, creating the entry if needed. An empty key clears
    /// it instead.
    pub fn set_key(&mut self, id: &str, key: &str) {
        let entry = match self.accounts.iter_mut().find(|a| a.id == id) {
            Some(slot) => slot,
            None => {
                self.accounts.push(StoredAccount {
                    id: id.to_string(),
                    key: None,
                });
                self.accounts.last_mut().expect("just pushed")
            }
        };
        entry.key = (!key.is_empty()).then(|| key.to_string());
    }

    /// Forget one connection's secrets.
    pub fn forget_account(&mut self, id: &str) {
        self.accounts.retain(|a| a.id != id);
    }

    /// Whether anything worth encrypting is here.
    pub fn is_empty(&self) -> bool {
        self.accounts
            .iter()
            .all(|a| a.key.as_ref().map(|key| key.is_empty()).unwrap_or(true))
    }
}

/// Directory holding the config and the encrypted key blob.
///
/// `MAGPIE_PANEL_HOME` selects a profile; the old override remains compatible.
pub fn base_dir() -> PathBuf {
    if let Some(dir) = profile_override(
        std::env::var("MAGPIE_PANEL_HOME").ok().as_deref(),
        std::env::var("AH_PANEL_HOME").ok().as_deref(),
    ) {
        return dir;
    }
    let root = std::env::var("APPDATA")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    default_profile_dir(&root)
}

fn profile_override(current: Option<&str>, legacy: Option<&str>) -> Option<PathBuf> {
    current
        .filter(|s| !s.is_empty())
        .or_else(|| legacy.filter(|s| !s.is_empty()))
        .map(PathBuf::from)
}

fn default_profile_dir(root: &Path) -> PathBuf {
    let current = root.join("magpie-panel");
    let legacy = root.join("ah-panel");
    // Keep old profiles in place, including their DPAPI-encrypted keys. Only
    // existence is checked here; selecting a profile does not read its contents.
    if !current.join("config.json").is_file()
        && !current.join("credentials.bin").is_file()
        && (legacy.join("config.json").is_file() || legacy.join("credentials.bin").is_file())
    {
        legacy
    } else {
        current
    }
}

pub fn config_path() -> PathBuf {
    base_dir().join("config.json")
}

fn credentials_path_in(dir: &Path) -> PathBuf {
    dir.join("credentials.bin")
}

/// Outcome of a config load.
pub enum ConfigLoadStatus {
    Ok,
    Corrupt,
}

impl Config {
    /// Keep the existing two-flag format; bottom wins for conflicting old configs.
    pub fn window_level(&self) -> WindowLevel {
        if self.always_on_bottom {
            WindowLevel::Bottommost
        } else if self.always_on_top {
            WindowLevel::Topmost
        } else {
            WindowLevel::Normal
        }
    }

    pub fn set_window_level(&mut self, level: WindowLevel) {
        self.always_on_top = level == WindowLevel::Topmost;
        self.always_on_bottom = level == WindowLevel::Bottommost;
    }

    /// Load the config, reporting whether a corrupt file had to be discarded.
    pub fn load_with_status() -> (Config, ConfigLoadStatus) {
        match std::fs::read_to_string(config_path()) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                (Config::default(), ConfigLoadStatus::Ok)
            }
            Err(_) => {
                warn!(
                    "配置文件存在但无法读取,回退默认配置: {}",
                    config_path().display()
                );
                (Config::default(), ConfigLoadStatus::Corrupt)
            }
            Ok(raw) => match serde_json::from_str::<Config>(&raw) {
                Ok(mut config) => {
                    if !config.font_size.is_finite()
                        || !(FONT_SIZE_MIN..=FONT_SIZE_MAX).contains(&config.font_size)
                    {
                        config.font_size = 12.5;
                    }
                    if !(OPACITY_MIN..=OPACITY_MAX).contains(&config.opacity) {
                        config.opacity = 100;
                    }
                    if config.period.trim().is_empty() {
                        config.period = "30d".into();
                    }
                    (config, ConfigLoadStatus::Ok)
                }
                Err(_) => {
                    warn!(
                        "配置文件损坏,已备份并回退默认配置: {}",
                        config_path().display()
                    );
                    preserve_corrupt(&config_path(), &raw);
                    (Config::default(), ConfigLoadStatus::Corrupt)
                }
            },
        }
    }

    pub fn load() -> Self {
        Self::load_with_status().0
    }

    /// Save, returning the underlying I/O error instead of swallowing it.
    pub fn try_save(&self) -> std::io::Result<()> {
        let dir = base_dir();
        std::fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(config_path(), json)?;
        Ok(())
    }

    pub fn save(&self) {
        if let Err(e) = self.try_save() {
            warn!("配置保存失败: {e}");
        }
    }

    /// Persist the encrypted key blob.
    pub fn store(&self, stored: &Stored) {
        store_in(&base_dir(), stored);
    }

    /// The saved connection with this id.
    pub fn account(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    /// Insert or replace a connection, keeping the list in arrival order.
    pub fn upsert_account(&mut self, account: Account) {
        match self.accounts.iter_mut().find(|a| a.id == account.id) {
            Some(slot) => *slot = account,
            None => self.accounts.push(account),
        }
    }

    /// Drop one connection's metadata.
    pub fn remove_account(&mut self, id: &str) {
        self.accounts.retain(|a| a.id != id);
        self.hidden_providers.retain(|entry| entry.account_id != id);
    }

    /// An id no connection is using.
    pub fn fresh_account_id(&self) -> String {
        let mut n = self.accounts.len() + 1;
        loop {
            let id = format!("a{n}");
            if self.account(&id).is_none() {
                return id;
            }
            n += 1;
        }
    }

    /// Whether one connection's provider is hidden from the merged list.
    pub fn hides_provider(&self, account_id: &str, provider: &str) -> bool {
        self.hidden_providers
            .iter()
            .any(|h| h.account_id == account_id && h.provider == provider)
    }

    /// Hide a provider, or show it again if it was already hidden.
    pub fn toggle_hidden_provider(&mut self, account_id: &str, provider: &str) {
        let entry = HiddenProvider {
            account_id: account_id.to_string(),
            provider: provider.to_string(),
        };
        match self.hidden_providers.iter().position(|h| *h == entry) {
            Some(index) => {
                self.hidden_providers.remove(index);
            }
            None => self.hidden_providers.push(entry),
        }
    }

    /// The alias list one kind rewrites.
    pub fn aliases(&self, kind: AliasKind) -> &[NameAlias] {
        match kind {
            AliasKind::Model => &self.model_aliases,
            AliasKind::Provider => &self.provider_aliases,
            AliasKind::Agent => &self.agent_aliases,
        }
    }

    fn aliases_mut(&mut self, kind: AliasKind) -> &mut Vec<NameAlias> {
        match kind {
            AliasKind::Model => &mut self.model_aliases,
            AliasKind::Provider => &mut self.provider_aliases,
            AliasKind::Agent => &mut self.agent_aliases,
        }
    }

    /// Add an abbreviation, or update the short form of an existing `from`
    /// (compared case-insensitively) within one kind.
    pub fn upsert_alias(&mut self, kind: AliasKind, from: &str, to: &str) {
        let key = from.to_lowercase();
        let aliases = self.aliases_mut(kind);
        match aliases.iter_mut().find(|a| a.from.to_lowercase() == key) {
            Some(slot) => slot.to = to.to_string(),
            None => aliases.push(NameAlias {
                from: from.to_string(),
                to: to.to_string(),
            }),
        }
    }

    /// Drop the abbreviation at `index` of one kind.
    pub fn remove_alias(&mut self, kind: AliasKind, index: usize) {
        let aliases = self.aliases_mut(kind);
        if index < aliases.len() {
            aliases.remove(index);
        }
    }

    /// Whether the table shows this column.
    pub fn shows(&self, field: DisplayField) -> bool {
        !self.hidden_fields.contains(&field)
    }

    /// Switch a column off, or back on.
    pub fn toggle_field(&mut self, field: DisplayField) {
        match self.hidden_fields.iter().position(|f| *f == field) {
            Some(index) => {
                self.hidden_fields.remove(index);
            }
            None => self.hidden_fields.push(field),
        }
    }

    /// Show every column again.
    pub fn show_all_fields(&mut self) {
        self.hidden_fields.clear();
    }
}

/// The host part of an endpoint URL, for naming a connection the user has not
/// named themselves.
pub fn endpoint_host(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    let after_scheme = trimmed.split("://").last().unwrap_or(trimmed);
    let host = after_scheme.split('/').next().unwrap_or(after_scheme);
    if host.is_empty() {
        trimmed.to_string()
    } else {
        host.to_string()
    }
}

/// Keep a byte-for-byte copy of a config file we failed to parse.
fn preserve_corrupt(path: &Path, raw: &str) {
    let corrupt_path = path.with_extension("json.corrupt");
    let target = if corrupt_path.exists() {
        path.with_extension(format!("json.corrupt.{}", unix_now()))
    } else {
        corrupt_path
    };

    if std::fs::rename(path, &target).is_ok() {
        warn!("已备份损坏的配置文件到: {}", target.display());
        return;
    }
    if std::fs::write(&target, raw.as_bytes()).is_ok() {
        warn!("已备份损坏的配置文件到: {}", target.display());
        let _ = std::fs::remove_file(path);
        return;
    }
    warn!("无法备份损坏的配置文件(字节已丢失): {}", path.display());
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Write the key blob into `dir`, or clear what is there when nothing is left.
fn store_in(dir: &Path, stored: &Stored) {
    if stored.is_empty() {
        clear_stored_in(dir);
        return;
    }
    let Ok(plain) = serde_json::to_vec(stored) else {
        return;
    };
    let Some(blob) = dpapi::protect(&plain) else {
        // 绝对不能把明文网关密钥落到磁盘，因此直接放弃写入。
        warn!("密钥加密(DPAPI)失败,未写入凭据文件");
        return;
    };
    if std::fs::create_dir_all(dir).is_err() {
        warn!("无法创建凭据目录,未写入凭据: {}", dir.display());
        return;
    }
    if let Err(e) = std::fs::write(credentials_path_in(dir), blob) {
        warn!("凭据文件写入失败: {e}");
    }
}

/// Read a gateway key from the configured environment variable, if it is set
/// and non-empty. Checked before the stored blob so an environment override
/// always wins.
pub fn key_from_env(var: &str) -> Option<String> {
    let name = var.trim();
    if name.is_empty() {
        return None;
    }
    let value = std::env::var(name).ok()?;
    let trimmed = value.trim().trim_start_matches("Bearer ").trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Decrypt the stored record. `None` covers "nothing stored" and "unreadable".
pub fn load_stored() -> Option<Stored> {
    read_stored_in(&base_dir())
}

fn read_stored_in(dir: &Path) -> Option<Stored> {
    let blob = std::fs::read(credentials_path_in(dir)).ok()?;
    let plain = match dpapi::unprotect(&blob) {
        Some(p) => p,
        None => {
            warn!("凭据解密(DPAPI)失败,忽略已保存凭据");
            return None;
        }
    };
    match serde_json::from_slice(&plain) {
        Ok(s) => Some(s),
        Err(_) => {
            warn!("凭据文件内容无法解析,忽略已保存凭据");
            None
        }
    }
}

/// Forget the stored gateway keys. Deliberately does not touch `config.json`.
pub fn clear_stored() {
    clear_stored_in(&base_dir());
}

fn clear_stored_in(dir: &Path) {
    let _ = std::fs::remove_file(credentials_path_in(dir));
}

/// Smallest window the panel allows, in device pixels.
pub fn min_window_size(scale: f32) -> (i32, i32) {
    (
        crate::ui::layout::device_px(320, scale),
        crate::ui::layout::device_px(80, scale),
    )
}

/// Bring the window back on screen if a display change orphaned it, shrinking it
/// to fit the work area when the display got smaller.
pub fn clamp_to_virtual_screen(
    state: WindowState,
    screen: (i32, i32, i32, i32),
    scale: f32,
) -> WindowState {
    let (left, top, right, bottom) = screen;
    let (min_w, min_h) = min_window_size(scale);
    let width = state.width.clamp(min_w, (right - left).max(min_w));
    let height = state.height.clamp(min_h, (bottom - top).max(min_h));
    let x = state.x.clamp(left, (right - width).max(left));
    let y = state.y.clamp(top, (bottom - height).max(top));
    WindowState {
        x,
        y,
        width,
        height,
    }
}

/// Minimal DPAPI wrapper: current-user scope, no UI prompts.
mod dpapi {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    };
    use windows::core::PCWSTR;

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        }
    }

    fn take(out: CRYPT_INTEGER_BLOB) -> Option<Vec<u8>> {
        if out.pbData.is_null() {
            return None;
        }
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec() };
        unsafe {
            let _ = LocalFree(Some(HLOCAL(out.pbData as *mut _)));
        }
        Some(bytes)
    }

    pub fn protect(plain: &[u8]) -> Option<Vec<u8>> {
        unsafe {
            let input = blob(plain);
            let mut out = CRYPT_INTEGER_BLOB::default();
            CryptProtectData(
                &input,
                PCWSTR::null(),
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .ok()?;
            take(out)
        }
    }

    pub fn unprotect(blob_bytes: &[u8]) -> Option<Vec<u8>> {
        unsafe {
            let input = blob(blob_bytes);
            let mut out = CRYPT_INTEGER_BLOB::default();
            CryptUnprotectData(
                &input,
                None,
                None,
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
            .ok()?;
            take(out)
        }
    }
}

#[cfg(test)]
#[path = "tests/config.rs"]
mod tests;
