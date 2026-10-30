use super::*;
use crate::config::{Config, DisplayField, NameAlias};
use crate::model::{Route, UsageRecord};

fn row(json: &str) -> Row {
    Row::from_record(&serde_json::from_str::<UsageRecord>(json).expect("record"))
}

const FULL: &str = r#"{
  "t":"2026-10-20T09:00:00Z","agentName":"app-secret","providerName":"Anthropic",
  "providerAccount":"account-secret","providerKeyName":"main","host":"host-secret",
  "ep":"endpoint-secret","req":"requested-secret","model":"vendor/model","served":"served-secret",
  "effort":"effort-secret","in":1000,"out":200,"cache_read":4000,"cache_write":500,
  "ms":5000,"status":502,"err":"upstream said no","tries":2,"handed_on":true,"route_id":77,
  "rid":"request-secret","response_id":"response-secret","session":"session-secret",
  "cost":0.25,"priced":true,"credits":1.5,"source":"source-secret"
}"#;

#[test]
fn details_only_contain_the_five_requested_fields() {
    let doc = Doc::with_detail(&row(FULL), &ExecutionDetail::loading(), &Config::default());
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(
        doc.rows[0].cells(),
        ["502", "Anthropic(main)", "model", "", "upstream said no"]
    );
    let text = plain_text(&doc);
    for label in HEADERS {
        assert!(text.contains(&format!("{label}:")));
    }
    for unwanted in [
        "app-secret",
        "account-secret",
        "host-secret",
        "endpoint-secret",
        "requested-secret",
        "served-secret",
        "effort-secret",
        "request-secret",
        "response-secret",
        "session-secret",
        "source-secret",
        "耗时:",
        "费用:",
        "令牌:",
        "路由:",
        "应用:",
    ] {
        assert!(!text.contains(unwanted), "unexpected {unwanted}");
    }
}

