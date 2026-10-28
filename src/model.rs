//! Wire types for Magpie's usage log, and the paint-ready row built from them.
//!
//! Only the fields the panel renders are declared. Magpie's JSON is
//! snake_case (`cache_read`, `ttft_ms`) with a few camelCase display names
//! (`agentName`, `providerAccount`), so fields are mapped explicitly rather
//! than through a `rename_all`.
//!
//! Two semantics differ from the obvious reading and are handled here, not in
//! the UI:
//!
//! * `in` excludes cache reads and writes; `out` already includes reasoning
//!   tokens. Totals are derived rather than summed blindly.
//! * `cost` is an *estimate* in USD, and `priced == false` means no price basis
//!   was found. A zero cost is therefore not "free".

use serde::Deserialize;

/// Strips the vendor prefix from a model id: `openai/gpt-5` -> `gpt-5`.
pub fn short_model_name(model: &str) -> &str {
    match model.rsplit_once('/') {
        Some((_, name)) => name,
        None => model,
    }
}

/// The model name the panel displays, with the user's abbreviations applied.
pub fn display_model_name(raw: &str, aliases: &[crate::config::NameAlias]) -> String {
    apply_aliases(short_model_name(raw), aliases)
}

/// Rewrite a display name with the user's abbreviations, in order. An empty
/// `to` deletes every match; an empty `from` is skipped.
pub fn apply_aliases(raw: &str, aliases: &[crate::config::NameAlias]) -> String {
    let mut current = raw.to_string();
    if aliases.is_empty() || current.is_empty() {
        return current;
    }
    for alias in aliases {
        let from = alias.from.trim();
        if from.is_empty() {
            continue;
        }
        current = replace_all_ci(&current, from, alias.to.trim());
    }
    current
}

/// Case-insensitive `from` -> `to` over every occurrence, without regex.
fn replace_all_ci(haystack: &str, from: &str, to: &str) -> String {
    let needle = from.as_bytes();
    if needle.is_empty() {
        return haystack.to_string();
    }
    let bytes = haystack.as_bytes();
    let mut out = Vec::with_capacity(haystack.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + needle.len() <= bytes.len()
            && bytes[i..i + needle.len()].eq_ignore_ascii_case(needle)
        {
            out.extend_from_slice(to.as_bytes());
            i += needle.len();
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| haystack.to_string())
}

/// `GET /v1/magpie/usage/requests` response.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsagePage {
    #[serde(default)]
    pub rows: Vec<UsageRecord>,
    #[serde(default)]
    pub total: Option<i64>,
}

/// One line of `usage.jsonl`, as the HTTP API reports it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageRecord {
    /// Start time of the request (or of a handed-on attempt), RFC3339.
    #[serde(default)]
    pub t: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default, rename = "agentName")]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default, rename = "providerName")]
    pub provider_name: Option<String>,
    #[serde(default, rename = "providerAccount")]
    pub provider_account: Option<String>,
    #[serde(default, rename = "providerKeyName")]
    pub provider_key_name: Option<String>,
    /// Host the upstream call was sent to.
    #[serde(default)]
    pub host: Option<String>,
    /// `"/v1/messages → /v1/chat/completions"` when the protocol changed.
    #[serde(default)]
    pub ep: Option<String>,
    /// The model the client asked for: a name, alias or routing group.
    #[serde(default)]
    pub req: Option<String>,
    /// The upstream model actually selected.
    #[serde(default)]
    pub model: Option<String>,
    /// The model the provider's reply declared, when it said anything.
    #[serde(default)]
    pub served: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// Input tokens, excluding cache reads and writes.
    #[serde(default, rename = "in")]
    pub input: Option<i64>,
    /// Output tokens, reasoning included.
    #[serde(default)]
    pub out: Option<i64>,
    #[serde(default)]
    pub cache_read: Option<i64>,
    #[serde(default)]
    pub cache_write: Option<i64>,
    #[serde(default)]
    pub reasoning: Option<i64>,
    /// Whole-request duration, including earlier retries.
    #[serde(default)]
    pub ms: Option<i64>,
    /// Time to first text, reasoning or tool-call content.
    #[serde(default)]
    pub ttft_ms: Option<i64>,
    /// HTTP status recorded for the request. `0` is not a pending marker.
    #[serde(default)]
    pub status: Option<i64>,
    #[serde(default)]
    pub err: Option<String>,
    #[serde(default)]
    pub err_type: Option<String>,
    #[serde(default)]
    pub tries: Option<i64>,
    /// This failed attempt was handed on to the next one; not a final result.
    #[serde(default)]
    pub handed_on: Option<bool>,
    /// Shared by every attempt of one route; links live rows to finished ones.
    #[serde(default)]
    pub route_id: Option<i64>,
    #[serde(default)]
    pub rid: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    /// Estimated USD cost.
    #[serde(default)]
    pub cost: Option<f64>,
    /// Whether a price basis was found. `false` means `cost == 0` is unknown.
    #[serde(default)]
    pub priced: Option<bool>,
}

