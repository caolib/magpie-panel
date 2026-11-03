use super::*;
use crate::model::Status;

fn row(json: &str, account: &str) -> Row {
    let record: crate::model::UsageRecord = serde_json::from_str(json).expect("valid record");
    Row::from_record(&record).with_account(account, account)
}

fn accounts() -> Vec<Account> {
    vec![
        Account {
            id: "a1".into(),
            name: "本机".into(),
            endpoint: "http://127.0.0.1:3425".into(),
        },
        Account {
            id: "a2".into(),
            name: "公司".into(),
            endpoint: "https://magpie.example".into(),
        },
    ]
}

fn view<'a>(rows: &'a [Row], accounts: &'a [Account]) -> ListView<'a> {
    ListView {
        rows,
        now: 1_800_000_000,
        scroll: 0.0,
        hover: None,
        selected: None,
        status_text: None,
        accounts,
        settings_alert: false,
        pinned: false,
        filter: crate::model::FILTER_NONE,
        hidden_fields: &[],
        name_color_theme: NameColorTheme::Classic,
        query: None,
        query_draft: "",
        query_editing: false,
        query_caret: true,
    }
}

#[test]
fn each_connection_gets_a_stable_tag_colour() {
    let accounts = accounts();
    assert_eq!(account_color(&accounts, "a1"), theme::MAUVE);
    assert_eq!(account_color(&accounts, "a2"), theme::ORANGE);
    // An unknown connection (e.g. a deleted one) falls back to the dim text.
    assert_eq!(account_color(&accounts, "zz"), theme::TEXT_DIM);
}

#[test]
fn the_header_has_one_chip_per_filter() {
    let m = Metrics::new(800.0, 300.0, 1.0);
    let rows: &[Row] = &[];
    let accounts: &[Account] = &[];
    let v = view(rows, accounts);
    let y = m.header_h / 2.0;
    for filter in [
        crate::model::Filter::All,
        crate::model::Filter::Completed,
        crate::model::Filter::Failed,
        crate::model::Filter::Live,
        crate::model::Filter::Retried,
    ] {
        let rect = chip_rect(m, &v, filter);
        assert_eq!(hit_filter(m, &v, rect.x + 1.0, y), Some(filter));
    }
    assert_eq!(hit_filter(m, &v, m.width - 2.0, y), None);
}

#[test]
fn pinned_mode_is_display_only() {
    let m = Metrics::new(800.0, 300.0, 1.0);
    let rows: &[Row] = &[];
    let accounts: &[Account] = &[];
    let mut v = view(rows, accounts);
    v.pinned = true;
    let rect = chip_rect(m, &v, crate::model::Filter::Failed);
    assert_eq!(hit_filter(m, &v, rect.x + 1.0, m.header_h / 2.0), None);
}

#[test]
fn the_query_box_sits_left_of_the_settings_button() {
    let m = Metrics::new(800.0, 300.0, 1.0);
    let query = query_box_rect(m);
    let settings = settings_button(m);
    assert!(query.x + query.w < settings.x);
    assert!((query.y - settings.y).abs() < 1.0);
    // A maximally narrow panel would still leave them apart, which is what
    // `header_min_width` reserves room for.
    let min = header_min_width(m);
    assert!(min > 2.0 * m.pad());
}