#[test]
fn recovered_success_keeps_each_failure_and_final_result_in_order() {
    let raw = row(r#"{"status":200,"tries":3,"route_id":77,"model":"vendor/model"}"#);
    let route: Route = serde_json::from_str(r#"{"id":77,"done":true,"order":[{"id":"same","name":"Provider","kind":"key","who":"friendly-key"}],"tries":[{"id":"same","model":"vendor/model","done":true,"status":429,"fail":"rate","error":"first failure"},{"id":"same","model":"vendor/model","done":true,"status":502,"error":"second failure\nmore details"},{"id":"same","model":"vendor/model","done":true,"status":200}]}"#).unwrap();
    let config = Config {
        hidden_fields: vec![
            DisplayField::Error,
            DisplayField::Provider,
            DisplayField::Model,
        ],
        ..Default::default()
    };
    let doc = Doc::with_detail(&raw, &ExecutionDetail::from_route(&route, &raw), &config);
    assert_eq!(
        doc.rows.iter().map(|r| r.code.as_str()).collect::<Vec<_>>(),
        ["429", "502", "200"]
    );
    assert_eq!(doc.rows[0].kind, "rate");
    assert_eq!(doc.rows[0].log, "first failure");
    assert_eq!(doc.rows[1].kind, "");
    assert_eq!(doc.rows[1].log, "second failure\nmore details");
    assert_eq!(doc.rows[2].log, fmt::BLANK);
    assert!(
        doc.rows
            .iter()
            .all(|r| r.provider == "Provider(friendly-key)" && r.model == "model")
    );
    assert!(doc.notice.is_empty());
    assert_eq!(doc.rows[0].color, theme::RED);
    assert_eq!(doc.rows[2].color, theme::GREEN);
}

#[test]
fn repeated_attempts_are_not_deduplicated() {
    let raw = row(r#"{"status":502,"err":"same error","model":"m","tries":2}"#);
    let mut detail = ExecutionDetail::loading();
    detail.attempts = vec![
        Attempt {
            status_code: Some(502),
            model: "m".into(),
            error: Some("same error".into()),
            ..Default::default()
        };
        2
    ];
    let doc = Doc::with_detail(&raw, &detail, &Config::default());
    assert_eq!(doc.rows.len(), 2);
    assert_eq!(plain_text(&doc).matches("same error").count(), 2);
}

#[test]
fn missing_status_is_not_fabricated_and_missing_reason_is_explicit() {
    let config = Config::default();
    let no_code = Doc::with_detail(
        &row(r#"{"status":0,"err":"timeout"}"#),
        &ExecutionDetail::loading(),
        &config,
    );
    assert_eq!(no_code.rows[0].code, fmt::BLANK);
    assert_eq!(no_code.rows[0].log, "timeout");
    let no_reason = Doc::with_detail(
        &row(r#"{"status":502}"#),
        &ExecutionDetail::unavailable("没有 route_id"),
        &config,
    );
    assert_eq!(no_reason.rows[0].log, "记录未提供错误原因");
    assert!(no_reason.notice.contains("尝试日志不可用"));
    let typed = Doc::with_detail(
        &row(r#"{"status":429,"err_type":"rate_limit_error","err":"slow down"}"#),
        &ExecutionDetail::loading(),
        &config,
    );
    assert_eq!(typed.rows[0].kind, "rate_limit_error");
    assert_eq!(typed.rows[0].log, "slow down");
    let unknown = Entry::from_attempt(&Attempt::default(), &config);
    assert_eq!(unknown.code, fmt::BLANK);
    assert_eq!(unknown.log, fmt::BLANK);
    assert_eq!(unknown.color, theme::TEXT_DIM);
}

#[test]
fn copy_preserves_long_multiline_errors_and_mapping_is_applied_once() {
    let error = format!("  leading\n{}\ntrailing  ", "长错误".repeat(700));
    let mut raw = row(r#"{"status":502,"model":"vendor/aa","providerName":"aa","agentName":"aa"}"#);
    raw.error = Some(error.clone());
    let alias = NameAlias {
        from: "aa".into(),
        to: "a".into(),
    };
    let config = Config {
        model_aliases: vec![alias.clone()],
        provider_aliases: vec![alias.clone()],
        agent_aliases: vec![alias],
        ..Default::default()
    };
    let detail = ExecutionDetail {
        source: DetailSource::History,
        attempts: vec![Attempt {
            model: "vendor/aa".into(),
            error: Some(error.clone()),
            provider: Some("aa".into()),
            provider_key: Some("aa-key".into()),
            status_code: Some(502),
            ..Default::default()
        }],
        route_error: None,
        route_status: None,
        notice: "历史错误摘要（可能不完整）".into(),
    };
    let doc = Doc::with_detail(&raw, &detail, &config);
    assert_eq!(doc.rows.len(), 1);
    assert_eq!(doc.rows[0].model, "a");
    assert_eq!(doc.rows[0].provider, "a(aa-key)");
    let text = plain_text(&doc);
    assert_eq!(text.matches(&error).count(), 1);
    assert!(text.contains("模型: a\n"));
    assert!(text.ends_with('\n'));
}

#[test]
fn trace_does_not_erase_a_selected_row_reason_it_did_not_contain() {
    let raw = row(r#"{"status":502,"err":"ledger-only reason","model":"m"}"#);
    let mut detail = ExecutionDetail::loading();
    detail.attempts.push(Attempt {
        status_code: Some(502),
        model: "m".into(),
        ..Default::default()
    });
    let doc = Doc::with_detail(&raw, &detail, &Config::default());
    assert!(plain_text(&doc).contains("ledger-only reason"));
}

#[test]
fn incomplete_trace_still_shows_the_known_final_result() {
    let raw = row(r#"{"status":200,"model":"m","providerName":"p","tries":2}"#);
    let mut detail = ExecutionDetail::loading();
    detail.attempts.push(Attempt {
        status_code: Some(429),
        model: "m".into(),
        provider: Some("p".into()),
        error: Some("first failure".into()),
        ..Default::default()
    });
    let doc = Doc::with_detail(&raw, &detail, &Config::default());
    assert_eq!(
        doc.rows.iter().map(|r| r.code.as_str()).collect::<Vec<_>>(),
        ["429", "200"]
    );
    assert_eq!(doc.rows[1].log, fmt::BLANK);
}

#[test]
fn route_error_is_kept_without_repeating_an_attempt_log() {
    let raw = row(r#"{"status":502,"err":"route reason","model":"m"}"#);
    let mut detail = ExecutionDetail::loading();
    detail.route_status = Some(502);
    detail.route_error = Some("route reason".into());
    let doc = Doc::with_detail(&raw, &detail, &Config::default());
    assert_eq!(doc.rows.len(), 1);
    detail.route_error = Some("another route reason".into());
    let doc = Doc::with_detail(&raw, &detail, &Config::default());
    assert!(plain_text(&doc).contains("another route reason"));
    assert_eq!(doc.rows.last().unwrap().provider, fmt::BLANK);
    assert_eq!(doc.rows.last().unwrap().model, fmt::BLANK);
}

#[test]
fn loading_unavailable_and_incomplete_history_remain_explicit() {
    let raw = row(r#"{"status":200,"tries":2}"#);
    let config = Config::default();
    let doc = Doc::with_detail(&raw, &ExecutionDetail::loading(), &config);
    assert!(doc.notice.contains("正在加载"));
    assert_eq!(doc.rows[0].log, fmt::BLANK);
    let mut detail = ExecutionDetail::unavailable("403 / 缓存过期");
    assert!(
        Doc::with_detail(&raw, &detail, &config)
            .notice
            .contains("403 / 缓存过期")
    );
    detail.source = DetailSource::History;
    detail.notice = "历史错误摘要（可能不完整）；分页已截断".into();
    let text = plain_text(&Doc::with_detail(&raw, &detail, &config));
    assert!(text.contains("历史错误摘要（可能不完整）"));
    assert!(text.contains("分页已截断"));
    detail.source = DetailSource::Trace;
    detail.notice = "近期缓存尝试记录（请求未结束或记录不完整）".into();
    assert!(
        Doc::with_detail(&raw, &detail, &config)
            .notice
            .contains("不完整")
    );
}

#[test]
fn one_click_hits_only_copy_or_close() {
    let m = Popup::new(POPUP_W, POPUP_H, 1.0);
    for button in [Button::Close, Button::Copy] {
        let rect = button_rect(&m, button);
        assert_eq!(
            hit_button(&m, rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
            Some(button)
        );
    }
    assert_eq!(
        hit_button(&m, m.pad() + 10.0, m.height - m.footer_h() / 2.0),
        None
    );
    assert_eq!(hit_button(&m, POPUP_W / 2.0, POPUP_H / 2.0), None);
}

fn measure_chars(text: &str) -> f32 {
    text.chars().count() as f32 * 7.0
}

fn sample_doc(log: &str) -> Doc {
    let raw = row(r#"{"status":502,"model":"m","providerName":"p","err":"short"}"#);
    let mut detail = ExecutionDetail::loading();
    detail.notice.clear();
    detail.attempts = vec![Attempt {
        model: "m".into(),
        provider: Some("p".into()),
        status_code: Some(502),
        error: Some(log.into()),
        ..Default::default()
    }];
    Doc::with_detail(&raw, &detail, &Config::default())
}

#[test]
fn first_line_fields_fit_their_text_and_narrow_windows_shrink_them() {
    let doc = sample_doc(&"长错误".repeat(200));
    let natural = natural_widths(&doc, 1.0, measure_chars);
    assert_eq!(natural[0], measure_chars("502") + CELL_PAD * 2.0);
    assert_eq!(natural[1], measure_chars("p") + CELL_PAD * 2.0);
    assert_eq!(natural[2], measure_chars("m") + CELL_PAD * 2.0);
    assert_eq!(natural[3], CELL_PAD * 2.0);

    // The log no longer owns a column: it only caps how wide the popup may get.
    assert_eq!(log_width(&doc, 1.0, measure_chars), MAX_LOG_W);
    let narrow = Popup::new(MIN_W, POPUP_H, 1.0);
    assert_eq!(
        desired_width(&narrow, &doc, measure_chars),
        MAX_LOG_W + PAD * 2.0
    );

    // Narrower than the fields need: they shrink together.
    let wide_fields = [300.0f32; 4];
    let fit = fitted_widths(wide_fields, MIN_W, 1.0);
    let total = fit.iter().sum::<f32>() + COLUMN_GAP * 3.0 + PAD * 2.0;
    assert!((total - MIN_W).abs() < 0.01);
    // Wider: the fields keep their own width and the rest stays blank.
    assert_eq!(fitted_widths(natural, 2000.0, 1.0), natural);
}

#[test]
fn short_records_keep_the_popup_at_its_minimum_width() {
    let doc = sample_doc("short");
    let m = Popup::new(MIN_W, POPUP_H, 1.0);
    assert_eq!(desired_width(&m, &doc, measure_chars), MIN_W);

    let scaled = natural_widths(&doc, 2.0, |text| measure_chars(text) * 2.0);
    for (a, b) in natural_widths(&doc, 1.0, measure_chars)
        .iter()
        .zip(scaled.iter())
    {
        assert!((a * 2.0 - b).abs() < 0.001);
    }
    let m2 = Popup::new(MIN_W * 2.0, POPUP_H * 2.0, 2.0);
    assert!(
        (desired_width(&m, &doc, measure_chars) * 2.0
            - desired_width(&m2, &doc, |text| measure_chars(text) * 2.0))
        .abs()
            < 0.001
    );
}

#[test]
fn failure_type_gets_its_own_column_and_the_log_keeps_the_prose() {
    let raw = row(r#"{"status":502,"model":"m"}"#);
    let mut detail = ExecutionDetail::loading();
    detail.attempts = vec![Attempt {
        status_code: Some(502),
        model: "m".into(),
        fail: Some("rate".into()),
        error_type: Some("rate_limit_error".into()),
        error: Some("slow down".into()),
        ..Default::default()
    }];
    let doc = Doc::with_detail(&raw, &detail, &Config::default());
    assert_eq!(doc.rows[0].kind, "rate · rate_limit_error");
    assert_eq!(doc.rows[0].log, "slow down");
    let text = plain_text(&doc);
    assert!(text.contains("类型: rate · rate_limit_error\n"));
    assert!(text.contains("错误日志: slow down\n"));
}

#[test]
fn the_log_starts_a_new_line_below_the_fields_without_indent() {
    let doc = sample_doc("first\nsecond");
    let widths = fitted_widths(natural_widths(&doc, 1.0, measure_chars), MIN_W, 1.0);
    for width in [MIN_W, MAX_LOG_W + PAD * 2.0 + 100.0] {
        let m = Popup::new(width, POPUP_H, 1.0);
        let table = layout(&m, &doc, &widths, wrap_chars);
        let find = |text: &str| {
            table
                .visuals
                .iter()
                .find(|v| v.text == text)
                .unwrap_or_else(|| panic!("missing {text}"))
                .rect
        };
        let code = find("502");
        let first = find("first");
        let second = find("second");
        assert_eq!(code.y, CELL_PAD * 2.0);
        assert_eq!(first.y, CELL_PAD * 2.0 + ROW_H);
        assert_eq!(second.y, CELL_PAD * 2.0 + ROW_H * 2.0);
        assert!(code.x > m.pad());
        assert_eq!(first.x, m.pad());
        assert_eq!(first.w, m.width - m.pad() * 2.0);
        assert_eq!(first.w, second.w);
    }
}

fn wrap_chars(text: &str, width: f32) -> Vec<String> {
    let columns = (width / 7.0).floor().max(1.0) as usize;
    text.split('\n')
        .flat_map(|line| {
            let chars: Vec<_> = line.chars().collect();
            if chars.is_empty() {
                vec![String::new()]
            } else {
                chars
                    .chunks(columns)
                    .map(|chunk| chunk.iter().collect())
                    .collect()
            }
        })
        .collect()
}

#[test]
fn long_errors_wrap_inside_fixed_columns_and_rows_never_overlap() {
    let raw = row(r#"{"status":502,"model":"m","providerName":"p","err":"short"}"#);
    let mut detail = ExecutionDetail::loading();
    detail.notice.clear();
    detail.attempts = vec![
        Attempt {
            model: "m".into(),
            provider: Some("p".into()),
            status_code: Some(502),
            error: Some("长错误".repeat(1000)),
            ..Default::default()
        },
        Attempt {
            model: "m".into(),
            status_code: Some(429),
            error: Some("last error".into()),
            ..Default::default()
        },
    ];
    let doc = Doc::with_detail(&raw, &detail, &Config::default());
    let m = Popup::new(MIN_W, POPUP_H, 1.0);
    let widths = fitted_widths(natural_widths(&doc, 1.0, measure_chars), MIN_W, 1.0);
    let table = layout(&m, &doc, &widths, wrap_chars);
    assert!(table.height > m.content_view_h());
    assert!(table.bands.windows(2).all(|r| r[0].y + r[0].h <= r[1].y));
    assert!(
        table
            .visuals
            .iter()
            .all(|v| v.rect.x + v.rect.w <= m.width - m.pad() + 0.001)
    );
    assert!(table.visuals.iter().any(|v| v.text == "last error"));
    assert!(
        table
            .visuals
            .iter()
            .all(|v| v.rect.y + v.rect.h <= table.height)
    );
}

#[test]
fn the_popup_height_follows_its_content_between_a_floor_and_a_ceiling() {
    // Between the limits the document height is used as-is, chrome included.
    assert_eq!(
        height_for_content(200.0, 1.0, 10_000),
        HEADER_H + 200.0 + FOOTER_H
    );

    // Title, footer and one unwrapped record: no blank space under a short log.
    assert_eq!(height_for_content(10.0, 1.0, 10_000), min_height(1.0));
    assert_eq!(
        min_height(1.0),
        HEADER_H + FOOTER_H + ROW_H + CELL_PAD * 4.0
    );

    // Long logs stop at the logical ceiling...
    assert_eq!(height_for_content(10_000.0, 1.0, 10_000), MAX_H);
    // ...unless the work area's share is smaller.
    let share = height_for_content(10_000.0, 1.0, 500);
    assert!((share - 500.0 * MAX_H_WORK_FRACTION).abs() < 0.01);
}

#[test]
fn the_height_limits_scale_with_the_panel() {
    assert!((min_height(2.0) - min_height(1.0) * 2.0).abs() < 0.001);
    assert!((max_height(2.0, 10_000) - MAX_H * 2.0).abs() < 0.001);
    let doubled = height_for_content(400.0, 2.0, 10_000);
    assert!((doubled - (HEADER_H + FOOTER_H) * 2.0 - 400.0).abs() < 0.001);
}

#[test]
fn a_tiny_work_area_never_inverts_the_height_limits() {
    assert_eq!(max_height(1.0, 40), min_height(1.0));
    assert_eq!(height_for_content(10_000.0, 1.0, 40), min_height(1.0));
}

#[test]
fn short_records_hit_the_floor_and_long_ones_the_ceiling() {
    // A record with no log is one line: exactly the floor, no blank space.
    let bare = Doc {
        rows: vec![Entry {
            code: "200".into(),
            provider: String::new(),
            model: String::new(),
            kind: String::new(),
            log: String::new(),
            color: theme::TEXT,
        }],
        notice: String::new(),
    };
    let m = Popup::new(MIN_W, POPUP_H, 1.0);
    let widths = fitted_widths(natural_widths(&bare, 1.0, measure_chars), MIN_W, 1.0);
    let table = layout(&m, &bare, &widths, wrap_chars);
    assert_eq!(
        height_for_content(table.height, 1.0, 10_000),
        min_height(1.0)
    );

    // One short record with its log line sits just above the floor.
    let short = sample_doc("short");
    let widths = fitted_widths(natural_widths(&short, 1.0, measure_chars), MIN_W, 1.0);
    let table = layout(&m, &short, &widths, wrap_chars);
    let h = height_for_content(table.height, 1.0, 10_000);
    assert_eq!(h, HEADER_H + table.height + FOOTER_H);
    assert!(h > min_height(1.0) && h < MAX_H);

    let long = sample_doc(&"长错误".repeat(5000));
    let widths = fitted_widths(natural_widths(&long, 1.0, measure_chars), MIN_W, 1.0);
    let table = layout(&m, &long, &widths, wrap_chars);
    assert!(table.height > MAX_H);
    assert_eq!(height_for_content(table.height, 1.0, 10_000), MAX_H);
    // The public measuring path agrees with the layout it is built on.
    assert_eq!(
        desired_height(&m, &long, 10_000, measure_chars, wrap_chars),
        MAX_H
    );
}
