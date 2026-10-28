use super::*;

/// The wire shape the gateway actually returns: snake_case, with the few
/// camelCase display names Magpie adds.
fn record(json: &str) -> UsageRecord {
    serde_json::from_str(json).expect("valid usage record")
}

const FULL: &str = r#"{
  "t": "2026-10-20T09:00:00Z",
  "agent": "claude-cli",
  "agentName": "Claude Code",
  "provider": "anthropic",
  "providerName": "Anthropic",
  "providerAccount": "team",
  "providerKeyName": "main-key",
  "host": "api.anthropic.com",
  "ep": "/v1/messages → /v1/chat/completions",
  "req": "claude-group",
  "model": "anthropic/claude-sonnet-4",
  "served": "claude-sonnet-4-20250514",
  "effort": "high",
  "kind": "chat",
  "in": 1000,
  "out": 200,
  "cache_read": 4000,
  "cache_write": 500,
  "reasoning": 80,
  "ms": 5000,
  "ttft_ms": 1000,
  "first_text_ms": 1200,
  "flow_ms": 3800,
  "own_ms": 40,
  "status": 200,
  "tries": 2,
  "handed_on": false,
  "route_id": 77,
  "rid": "req_1",
  "response_id": "msg_1",
  "session": "sess_1",
  "cost": 0.25,
  "priced": true,
  "credits": 1.5,
  "source": "log"
}"#;

#[test]
fn maps_every_wire_field_the_table_renders() {
    let row = Row::from_record(&record(FULL));
    assert_eq!(row.status, Status::Completed);
    assert_eq!(row.status_code, Some(200));
    assert_eq!(row.requested_model, "claude-group");
    assert_eq!(row.model, "anthropic/claude-sonnet-4");
    assert_eq!(
        row.served_model.as_deref(),
        Some("claude-sonnet-4-20250514")
    );
    assert_eq!(row.provider.as_deref(), Some("Anthropic"));
    assert_eq!(row.provider_id.as_deref(), Some("anthropic"));
    assert_eq!(row.provider_account.as_deref(), Some("team"));
    assert_eq!(row.provider_key.as_deref(), Some("main-key"));
    assert_eq!(
        row.provider_with_key().as_deref(),
        Some("Anthropic(main-key)")
    );
    assert_eq!(row.agent.as_deref(), Some("Claude Code"));
    assert_eq!(row.host.as_deref(), Some("api.anthropic.com"));
    assert_eq!(row.tries, 2);
    assert_eq!(row.route_id, Some(77));
    assert_eq!(row.prompt_tokens, 1000);
    assert_eq!(row.cached_tokens, 4000);
    assert_eq!(row.write_cached_tokens, 500);
    assert_eq!(row.completion_tokens, 200);
    assert_eq!(row.reasoning_tokens, 80);
    assert_eq!(row.total_cost, Some(0.25));
}

