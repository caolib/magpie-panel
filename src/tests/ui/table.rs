use super::*;
use crate::ui::panel::ListView;

fn row(json: &str) -> Row {
    let record: crate::model::UsageRecord = serde_json::from_str(json).expect("valid record");
    Row::from_record(&record).with_account("a1", "本机")
}

fn view<'a>(rows: &'a [Row], hidden: &'a [DisplayField]) -> ListView<'a> {
    ListView {
        rows,
        now: crate::time::parse_unix("2026-10-20T10:00:00Z").unwrap(),
        scroll: 0.0,
        hover: None,
        selected: None,
        status_text: None,
        accounts: &[],
        settings_alert: false,
        pinned: false,
        filter: crate::model::FILTER_NONE,
        hidden_fields: hidden,
        field_order: DisplayField::ALL.to_vec(),
        name_color_theme: crate::config::NameColorTheme::Classic,
        query: None,
        query_draft: "",
        query_editing: false,
        query_caret: true,
    }
}

fn fields(rows: &[Row], hidden: &[DisplayField]) -> Vec<DisplayField> {
    Column::all(&view(rows, hidden))
        .into_iter()
        .filter_map(|c| match c {
            Column::Field(field) => Some(field),
            Column::Status => None,
        })
        .collect()
}

const FULL: &str = r#"{
  "t":"2026-10-20T09:59:00Z","agentName":"Claude Code","providerName":"Anthropic",
  "providerKeyName":"main","req":"sonnet-group","model":"anthropic/claude-sonnet-4",
  "served":"claude-sonnet-4","ep":"/v1/messages → /v1/chat/completions",
  "in":1000,"out":200,"cache_read":4000,"cache_write":500,"reasoning":80,
  "ms":5000,"ttft_ms":1000,"cost":0.25,"priced":true
}"#;

#[test]
fn the_status_dot_always_leads() {
    let rows = [row(FULL)];
    let columns = Column::all(&view(&rows, &[]));
    assert_eq!(columns[0], Column::Status);
    assert_eq!(columns[0].text(&rows[0], 0), "●");
}