#[test]
fn a_row_is_hit_only_inside_the_list_proper() {
    let m = Metrics::new(800.0, 400.0, 1.0);
    let rows = vec![row(r#"{"model":"gpt-5"}"#, "a1")];
    let accounts = accounts();
    let v = view(&rows, &accounts);
    let rect = m.row_rect(0, 0.0);
    assert_eq!(hit_row(m, &v, rect.x + 5.0, rect.y + 2.0), Some(0));
    // The window padding either side is a drag handle, not a row.
    assert_eq!(hit_row(m, &v, 1.0, rect.y + 2.0), None);
    assert_eq!(hit_row(m, &v, 400.0, 1.0), None);
}

#[test]
fn only_rows_with_something_to_explain_are_clickable() {
    let m = Metrics::new(800.0, 400.0, 1.0);
    let rows = vec![
        row(r#"{"model":"ok"}"#, "a1"),
        row(r#"{"status":500,"model":"bad"}"#, "a1"),
    ];
    let accounts = accounts();
    let v = view(&rows, &accounts);
    let ok = m.row_rect(0, 0.0);
    let bad = m.row_rect(1, 0.0);
    assert_eq!(hit_error_row(m, &v, ok.x + 5.0, ok.y + 2.0), None);
    assert_eq!(hit_error_row(m, &v, bad.x + 5.0, bad.y + 2.0), Some(1));
    assert_eq!(rows[1].status, Status::Failed);
}

#[test]
fn hidden_filter_chips_do_not_shift_the_others() {
    // The chip row never reflows: filters are local, so their geometry is
    // fixed and a click always lands on the chip that is drawn there.
    let m = Metrics::new(800.0, 300.0, 1.0);
    let rows: &[Row] = &[];
    let accounts: &[Account] = &[];
    let v = view(rows, accounts);
    let all = chip_rect(m, &v, crate::model::Filter::All);
    let failed = chip_rect(m, &v, crate::model::Filter::Failed);
    assert!(failed.x > all.x);
    assert_eq!(failed.x - all.x, 2.0 * (FILTER_W + FILTER_GAP) * m.scale);
}

#[test]
fn name_colours_follow_first_encounter_from_the_bottom() {
    let rows = vec![
        row(r#"{"model":"A","provider":"Y"}"#, "a2"),
        row(r#"{"model":"B","provider":"X"}"#, "a1"),
        row(r#"{"model":"A","provider":"X"}"#, "a1"),
        row(r#"{"model":"A","provider":"Y"}"#, "a1"),
    ];
    assert_eq!(
        model_palette(&rows, NameColorTheme::Classic),
        vec![(("A", None), theme::CYAN), (("B", None), theme::ORANGE)]
    );
    assert_eq!(
        provider_palette(&rows, NameColorTheme::Classic),
        vec![("Y", theme::CYAN), ("X", theme::ORANGE)]
    );
}

#[test]
fn new_top_rows_do_not_recolour_names_still_present_below() {
    let mut rows = vec![
        row(r#"{"model":"B","provider":"Y"}"#, "a1"),
        row(r#"{"model":"A","provider":"X"}"#, "a1"),
    ];
    for json in [
        r#"{"model":"A","provider":"X"}"#,
        r#"{"model":"C","provider":"Z"}"#,
        r#"{"model":"B","provider":"Y"}"#,
    ] {
        rows.insert(0, row(json, "a2"));
        let models = model_palette(&rows, NameColorTheme::Classic);
        let providers = provider_palette(&rows, NameColorTheme::Classic);
        assert_eq!(models[0], (("A", None), theme::CYAN));
        assert_eq!(models[1], (("B", None), theme::ORANGE));
        assert_eq!(providers[0], ("X", theme::CYAN));
        assert_eq!(providers[1], ("Y", theme::ORANGE));
    }
    assert_eq!(
        model_palette(&rows, NameColorTheme::Classic)[2],
        (("C", None), NAME_PALETTE[2])
    );
    assert_eq!(
        provider_palette(&rows, NameColorTheme::Classic)[2],
        ("Z", NAME_PALETTE[2])
    );
}

#[test]
fn empty_rows_and_missing_providers_do_not_take_palette_slots() {
    assert!(model_palette(&[], NameColorTheme::Classic).is_empty());
    assert!(provider_palette(&[], NameColorTheme::Classic).is_empty());
    let rows = vec![
        row(r#"{"model":"A","provider":"X"}"#, "a1"),
        row(r#"{"model":"A"}"#, "a1"),
    ];
    assert_eq!(
        model_palette(&rows, NameColorTheme::Classic),
        vec![(("A", None), theme::CYAN)]
    );
    assert_eq!(
        provider_palette(&rows, NameColorTheme::Classic),
        vec![("X", theme::CYAN)]
    );
}

#[test]
fn every_name_theme_uses_its_palette_in_bottom_up_order() {
    let rows = vec![
        row(r#"{"model":"B","provider":"Y"}"#, "a1"),
        row(r#"{"model":"A","provider":"X"}"#, "a1"),
    ];
    for (theme, palette) in [
        (NameColorTheme::Classic, &NAME_PALETTE[..]),
        (NameColorTheme::Soft, &SOFT_PALETTE[..]),
        (NameColorTheme::Bright, &BRIGHT_PALETTE[..]),
        (NameColorTheme::CoolWarm, &COOL_WARM_PALETTE[..]),
    ] {
        assert_eq!(name_palette(theme), palette);
        assert_eq!(
            model_palette(&rows, theme),
            vec![(("A", None), palette[0]), (("B", None), palette[1])]
        );
        assert_eq!(
            provider_palette(&rows, theme),
            vec![("X", palette[0]), ("Y", palette[1])]
        );
        assert!(palette.iter().all(|color| color >> 24 == 0xFF));
    }
}

#[test]
fn model_colours_distinguish_efforts_but_not_routing_brackets() {
    let rows = vec![
        row(r#"{"model":"A","effort":"high"}"#, "a1"),
        row(r#"{"req":"group","model":"A","effort":"low"}"#, "a2"),
        row(r#"{"model":"A","effort":"low"}"#, "a1"),
        row(r#"{"model":"A"}"#, "a1"),
    ];
    assert_eq!(
        model_palette(&rows, NameColorTheme::Classic),
        vec![
            (("A", None), NAME_PALETTE[0]),
            (("A", Some("low")), NAME_PALETTE[1]),
            (("A", Some("high")), NAME_PALETTE[2]),
        ]
    );
    assert_ne!(rows[1].display_model(), rows[2].display_model());
    assert_eq!(model_color_key(&rows[1]), model_color_key(&rows[2]));
}

#[test]
fn live_and_finished_models_share_effort_colour_identity() {
    let active: crate::model::ActiveRequest =
        serde_json::from_str(r#"{"model":"A","effort":"high","route":7}"#).unwrap();
    let rows = vec![
        Row::from_active(&active),
        row(r#"{"model":"A","effort":"high"}"#, "a1"),
        row(r#"{"model":"A","effort":"low"}"#, "a1"),
    ];
    assert_eq!(model_color_key(&rows[0]), model_color_key(&rows[1]));
    assert_eq!(
        model_palette(&rows, NameColorTheme::Classic),
        vec![
            (("A", Some("low")), NAME_PALETTE[0]),
            (("A", Some("high")), NAME_PALETTE[1]),
        ]
    );
}

#[test]
fn name_palette_overflow_does_not_reuse_category_colours() {
    for theme in NameColorTheme::ALL {
        let palette = name_palette(theme);
        let rows: Vec<_> = (0..palette.len() + 2)
            .rev()
            .map(|i| row(&format!(r#"{{"model":"M{i}","provider":"P{i}"}}"#), "a1"))
            .collect();
        let models = model_palette(&rows, theme);
        let providers = provider_palette(&rows, theme);
        for i in 0..rows.len() {
            let expected = palette.get(i).copied().unwrap_or(theme::TEXT);
            assert_eq!(models[i].1, expected);
            assert_eq!(providers[i].1, expected);
        }
    }
}

#[test]
fn every_column_in_the_table_has_a_distinct_colour_source() {
    let rows = vec![
        row(
            r#"{"model":"anthropic/claude","providerName":"Anthropic","provider":"anthropic","agentName":"Claude Code"}"#,
            "a1",
        ),
        row(
            r#"{"model":"openai/gpt-5","providerName":"OpenAI","provider":"openai"}"#,
            "a2",
        ),
    ];
    let accounts = accounts();
    let v = view(&rows, &accounts);
    let providers = provider_palette(&rows, NameColorTheme::Classic);
    let models = model_palette(&rows, NameColorTheme::Classic);
    assert_eq!(providers.len(), 2);
    assert_eq!(models.len(), 2);
    assert_ne!(providers[0].1, providers[1].1);
    assert!(v.shows(crate::config::DisplayField::Model));
}
