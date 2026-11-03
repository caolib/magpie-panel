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
