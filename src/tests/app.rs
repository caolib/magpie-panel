use super::*;
use crate::config::{DisplayField, NameAlias};
use crate::model::Status;

fn row(json: &str, account: &str) -> Row {
    let record: crate::model::UsageRecord = serde_json::from_str(json).expect("valid record");
    Row::from_record(&record).with_account(account, account)
}

fn live_row(json: &str, account: &str) -> Row {
    let active: crate::model::ActiveRequest =
        serde_json::from_str(json).expect("valid live request");
    Row::from_active(&active).with_account(account, account)
}

fn app_with(rows: Vec<Row>) -> App {
    let mut app = App::new(Config::default());
    app.apply_updates(vec![Update::Snapshot { rows }]);
    app
}

#[test]
fn normal_windows_remain_activatable_after_query_entry_ends() {
    let mut app = app_with(Vec::new());
    for level in [
        WindowLevel::Normal,
        WindowLevel::Topmost,
        WindowLevel::Bottommost,
    ] {
        app.config.set_window_level(level);
        app.view = View::List;
        for editing in [false, true, false] {
            app.query_editing = editing;
            assert_eq!(
                app.wants_activation(),
                editing || level == WindowLevel::Normal
            );
        }
        // Form entry always needs the keyboard, regardless of window level.
        app.view = View::Login;
        assert!(app.wants_activation());
        app.view = View::List;
        assert_eq!(app.wants_activation(), level == WindowLevel::Normal);
    }
}