/// Recent in-memory routes. No request/response bodies are declared or fetched.
#[derive(Clone, Default, Deserialize)]
pub struct TracePage {
    #[serde(default)]
    pub routes: Vec<Route>,
}

#[derive(Clone, Default, Deserialize)]
pub struct Route {
    pub id: i64,
    /// Official Route calls its start time `time`, not Try's `start`.
    #[serde(default)]
    pub time: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub status: Option<i64>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub order: Vec<RouteCandidate>,
    #[serde(default)]
    pub tries: Vec<RouteTry>,
}

#[derive(Clone, Default, Deserialize)]
pub struct RouteCandidate {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub who: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Clone, Default, Deserialize)]
pub struct RouteTry {
    /// Candidate identifier, NOT a gateway or upstream credential.
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub status: Option<i64>,
    #[serde(default)]
    pub fail: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

/// Identity is based exclusively on wire fields, never display aliases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailIdentity {
    pub account_id: String,
    pub route_id: Option<i64>,
    pub timestamp: Option<String>,
    pub row_id: String,
    pub provider_id: Option<String>,
    pub provider_account: Option<String>,
    pub provider_key_name: Option<String>,
    pub rid: Option<String>,
    pub session: Option<String>,
    pub handed_on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetailTicket {
    pub nonce: u64,
    /// Incremented on connection/credential edits, including same-origin edits.
    pub revision: u64,
    pub endpoint: String,
    pub identity: DetailIdentity,
}

impl DetailTicket {
    pub fn accepts(&self, incoming: &Self, revision: u64, endpoint: Option<&str>) -> bool {
        self == incoming && self.revision == revision && endpoint == Some(self.endpoint.as_str())
    }
}

#[derive(Clone, Default)]
pub struct Attempt {
    pub model: String,
    pub done: Option<bool>,
    pub status_code: Option<i64>,
    pub fail: Option<String>,
    pub error: Option<String>,
    pub error_type: Option<String>,
    pub provider: Option<String>,
    pub provider_key: Option<String>,
    pub handed_on: bool,
}

impl Attempt {
    pub fn from_record(record: &UsageRecord) -> Self {
        let row = Row::from_record(record);
        Self {
            model: row.model,
            done: Some(true),
            status_code: row.status_code,
            error: row.error,
            error_type: row.error_type,
            provider: row.provider,
            provider_key: row.provider_key,
            handed_on: row.handed_on,
            ..Self::default()
        }
    }
}

#[derive(Clone)]
pub enum DetailSource {
    Loading,
    Trace,
    History,
    Unavailable,
}

#[derive(Clone)]
pub struct ExecutionDetail {
    pub source: DetailSource,
    pub attempts: Vec<Attempt>,
    pub route_error: Option<String>,
    pub route_status: Option<i64>,
    /// Always visible and copyable, including pagination/cache limitations.
    pub notice: String,
}

impl ExecutionDetail {
    pub fn loading() -> Self {
        Self {
            source: DetailSource::Loading,
            attempts: Vec::new(),
            route_error: None,
            route_status: None,
            notice: "正在加载尝试记录…".into(),
        }
    }

    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self {
            source: DetailSource::Unavailable,
            attempts: Vec::new(),
            route_error: None,
            route_status: None,
            notice: format!("尝试日志不可用：{}", reason.into()),
        }
    }

