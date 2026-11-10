//! HTTP access to a Magpie gateway: the usage log and the live concurrency
//! feed.
//!
//! Both are public gateway endpoints on the same listener as the inference
//! routes (default `127.0.0.1:3425`). Loopback requests need no credentials;
//! a gateway shared over the network needs a Magpie gateway key.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::time::{Duration, Instant};

use tracing::warn;

use crate::model::{
    ActiveRequest, Attempt, Concurrency, DetailSource, ExecutionDetail, LiveTracePage, Row,
    TracePage, UsagePage, UsageRecord, history_anchor,
};

const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const DETAIL_PAGE_SIZE: usize = 500;
const DETAIL_MAX_PAGES: usize = 4;
const DETAIL_BUDGET: Duration = Duration::from_secs(20);

/// Cached subscription accounts only. No token, key, balance or error fields
/// are retained, and `kept=1` never asks a vendor to refresh its quota.
#[derive(Default, serde::Deserialize)]
struct AccountInventory {
    #[serde(default)]
    data: Vec<SubscriptionAccount>,
}

#[derive(serde::Deserialize)]
struct SubscriptionAccount {
    #[serde(default)]
    provider: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    user: Option<String>,
}

fn subscription_accounts(inventory: AccountInventory) -> HashMap<String, HashSet<String>> {
    let mut accounts: HashMap<String, HashSet<String>> = HashMap::new();
    for entry in inventory.data {
        let provider = entry.provider.trim();
        if entry.kind != "subscription" || provider.is_empty() {
            continue;
        }
        if let Some(user) = entry
            .user
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            accounts
                .entry(provider.into())
                .or_default()
                .insert(user.into());
        }
    }
    accounts
}

/// Every failure the panel can show, with the message already localized.
#[derive(Debug, Clone)]
pub enum ApiError {
    Unreachable(String),
    Rejected(String),
    /// The gateway key is missing, wrong, or not allowed here.
    Unauthorized,
    Malformed(String),
}

impl ApiError {
    pub fn message(&self) -> String {
        match self {
            ApiError::Unreachable(d) => format!("无法连接 Magpie: {d}"),
            ApiError::Rejected(m) => format!("请求被拒绝: {m}"),
            ApiError::Unauthorized => "网关密钥无效或不允许读取日志".into(),
            ApiError::Malformed(m) => format!("响应无法解析: {m}"),
        }
    }
}

#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
}

impl Client {
    pub fn new(timeout: Duration) -> Self {
        // Windows-only: TLS goes through schannel and the Windows trust store,
        // so a gateway behind an enterprise CA or a TLS-terminating proxy
        // validates the same way a browser does. Both the provider and the root
        // store must be named explicitly; omitting either leaves https broken.
        let tls = ureq::tls::TlsConfig::builder()
            .provider(ureq::tls::TlsProvider::NativeTls)
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            // Never follow a loopback trace redirect to a different service.
            .max_redirects(0)
            .tls_config(tls)
            .build();
        Client {
            agent: config.new_agent(),
        }
    }

    /// The usage log, newest first. `limit` is capped by the gateway at 500.
    pub fn usage(
        &self,
        endpoint: &str,
        key: Option<&str>,
        period: &str,
        limit: i64,
    ) -> Result<UsagePage, ApiError> {
        let url = format!(
            "{}/v1/magpie/usage/requests?period={}&limit={}&offset=0",
            endpoint.trim_end_matches('/'),
            encode_query(period),
            limit.clamp(1, 500)
        );
        self.get_json(&url, key)
    }

    /// Best-effort inventory on the same gateway listener; never refresh quotas
    /// or fall back to a console port when permissions deny it.
    pub fn provider_accounts(
        &self,
        endpoint: &str,
        key: Option<&str>,
    ) -> Result<HashMap<String, HashSet<String>>, ApiError> {
        let url = format!("{}/v1/magpie/quotas?kept=1", endpoint.trim_end_matches('/'));
        self.get_json(&url, key).map(subscription_accounts)
    }

    /// The in-flight requests. A restricted gateway key is refused this
    /// endpoint; the caller treats that as "nothing live", not as a failure.
    pub fn concurrency(
        &self,
        endpoint: &str,
        key: Option<&str>,
    ) -> Result<Vec<ActiveRequest>, ApiError> {
        let url = format!("{}/v1/magpie/concurrency", endpoint.trim_end_matches('/'));
        let page: Concurrency = self.get_json(&url, key)?;
        Ok(page.active)
    }