#[test]
fn a_snapshot_replaces_the_list_newest_first() {
    let app = app_with(vec![
        row(r#"{"t":"2026-10-20T09:00:00Z","model":"a"}"#, "a1"),
        row(r#"{"t":"2026-10-20T11:00:00Z","model":"b"}"#, "a1"),
    ]);
    assert_eq!(app.rows.len(), 2);
    assert_eq!(app.rows[0].model, "b");
}

#[test]
fn the_row_limit_caps_what_is_shown() {
    let mut app = app_with(
        (0..5)
            .map(|i| row(&format!(r#"{{"t":"2026-10-20T0{i}:00:00Z"}}"#), "a1"))
            .collect(),
    );
    app.config.row_limit = 2;
    app.rebuild();
    assert_eq!(app.rows.len(), 2);
}

#[test]
fn live_first_groups_all_connections_and_can_restore_time_order() {
    let mut app = app_with(vec![
        live_row(
            r#"{"started":"2026-10-20T09:00:00Z","model":"live-old"}"#,
            "a1",
        ),
        row(r#"{"t":"2026-10-20T12:00:00Z","model":"done"}"#, "a1"),
        live_row(
            r#"{"started":"2026-10-20T10:00:00Z","model":"live-new"}"#,
            "a2",
        ),
        row(
            r#"{"t":"2026-10-20T11:00:00Z","model":"failed","status":500}"#,
            "a2",
        ),
    ]);
    for (enabled, expected) in [
        (false, ["done", "failed", "live-new", "live-old"]),
        (true, ["live-new", "live-old", "done", "failed"]),
        (false, ["done", "failed", "live-new", "live-old"]),
    ] {
        app.config.live_first = enabled;
        app.rebuild();
        assert_eq!(
            app.rows
                .iter()
                .map(|r| r.model.as_str())
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn live_first_prioritizes_before_applying_the_row_limit() {
    let mut app = app_with(vec![
        row(r#"{"t":"2026-10-20T12:00:00Z","model":"done"}"#, "a1"),
        live_row(
            r#"{"started":"2026-10-20T09:00:00Z","model":"live-old"}"#,
            "a1",
        ),
        live_row(
            r#"{"started":"2026-10-20T10:00:00Z","model":"live-new"}"#,
            "a2",
        ),
    ]);
    app.config.live_first = true;
    for limit in [1, 2, 3] {
        app.config.row_limit = limit;
        app.rebuild();
        assert_eq!(
            app.rows
                .iter()
                .map(|r| r.model.as_str())
                .collect::<Vec<_>>(),
            ["live-new", "live-old", "done"][..limit as usize]
        );
    }
}

#[test]
fn live_first_still_respects_query_status_and_hidden_providers() {
    let mut app = app_with(vec![
        row(r#"{"t":"2026-10-20T12:00:00Z","model":"match-done"}"#, "a1"),
        live_row(
            r#"{"started":"2026-10-20T09:00:00Z","model":"match-hidden","provider":"p"}"#,
            "a1",
        ),
        live_row(
            r#"{"started":"2026-10-20T10:00:00Z","model":"match-live","provider":"p"}"#,
            "a2",
        ),
        live_row(
            r#"{"started":"2026-10-20T11:00:00Z","model":"other","provider":"p"}"#,
            "a2",
        ),
    ]);
    app.config.live_first = true;
    app.config.hidden_providers.push(HiddenProvider {
        account_id: "a1".into(),
        provider: "p".into(),
    });
    app.query_draft = "match".into();
    app.apply_query();
    assert_eq!(
        app.rows
            .iter()
            .map(|r| r.model.as_str())
            .collect::<Vec<_>>(),
        ["match-live", "match-done"]
    );
    app.click_filter(Filter::Completed);
    assert_eq!(app.rows.len(), 1);
    assert_eq!(app.rows[0].model, "match-done");
    app.click_filter(Filter::All);
    app.click_filter(Filter::Live);
    assert_eq!(app.rows.len(), 1);
    assert_eq!(app.rows[0].model, "match-live");
    app.query_draft = "missing".into();
    app.apply_query();
    assert!(app.rows.is_empty());
}

#[test]
fn finished_requests_leave_the_live_first_group_on_the_next_snapshot() {
    let newer = row(r#"{"t":"2026-10-20T12:00:00Z","model":"newer"}"#, "a2");
    let mut app = app_with(vec![
        newer.clone(),
        live_row(
            r#"{"started":"2026-10-20T09:00:00Z","model":"older"}"#,
            "a1",
        ),
    ]);
    app.config.live_first = true;
    app.rebuild();
    assert_eq!(app.rows[0].model, "older");
    for status in [200, 500] {
        app.apply_updates(vec![Update::Snapshot {
            rows: vec![
                newer.clone(),
                row(
                    &format!(r#"{{"t":"2026-10-20T09:00:00Z","model":"older","status":{status}}}"#),
                    "a1",
                ),
            ],
        }]);
        assert_eq!(app.rows[0].model, "newer");
        assert_eq!(app.rows[1].model, "older");
        assert!(app.rows.iter().all(|r| !r.live));
    }
}

#[test]
fn live_first_keeps_equal_times_stable_and_unknown_times_last_in_each_group() {
    let mut app = app_with(vec![
        row(r#"{"model":"done-unknown"}"#, "a1"),
        live_row(r#"{"started":"invalid","model":"live-unknown"}"#, "a1"),
        row(r#"{"t":"2026-10-20T12:00:00Z","model":"done-first"}"#, "a1"),
        live_row(
            r#"{"started":"2026-10-20T09:00:00Z","model":"live-first"}"#,
            "a1",
        ),
        live_row(
            r#"{"started":"2026-10-20T09:00:00Z","model":"live-second"}"#,
            "a2",
        ),
        row(
            r#"{"t":"2026-10-20T12:00:00Z","model":"done-second"}"#,
            "a2",
        ),
    ]);
    app.config.live_first = true;
    app.rebuild();
    assert_eq!(
        app.rows
            .iter()
            .map(|r| r.model.as_str())
            .collect::<Vec<_>>(),
        [
            "live-first",
            "live-second",
            "live-unknown",
            "done-first",
            "done-second",
            "done-unknown"
        ]
    );
}

fn model_group_fixture() -> Vec<Row> {
    [
        ("A", 6, "A-new"),
        ("B", 5, "B-new"),
        ("A", 4, "A-old"),
        ("C", 3, "C-new"),
        ("B", 2, "B-old"),
        ("C", 1, "C-old"),
    ]
    .into_iter()
    .map(|(model, second, agent)| {
        row(
            &format!(
                r#"{{"t":"2026-10-20T09:00:0{second}Z","model":"{model}","agent":"{agent}"}}"#
            ),
            "a1",
        )
    })
    .collect()
}

#[test]
fn model_grouping_defaults_off_and_can_restore_chronological_order() {
    let mut app = app_with(model_group_fixture());
    for (enabled, expected) in [
        (
            false,
            ["A-new", "B-new", "A-old", "C-new", "B-old", "C-old"],
        ),
        (true, ["A-new", "A-old", "B-new", "B-old", "C-new", "C-old"]),
        (
            false,
            ["A-new", "B-new", "A-old", "C-new", "B-old", "C-old"],
        ),
    ] {
        app.config.group_by_model = enabled;
        app.rebuild();
        assert_eq!(
            app.rows
                .iter()
                .map(|row| row.agent.as_deref().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
    }
}

#[test]
fn grouping_only_reorders_the_records_already_selected_by_the_row_limit() {
    let mut app = app_with(model_group_fixture());
    app.config.row_limit = 4;
    app.rebuild();
    let mut selected: Vec<_> = app.rows.iter().map(|r| r.id.clone()).collect();
    selected.sort();
    app.config.group_by_model = true;
    app.rebuild();
    let mut grouped: Vec<_> = app.rows.iter().map(|r| r.id.clone()).collect();
    grouped.sort();
    assert_eq!(grouped, selected);
    assert_eq!(
        app.rows
            .iter()
            .map(|row| row.agent.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["A-new", "A-old", "B-new", "C-new"]
    );
}

#[test]
fn grouping_never_moves_or_groups_live_rows_with_either_live_first_setting() {
    let mut fixture = model_group_fixture();
    fixture.extend([
        live_row(
            r#"{"route":1,"started":"2026-10-20T09:00:05Z","model":"A"}"#,
            "a2",
        ),
        live_row(
            r#"{"route":2,"started":"2026-10-20T09:00:03Z","model":"Z"}"#,
            "a1",
        ),
    ]);
    let mut app = app_with(fixture);
    for live_first in [false, true] {
        app.config.live_first = live_first;
        app.config.group_by_model = false;
        app.rebuild();
        let live_slots: Vec<_> = app
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.live)
            .map(|(i, r)| (i, r.id.clone()))
            .collect();
        app.config.group_by_model = true;
        app.rebuild();
        for (index, id) in live_slots {
            assert_eq!(app.rows[index].id, id);
            assert!(app.rows[index].live);
        }
        assert_eq!(
            app.rows
                .iter()
                .filter(|r| !r.live)
                .map(|r| r.agent.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["A-new", "A-old", "B-new", "B-old", "C-new", "C-old"]
        );
    }
    app.click_filter(Filter::Live);
    assert_eq!(
        app.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["live:1", "live:2"]
    );
}

#[test]
fn grouping_uses_display_model_and_effort_across_providers_connections_and_routing() {
    let mut app = app_with(vec![
        row(
            r#"{"t":"2026-10-20T09:00:05Z","model":"long-A","effort":"high","provider":"p1"}"#,
            "a1",
        ),
        row(
            r#"{"t":"2026-10-20T09:00:04Z","model":"A","effort":"low"}"#,
            "a1",
        ),
        row(
            r#"{"t":"2026-10-20T09:00:03Z","model":"A","req":"group","effort":"high","provider":"p2"}"#,
            "a2",
        ),
        row(r#"{"t":"2026-10-20T09:00:02Z","model":"A"}"#, "a1"),
        row(
            r#"{"t":"2026-10-20T09:00:01Z","model":"A","effort":"low"}"#,
            "a2",
        ),
    ]);
    app.config.model_aliases.push(NameAlias {
        from: "long-".into(),
        to: "".into(),
    });
    app.config.group_by_model = true;
    app.rebuild();
    assert_eq!(
        app.rows
            .iter()
            .map(|r| r.reasoning_effort.as_deref())
            .collect::<Vec<_>>(),
        [Some("high"), Some("high"), Some("low"), Some("low"), None]
    );
    assert_ne!(app.rows[0].display_model(), app.rows[1].display_model());
    assert_eq!(app.rows[0].model_key(), app.rows[1].model_key());
    assert_ne!(app.rows[0].account_id, app.rows[1].account_id);
    assert_ne!(app.rows[0].provider, app.rows[1].provider);
}

#[test]
fn grouping_respects_search_status_and_provider_filters_before_grouping() {
    let mut fixture = model_group_fixture();
    fixture[0].provider_id = Some("hidden".into());
    fixture[2].status = Status::Failed;
    let mut app = app_with(fixture);
    app.config.group_by_model = true;
    app.config.hidden_providers.push(HiddenProvider {
        account_id: "a1".into(),
        provider: "hidden".into(),
    });
    app.rebuild();
    assert_eq!(
        app.rows
            .iter()
            .map(|r| r.agent.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["B-new", "B-old", "A-old", "C-new", "C-old"]
    );
    app.click_filter(Filter::Completed);
    assert!(app.rows.iter().all(|r| r.status == Status::Completed));
    assert_eq!(
        app.rows
            .iter()
            .map(|r| r.agent.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["B-new", "B-old", "C-new", "C-old"]
    );
    app.query_draft = "C".into();
    app.apply_query();
    assert_eq!(
        app.rows
            .iter()
            .map(|r| r.agent.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["C-new", "C-old"]
    );
}

#[test]
fn a_finished_request_joins_its_model_group_on_the_next_snapshot() {
    let mut app = app_with(vec![
        row(r#"{"t":"2026-10-20T09:00:06Z","model":"A"}"#, "a1"),
        row(r#"{"t":"2026-10-20T09:00:05Z","model":"B"}"#, "a1"),
        live_row(r#"{"started":"2026-10-20T09:00:04Z","model":"A"}"#, "a1"),
    ]);
    app.config.group_by_model = true;
    app.rebuild();
    assert_eq!(
        app.rows
            .iter()
            .map(|r| r.model.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "A"]
    );
    for status in [200, 500] {
        let mut rows = app.raw_rows().to_vec();
        rows[2] = row(
            &format!(r#"{{"t":"2026-10-20T09:00:04Z","model":"A","status":{status}}}"#),
            "a1",
        );
        app.apply_updates(vec![Update::Snapshot { rows }]);
        assert_eq!(
            app.rows
                .iter()
                .map(|r| r.model.as_str())
                .collect::<Vec<_>>(),
            ["A", "A", "B"]
        );
    }
}

#[test]
fn model_group_permutations_keep_stable_order_and_live_slots() {
    // Exhaustive small inputs exercise both permutation directions and empty/single groups.
    for length in 0..=5 {
        for pattern in 0..3usize.pow(length) {
            let mut digits = pattern;
            let original: Vec<_> = (0..length)
                .map(|index| {
                    let category = digits % 3;
                    digits /= 3;
                    let model = ["A", "B", "live"][category];
                    if category == 2 {
                        live_row(&format!(r#"{{"route":{index},"model":"{model}"}}"#), "a1")
                    } else {
                        row(
                            &format!(r#"{{"model":"{model}","session":"{index}"}}"#),
                            "a1",
                        )
                    }
                })
                .collect();
            let mut expected = Vec::new();
            let mut names = Vec::new();
            for row in original.iter().filter(|r| !r.live) {
                if !names.contains(&row.model.as_str()) {
                    names.push(row.model.as_str());
                }
            }
            for model in names {
                expected.extend(
                    original
                        .iter()
                        .filter(|r| !r.live && r.model == model)
                        .map(|r| r.id.clone()),
                );
            }
            let mut grouped = original.clone();
            group_model_rows(&mut grouped);
            assert_eq!(
                grouped
                    .iter()
                    .filter(|r| !r.live)
                    .map(|r| r.id.clone())
                    .collect::<Vec<_>>(),
                expected
            );
            for (index, row) in original.iter().enumerate().filter(|(_, r)| r.live) {
                assert!(grouped[index].live);
                assert_eq!(grouped[index].id, row.id);
            }
        }
    }
}

#[test]
fn model_aliases_apply_to_every_model_field() {
    let mut app = app_with(vec![row(
        r#"{"req":"claude-x","model":"anthropic/claude-y","served":"claude-z"}"#,
        "a1",
    )]);
    app.config.model_aliases = vec![NameAlias {
        from: "claude".into(),
        to: "c".into(),
    }];
    app.rebuild();
    assert_eq!(app.rows[0].requested_model, "c-x");
    assert_eq!(app.rows[0].model, "c-y");
    assert_eq!(app.rows[0].served_model.as_deref(), Some("c-z"));
}

#[test]
fn provider_and_caller_aliases_only_rewrite_display_names() {
    let mut app = app_with(vec![row(
        r#"{"provider":"anthropic","providerName":"Anthropic","agentName":"Claude Code"}"#,
        "a1",
    )]);
    app.config.provider_aliases = vec![NameAlias {
        from: "Anthropic".into(),
        to: "A社".into(),
    }];
    app.config.agent_aliases = vec![NameAlias {
        from: "Claude Code".into(),
        to: "CC".into(),
    }];
    app.rebuild();
    assert_eq!(app.rows[0].provider.as_deref(), Some("A社"));
    assert_eq!(app.rows[0].agent.as_deref(), Some("CC"));
    // The wire id stays raw so provider filters still match Magpie's names.
    assert_eq!(app.rows[0].provider_id.as_deref(), Some("anthropic"));
    // Rebuild works from the raw rows, so it can run again after an edit.
    assert_eq!(app.raw_rows[0].provider.as_deref(), Some("Anthropic"));
}

#[test]
fn live_and_finished_requests_share_the_provider_display_name() {
    let live = live_row(
        r#"{"route":9,"started":"2026-10-20T09:00:01Z","provider":"workbuddy"}"#,
        "a1",
    );
    let identity = live.detail_identity();
    let mut app = app_with(vec![
        row(
            r#"{"t":"2026-10-20T09:00:00Z","provider":"workbuddy","providerName":"trae cn"}"#,
            "a1",
        ),
        live,
    ]);
    app.config.row_limit = 1;
    app.click_filter(Filter::Live);
    app.query_draft = "trae cn".into();
    app.apply_query();
    assert_eq!(app.rows.len(), 1);
    assert!(app.rows[0].live);
    assert_eq!(app.rows[0].provider.as_deref(), Some("trae cn"));
    assert_eq!(app.rows[0].provider_id.as_deref(), Some("workbuddy"));
    assert_eq!(app.rows[0].detail_identity(), identity);
    assert_eq!(app.raw_rows[1].provider.as_deref(), Some("workbuddy"));

    app.click_filter(Filter::All);
    app.apply_updates(vec![Update::Snapshot {
        rows: vec![row(
            r#"{"t":"2026-10-20T09:00:01Z","route_id":9,"provider":"workbuddy","providerName":"trae cn","status":200}"#,
            "a1",
        )],
    }]);
    assert_eq!(app.rows.len(), 1);
    assert!(!app.rows[0].live);
    assert_eq!(app.rows[0].provider.as_deref(), Some("trae cn"));
}

#[test]
fn live_provider_names_require_the_same_connection_and_provider_id() {
    let mut app = app_with(vec![
        row(r#"{"provider":"workbuddy","providerName":"trae cn"}"#, "a1"),
        row(
            r#"{"provider":"workbuddy","providerName":"Other Gateway"}"#,
            "a2",
        ),
        row(r#"{"providerName":"workbuddy"}"#, "a3"),
        live_row(r#"{"route":1,"provider":"workbuddy"}"#, "a1"),
        live_row(r#"{"route":2,"provider":"workbuddy"}"#, "a2"),
        live_row(r#"{"route":3,"provider":"workbuddy"}"#, "a3"),
        live_row(r#"{"route":4,"provider":"different"}"#, "a1"),
        live_row(r#"{"route":5}"#, "a1"),
    ]);
    app.click_filter(Filter::Live);
    assert_eq!(
        app.rows
            .iter()
            .map(|r| r.provider.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("trae cn"),
            Some("Other Gateway"),
            Some("workbuddy"),
            Some("different"),
            None
        ]
    );
    app.config.hidden_providers.push(HiddenProvider {
        account_id: "a1".into(),
        provider: "workbuddy".into(),
    });
    app.rebuild();
    assert!(!app.rows.iter().any(|r| r.id == "live:1"));
    assert!(app.rows.iter().any(|r| r.id == "live:2"));
}

#[test]
fn live_provider_names_use_the_newest_named_record_not_input_order() {
    let rows = vec![
        row(
            r#"{"t":"2026-10-20T09:00:02Z","provider":"workbuddy"}"#,
            "a1",
        ),
        row(
            r#"{"t":"2026-10-20T09:00:01Z","provider":"workbuddy","providerName":"trae cn"}"#,
            "a1",
        ),
        row(
            r#"{"t":"2026-10-20T09:00:00Z","provider":"workbuddy","providerName":"Old Name"}"#,
            "a1",
        ),
        live_row(r#"{"route":9,"provider":"workbuddy"}"#, "a1"),
    ];
    for rows in [rows.clone(), rows.into_iter().rev().collect()] {
        let mut app = app_with(rows);
        app.click_filter(Filter::Live);
        assert_eq!(app.rows[0].provider.as_deref(), Some("trae cn"));
    }
}

#[test]
fn live_provider_names_apply_aliases_once_and_reapply_after_edits() {
    let mut app = app_with(vec![
        row(r#"{"provider":"workbuddy","providerName":"trae cn"}"#, "a1"),
        live_row(r#"{"route":9,"provider":"workbuddy"}"#, "a1"),
    ]);
    app.config.provider_aliases = vec![NameAlias {
        from: "trae".into(),
        to: "trae alias".into(),
    }];
    for _ in 0..2 {
        app.rebuild();
        assert!(
            app.rows
                .iter()
                .all(|r| r.provider.as_deref() == Some("trae alias cn"))
        );
    }
    app.config.provider_aliases[0].to = "T".into();
    app.rebuild();
    assert!(
        app.rows
            .iter()
            .all(|r| r.provider.as_deref() == Some("T cn"))
    );
}

#[test]
fn hidden_providers_are_scoped_to_their_connection() {
    let rows = vec![
        row(r#"{"provider":"openai","model":"a"}"#, "a1"),
        row(r#"{"provider":"openai","model":"b"}"#, "a2"),
    ];
    let mut app = app_with(rows);
    app.config.hidden_providers = vec![HiddenProvider {
        account_id: "a1".into(),
        provider: "openai".into(),
    }];
    app.rebuild();
    assert_eq!(app.rows.len(), 1);
    assert_eq!(app.rows[0].account_id, "a2");
}

#[test]
fn a_second_click_on_a_filter_chip_deselects_it() {
    let mut app = app_with(vec![
        row(r#"{"model":"ok"}"#, "a1"),
        row(r#"{"status":500,"model":"bad"}"#, "a1"),
    ]);
    assert!(app.click_filter(crate::model::Filter::Failed));
    assert_eq!(app.rows.len(), 1);
    assert_eq!(app.rows[0].status, Status::Failed);

    // 全部 clears the whole mask, not just the failed bit.
    assert!(app.click_filter(crate::model::Filter::Completed));
    assert!(app.click_filter(crate::model::Filter::All));
    assert_eq!(app.filter, crate::model::FILTER_NONE);
    assert_eq!(app.rows.len(), 2);
}

#[test]
fn the_query_box_narrows_by_model_application_and_ids() {
    let mut app = app_with(vec![
        row(r#"{"model":"gpt-5","agentName":"Codex"}"#, "a1"),
        row(r#"{"model":"claude","agentName":"Claude Code"}"#, "a1"),
    ]);
    app.query_draft = "codex".into();
    app.apply_query();
    assert_eq!(app.rows.len(), 1);
    assert_eq!(app.rows[0].model, "gpt-5");
    assert_eq!(app.query.as_deref(), Some("codex"));

    // An empty box clears the query and shows everything again.
    app.query_draft = "   ".into();
    app.apply_query();
    assert_eq!(app.rows.len(), 2);
    assert!(app.query.is_none());
}

#[test]
fn an_empty_result_of_a_submitted_query_says_so() {
    let mut app = app_with(vec![row(r#"{"model":"gpt-5"}"#, "a1")]);
    app.query_draft = "nothing".into();
    app.apply_query();
    assert!(app.rows.is_empty());
    let (message, error) = app.status.clone().expect("status");
    assert!(message.contains("筛选"));
    assert!(!error);
}

#[test]
fn a_broken_connection_is_reported_before_anything_else() {
    let mut app = app_with(vec![]);
    app.apply_updates(vec![Update::Accounts {
        open: vec![],
        broken: vec![crate::worker::AccountIssue {
            id: "a1".into(),
            name: "本机".into(),
            message: "连接被拒绝".into(),
        }],
    }]);
    let (message, error) = app.status.clone().expect("status");
    assert!(error);
    assert!(message.contains("连接被拒绝"));
    assert!(app.any_issue());
    assert_eq!(app.account_state("a1"), Some("连接失败"));
    assert!(app.open_accounts.is_empty());
}

#[test]
fn a_probe_answer_is_applied_only_to_the_connection_that_asked() {
    let mut app = App::new(Config::default());
    app.begin_probe("a1");
    app.apply_updates(vec![Update::Probe {
        id: "zz".into(),
        result: Ok(1),
    }]);
    assert_eq!(
        app.probe.as_ref().map(|p| &p.state),
        Some(&ProbeState::Running)
    );

    app.apply_updates(vec![Update::Probe {
        id: "a1".into(),
        result: Ok(42),
    }]);
    assert_eq!(
        app.probe.as_ref().map(|p| &p.state),
        Some(&ProbeState::Ok(42))
    );
}

#[test]
fn the_provider_filters_offer_seen_pairs_and_keep_hidden_ones_listed() {
    let mut app = app_with(vec![row(r#"{"provider":"openai"}"#, "a1")]);
    app.config.hidden_providers = vec![HiddenProvider {
        account_id: "a9".into(),
        provider: "anthropic".into(),
    }];
    let filters = app.provider_filters();
    assert!(filters.contains(&HiddenProvider {
        account_id: "a1".into(),
        provider: "openai".into(),
    }));
    // A hidden pair is absent from the rows by definition and must remain
    // listed so it can be shown again.
    assert!(filters.contains(&HiddenProvider {
        account_id: "a9".into(),
        provider: "anthropic".into(),
    }));
}

#[test]
fn a_saved_account_opens_the_form_prefilled() {
    let mut app = App::new(Config::default());
    app.config.accounts.push(Account {
        id: "a1".into(),
        name: "本机".into(),
        endpoint: "http://127.0.0.1:3425".into(),
    });
    app.stored.set_key("a1", "secret");
    app.open_login(None, LoginTarget::Account("a1".into()));
    let form = app.login.as_ref().expect("form");
    assert_eq!(form.name, "本机");
    assert_eq!(form.endpoint, "http://127.0.0.1:3425");
    assert_eq!(form.key, "secret");
    assert!(app.is_login());
}

#[test]
fn columns_hidden_by_the_user_stay_hidden_after_a_rebuild() {
    let mut app = app_with(vec![row(r#"{"model":"gpt-5"}"#, "a1")]);
    app.config.toggle_field(DisplayField::Speed);
    app.rebuild();
    assert!(!app.config.shows(DisplayField::Speed));
    assert_eq!(app.rows.len(), 1);
}