    pub fn from_route(route: &Route, row: &Row) -> Self {
        // Do NOT deduplicate by model, key or candidate id: retries may repeat all three.
        let attempts: Vec<_> = route
            .tries
            .iter()
            .map(|attempt| {
                let candidate = route
                    .order
                    .iter()
                    .find(|c| !attempt.id.is_empty() && c.id == attempt.id);
                Attempt {
                    model: attempt.model.clone().unwrap_or_default(),
                    done: attempt.done,
                    status_code: attempt.status,
                    fail: attempt.fail.clone(),
                    error: attempt.error.clone(),
                    provider: candidate.and_then(|c| {
                        non_empty(c.name.as_deref()).or_else(|| non_empty(c.provider.as_deref()))
                    }),
                    provider_key: candidate
                        .filter(|c| c.kind.as_deref() == Some("key"))
                        .and_then(|c| non_empty(c.who.as_deref())),
                    ..Attempt::default()
                }
            })
            .collect();
        let incomplete = route.done != Some(true) || attempts.len() < row.tries as usize;
        Self {
            source: DetailSource::Trace,
            attempts,
            route_error: route.error.clone(),
            route_status: route.status,
            notice: if incomplete {
                "近期缓存尝试记录（请求未结束或记录不完整）"
            } else {
                "近期缓存尝试记录（非原始 HTTP 响应正文）"
            }
            .into(),
        }
    }
}

/// Popup-owned state with pure acceptance and poll-refresh rules.
pub struct DetailSession {
    pub raw_row: Row,
    pub ticket: DetailTicket,
    pub detail: ExecutionDetail,
}

impl DetailSession {
    pub fn new(raw_row: Row, ticket: DetailTicket) -> Self {
        Self {
            raw_row,
            ticket,
            detail: ExecutionDetail::loading(),
        }
    }
    pub fn refresh(&mut self, rows: &[Row]) {
        if let Some(row) = rows
            .iter()
            .find(|row| row.detail_identity() == self.ticket.identity)
        {
            self.raw_row = row.clone(); // preserve the fetched attempts
        }
    }
    pub fn receive(
        &mut self,
        ticket: &DetailTicket,
        detail: ExecutionDetail,
        revision: u64,
        endpoint: Option<&str>,
    ) -> bool {
        if !self.ticket.accepts(ticket, revision, endpoint) {
            return false;
        }
        self.detail = detail;
        true
    }
}