#[test]
fn the_provider_cell_carries_the_key_name_in_parentheses() {
    let both = Row::from_record(&record(
        r#"{"providerName":"Anthropic","providerKeyName":"k"}"#,
    ));
    assert_eq!(both.provider_with_key().as_deref(), Some("Anthropic(k)"));

    let provider_only = Row::from_record(&record(r#"{"providerName":"Anthropic"}"#));
    assert_eq!(
        provider_only.provider_with_key().as_deref(),
        Some("Anthropic")
    );

    let key_only = Row::from_record(&record(r#"{"providerKeyName":"k"}"#));
    assert_eq!(key_only.provider_with_key().as_deref(), Some("(k)"));

    assert_eq!(Row::from_record(&record("{}")).provider_with_key(), None);
}

#[test]
fn cache_reads_and_writes_are_prompt_tokens_too() {
    let row = Row::from_record(&record(FULL));
    assert_eq!(row.prompt_total(), 5500);
    assert_eq!(row.total_tokens(), 5700);
    // `out` already includes reasoning, so nothing is added twice.
    assert_eq!(row.completion_tokens, 200);
}

#[test]
fn an_unpriced_request_has_no_cost_even_when_the_number_is_zero() {
    let row = Row::from_record(&record(r#"{"in":1,"out":1,"cost":0,"priced":false}"#));
    assert_eq!(row.total_cost, None);
}

#[test]
fn a_failing_status_or_error_marks_the_row_failed() {
    assert_eq!(
        Row::from_record(&record(r#"{"status":502}"#)).status,
        Status::Failed
    );
    assert_eq!(
        Row::from_record(&record(r#"{"status":0,"err":"upstream timeout"}"#)).status,
        Status::Failed
    );
    // `status: 0` on its own is not a pending marker.
    assert_eq!(
        Row::from_record(&record(r#"{"status":0}"#)).status,
        Status::Completed
    );
}

#[test]
fn active_requests_are_live_and_carry_the_client_model() {
    let active: ActiveRequest = serde_json::from_str(
        r#"{"id":3,"route":9,"started":"2026-10-20T09:00:00Z","agent":"codex","model":"gpt-5","provider":"openai","ms":850}"#,
    )
    .expect("valid active request");
    let row = Row::from_active(&active);
    assert_eq!(row.status, Status::Processing);
    assert!(row.live);
    assert_eq!(row.id, "live:9");
    assert_eq!(row.model, "gpt-5");
    assert_eq!(row.provider.as_deref(), Some("openai"));
    assert_eq!(row.latency_ms, Some(850));
    assert!(row.total_cost.is_none());
}

#[test]
fn routed_models_are_bracketed_and_carry_their_effort() {
    let row = Row::from_record(&record(FULL));
    assert!(row.is_routed());
    assert_eq!(row.display_model(), "[claude-sonnet-4(high)]");

    let plain = Row::from_record(&record(r#"{"req":"gpt-5","model":"gpt-5"}"#));
    assert!(!plain.is_routed());
    assert_eq!(plain.display_model(), "gpt-5");
}

#[test]
fn protocol_distinguishes_conversion_from_a_match() {
    let converted = Row::from_record(&record(FULL));
    assert_eq!(
        converted.protocol(),
        Protocol::Converted {
            from: "mess".into(),
            to: "chat".into()
        }
    );

    let same = Row::from_record(&record(r#"{"ep":"/v1/messages → /v1/messages"}"#));
    assert_eq!(same.protocol(), Protocol::Same("mess".into()));

    let single = Row::from_record(&record(r#"{"ep":"/v1/responses"}"#));
    assert_eq!(single.protocol(), Protocol::Single("resp".into()));

    assert_eq!(
        Row::from_record(&record("{}")).protocol(),
        Protocol::Unknown
    );
}

#[test]
fn throughput_excludes_prompt_processing() {
    let row = Row::from_record(&record(FULL));
    // 200 tokens over (5000 - 1000) ms.
    assert_eq!(row.tps(), Some(50.0));

    let no_ttft = Row::from_record(&record(r#"{"out":100,"ms":2000}"#));
    assert_eq!(no_ttft.tps(), Some(50.0));
}

#[test]
fn cache_rate_needs_a_cached_read_and_says_when_it_is_weak() {
    let row = Row::from_record(&record(FULL));
    assert!((row.cache_hit_rate().unwrap() - 4000.0 / 5500.0 * 100.0).abs() < 1e-9);
    assert!(!row.cache_hit_is_low());

    let weak = Row::from_record(&record(r#"{"in":50000,"cache_read":1000}"#));
    assert!(weak.cache_hit_is_low());

    let uncached = Row::from_record(&record(r#"{"in":1000}"#));
    assert!(uncached.cache_hit_rate().is_none());
}

#[test]
fn retries_recovered_on_the_last_attempt_still_open_the_detail() {
    let ok = Row::from_record(&record(r#"{"tries":3}"#));
    assert_eq!(ok.status, Status::Completed);
    assert!(ok.is_error());

    let handed = Row::from_record(&record(r#"{"handed_on":true}"#));
    assert!(handed.is_error());

    assert!(!Row::from_record(&record("{}")).is_error());
}

#[test]
fn a_usage_page_parses_its_rows() {
    let page: UsagePage =
        serde_json::from_str(r#"{"rows":[{"model":"a"},{"model":"b"}],"total":2}"#)
            .expect("valid page");
    assert_eq!(page.rows.len(), 2);
}

#[test]
fn filters_group_by_status_liveness_and_retries() {
    let ok = Row::from_record(&record("{}"));
    let failed = Row::from_record(&record(r#"{"status":500}"#));
    let live = Row::from_active(&ActiveRequest::default());
    let retried = Row::from_record(&record(r#"{"tries":2}"#));

    assert!(Filter::Completed.matches(&ok));
    assert!(Filter::Failed.matches(&failed));
    assert!(Filter::Live.matches(&live));
    assert!(Filter::Retried.matches(&retried));
    assert!(!Filter::Completed.matches(&failed));

    assert!(passes(FILTER_NONE, &failed));
    assert!(passes(FILTER_FAILED | FILTER_COMPLETED, &ok));
    assert!(!passes(FILTER_FAILED, &ok));
    assert!(passes(FILTER_LIVE, &live));
    assert!(passes(FILTER_RETRIED, &retried));
}

#[test]
fn model_names_are_shortened_and_aliases_apply_in_order() {
    let aliases = [
        crate::config::NameAlias {
            from: "claude".into(),
            to: "c".into(),
        },
        crate::config::NameAlias {
            from: "sonnet".into(),
            to: "s".into(),
        },
    ];
    assert_eq!(
        display_model_name("anthropic/claude-sonnet-4", &aliases),
        "c-s-4"
    );
    // An empty short form deletes the match.
    let delete = [crate::config::NameAlias {
        from: "-20250514".into(),
        to: String::new(),
    }];
    assert_eq!(
        display_model_name("openai/gpt-x-20250514", &delete),
        "gpt-x"
    );
}

#[test]
fn aliases_rewrite_whole_names_without_shortening_them() {
    let aliases = [crate::config::NameAlias {
        from: "anthropic".into(),
        to: "A".into(),
    }];
    assert_eq!(apply_aliases("Anthropic", &aliases), "A");
    assert_eq!(apply_aliases("Claude Code", &[]), "Claude Code");
}

#[test]
fn error_type_is_a_failure_reason_and_error_whitespace_is_preserved() {
    let row = Row::from_record(&record(
        r#"{"status":0,"err_type":"rate_limit_error","err":"  first\nsecond  "}"#,
    ));
    assert_eq!(row.status, Status::Failed);
    assert_eq!(row.error_type.as_deref(), Some("rate_limit_error"));
    assert_eq!(row.error.as_deref(), Some("  first\nsecond  "));
}

fn detail_row() -> Row {
    Row::from_record(&record(r#"{"t":"2026-10-20T09:00:00Z","agent":"cli","model":"raw/model","provider":"p","providerKeyName":"key-name","route_id":77,"session":"s","status":200,"tries":3}"#)).with_account("a1", "one")
}

fn ticket(row: &Row, nonce: u64) -> DetailTicket {
    DetailTicket {
        nonce,
        revision: 0,
        endpoint: "http://127.0.0.1:3425".into(),
        identity: row.detail_identity(),
    }
}

#[test]
fn detail_identity_separates_accounts_routes_and_provider_attempts() {
    let a = detail_row();
    let mut b = a.clone();
    b.account_id = "a2".into();
    assert_eq!(a.id, b.id);
    assert_ne!(a.detail_identity(), b.detail_identity());
    b = a.clone();
    b.provider_id = Some("other-provider".into());
    assert_eq!(a.id, b.id);
    assert_ne!(a.detail_identity(), b.detail_identity());
    b = a.clone();
    b.provider_key = Some("other-key-name".into());
    assert_ne!(a.detail_identity(), b.detail_identity());
    b = a.clone();
    b.route_id = Some(78);
    assert_ne!(a.detail_identity(), b.detail_identity());
}

#[test]
fn route_matching_requires_id_time_and_supplied_session_agent() {
    let row = detail_row();
    let mut route: Route = serde_json::from_str(
        r#"{"id":77,"time":"2026-10-20T09:00:00.500Z","agent":"cli","session":"s"}"#,
    )
    .unwrap();
    assert!(route.matches(&row));
    route.id = 78;
    assert!(!route.matches(&row));
    route.id = 77;
    route.session = Some("another".into());
    assert!(!route.matches(&row));
    route.session = Some("s".into());
    route.time = Some("2026-10-19T09:00:00Z".into());
    assert!(!route.matches(&row));
    route.tries.push(RouteTry {
        start: row.started_at.clone(),
        ..RouteTry::default()
    });
    assert!(route.matches(&row)); // selected handed-on attempt may start later
    route.agent = Some("another".into());
    assert!(!route.matches(&row));
}

#[test]
fn trace_keeps_repeated_candidates_and_does_not_invent_success() {
    let route: Route = serde_json::from_str(r#"{"id":77,"done":true,"order":[{"id":"candidate","provider":"p","name":"Provider","kind":"key","who":"friendly-name"}],"tries":[{"id":"candidate","model":"m","done":true,"status":429,"error":"first"},{"id":"candidate","model":"m","done":true,"status":502,"error":"second"},{"id":"candidate","model":"m","done":true,"status":200},{"id":"other"}]}"#).unwrap();
    let detail = ExecutionDetail::from_route(&route, &detail_row());
    assert_eq!(detail.attempts.len(), 4);
    assert_eq!(detail.attempts[0].error.as_deref(), Some("first"));
    assert_eq!(detail.attempts[1].error.as_deref(), Some("second"));
    assert_eq!(detail.attempts[0].provider.as_deref(), Some("Provider"));
    assert_eq!(
        detail.attempts[0].provider_key.as_deref(),
        Some("friendly-name")
    );
    assert_eq!(detail.attempts[3].status_code, None);
    assert_eq!(detail.attempts[3].done, None);
}

#[test]
fn late_close_reopen_and_connection_edit_results_are_rejected() {
    let row = detail_row();
    let old = ticket(&row, 1);
    let new = ticket(&row, 2);
    let endpoint = Some(new.endpoint.as_str());
    let mut open = DetailSession::new(row, new.clone());
    assert!(!open.receive(&old, ExecutionDetail::unavailable("old"), 0, endpoint));
    assert!(!open.receive(&new, ExecutionDetail::unavailable("edited"), 1, endpoint));
    assert!(!open.receive(&new, ExecutionDetail::unavailable("deleted"), 0, None));
    assert!(!open.receive(
        &new,
        ExecutionDetail::unavailable("changed origin"),
        0,
        Some("http://127.0.0.1:9999")
    ));
    assert!(open.receive(&new, ExecutionDetail::unavailable("current"), 0, endpoint));
    assert!(open.detail.notice.contains("current"));
}

#[test]
fn poll_refreshes_error_but_preserves_detail_attempts_and_raw_identity() {
    let row = detail_row();
    let mut open = DetailSession::new(row.clone(), ticket(&row, 1));
    open.detail.attempts.push(Attempt {
        error: Some("previous failure".into()),
        ..Attempt::default()
    });
    let mut update = row.clone();
    update.error = Some("newly recorded error".into());
    open.refresh(&[update]);
    assert_eq!(open.raw_row.error.as_deref(), Some("newly recorded error"));
    assert_eq!(
        open.detail.attempts[0].error.as_deref(),
        Some("previous failure")
    );
    let mut other = row;
    other.account_id = "a2".into();
    other.error = Some("wrong account".into());
    open.refresh(&[other]);
    assert_eq!(open.raw_row.error.as_deref(), Some("newly recorded error"));
}
