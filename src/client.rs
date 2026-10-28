//! HTTP access to a Magpie gateway: the usage log and the live concurrency
//! feed.
//!
//! Both are public gateway endpoints on the same listener as the inference
//! routes (default `127.0.0.1:3425`). Loopback requests need no credentials;
//! a gateway shared over the network needs a Magpie gateway key.

use std::io::Read;
use std::time::{Duration, Instant};

use tracing::warn;

use crate::model::{
    ActiveRequest, Attempt, Concurrency, DetailSource, ExecutionDetail, Row, TracePage, UsagePage,
    UsageRecord, history_anchor,
};

const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;
const DETAIL_PAGE_SIZE: usize = 500;
const DETAIL_MAX_PAGES: usize = 4;
const DETAIL_BUDGET: Duration = Duration::from_secs(20);

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