/// Compare instants at the ledger's seconds precision; retain the original
/// timestamp in identities to distinguish separate ledger rows.
fn same_instant(a: Option<&str>, b: Option<&str>) -> bool {
    match (
        a.and_then(crate::time::parse_unix),
        b.and_then(crate::time::parse_unix),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

impl Route {
    pub fn matches(&self, row: &Row) -> bool {
        if row.route_id != Some(self.id) {
            return false;
        }
        if let Some(session) = row.session.as_deref()
            && self.session.as_deref() != Some(session)
        {
            return false;
        }
        if let Some(agent) = row.agent_id.as_deref()
            && self.agent.as_deref() != Some(agent)
        {
            return false;
        }
        same_instant(self.time.as_deref(), row.started_at.as_deref())
            || self
                .tries
                .iter()
                .any(|t| same_instant(t.start.as_deref(), row.started_at.as_deref()))
    }
}

/// Require the selected ledger row as an anchor before accepting a route
/// summary; a reused integer route id from another day must not join history.
pub fn history_anchor(record: &UsageRecord, row: &Row) -> bool {
    Row::from_record(record)
        .with_account(&row.account_id, &row.account_name)
        .detail_identity()
        == row.detail_identity()
}

/// `GET /v1/magpie/concurrency` response; only `active` is used.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Concurrency {
    #[serde(default)]
    pub active: Vec<ActiveRequest>,
}

/// One in-flight request. Field names are the client's, not the upstream
/// model's: `model` is what the agent asked for, `provider` the initial route.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ActiveRequest {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub started: Option<String>,
    #[serde(default)]
    pub ms: Option<i64>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    /// Matches a finished row's `route_id`.
    #[serde(default)]
    pub route: Option<i64>,
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// A single table row, pre-resolved so painting never walks the wire types.
#[derive(Debug, Clone)]
pub struct Row {
    /// A display key: Magpie has no universal request id, so it is built from
    /// the fields its own UI keeps a row under.
    pub id: String,
    /// Which saved connection the row came from.
    pub account_id: String,
    pub account_name: String,
    pub started_at: Option<String>,
    pub status: Status,
    pub status_code: Option<i64>,
    pub error: Option<String>,
    pub error_type: Option<String>,
    /// Raw agent id, never rewritten by display aliases.
    pub agent_id: Option<String>,
    /// What the client asked for (`req`).
    pub requested_model: String,
    /// The upstream model actually selected (`model`).
    pub model: String,
    /// The model the reply declared (`served`), when present.
    pub served_model: Option<String>,
    /// The provider's display name, falling back to its id.
    pub provider: Option<String>,
    /// The raw provider id, used for filtering.
    pub provider_id: Option<String>,
    /// Immutable display-name fallback for detail identity when id is absent.
    pub raw_provider: Option<String>,
    pub provider_account: Option<String>,
    pub provider_key: Option<String>,
    /// The agent's display name, falling back to its id.
    pub agent: Option<String>,
    pub host: Option<String>,
    /// Client endpoint, e.g. `/v1/messages → /v1/responses`.
    pub endpoint: Option<String>,
    pub reasoning_effort: Option<String>,
    /// Attempts recorded for the final request; above one means retries.
    pub tries: i64,
    /// This row is a failed attempt handed on to the next one.
    pub handed_on: bool,
    pub route_id: Option<i64>,
    pub rid: Option<String>,
    pub session: Option<String>,
    /// Input tokens, excluding cache reads and writes.
    pub prompt_tokens: i64,
    pub cached_tokens: i64,
    pub write_cached_tokens: i64,
    /// Output tokens, reasoning included.
    pub completion_tokens: i64,
    pub reasoning_tokens: i64,
    pub latency_ms: Option<i64>,
    pub first_token_ms: Option<i64>,
    /// Estimated USD cost; `None` when no price basis was found.
    pub total_cost: Option<f64>,
    /// In flight right now, taken from `/v1/magpie/concurrency`.
    pub live: bool,
}

impl Row {
    pub fn detail_identity(&self) -> DetailIdentity {
        DetailIdentity {
            account_id: self.account_id.clone(),
            route_id: self.route_id,
            timestamp: self.started_at.clone(),
            row_id: self.id.clone(),
            provider_id: self
                .provider_id
                .clone()
                .or_else(|| self.raw_provider.clone()),
            provider_account: self.provider_account.clone(),
            provider_key_name: self.provider_key.clone(),
            rid: self.rid.clone(),
            session: self.session.clone(),
            handed_on: self.handed_on,
        }
    }

    /// Call only on a raw row. Shared by the list and detail so aliases are
    /// never applied twice, and updates can reapply the current configuration.
    pub fn for_display(&self, config: &crate::config::Config) -> Self {
        let mut row = self.clone();
        if !config.model_aliases.is_empty() {
            row.requested_model = display_model_name(&row.requested_model, &config.model_aliases);
            row.model = display_model_name(&row.model, &config.model_aliases);
            row.served_model = row
                .served_model
                .map(|s| display_model_name(&s, &config.model_aliases));
        }
        row.provider = row
            .provider
            .map(|p| apply_aliases(&p, &config.provider_aliases));
        row.agent = row.agent.map(|a| apply_aliases(&a, &config.agent_aliases));
        row
    }

    /// `provider(key)`: the provider with Magpie's upstream key name appended,
    /// so one table cell carries both. Either side may be missing.
    pub fn provider_with_key(&self) -> Option<String> {
        match (self.provider.as_deref(), self.provider_key.as_deref()) {
            (Some(provider), Some(key)) => Some(format!("{provider}({key})")),
            (Some(provider), None) => Some(provider.to_string()),
            (None, Some(key)) => Some(format!("({key})")),
            (None, None) => None,
        }
    }

    pub fn from_record(record: &UsageRecord) -> Self {
        let failed = record.status.is_some_and(|code| code >= 400)
            || record.err.as_deref().is_some_and(|e| !e.trim().is_empty())
            || record
                .err_type
                .as_deref()
                .is_some_and(|e| !e.trim().is_empty())
            || record.handed_on.unwrap_or(false);
        let requested = non_empty(record.req.as_deref())
            .or_else(|| non_empty(record.model.as_deref()))
            .unwrap_or_default();
        let model = non_empty(record.model.as_deref()).unwrap_or_default();
        let id = row_key(
            record.t.as_deref(),
            record.agent.as_deref(),
            record.model.as_deref(),
            record.rid.as_deref(),
            record.session.as_deref(),
        );
        Row {
            id,
            account_id: String::new(),
            account_name: String::new(),
            started_at: record.t.clone(),
            status: if failed {
                Status::Failed
            } else {
                Status::Completed
            },
            status_code: record.status,
            error: record.err.clone().filter(|e| !e.trim().is_empty()),
            error_type: non_empty(record.err_type.as_deref()),
            agent_id: non_empty(record.agent.as_deref()),
            requested_model: requested,
            model,
            served_model: non_empty(record.served.as_deref()),
            provider: non_empty(record.provider_name.as_deref())
                .or_else(|| non_empty(record.provider.as_deref())),
            provider_id: non_empty(record.provider.as_deref()),
            raw_provider: non_empty(record.provider_name.as_deref()),
            provider_account: non_empty(record.provider_account.as_deref()),
            provider_key: non_empty(record.provider_key_name.as_deref()),
            agent: non_empty(record.agent_name.as_deref())
                .or_else(|| non_empty(record.agent.as_deref())),
            host: non_empty(record.host.as_deref()),
            endpoint: non_empty(record.ep.as_deref()),
            reasoning_effort: non_empty(record.effort.as_deref()),
            tries: record.tries.unwrap_or(1).max(1),
            handed_on: record.handed_on.unwrap_or(false),
            route_id: record.route_id,
            rid: non_empty(record.rid.as_deref()),
            session: non_empty(record.session.as_deref()),
            prompt_tokens: record.input.unwrap_or(0).max(0),
            cached_tokens: record.cache_read.unwrap_or(0).max(0),
            write_cached_tokens: record.cache_write.unwrap_or(0).max(0),
            completion_tokens: record.out.unwrap_or(0).max(0),
            reasoning_tokens: record.reasoning.unwrap_or(0).max(0),
            latency_ms: record.ms.filter(|ms| *ms >= 0),
            first_token_ms: record.ttft_ms.filter(|ms| *ms >= 0),
            total_cost: record
                .cost
                .filter(|c| c.is_finite() && *c >= 0.0)
                .filter(|_| record.priced.unwrap_or(false)),
            live: false,
        }
    }

    /// A request still in flight. The concurrency feed carries no tokens or
    /// cost, so those stay at zero.
    pub fn from_active(active: &ActiveRequest) -> Self {
        let model = non_empty(active.model.as_deref()).unwrap_or_default();
        let id = match (active.route, active.id) {
            (Some(route), _) => format!("live:{route}"),
            (None, Some(id)) => format!("live:{id}"),
            (None, None) => format!("live:{}", model),
        };
        Row {
            id,
            account_id: String::new(),
            account_name: String::new(),
            started_at: active.started.clone(),
            status: Status::Processing,
            status_code: None,
            error: None,
            error_type: None,
            agent_id: non_empty(active.agent.as_deref()),
            requested_model: model.clone(),
            model,
            served_model: None,
            provider: non_empty(active.provider.as_deref()),
            provider_id: non_empty(active.provider.as_deref()),
            raw_provider: non_empty(active.provider.as_deref()),
            provider_account: None,
            provider_key: None,
            agent: non_empty(active.agent.as_deref()),
            host: None,
            endpoint: None,
            reasoning_effort: None,
            tries: 0,
            handed_on: false,
            route_id: active.route,
            rid: None,
            session: non_empty(active.session.as_deref()),
            prompt_tokens: 0,
            cached_tokens: 0,
            write_cached_tokens: 0,
            completion_tokens: 0,
            reasoning_tokens: 0,
            latency_ms: active.ms,
            first_token_ms: None,
            total_cost: None,
            live: true,
        }
    }

    /// Stamp the row with the connection it was polled from.
    pub fn with_account(mut self, id: &str, name: &str) -> Self {
        self.account_id = id.to_string();
        self.account_name = name.to_string();
        self
    }

    /// Total prompt tokens: input plus cached reads and writes, which Magpie
    /// reports separately.
    pub fn prompt_total(&self) -> i64 {
        self.prompt_tokens + self.cached_tokens + self.write_cached_tokens
    }

    /// Everything the request moved, in tokens.
    pub fn total_tokens(&self) -> i64 {
        self.prompt_total() + self.completion_tokens
    }

    /// Cache hit rate over the prompt.
    pub fn cache_hit_rate(&self) -> Option<f64> {
        let prompt = self.prompt_total();
        if self.cached_tokens <= 0 || prompt <= 0 {
            return None;
        }
        Some((self.cached_tokens as f64 / prompt as f64 * 100.0).min(100.0))
    }

    /// Flags a weak hit rate only for sizeable prompts.
    pub fn cache_hit_is_low(&self) -> bool {
        matches!(self.cache_hit_rate(), Some(r) if r < 80.0 && self.prompt_total() >= 40_000)
    }

    /// Output throughput over generation time. `out` includes reasoning, so it
    /// is used as-is; TTFT is subtracted because it is prompt processing.
    pub fn tps(&self) -> Option<f64> {
        let ms = self.latency_ms?;
        if ms <= 0 || self.completion_tokens <= 0 {
            return None;
        }
        let gen_ms = (ms - self.first_token_ms.unwrap_or(0)).max(1);
        Some(self.completion_tokens as f64 / (gen_ms as f64 / 1000.0))
    }

    /// The model that actually served the request.
    pub fn served_model(&self) -> &str {
        if self.model.is_empty() {
            "未知模型"
        } else {
            short_model_name(&self.model)
        }
    }

    /// Whether the request was served by a model other than the one asked for.
    pub fn is_routed(&self) -> bool {
        !self.requested_model.is_empty()
            && !self.model.is_empty()
            && self.requested_model != self.model
    }

    /// The served model with its effort, e.g. `gpt-5.2(high)`. A routed model
    /// is wrapped in brackets so the distinction is unambiguous.
    pub fn display_model(&self) -> String {
        let model = self.served_model();
        match (&self.reasoning_effort, self.is_routed()) {
            (Some(effort), true) => format!("[{}({})]", model, effort),
            (Some(effort), false) => format!("{}({})", model, effort),
            (None, true) => format!("[{}]", model),
            (None, false) => model.to_string(),
        }
    }

    /// How the inbound and upstream endpoints relate, for the protocol cell.
    pub fn protocol(&self) -> Protocol {
        let (inbound, upstream) = split_endpoint(self.endpoint.as_deref());
        match (inbound, upstream) {
            (Some(i), Some(u)) if i != u => Protocol::Converted { from: i, to: u },
            (Some(i), Some(_)) => Protocol::Same(i),
            (Some(one), None) | (None, Some(one)) => Protocol::Single(one),
            (None, None) => Protocol::Unknown,
        }
    }

    /// Whether the row opens the detail popup: failures carry a reason worth
    /// reading, and a recovered retry is as interesting as a hard failure.
    pub fn is_error(&self) -> bool {
        self.status == Status::Failed || self.tries > 1 || self.handed_on
    }

    /// Sort key: the request's start time in Unix seconds.
    pub fn age_key(&self) -> i64 {
        self.started_at
            .as_deref()
            .and_then(crate::time::parse_unix)
            .unwrap_or(i64::MIN)
    }
}

/// The key Magpie's own UI keeps a row under. It is not guaranteed unique
/// server-side, which is why it is only ever compared, never trusted alone.
fn row_key(
    t: Option<&str>,
    agent: Option<&str>,
    model: Option<&str>,
    rid: Option<&str>,
    session: Option<&str>,
) -> String {
    [
        t.unwrap_or_default(),
        agent.unwrap_or_default(),
        model.unwrap_or_default(),
        rid.unwrap_or_default(),
        session.unwrap_or_default(),
    ]
    .join("|")
}

/// Split `"/v1/messages → /v1/chat/completions"` into short protocol names.
fn split_endpoint(ep: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(ep) = ep.map(str::trim).filter(|ep| !ep.is_empty()) else {
        return (None, None);
    };
    let (inbound, upstream) = match ep.split_once('→') {
        Some((a, b)) => (a, b),
        None => (ep, ""),
    };
    (short_protocol(inbound), short_protocol(upstream))
}

/// Collapse an endpoint path to the badge the table shows.
fn short_protocol(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let lower = path.to_ascii_lowercase();
    let name = if lower.contains("chat") {
        "chat"
    } else if lower.contains("responses") {
        "resp"
    } else if lower.contains("messages") {
        "mess"
    } else if lower.contains("beta/models") {
        "gem"
    } else {
        path.trim_start_matches('/')
    };
    Some(name.to_string())
}

/// Relationship between the protocol the client spoke and the one used upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Protocol {
    /// Both known and identical; conversion did not happen.
    Same(String),
    /// Both known and different; Magpie converted in flight.
    Converted {
        from: String,
        to: String,
    },
    /// Only one side is known.
    Single(String),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Completed,
    Failed,
    /// In flight right now.
    Processing,
}