    /// Best-effort effort and attempt metadata from the same explicit loopback
    /// gateway. Failure never discards concurrency rows or changes connection health.
    pub fn enrich_live_metadata(
        &self,
        endpoint: &str,
        key: Option<&str>,
        active: &mut [ActiveRequest],
    ) {
        load_live_metadata(endpoint, active, || {
            self.get_json::<LiveTracePage>(
                &format!("{}/v1/magpie/trace", endpoint.trim_end_matches('/')),
                key,
            )
        });
    }

    /// A bounded detail round trip. Failures are data for the popup, never
    /// authentication/connection-health changes. Same origin only; never guess
    /// a console port or substitute an unrestricted credential.
    pub fn detail(&self, endpoint: &str, key: Option<&str>, row: &Row) -> ExecutionDetail {
        let started = Instant::now();
        load_detail(
            row,
            || {
                if !is_loopback(endpoint) {
                    return Err("完整 trace 仅支持显式本机回环地址".into());
                }
                self.get_json::<TracePage>(
                    &format!("{}/v1/magpie/trace", endpoint.trim_end_matches('/')),
                    key,
                )
                .map_err(|error| format!("trace {}", detail_error(&error)))
            },
            |offset| {
                if started.elapsed() >= DETAIL_BUDGET {
                    return Err("详情加载已达到时间上限".into());
                }
                let url = format!(
                    "{}/v1/magpie/usage/requests?period=all&route={}&limit={DETAIL_PAGE_SIZE}&offset={offset}",
                    endpoint.trim_end_matches('/'),
                    row.route_id.unwrap_or_default()
                );
                self.get_json::<UsagePage>(&url, key)
                    .map_err(|error| format!("历史摘要 {}", detail_error(&error)))
            },
        )
    }

    /// One authenticated GET returning JSON.
    fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        key: Option<&str>,
    ) -> Result<T, ApiError> {
        let mut request = self.agent.get(url);
        if let Some(key) = key.map(str::trim).filter(|key| !key.is_empty()) {
            request = request.header("Authorization", &format!("Bearer {key}"));
        }
        let response = request.call().map_err(map_transport)?;
        let mut body = response.into_body();
        let mut text = String::new();
        body.as_reader()
            .take(MAX_JSON_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|_| ApiError::Malformed("读取响应失败或编码无效".into()))?;
        if text.len() as u64 > MAX_JSON_BYTES {
            return Err(ApiError::Malformed(
                "响应超过 8 MiB 上限，未加载完整记录".into(),
            ));
        }
        serde_json::from_str(&text).map_err(|e| ApiError::Malformed(e.to_string()))
    }
}

/// Safe popup reason: never include transport URLs, credentials or JSON/error
/// body fragments. Only fixed messages and known HTTP status labels escape.
fn detail_error(error: &ApiError) -> &'static str {
    match error {
        ApiError::Unauthorized => "无权限（HTTP 401 / 受限密钥）",
        ApiError::Rejected(message) if message == "HTTP 404" => "接口不支持（HTTP 404）",
        ApiError::Rejected(message) if message == "网关未开启该访问方式" => {
            "无权限（HTTP 403）"
        }
        ApiError::Rejected(_) => "被服务端拒绝",
        ApiError::Malformed(message) if message.starts_with("响应超过") => {
            "响应超过 8 MiB 上限，记录未完整加载"
        }
        ApiError::Malformed(_) => "响应格式不支持或无法解析",
        ApiError::Unreachable(_) => "网络读取失败或超时",
    }
}

/// Literal loopback only. Do not resolve arbitrary host names or infer a
/// loopback service from a remote origin; the gateway still decides permissions.
fn is_loopback(endpoint: &str) -> bool {
    let Ok(uri) = endpoint.parse::<ureq::http::Uri>() else {
        return false;
    };
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return false;
    }
    let Some(host) = uri.host() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// No extra read for remote gateways, empty feeds or requests without a