#[test]
fn columns_with_nothing_to_show_are_dropped() {
    let quiet = vec![row(r#"{"model":"gpt-5"}"#)];
    let shown = fields(&quiet, &[]);
    for absent in [
        DisplayField::Error,
        DisplayField::UpstreamModel,
        DisplayField::Retry,
    ] {
        assert!(!shown.contains(&absent), "{absent:?} should be hidden");
    }

    // The error column only appears when the cache column is off: a failed
    // row's status code already rides in the cache cell.
    let rich = vec![row(
        r#"{"model":"a","served":"b","providerKeyName":"k","status":500,"err":"boom","tries":2}"#,
    )];
    let shown = fields(&rich, &[DisplayField::Cache]);
    for present in [
        DisplayField::Error,
        DisplayField::UpstreamModel,
        DisplayField::Retry,
    ] {
        assert!(shown.contains(&present), "{present:?} should be shown");
    }
}

#[test]
fn the_error_column_steps_aside_for_the_cache_column() {
    let failed = [row(r#"{"model":"a","status":502}"#)];
    assert!(!fields(&failed, &[]).contains(&DisplayField::Error));
    assert!(fields(&failed, &[DisplayField::Cache]).contains(&DisplayField::Error));
}

#[test]
fn the_agent_column_steps_aside_for_one_caller() {
    let same = [
        row(r#"{"model":"a","agentName":"Claude Code"}"#),
        row(r#"{"model":"b","agent":"Claude Code"}"#),
    ];
    assert!(!fields(&same, &[]).contains(&DisplayField::Agent));
    assert_eq!(view(&same, &[]).single_agent(), Some("Claude Code"));

    // A second caller brings the column back.
    let mixed = [
        row(r#"{"model":"a","agentName":"Claude Code"}"#),
        row(r#"{"model":"b","agent":"Codex"}"#),
    ];
    assert!(fields(&mixed, &[]).contains(&DisplayField::Agent));
    assert_eq!(view(&mixed, &[]).single_agent(), None);

    // One row without a caller is not proof of "only this caller".
    let partial = [
        row(r#"{"model":"a","agentName":"Claude Code"}"#),
        row(r#"{"model":"b"}"#),
    ];
    assert!(fields(&partial, &[]).contains(&DisplayField::Agent));
    assert_eq!(view(&partial, &[]).single_agent(), None);

    // A single row counts: every row names the same caller.
    let single = [row(r#"{"model":"a","agentName":"Claude Code"}"#)];
    assert!(!fields(&single, &[]).contains(&DisplayField::Agent));
    assert_eq!(view(&single, &[]).single_agent(), Some("Claude Code"));

    // The settings window has the final say.
    let hidden = [DisplayField::Agent];
    assert!(!fields(&same, &hidden).contains(&DisplayField::Agent));
    assert_eq!(view(&same, &hidden).single_agent(), None);
    assert_eq!(view(&[], &[]).single_agent(), None);
}

#[test]
fn switching_a_column_off_removes_it() {
    let rows = [row(FULL)];
    let shown = fields(&rows, &[DisplayField::Speed, DisplayField::Cost]);
    assert!(!shown.contains(&DisplayField::Speed));
    assert!(!shown.contains(&DisplayField::Cost));
    assert!(shown.contains(&DisplayField::Model));
}

#[test]
fn saved_column_order_keeps_the_status_dot_first() {
    let rows = [row(FULL)];
    let mut v = view(&rows, &[]);
    v.field_order.reverse();
    let columns = Column::all(&v);
    assert_eq!(columns[0], Column::Status);
    let shown: Vec<_> = columns
        .into_iter()
        .filter_map(|column| match column {
            Column::Status => None,
            Column::Field(field) => Some(field),
        })
        .collect();
    let expected: Vec<_> = fields(&rows, &[]).into_iter().rev().collect();
    assert_eq!(shown, expected);
}

#[test]
fn hidden_and_dynamic_columns_reappear_at_their_saved_positions() {
    let mut config = crate::config::Config {
        field_order: vec![
            DisplayField::Retry,
            DisplayField::Error,
            DisplayField::UpstreamModel,
            DisplayField::Model,
        ],
        hidden_fields: DisplayField::ALL
            .into_iter()
            .filter(|field| {
                !matches!(
                    field,
                    DisplayField::Retry
                        | DisplayField::Error
                        | DisplayField::UpstreamModel
                        | DisplayField::Model
                )
            })
            .collect(),
        ..crate::config::Config::default()
    };
    let quiet = [row(r#"{"model":"a"}"#)];
    let mut v = view(&quiet, &config.hidden_fields);
    v.field_order = config.ordered_fields();
    assert_eq!(
        Column::all(&v),
        vec![Column::Status, Column::Field(DisplayField::Model)]
    );
    let rich = [row(r#"{"model":"a","served":"b","status":500,"tries":2}"#)];
    let mut v = view(&rich, &config.hidden_fields);
    v.field_order = config.ordered_fields();
    let expected = vec![
        Column::Status,
        Column::Field(DisplayField::Retry),
        Column::Field(DisplayField::Error),
        Column::Field(DisplayField::UpstreamModel),
        Column::Field(DisplayField::Model),
    ];
    assert_eq!(Column::all(&v), expected);
    config.toggle_field(DisplayField::Retry);
    let mut v = view(&rich, &config.hidden_fields);
    v.field_order = config.ordered_fields();
    assert_eq!(
        Column::all(&v),
        [expected[..1].to_vec(), expected[2..].to_vec()].concat()
    );
    config.toggle_field(DisplayField::Retry);
    let mut v = view(&rich, &config.hidden_fields);
    v.field_order = config.ordered_fields();
    assert_eq!(Column::all(&v), expected);
}

#[test]
fn provider_cell_distinguishes_accounts_and_keeps_key_only_labels() {
    let column = Column::Field(DisplayField::Provider);
    let multi_account_providers =
        std::sync::Arc::new(std::collections::HashSet::from(["Provider".to_string()]));
    for (json, expected) in [
        (
            r#"{"providerName":"Provider","providerAccount":"first"}"#,
            "Provider(first)",
        ),
        (
            r#"{"providerName":"Provider","providerAccount":"second"}"#,
            "Provider(second)",
        ),
        (
            r#"{"providerName":"Provider","providerKeyName":"key"}"#,
            "Provider(key)",
        ),
        (
            r#"{"providerName":"Other","providerAccount":"only"}"#,
            "Other",
        ),
        (
            r#"{"providerName":"Other","providerAccount":"only","providerKeyName":"key"}"#,
            "Other(key)",
        ),
    ] {
        let mut row = row(json);
        row.multi_account_providers = std::sync::Arc::clone(&multi_account_providers);
        assert_eq!(column.text(&row, 0), expected);
    }
    assert_eq!(
        column.text(
            &row(r#"{"providerName":"Provider","providerAccount":"only"}"#),
            0
        ),
        "Provider"
    );
}

#[test]
fn the_model_cell_prefers_the_served_model_with_its_effort() {
    let rows = [row(FULL)];
    let column = Column::Field(DisplayField::Model);
    assert_eq!(column.text(&rows[0], 0), "[claude-sonnet-4]");
}

#[test]
fn token_cells_are_compact_and_the_cache_cell_carries_the_status_code() {
    let rows = [row(FULL)];
    let tokens = Column::Field(DisplayField::Tokens).text(&rows[0], 0);
    assert_eq!(tokens, fmt::tokens_compact(5700));

    let cache = Column::Field(DisplayField::Cache).text(&rows[0], 0);
    assert!(cache.ends_with('%'));

    let failed = row(r#"{"model":"a","status":502,"in":100,"cache_read":900}"#);
    assert_eq!(Column::Field(DisplayField::Cache).text(&failed, 0), "502");
    let failed_rows = [failed.clone()];
    assert_eq!(
        Column::Field(DisplayField::Cache).color(&failed, &view(&failed_rows, &[]), &[], &[]),
        theme::RED
    );
}

#[test]
fn retry_cells_distinguish_a_retry_from_a_handover() {
    assert_eq!(
        Column::Field(DisplayField::Retry).text(&row(r#"{"tries":3}"#), 0),
        "2×"
    );
    assert_eq!(
        Column::Field(DisplayField::Retry).text(&row(r#"{"handed_on":true}"#), 0),
        "交"
    );
    assert_eq!(Column::Field(DisplayField::Retry).text(&row("{}"), 0), "");
}

#[test]
fn live_retries_show_the_attempt_count_and_latest_failure_code() {
    let mut active: crate::model::ActiveRequest =
        serde_json::from_str(r#"{"route":9,"started":"2026-10-20T09:00:00Z","model":"gpt-5"}"#)
            .unwrap();
    active.trace_attempts = 2;
    active.trace_status = Some(429);
    let live = Row::from_active(&active);

    assert_eq!(Column::Field(DisplayField::Retry).text(&live, 0), "1×");
    assert_eq!(Column::Field(DisplayField::Cache).text(&live, 0), "429");
    assert_eq!(Column::Field(DisplayField::Error).text(&live, 0), "429");
    assert!(fields(std::slice::from_ref(&live), &[]).contains(&DisplayField::Retry));
}

#[test]
fn source_and_completion_cells_read_as_the_design_describes() {
    let row = row(r#"{"ep":"chat","host":"api.example","out":1200,"reasoning":300}"#);
    assert_eq!(
        Column::Field(DisplayField::Source).text(&row, 0),
        "chat · api.example"
    );
    assert_eq!(
        Column::Field(DisplayField::Completion).text(&row, 0),
        "出 1.2k 思 300"
    );
}

#[test]
fn error_cells_only_show_codes_and_long_messages_never_widen_the_table() {
    let short = row(r#"{"model":"a","status":500,"err":"short"}"#);
    let mut long = short.clone();
    long.error = Some("很长的错误\n".repeat(1000));
    long.error_type = Some("a_very_long_error_type".repeat(100));
    let columns = Column::all(&view(std::slice::from_ref(&long), &[]));
    let widths = |row: &Row| {
        column_widths(std::slice::from_ref(row), &columns, 0, 1.0, |s| {
            s.chars().count() as f32
        })
    };
    assert_eq!(Column::Field(DisplayField::Error).text(&long, 0), "500");
    assert_eq!(widths(&short), widths(&long));
    assert_eq!(
        long.error.as_deref(),
        Some("很长的错误\n".repeat(1000).as_str())
    );
}

#[test]
fn error_column_can_show_a_status_only_record_but_not_an_error_message_without_a_code() {
    let failed = [row(r#"{"status":502}"#)];
    assert!(fields(&failed, &[DisplayField::Cache]).contains(&DisplayField::Error));
    assert_eq!(
        Column::Field(DisplayField::Error).text(&failed[0], 0),
        "502"
    );
    let without_code = [row(r#"{"status":0,"err":"timeout"}"#)];
    assert!(!fields(&without_code, &[DisplayField::Cache]).contains(&DisplayField::Error));
    assert_eq!(
        Column::Field(DisplayField::Error).text(&without_code[0], 0),
        ""
    );
    let recovered = row(r#"{"status":200,"tries":3}"#);
    assert_eq!(Column::Field(DisplayField::Error).text(&recovered, 0), "");
}

#[test]
fn a_low_cache_rate_is_red_and_a_healthy_one_green() {
    let healthy = row(FULL);
    let weak = row(r#"{"in":50000,"cache_read":1000}"#);
    let rows = [healthy.clone(), weak.clone()];
    let v = view(&rows, &[]);
    let color = |row: &Row| Column::Field(DisplayField::Cache).color(row, &v, &[], &[]);
    assert_eq!(color(&healthy), theme::GREEN);
    assert_eq!(color(&weak), theme::RED);
}

#[test]
fn a_provider_keeps_the_colour_the_palette_gave_it() {
    let rows = vec![
        row(r#"{"model":"a","provider":"openai"}"#),
        row(r#"{"model":"b","provider":"anthropic"}"#),
    ];
    let v = view(&rows, &[]);
    let providers = panel::provider_palette(&rows, v.name_color_theme);
    let color = Column::Field(DisplayField::Provider).color(&rows[0], &v, &providers, &[]);
    let expected = providers
        .iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, c)| *c);
    assert_eq!(Some(color), expected);
}

#[test]
fn theme_and_model_effort_colours_reach_cells_without_changing_status_colours() {
    let rows = vec![
        row(r#"{"model":"a","effort":"high","provider":"X","status":502}"#),
        row(r#"{"model":"a","effort":"low","provider":"X","status":200}"#),
        row(r#"{"model":"a","provider":"Y","status":200}"#),
    ];
    for theme in crate::config::NameColorTheme::ALL {
        let mut v = view(&rows, &[]);
        v.name_color_theme = theme;
        let providers = panel::provider_palette(&rows, v.name_color_theme);
        let models = panel::model_palette(&rows, v.name_color_theme);
        let model = Column::Field(DisplayField::Model);
        let colours: Vec<_> = rows
            .iter()
            .map(|row| model.color(row, &v, &providers, &models))
            .collect();
        assert_ne!(colours[0], colours[1]);
        assert_ne!(colours[1], colours[2]);
        assert_ne!(colours[0], colours[2]);
        for row in &rows {
            let key = panel::model_color_key(row);
            let expected = models.iter().find(|(name, _)| *name == key).unwrap().1;
            assert_eq!(model.color(row, &v, &providers, &models), expected);
        }
        assert_eq!(
            Column::Field(DisplayField::Provider).color(&rows[0], &v, &providers, &models),
            providers[1].1
        );
        assert_eq!(
            Column::Status.color(&rows[0], &v, &providers, &models),
            theme::RED
        );
        assert_eq!(
            Column::Status.color(&rows[1], &v, &providers, &models),
            theme::GREEN
        );
        assert_eq!(
            Column::Field(DisplayField::Cache).color(&rows[0], &v, &providers, &models),
            theme::RED
        );
        assert_eq!(
            Column::Field(DisplayField::Agent).color(&rows[0], &v, &providers, &models),
            theme::RED
        );
    }
}

#[test]
fn a_slow_first_token_warms_the_whole_latency_cell() {
    let rows = [
        row(r#"{"model":"a","ms":59999,"ttft_ms":59999}"#),
        row(r#"{"model":"a","ms":60000,"ttft_ms":60000}"#),
        row(r#"{"model":"a","ms":119999,"ttft_ms":119999}"#),
        row(r#"{"model":"a","ms":120000,"ttft_ms":120000}"#),
    ];
    let v = view(&rows, &[]);
    let color = |row: &Row| Column::Field(DisplayField::Latency).color(row, &v, &[], &[]);
    assert_eq!(color(&rows[0]), theme::TEXT_DIM);
    assert_eq!(color(&rows[1]), theme::YELLOW);
    assert_eq!(color(&rows[2]), theme::YELLOW);
    assert_eq!(color(&rows[3]), theme::RED);
}

#[test]
fn a_latency_cell_without_a_first_token_judges_the_running_total() {
    // A request still in flight has only its elapsed time so far.
    let live = row(r#"{"model":"a","ms":130000}"#);
    // A fast first token keeps the cell calm however long the rest took.
    let long_generation = row(r#"{"model":"a","ms":200000,"ttft_ms":1000}"#);
    // Zero is a real first-token reading, not a missing one.
    let immediate = row(r#"{"model":"a","ms":200000,"ttft_ms":0}"#);
    let rows = [live.clone(), long_generation.clone(), immediate.clone()];
    let v = view(&rows, &[]);
    let color = |row: &Row| Column::Field(DisplayField::Latency).color(row, &v, &[], &[]);
    assert_eq!(color(&live), theme::RED);
    assert_eq!(color(&long_generation), theme::TEXT_DIM);
    assert_eq!(color(&immediate), theme::TEXT_DIM);
}

#[test]
fn supplier_width_is_capped_at_each_density_without_capping_other_columns() {
    let mut long = row(r#"{"provider":"p","providerAccount":"account","model":"m"}"#);
    long.multi_account_providers =
        std::sync::Arc::new(std::collections::HashSet::from(["p".into()]));
    long.provider_account = Some("长账号".repeat(100));
    long.model = "x".repeat(100);
    let rows = [long];
    let columns = [
        Column::Field(DisplayField::Provider),
        Column::Field(DisplayField::Model),
    ];
    for scale in [1.0, 1.5, 2.0] {
        let measure = |s: &str| s.chars().count() as f32 * 8.0 * scale;
        let widths = column_widths(&rows, &columns, 0, scale, measure);
        assert_eq!(widths[0], PROVIDER_MAX_WIDTH * scale);
        assert!(widths[1] > PROVIDER_MAX_WIDTH * scale);
        let full = columns[0].text(&rows[0], 0);
        let shown = ellipsize(&full, widths[0] - CELL_HPAD * scale * 2.0, measure);
        assert!(shown.ends_with("…)"));
        assert!(measure(&shown) <= widths[0] - CELL_HPAD * scale * 2.0);
        assert_eq!(
            rows[0].provider_account.as_ref().unwrap().chars().count(),
            300
        );
    }
}

#[test]
fn supplier_ellipsis_preserves_short_text_and_never_splits_unicode() {
    let measure = |s: &str| {
        s.chars()
            .map(|c| if c.is_ascii() { 1.0 } else { 2.0 })
            .sum::<f32>()
    };
    assert_eq!(ellipsize("P(account)", 20.0, measure), "P(account)");
    assert_eq!(ellipsize("P(中文账号🙂)", 9.0, measure), "P(中文…)");
    assert_eq!(ellipsize("long-provider", 6.0, measure), "long…");
    assert_eq!(ellipsize("long-provider", 1.0, measure), "");
    assert_eq!(ellipsize("P(account)", 2.0, measure), "…");
    assert_eq!(ellipsize("", 0.0, measure), "");
}

#[test]
fn columns_are_as_wide_as_their_widest_cell() {
    let rows = vec![
        row(r#"{"model":"short"}"#),
        row(r#"{"model":"a-much-longer-model-name"}"#),
    ];
    let columns = Column::all(&view(&rows, &[]));
    let widths = column_widths(&rows, &columns, 0, 2.0, |text| text.chars().count() as f32);
    assert_eq!(widths.len(), columns.len());
    let model_index = columns
        .iter()
        .position(|c| *c == Column::Field(DisplayField::Model))
        .expect("model column");
    assert_eq!(widths[model_index], 24.0 + 4.0);
}