impl Status {
    /// Whether the row is still in flight; drives the faster poll cadence.
    pub fn is_active(self) -> bool {
        self == Status::Processing
    }
}

/// Which rows the table shows. The header chips mirror this enum, plus 全 for
/// the empty mask. Filtering happens locally on the fetched page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Completed,
    Failed,
    Live,
    Retried,
}

/// Bitmask of selected `Filter`s; the empty mask shows everything (`All`).
pub type FilterMask = u8;

pub const FILTER_NONE: FilterMask = 0;
pub const FILTER_COMPLETED: FilterMask = 1 << 0;
pub const FILTER_FAILED: FilterMask = 1 << 1;
pub const FILTER_LIVE: FilterMask = 1 << 2;
pub const FILTER_RETRIED: FilterMask = 1 << 3;

impl Filter {
    /// Header chip labels: one character each.
    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "全",
            Filter::Completed => "成",
            Filter::Failed => "败",
            Filter::Live => "行",
            Filter::Retried => "重",
        }
    }

    /// The single bit this chip toggles; `All` maps to the empty mask.
    pub fn mask(self) -> FilterMask {
        match self {
            Filter::All => FILTER_NONE,
            Filter::Completed => FILTER_COMPLETED,
            Filter::Failed => FILTER_FAILED,
            Filter::Live => FILTER_LIVE,
            Filter::Retried => FILTER_RETRIED,
        }
    }

    /// Whether `row` passes this chip.
    pub fn matches(self, row: &Row) -> bool {
        match self {
            Filter::All => true,
            Filter::Completed => row.status == Status::Completed,
            Filter::Failed => row.status == Status::Failed,
            Filter::Live => row.live,
            Filter::Retried => row.tries > 1 || row.handed_on,
        }
    }
}

/// Whether a row passes a chip mask. An empty mask shows everything.
pub fn passes(mask: FilterMask, row: &Row) -> bool {
    if mask == FILTER_NONE {
        return true;
    }
    [
        Filter::Completed,
        Filter::Failed,
        Filter::Live,
        Filter::Retried,
    ]
    .into_iter()
    .any(|filter| mask & filter.mask() != 0 && filter.matches(row))
}

#[cfg(test)]
#[path = "tests/model.rs"]
mod tests;