/// verifiable route identity.
fn load_live_metadata(
    endpoint: &str,
    active: &mut [ActiveRequest],
    trace: impl FnOnce() -> Result<LiveTracePage, ApiError>,
) {
    let can_enrich = |request: &ActiveRequest| {
        request.route.is_some_and(|id| id > 0)
            && request
                .started
                .as_deref()
                .and_then(crate::time::parse_unix)
                .is_some()
    };
    if !is_loopback(endpoint) || !active.iter().any(can_enrich) {
        return;
    }
    let Ok(page) = trace() else { return };
    for request in active.iter_mut().filter(|r| can_enrich(r)) {
        request.trace_attempts = 0;
        request.trace_status = None;
        request.trace_error = None;
        request.trace_fail = None;
        request.trace_provider = None;
        request.trace_provider_name = None;
        request.trace_provider_account = None;
        request.trace_provider_key = None;
        let mut matches = page.routes.iter().filter(|route| route.matches(request));
        if let Some(route) = matches.next()
            && matches.next().is_none()
        {
            if request
                .effort
                .as_deref()
                .is_none_or(|effort| effort.trim().is_empty())
            {
                request.effort = route
                    .effort
                    .as_deref()
                    .map(str::trim)
                    .filter(|effort| !effort.is_empty())
                    .map(str::to_string);
            }
            request.trace_attempts = i64::try_from(route.tries.len()).unwrap_or(i64::MAX);
            // The feed's provider is only the initial route. Candidate order
            // is not execution order; the last try identifies the actual provider.
            if let Some(attempt) = route.tries.last().filter(|t| !t.id.trim().is_empty()) {
                let mut candidates = route.order.iter().filter(|c| c.id == attempt.id);
                if let Some(candidate) = candidates.next()
                    && candidates.next().is_none()
                    && let Some(provider) = candidate
                        .provider
                        .as_deref()
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                {
                    request.trace_provider = Some(provider.to_string());
                    request.trace_provider_name = candidate.name.clone();
                    match candidate.kind.as_deref() {
                        Some("account") => request.trace_provider_account = candidate.who.clone(),
                        Some("key") => request.trace_provider_key = candidate.who.clone(),
                        _ => {}
                    }
                }
            }
            if let Some(failure) = route.tries.iter().rev().find(|attempt| {
                attempt.status.is_some_and(|status| status >= 400)
                    || attempt
                        .fail
                        .as_deref()
                        .is_some_and(|fail| !fail.trim().is_empty())
                    || attempt
                        .error
                        .as_deref()
                        .is_some_and(|error| !error.trim().is_empty())
            }) {
                request.trace_status = failure.status;
                request.trace_error = failure.error.clone();
                request.trace_fail = failure.fail.clone();
            }
        }
    }
}

/// Transport injection keeps identity, failure and paging tests offline.
fn load_detail(
    row: &Row,
    trace: impl FnOnce() -> Result<TracePage, String>,
    mut usage: impl FnMut(usize) -> Result<UsagePage, String>,
) -> ExecutionDetail {
    let Some(route_id) = row.route_id.filter(|id| *id > 0) else {
        return ExecutionDetail::unavailable("记录没有 route_id；仅显示当前行错误摘要");
    };
    let reason = match trace() {
        Ok(page) => match page.routes.iter().find(|route| route.matches(row)) {
            Some(route) if !route.tries.is_empty() => {
                return ExecutionDetail::from_route(route, row);
            }
            _ => "近期缓存没有匹配的尝试记录（可能已过期）".into(),
        },
        Err(reason) => reason,
    };
    let mut records: Vec<UsageRecord> = Vec::new();
    let mut offset = 0;
    let mut truncated = true;
    let mut paging_error = None;
    for _ in 0..DETAIL_MAX_PAGES {
        let page = match usage(offset) {
            Ok(page) => page,
            Err(error) => {
                paging_error = Some(error);
                break;
            }
        };
        let count = page.rows.len();
        // Also cap misbehaving servers which ignore limit. Never silently drop.
        let oversized = count > DETAIL_PAGE_SIZE;
        records.extend(
            page.rows
                .into_iter()
                .take(DETAIL_PAGE_SIZE)
                .filter(|r| r.route_id == Some(route_id)),
        );
        offset += count.min(DETAIL_PAGE_SIZE);
        if oversized {
            break;
        }
        if count == 0
            || page
                .total
                .is_some_and(|total| total >= 0 && offset as i64 >= total)
            || (page.total.is_none() && count < DETAIL_PAGE_SIZE)
        {
            truncated = false;
            break;
        }
    }
    if !records.iter().any(|record| history_anchor(record, row)) {
        return ExecutionDetail::unavailable(format!(
            "{reason}；历史摘要未匹配当前行的时间/尝试身份{}",
            paging_error
                .as_ref()
                .map(|e| format!("；{e}"))
                .unwrap_or_else(|| if truncated {
                    "（分页未完成或已截断）".into()
                } else {
                    String::new()
                })
        ));
    }
    // The exact row anchors the route. Its earlier tries may cross midnight;
    // filtering by the selected day would lose those failures.
    records.retain(|r| {
        let same = |raw: Option<&str>, expected: Option<&str>| {
            raw.map(str::trim).filter(|s| !s.is_empty()) == expected
        };
        same(r.session.as_deref(), row.session.as_deref())
            && same(r.agent.as_deref(), row.agent_id.as_deref())
    });
    records.sort_by(|a, b| {
        let instant = |record: &UsageRecord| record.t.as_deref().and_then(crate::time::parse_unix);
        // A final row's t is the request start, not the final try's start.
        // Handed-on attempts must therefore precede it even when t is later.
        b.handed_on
            .unwrap_or(false)
            .cmp(&a.handed_on.unwrap_or(false))
            .then_with(|| instant(a).cmp(&instant(b)))
            .then_with(|| a.t.cmp(&b.t))
    });
    let attempts = records.iter().map(Attempt::from_record).collect();
    let mut notice = format!(
        "历史错误摘要（可能不完整）：{reason}。账本不覆盖全部重试，错误文本可能已截短；非原始 HTTP 响应正文。"
    );
    if truncated {
        notice.push_str(" 分页已截断或未完成（最多 4 页 / 2000 条）。");
    }
    if let Some(error) = paging_error {
        notice.push_str(&format!(" {error}。"));
    }
    ExecutionDetail {
        source: DetailSource::History,
        attempts,
        route_error: None,
        route_status: None,
        notice,
    }
}

