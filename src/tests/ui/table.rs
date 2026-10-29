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
fn switching_a_column_off_removes_it() {
    let rows = [row(FULL)];
    let shown = fields(&rows, &[DisplayField::Speed, DisplayField::Cost]);
    assert!(!shown.contains(&DisplayField::Speed));
    assert!(!shown.contains(&DisplayField::Cost));
    assert!(shown.contains(&DisplayField::Model));
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
    let providers = panel::provider_palette(&rows);
    let color = Column::Field(DisplayField::Provider).color(&rows[0], &v, &providers, &[]);
    let expected = providers
        .iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, c)| *c);
    assert_eq!(Some(color), expected);
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