/// Percent-encode a query value, keeping the characters a period expression
/// legitimately uses (`:`, `.`, `-`, `..`).
fn encode_query(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.trim().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b':' | b'.' | b'-' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Map a transport failure to a message the status strip can show.
fn map_transport(err: ureq::Error) -> ApiError {
    match err {
        ureq::Error::StatusCode(code) => match code {
            401 => ApiError::Unauthorized,
            403 => ApiError::Rejected("网关未开启该访问方式".into()),
            429 => ApiError::Rejected("请求过于频繁".into()),
            _ => {
                warn!("请求被服务端拒绝: HTTP {code}");
                ApiError::Rejected(format!("HTTP {code}"))
            }
        },
        ureq::Error::Timeout(_) => {
            warn!("连接 Magpie 超时");
            ApiError::Unreachable("连接超时".into())
        }
        // TLS failures get their own message: the raw error is an OS string
        // that tells a Chinese-speaking user nothing about what to check.
        ureq::Error::Tls(_) | ureq::Error::NativeTls(_) => {
            warn!("TLS 握手失败: {err}");
            ApiError::Unreachable("TLS 握手失败:证书不受信任或已过期".into())
        }
        other => {
            warn!("传输层错误: {other}");
            ApiError::Unreachable(other.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_inventory_counts_distinct_subscription_users_not_api_keys() {
        let inventory = serde_json::from_value(serde_json::json!({"data":[
            {"provider":"multi", "kind":"subscription", "user":" first "},
            {"provider":"multi", "kind":"subscription", "user":"first"},
            {"provider":"multi", "kind":"subscription", "user":"second"},
            {"provider":"single", "kind":"subscription", "user":"first"},
            {"provider":"multi", "kind":"subscription", "user":" "},
            {"provider":" ", "kind":"subscription", "user":"ignored"},
            {"provider":"api", "kind":"plan", "user":"key-one"},
            {"provider":"api", "kind":"balance", "user":"key-two"},
            {"provider":"unknown", "kind":"subscription"}
        ]}))
        .unwrap();
        let accounts = subscription_accounts(inventory);
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts["multi"].len(), 2);
        assert_eq!(accounts["single"].len(), 1);
        assert!(subscription_accounts(serde_json::from_str("{}").unwrap()).is_empty());
    }

    #[test]
    fn period_values_keep_their_punctuation() {
        assert_eq!(encode_query("last:24h"), "last:24h");
        assert_eq!(
            encode_query("2026-10-01..2026-10-28"),
            "2026-10-01..2026-10-28"
        );
        assert_eq!(encode_query(" 30d "), "30d");
    }

    fn selected() -> (UsageRecord, Row) {
        let record: UsageRecord = serde_json::from_str(r#"{"t":"2026-10-20T09:00:00Z","session":"s","agent":"cli","model":"m","provider":"p","route_id":7,"status":200,"tries":3}"#).unwrap();
        let row = Row::from_record(&record).with_account("a1", "one");
        (record, row)
    }

    fn live_request() -> ActiveRequest {
        serde_json::from_str(r#"{"route":7,"started":"2026-10-20T09:00:00Z","session":"s","agent":"cli","model":"m"}"#).unwrap()
    }

    fn live_trace() -> serde_json::Value {
        serde_json::json!({"routes":[{
            "id":7,"time":"2026-10-20T09:00:00Z","session":"s","agent":"cli","model":"m","effort":" high ",
            "tries":[
                {"model":"m","done":true,"status":429,"fail":"rate_limit","error":"try again"},
                {"model":"m","done":false}
            ]
        }]})
    }

    #[test]
    fn local_trace_enriches_live_effort_and_attempts_without_overwriting_the_feed() {
        for endpoint in [
            "http://localhost:3425",
            "http://127.0.0.1:3425",
            "http://[::1]:3425",
        ] {
            let mut known = live_request();
            known.effort = Some("low".into());
            let mut active = vec![live_request(), known];
            load_live_metadata(endpoint, &mut active, || {
                Ok(serde_json::from_value(live_trace()).unwrap())
            });
            assert_eq!(active[0].effort.as_deref(), Some("high"));
            assert_eq!(active[1].effort.as_deref(), Some("low"));
            for request in &active {
                assert_eq!(request.trace_attempts, 2);
                assert_eq!(request.trace_status, Some(429));
                assert_eq!(request.trace_fail.as_deref(), Some("rate_limit"));
                assert_eq!(request.trace_error.as_deref(), Some("try again"));
            }
            let row = Row::from_active(&active[0]);
            assert!(row.live);
            assert_eq!(row.display_model(), "m(high)");
            assert_eq!(row.tries, 2);
            assert_eq!(row.status_code, Some(429));
            assert!(row.is_error());
        }
    }

    fn provider_trace() -> serde_json::Value {
        serde_json::json!({"routes":[{
            "id":7,"time":"2026-10-20T09:00:00Z","session":"s","agent":"cli","model":"m",
            "order":[
                {"id":"workbuddy","provider":"workbuddy","name":"WorkBuddy"},
                {"id":"trae-account","provider":"trae-cn","name":"Trae CN"}
            ],
            "tries":[{"id":"trae-account","done":false}]
        }]})
    }

    #[test]
    fn live_provider_comes_from_the_current_attempt_not_the_initial_route() {
        let mut request = live_request();
        request.provider = Some("workbuddy".into());
        let mut active = vec![request];
        load_live_metadata("http://localhost:3425", &mut active, || {
            Ok(serde_json::from_value(provider_trace()).unwrap())
        });
        assert_eq!(active[0].provider.as_deref(), Some("workbuddy"));
        let row = Row::from_active(&active[0]);
        assert_eq!(row.provider_id.as_deref(), Some("trae-cn"));
        assert_eq!(row.provider.as_deref(), Some("Trae CN"));
        assert_eq!(row.tries, 1);
        assert!(!row.is_error());
        let finished = Row::from_record(
            &serde_json::from_value(serde_json::json!({
                "provider":"trae-cn","providerName":"Trae CN","status":200
            }))
            .unwrap(),
        );
        assert_eq!(row.provider, finished.provider);
        assert_eq!(row.provider_id, finished.provider_id);
    }

    #[test]
    fn live_provider_tracks_the_last_attempt_and_not_candidate_order() {
        let mut page = provider_trace();
        page["routes"][0]["tries"] = serde_json::json!([
            {"id":"trae-account","done":true,"status":429},
            {"id":"workbuddy","done":false}
        ]);
        let mut active = vec![live_request()];
        load_live_metadata("http://localhost:3425", &mut active, || {
            Ok(serde_json::from_value(page).unwrap())
        });
        let row = Row::from_active(&active[0]);
        assert_eq!(row.provider_id.as_deref(), Some("workbuddy"));
        assert_eq!(row.provider.as_deref(), Some("WorkBuddy"));
        assert_eq!(row.tries, 2);
        assert_eq!(row.status_code, Some(429));
    }

    #[test]
    fn live_account_and_key_labels_follow_the_actual_attempt_and_clear_on_switch() {
        let mut active = vec![live_request()];
        for (kind, who, expected_account, expected_key, expected_label) in [
            ("account", "first", Some("first"), None, "Trae CN(first)"),
            (
                "account",
                " second ",
                Some("second"),
                None,
                "Trae CN(second)",
            ),
            (
                "key",
                "main-key",
                None,
                Some("main-key"),
                "Trae CN(main-key)",
            ),
            ("provider", "not-an-account", None, None, "Trae CN"),
            ("account", " ", None, None, "Trae CN"),
        ] {
            let mut page = provider_trace();
            page["routes"][0]["order"][0]["kind"] = serde_json::json!("account");
            page["routes"][0]["order"][0]["who"] = serde_json::json!("not-the-current-account");
            page["routes"][0]["order"][1]["kind"] = serde_json::json!(kind);
            page["routes"][0]["order"][1]["who"] = serde_json::json!(who);
            load_live_metadata("http://localhost:3425", &mut active, || {
                Ok(serde_json::from_value(page).unwrap())
            });
            let mut row = Row::from_active(&active[0]);
            row.multi_account_providers = std::sync::Arc::new(HashSet::from(["trae-cn".into()]));
            assert_eq!(row.provider_account.as_deref(), expected_account);
            assert_eq!(row.provider_key.as_deref(), expected_key);
            assert_eq!(row.provider_label().as_deref(), Some(expected_label));
        }
        active[0].trace_provider_account = Some("stale-account".into());
        active[0].trace_provider_key = Some("stale-key".into());
        let mut page = provider_trace();
        page["routes"][0]["tries"] = serde_json::json!([{"id":"missing"}]);
        load_live_metadata("http://localhost:3425", &mut active, || {
            Ok(serde_json::from_value(page).unwrap())
        });
        let row = Row::from_active(&active[0]);
        assert!(row.provider_account.is_none());
        assert!(row.provider_key.is_none());
    }

    #[test]
    fn live_provider_requires_a_unique_candidate_with_a_provider_id() {
        for patch in [
            serde_json::json!({"tries":[]}),
            serde_json::json!({"tries":[{"id":"missing","done":false}]}),
            serde_json::json!({"order":[]}),
            serde_json::json!({"order":[{"id":"trae-account","name":"Trae CN"}]}),
            serde_json::json!({"order":[
                {"id":"trae-account","provider":"trae-cn","name":"Trae CN"},
                {"id":"trae-account","provider":"different","name":"Other"}
            ]}),
            serde_json::json!({"time":"2026-10-19T09:00:00Z"}),
        ] {
            let mut page = provider_trace();
            for (key, value) in patch.as_object().unwrap() {
                page["routes"][0][key] = value.clone();
            }
            let mut request = live_request();
            request.provider = Some("workbuddy".into());
            let mut active = vec![request];
            load_live_metadata("http://localhost:3425", &mut active, || {
                Ok(serde_json::from_value(page).unwrap())
            });
            let row = Row::from_active(&active[0]);
            assert_eq!(row.provider_id.as_deref(), Some("workbuddy"), "{patch}");
            assert_eq!(row.provider.as_deref(), Some("workbuddy"), "{patch}");
        }
    }

    #[test]
    fn live_trace_is_not_requested_for_remote_or_unverifiable_requests() {
        for endpoint in [
            "https://example.com",
            "http://192.168.1.1:3425",
            "http://localhost.example",
            "not-a-url",
        ] {
            load_live_metadata(endpoint, &mut [live_request()], || {
                panic!("must not fetch remote trace")
            });
        }
        load_live_metadata("http://localhost:3425", &mut [], || {
            panic!("must not fetch an empty feed")
        });
        for patch in [
            serde_json::json!({"route":null}),
            serde_json::json!({"route":0}),
            serde_json::json!({"started":null}),
            serde_json::json!({"started":"invalid"}),
        ] {
            let mut request = serde_json::json!({
                "route":7,"started":"2026-10-20T09:00:00Z","model":"m"
            });
            for (key, value) in patch.as_object().unwrap() {
                request[key] = value.clone();
            }
            let mut active = vec![serde_json::from_value(request).unwrap()];
            load_live_metadata("http://localhost:3425", &mut active, || {
                panic!("must not fetch unverifiable metadata")
            });
        }
    }

    #[test]
    fn live_effort_requires_unique_verified_route_identity() {
        for (field, value) in [
            ("id", serde_json::json!(8)),
            ("time", serde_json::json!("2026-10-19T09:00:00Z")),
            ("time", serde_json::json!(null)),
            ("session", serde_json::json!("other")),
            ("session", serde_json::json!(null)),
            ("agent", serde_json::json!("other")),
            ("model", serde_json::json!("other")),
        ] {
            let mut page = live_trace();
            page["routes"][0][field] = value;
            let mut active = vec![live_request()];
            load_live_metadata("http://localhost:3425", &mut active, || {
                Ok(serde_json::from_value(page).unwrap())
            });
            assert!(active[0].effort.is_none(), "mismatched {field}");
        }
        let mut page = live_trace();
        let duplicate = page["routes"][0].clone();
        page["routes"].as_array_mut().unwrap().push(duplicate);
        let mut active = vec![live_request()];
        load_live_metadata("http://localhost:3425", &mut active, || {
            Ok(serde_json::from_value(page).unwrap())
        });
        assert!(active[0].effort.is_none());
    }

    #[test]
    fn unavailable_or_empty_live_trace_keeps_requests_and_unknown_effort() {
        for error in [
            ApiError::Unauthorized,
            ApiError::Rejected("HTTP 404".into()),
            ApiError::Unreachable("timeout".into()),
            ApiError::Malformed("invalid JSON".into()),
        ] {
            let mut active = vec![live_request()];
            load_live_metadata("http://localhost:3425", &mut active, || Err(error));
            assert_eq!(active.len(), 1);
            assert!(Row::from_active(&active[0]).live);
            assert!(active[0].effort.is_none());
        }
        for effort in [
            serde_json::json!(null),
            serde_json::json!(""),
            serde_json::json!("  "),
        ] {
            let mut page = live_trace();
            page["routes"][0]["effort"] = effort;
            let mut active = vec![live_request()];
            load_live_metadata("http://localhost:3425", &mut active, || {
                Ok(serde_json::from_value(page).unwrap())
            });
            assert!(active[0].effort.is_none());
        }
    }

    #[test]
    fn loopback_detection_covers_localhost_ipv4_ipv6_without_remote_dns() {
        for url in [
            "http://localhost:3425",
            "http://127.0.0.1:3425",
            "http://127.1.2.3:3425",
            "http://[::1]:3425",
        ] {
            assert!(is_loopback(url), "{url}");
        }
        for url in [
            "http://192.168.1.1:3425",
            "https://example.com",
            "http://localhost.example",
            "not-a-url",
            "ftp://localhost",
        ] {
            assert!(!is_loopback(url), "{url}");
        }
    }

    #[test]
    fn missing_route_never_invokes_any_transport() {
        let row = Row::from_record(&UsageRecord::default());
        let detail = load_detail(
            &row,
            || panic!("must not fetch trace"),
            |_| panic!("must not fetch usage"),
        );
        assert!(matches!(detail.source, DetailSource::Unavailable));
        assert!(detail.notice.contains("没有 route_id"));
    }

    #[test]
    fn matched_trace_preserves_three_attempts_and_skips_history() {
        let (_, row) = selected();
        let page: TracePage = serde_json::from_str(r#"{"routes":[{"id":8,"time":"2026-10-20T09:00:00Z","tries":[{"error":"wrong route"}]},{"id":7,"time":"2026-10-20T09:00:00Z","session":"s","agent":"cli","done":true,"tries":[{"id":"same","model":"m","done":true,"status":429,"error":"first"},{"id":"same","model":"m","done":true,"status":502,"error":"second"},{"id":"same","model":"m","done":true,"status":200}]}]}"#).unwrap();
        let detail = load_detail(&row, || Ok(page), |_| panic!("must not fetch history"));
        assert!(matches!(detail.source, DetailSource::Trace));
        assert_eq!(detail.attempts.len(), 3);
        assert_eq!(detail.attempts[0].error.as_deref(), Some("first"));
        assert_eq!(detail.attempts[1].error.as_deref(), Some("second"));
    }

    #[test]
    fn empty_403_and_404_trace_fall_back_to_explicit_incomplete_ledger() {
        for result in [
            Ok(TracePage::default()),
            Err("HTTP 403".into()),
            Err("HTTP 404".into()),
        ] {
            let (record, row) = selected();
            let mut fail = record.clone();
            fail.status = Some(429);
            fail.err_type = Some("rate_limit_error".into());
            fail.err = Some("previous fail".into());
            fail.handed_on = Some(true);
            let detail = load_detail(
                &row,
                || result,
                |offset| {
                    assert_eq!(offset, 0);
                    Ok(UsagePage {
                        rows: vec![record.clone(), fail.clone()],
                        total: Some(2),
                    })
                },
            );
            assert!(matches!(detail.source, DetailSource::History));
            assert_eq!(detail.attempts.len(), 2);
            assert!(detail.notice.contains("可能不完整"));
            assert!(detail.notice.contains("账本不覆盖全部重试"));
        }
    }

    #[test]
    fn bounded_pagination_never_silently_loses_attempts() {
        let (record, row) = selected();
        let mut offsets = Vec::new();
        let detail = load_detail(
            &row,
            || Err("HTTP 403".into()),
            |offset| {
                offsets.push(offset);
                Ok(UsagePage {
                    rows: vec![record.clone(); 500],
                    total: Some(2500),
                })
            },
        );
        assert_eq!(offsets, vec![0, 500, 1000, 1500]);
        assert_eq!(detail.attempts.len(), 2000);
        assert!(detail.notice.contains("分页已截断"));
    }

    #[test]
    fn second_page_anchor_and_partial_page_failure_are_supported() {
        let (record, row) = selected();
        let mut unrelated = record.clone();
        unrelated.provider = Some("other".into());
        let detail = load_detail(
            &row,
            || Ok(TracePage::default()),
            |offset| {
                Ok(UsagePage {
                    rows: if offset == 0 {
                        vec![unrelated.clone(); 500]
                    } else {
                        vec![record.clone()]
                    },
                    total: Some(501),
                })
            },
        );
        assert!(matches!(detail.source, DetailSource::History));
        assert_eq!(detail.attempts.len(), 501);
        assert!(!detail.notice.contains("分页已截断"));
        let detail = load_detail(
            &row,
            || Err("HTTP 404".into()),
            |offset| {
                if offset == 0 {
                    Ok(UsagePage {
                        rows: vec![record.clone(); 500],
                        total: Some(501),
                    })
                } else {
                    Err("timeout".into())
                }
            },
        );
        assert_eq!(detail.attempts.len(), 500);
        assert!(detail.notice.contains("未完成"));
        assert!(detail.notice.contains("timeout"));
    }

    #[test]
    fn history_keeps_failures_before_the_final_row_even_across_midnight() {
        let (mut final_record, _) = selected();
        final_record.t = Some("2026-10-21T00:00:01Z".into());
        let row = Row::from_record(&final_record).with_account("a1", "one");
        let mut first = final_record.clone();
        first.t = Some("2026-10-20T23:59:59Z".into());
        first.status = Some(429);
        first.err = Some("before midnight".into());
        first.handed_on = Some(true);
        let mut second = final_record.clone();
        second.status = Some(502);
        second.err = Some("second attempt".into());
        second.handed_on = Some(true);
        let mut another = first.clone();
        another.route_id = Some(8);
        let detail = load_detail(
            &row,
            || Ok(TracePage::default()),
            |_| {
                Ok(UsagePage {
                    rows: vec![
                        final_record.clone(),
                        second.clone(),
                        first.clone(),
                        another.clone(),
                    ],
                    total: Some(4),
                })
            },
        );
        assert_eq!(detail.attempts.len(), 3);
        assert_eq!(detail.attempts[0].error.as_deref(), Some("before midnight"));
        assert_eq!(detail.attempts[1].error.as_deref(), Some("second attempt"));
        assert_eq!(detail.attempts[2].status_code, Some(200));
    }

    #[test]
    fn final_ledger_row_is_last_even_when_its_request_timestamp_is_first() {
        let (final_record, row) = selected();
        let mut first = final_record.clone();
        first.status = Some(429);
        first.handed_on = Some(true);
        first.err = Some("first".into());
        let mut second = first.clone();
        second.t = Some("2026-10-20T09:00:05Z".into());
        second.status = Some(502);
        second.err = Some("second".into());
        let detail = load_detail(
            &row,
            || Ok(TracePage::default()),
            |_| {
                Ok(UsagePage {
                    rows: vec![final_record.clone(), second.clone(), first.clone()],
                    total: Some(3),
                })
            },
        );
        assert_eq!(
            detail
                .attempts
                .iter()
                .map(|a| a.status_code)
                .collect::<Vec<_>>(),
            vec![Some(429), Some(502), Some(200)]
        );
    }

    #[test]
    fn reused_route_id_from_wrong_time_or_session_is_not_joined() {
        let (mut record, row) = selected();
        record.t = Some("2026-10-19T09:00:00Z".into());
        let trace: TracePage = serde_json::from_str(r#"{"routes":[{"id":7,"time":"2026-10-19T09:00:00Z","session":"s","agent":"cli","tries":[{"error":"wrong day"}]}]}"#).unwrap();
        let detail = load_detail(
            &row,
            || Ok(trace),
            |_| {
                Ok(UsagePage {
                    rows: vec![record.clone()],
                    total: Some(1),
                })
            },
        );
        assert!(matches!(detail.source, DetailSource::Unavailable));
        assert!(detail.attempts.is_empty());
    }

    #[test]
    fn no_permission_anywhere_is_popup_only_unavailable_data() {
        let (_, row) = selected();
        let detail = load_detail(&row, || Err("HTTP 403".into()), |_| Err("HTTP 403".into()));
        assert!(matches!(detail.source, DetailSource::Unavailable));
        assert!(detail.notice.contains("HTTP 403"));
    }
}
