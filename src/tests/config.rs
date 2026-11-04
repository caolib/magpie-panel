use super::*;

#[test]
fn default_config_points_at_the_local_gateway() {
    let config = Config::default();
    assert_eq!(config.endpoint, DEFAULT_ENDPOINT);
    assert_eq!(config.period, "30d");
    assert_eq!(config.row_limit, DEFAULT_ROWS as i64);
    assert!(config.accounts.is_empty());
    assert_eq!(config.hidden_fields, DEFAULT_HIDDEN_FIELDS);
}

#[test]
fn name_color_theme_defaults_to_classic_without_changing_existing_settings() {
    assert_eq!(NameColorTheme::default(), NameColorTheme::Classic);
    assert_eq!(Config::default().name_color_theme, NameColorTheme::Classic);
    let existing: Config = serde_json::from_str(
        r#"{"opacity":73,"fontFamily":"Arial","hiddenFields":[],"liveFirst":true}"#,
    )
    .unwrap();
    assert_eq!(existing.name_color_theme, NameColorTheme::Classic);
    assert_eq!(existing.opacity, 73);
    assert_eq!(existing.font_family, "Arial");
    assert!(existing.hidden_fields.is_empty());
    assert!(existing.live_first);
}

#[test]
fn name_color_themes_have_stable_labels_and_round_trip_as_camel_case() {
    assert_eq!(
        NameColorTheme::ALL,
        [
            NameColorTheme::Classic,
            NameColorTheme::Nord,
            NameColorTheme::OneDark,
            NameColorTheme::Dracula,
        ]
    );
    for (theme, (label, wire)) in NameColorTheme::ALL.into_iter().zip([
        ("经典", "classic"),
        ("Nord", "nord"),
        ("One Dark", "oneDark"),
        ("Dracula", "dracula"),
    ]) {
        assert_eq!(theme.label(), label);
        let config = Config {
            name_color_theme: theme,
            opacity: 73,
            ..Config::default()
        };
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["nameColorTheme"], wire);
        assert!(json.get("name_color_theme").is_none());
        let back: Config = serde_json::from_value(json).unwrap();
        assert_eq!(back.name_color_theme, theme);
        assert_eq!(back.opacity, 73);
    }
}

#[test]
fn an_unknown_name_color_theme_falls_back_without_discarding_the_config() {
    for unknown in ["futureTheme", "", "CLASSIC", "soft", "bright", "coolWarm"] {
        let config: Config = serde_json::from_value(serde_json::json!({
            "nameColorTheme": unknown,
            "opacity": 73,
            "fontFamily": "Arial",
            "hiddenFields": ["model"],
            "liveFirst": true,
        }))
        .unwrap();
        assert_eq!(config.name_color_theme, NameColorTheme::Classic);
        assert_eq!(config.opacity, 73);
        assert_eq!(config.font_family, "Arial");
        assert_eq!(config.hidden_fields, vec![DisplayField::Model]);
        assert!(config.live_first);
        assert_eq!(
            serde_json::to_value(config).unwrap()["nameColorTheme"],
            "classic"
        );
    }
}

#[test]
fn live_first_defaults_off_for_new_and_existing_configs() {
    assert!(!Config::default().live_first);
    let existing: Config = serde_json::from_str(r#"{"hiddenFields":[],"rowLimit":5}"#).unwrap();
    assert!(!existing.live_first);
    assert!(existing.hidden_fields.is_empty());
    assert_eq!(existing.row_limit, 5);
}

#[test]
fn live_first_survives_restart_and_show_all_fields() {
    for enabled in [false, true] {
        let mut config = Config {
            live_first: enabled,
            ..Config::default()
        };
        config.show_all_fields();
        assert_eq!(config.live_first, enabled);
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["liveFirst"], enabled);
        let back: Config = serde_json::from_value(json).unwrap();
        assert_eq!(back.live_first, enabled);
    }
}

#[test]
fn group_by_model_defaults_off_without_changing_existing_config() {
    assert!(!Config::default().group_by_model);
    for json in ["{}", r#"{"hiddenFields":[],"rowLimit":5,"liveFirst":true}"#] {
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(!config.group_by_model);
    }
    let existing: Config =
        serde_json::from_str(r#"{"hiddenFields":[],"rowLimit":5,"liveFirst":true,"opacity":73}"#)
            .unwrap();
    assert!(existing.hidden_fields.is_empty());
    assert_eq!(existing.row_limit, 5);
    assert!(existing.live_first);
    assert_eq!(existing.opacity, 73);
}

#[test]
fn group_by_model_round_trips_and_show_all_keeps_both_sort_switches() {
    for group_by_model in [false, true] {
        for live_first in [false, true] {
            let mut config = Config {
                group_by_model,
                live_first,
                ..Config::default()
            };
            config.show_all_fields();
            assert!(config.hidden_fields.is_empty());
            assert_eq!(config.group_by_model, group_by_model);
            assert_eq!(config.live_first, live_first);
            let json = serde_json::to_value(&config).unwrap();
            assert_eq!(json["groupByModel"], group_by_model);
            assert_eq!(json["liveFirst"], live_first);
            assert!(json.get("group_by_model").is_none());
            let back: Config = serde_json::from_value(json).unwrap();
            assert_eq!(back.group_by_model, group_by_model);
            assert_eq!(back.live_first, live_first);
        }
    }
}

#[test]
fn window_levels_are_mutually_exclusive_and_survive_restart() {
    for (top, bottom) in [(false, false), (true, false), (false, true), (true, true)] {
        for level in [
            WindowLevel::Normal,
            WindowLevel::Topmost,
            WindowLevel::Bottommost,
        ] {
            let mut config = Config {
                always_on_top: top,
                always_on_bottom: bottom,
                pin_position: true,
                ..Config::default()
            };
            config.set_window_level(level);
            // Selecting the checked item again must not toggle into another mode.
            config.set_window_level(level);
            assert_eq!(config.window_level(), level);
            assert_eq!(config.always_on_top, level == WindowLevel::Topmost);
            assert_eq!(config.always_on_bottom, level == WindowLevel::Bottommost);
            assert!(config.pin_position);

            let json = serde_json::to_string(&config).unwrap();
            let back: Config = serde_json::from_str(&json).unwrap();
            assert_eq!(back.window_level(), level);
        }
    }
}

#[test]
fn legacy_window_flags_keep_their_existing_meaning() {
    assert_eq!(Config::default().window_level(), WindowLevel::Topmost);
    for (json, level) in [
        (
            r#"{"alwaysOnTop":false,"alwaysOnBottom":false}"#,
            WindowLevel::Normal,
        ),
        (
            r#"{"alwaysOnTop":true,"alwaysOnBottom":false}"#,
            WindowLevel::Topmost,
        ),
        (
            r#"{"alwaysOnTop":false,"alwaysOnBottom":true}"#,
            WindowLevel::Bottommost,
        ),
        (
            r#"{"alwaysOnTop":true,"alwaysOnBottom":true}"#,
            WindowLevel::Bottommost,
        ),
    ] {
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.window_level(), level);
    }
}

#[test]
fn initial_columns_show_exactly_the_requested_fields() {
    let expected = vec![
        DisplayField::Model,
        DisplayField::Agent,
        DisplayField::Provider,
        DisplayField::Tokens,
        DisplayField::Cache,
        DisplayField::Speed,
        DisplayField::Retry,
        DisplayField::Latency,
        DisplayField::Error,
        DisplayField::CreatedAt,
    ];
    for config in [
        Config::default(),
        serde_json::from_str::<Config>("{}").unwrap(),
    ] {
        let shown: Vec<_> = DisplayField::ALL
            .into_iter()
            .filter(|field| config.shows(*field))
            .collect();
        assert_eq!(shown, expected);
    }
}

#[test]
fn existing_column_choices_are_not_replaced_by_new_defaults() {
    let custom: Config = serde_json::from_str(r#"{"hiddenFields":["model","error"]}"#).unwrap();
    assert_eq!(
        custom.hidden_fields,
        vec![DisplayField::Model, DisplayField::Error]
    );
    let all: Config = serde_json::from_str(r#"{"hiddenFields":[]}"#).unwrap();
    assert!(all.hidden_fields.is_empty());
}

#[test]
fn renamed_profile_override_prefers_the_new_variable() {
    assert_eq!(
        profile_override(Some("new"), Some("old")),
        Some(PathBuf::from("new"))
    );
    assert_eq!(
        profile_override(Some(""), Some("old")),
        Some(PathBuf::from("old"))
    );
    assert_eq!(
        profile_override(None, Some("old")),
        Some(PathBuf::from("old"))
    );
    assert_eq!(profile_override(None, Some("")), None);
}

#[test]
fn profile_rename_keeps_legacy_data_without_reading_or_moving_it() {
    let root = std::env::temp_dir().join(format!(
        "magpie-panel-profile-test-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let current = root.join("magpie-panel");
    let legacy = root.join("ah-panel");
    assert_eq!(default_profile_dir(&root), current);
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("config.json"), b"synthetic config").unwrap();
    std::fs::write(legacy.join("credentials.bin"), b"synthetic blob").unwrap();
    assert_eq!(default_profile_dir(&root), legacy);
    std::fs::create_dir_all(&current).unwrap();
    assert_eq!(default_profile_dir(&root), legacy);
    std::fs::write(current.join("config.json"), b"new synthetic config").unwrap();
    assert_eq!(default_profile_dir(&root), current);
    std::fs::remove_file(current.join("config.json")).unwrap();
    std::fs::write(current.join("credentials.bin"), b"new synthetic blob").unwrap();
    assert_eq!(default_profile_dir(&root), current);
    assert_eq!(
        std::fs::read(legacy.join("config.json")).unwrap(),
        b"synthetic config"
    );
    assert_eq!(
        std::fs::read(legacy.join("credentials.bin")).unwrap(),
        b"synthetic blob"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_config_round_trips_through_json() {
    let mut config = Config {
        period: "today".into(),
        ..Config::default()
    };
    config.row_limit = 5;
    config.accounts.push(Account {
        id: "a1".into(),
        name: "本机".into(),
        endpoint: "http://127.0.0.1:3425".into(),
    });
    config.hidden_fields = vec![DisplayField::Speed];
    config.field_order.reverse();
    config.model_aliases.push(NameAlias {
        from: "deepseek".into(),
        to: "ds".into(),
    });
    config.provider_aliases.push(NameAlias {
        from: "Anthropic".into(),
        to: "A社".into(),
    });

    let json = serde_json::to_string(&config).expect("serializable");
    let back: Config = serde_json::from_str(&json).expect("deserializable");
    assert_eq!(back.period, "today");
    assert_eq!(back.row_limit, 5);
    assert_eq!(back.accounts[0].name, "本机");
    assert_eq!(back.hidden_fields, vec![DisplayField::Speed]);
    assert_eq!(back.ordered_fields(), config.ordered_fields());
    assert!(json.contains("\"fieldOrder\""));
    assert_eq!(back.model_aliases[0].to, "ds");
    assert_eq!(back.provider_aliases[0].to, "A社");
    assert!(back.agent_aliases.is_empty());
}

#[test]
fn unknown_fields_are_ignored_and_missing_ones_take_defaults() {
    let config: Config =
        serde_json::from_str(r#"{"period":"7d","somethingNew":1}"#).expect("tolerant");
    assert_eq!(config.period, "7d");
    assert_eq!(config.font_size, 12.5);
    assert_eq!(config.opacity, 100);
    assert!(config.provider_aliases.is_empty());
    assert!(config.agent_aliases.is_empty());
}

#[test]
fn the_encrypted_blob_holds_one_key_per_connection() {
    let mut stored = Stored::default();
    stored.set_key("a1", "k1");
    stored.set_key("a2", "k2");
    assert_eq!(stored.key_for("a1").as_deref(), Some("k1"));
    assert!(!stored.is_empty());

    // An empty key clears the entry rather than storing an empty secret.
    stored.set_key("a1", "");
    assert_eq!(stored.key_for("a1"), None);

    stored.forget_account("a2");
    assert!(stored.is_empty());
}

#[test]
fn a_stored_key_is_optional_for_loopback_gateways() {
    let mut stored = Stored::default();
    stored.set_key("a1", "");
    assert!(stored.key_for("a1").is_none());
    assert!(stored.is_empty());
}

#[test]
fn accounts_are_replaced_in_place_and_removed_cleanly() {
    let mut config = Config::default();
    config.upsert_account(Account {
        id: "a1".into(),
        name: "one".into(),
        endpoint: "http://127.0.0.1:3425".into(),
    });
    config.upsert_account(Account {
        id: "a1".into(),
        name: "renamed".into(),
        endpoint: "http://127.0.0.1:3426".into(),
    });
    assert_eq!(config.accounts.len(), 1);
    assert_eq!(config.accounts[0].name, "renamed");

    config.hidden_providers.push(HiddenProvider {
        account_id: "a1".into(),
        provider: "openai".into(),
    });
    config.remove_account("a1");
    assert!(config.accounts.is_empty());
    // Removing a connection removes the filters that only applied to it.
    assert!(config.hidden_providers.is_empty());
}

#[test]
fn fresh_ids_do_not_collide_with_saved_ones() {
    let mut config = Config::default();
    assert_eq!(config.fresh_account_id(), "a1");
    config.upsert_account(Account {
        id: "a1".into(),
        name: String::new(),
        endpoint: DEFAULT_ENDPOINT.into(),
    });
    assert_eq!(config.fresh_account_id(), "a2");
}

#[test]
fn hiding_a_provider_toggles() {
    let mut config = Config::default();
    assert!(!config.hides_provider("a1", "openai"));
    config.toggle_hidden_provider("a1", "openai");
    assert!(config.hides_provider("a1", "openai"));
    assert!(!config.hides_provider("a2", "openai"));
    config.toggle_hidden_provider("a1", "openai");
    assert!(!config.hides_provider("a1", "openai"));
}

#[test]
fn model_aliases_update_case_insensitively() {
    let mut config = Config::default();
    config.upsert_alias(AliasKind::Model, "DeepSeek", "ds");
    config.upsert_alias(AliasKind::Model, "deepseek", "d");
    assert_eq!(config.model_aliases.len(), 1);
    assert_eq!(config.model_aliases[0].to, "d");

    config.remove_alias(AliasKind::Model, 0);
    assert!(config.model_aliases.is_empty());
}

#[test]
fn each_alias_kind_has_its_own_list() {
    let mut config = Config::default();
    config.upsert_alias(AliasKind::Provider, "Anthropic", "A社");
    config.upsert_alias(AliasKind::Agent, "Claude Code", "CC");
    assert_eq!(config.aliases(AliasKind::Provider).len(), 1);
    assert!(config.aliases(AliasKind::Model).is_empty());

    // The same match text in another kind must not touch the first list.
    config.upsert_alias(AliasKind::Agent, "anthropic", "x");
    assert_eq!(config.provider_aliases[0].to, "A社");
    assert_eq!(config.agent_aliases.len(), 2);

    config.remove_alias(AliasKind::Provider, 0);
    assert!(config.provider_aliases.is_empty());
    assert_eq!(config.agent_aliases.len(), 2);
}

#[test]
fn columns_toggle_and_show_all_again() {
    let mut config = Config::default();
    config.hidden_fields.clear();
    assert!(config.shows(DisplayField::Model));
    config.toggle_field(DisplayField::Model);
    assert!(!config.shows(DisplayField::Model));
    config.toggle_field(DisplayField::Model);
    assert!(config.shows(DisplayField::Model));

    config.toggle_field(DisplayField::Cost);
    config.show_all_fields();
    assert!(config.hidden_fields.is_empty());
}

#[test]
fn missing_or_empty_column_order_preserves_the_current_default() {
    for config in [
        Config::default(),
        serde_json::from_str::<Config>(r#"{"hiddenFields":["model"]}"#).unwrap(),
        serde_json::from_str::<Config>(r#"{"fieldOrder":[]}"#).unwrap(),
    ] {
        assert_eq!(config.ordered_fields(), DisplayField::ALL);
    }
}

#[test]
fn column_order_discards_duplicates_and_legacy_fields_then_appends_missing_ones() {
    let config = Config {
        field_order: vec![
            DisplayField::CreatedAt,
            DisplayField::ProviderKey,
            DisplayField::Model,
            DisplayField::CreatedAt,
        ],
        ..Config::default()
    };
    let expected: Vec<_> = [DisplayField::CreatedAt, DisplayField::Model]
        .into_iter()
        .chain(
            DisplayField::ALL
                .into_iter()
                .filter(|field| !matches!(field, DisplayField::CreatedAt | DisplayField::Model)),
        )
        .collect();
    assert_eq!(config.ordered_fields(), expected);
}

#[test]
fn future_column_names_do_not_discard_other_settings() {
    let config: Config = serde_json::from_str(
        r#"{"fieldOrder":["createdAt","futureColumn","model","providerKey","createdAt"],"opacity":75,"hiddenFields":["model"]}"#,
    )
    .unwrap();
    let ordered = config.ordered_fields();
    assert_eq!(
        &ordered[..2],
        &[DisplayField::CreatedAt, DisplayField::Model]
    );
    assert_eq!(ordered.len(), DisplayField::ALL.len());
    assert!(!ordered.contains(&DisplayField::ProviderKey));
    assert_eq!(config.opacity, 75);
    assert!(!config.shows(DisplayField::Model));
}

#[test]
fn columns_move_one_step_without_changing_visibility() {
    let mut config = Config::default();
    let hidden = config.hidden_fields.clone();
    assert!(config.move_field(DisplayField::UpstreamModel, -1));
    assert_eq!(
        &config.ordered_fields()[..3],
        &[
            DisplayField::UpstreamModel,
            DisplayField::Model,
            DisplayField::Protocol
        ]
    );
    assert_eq!(config.hidden_fields, hidden);
    assert!(config.move_field(DisplayField::UpstreamModel, 1));
    assert_eq!(config.ordered_fields(), DisplayField::ALL);
    assert_eq!(config.hidden_fields, hidden);
}

#[test]
fn moving_past_column_boundaries_or_by_invalid_offsets_is_a_noop() {
    let mut config = Config::default();
    for (field, offset) in [
        (DisplayField::Model, -1),
        (DisplayField::CreatedAt, 1),
        (DisplayField::ProviderKey, -1),
        (DisplayField::Model, 0),
        (DisplayField::Model, 2),
        (DisplayField::Model, isize::MAX),
        (DisplayField::Model, isize::MIN),
    ] {
        assert!(!config.move_field(field, offset));
        assert_eq!(config.field_order, DisplayField::ALL);
    }
}

#[test]
fn column_order_reset_and_show_all_are_independent() {
    let mut config = Config::default();
    config.field_order.reverse();
    let hidden = config.hidden_fields.clone();
    let order = config.ordered_fields();
    config.toggle_field(DisplayField::Model);
    config.toggle_field(DisplayField::Model);
    assert_eq!(config.ordered_fields(), order);
    config.show_all_fields();
    assert!(config.hidden_fields.is_empty());
    assert_eq!(config.ordered_fields(), order);
    config.hidden_fields = hidden.clone();
    config.reset_field_order();
    assert_eq!(config.ordered_fields(), DisplayField::ALL);
    assert_eq!(config.hidden_fields, hidden);
}

#[test]
fn display_field_labels_use_the_requested_names() {
    for (field, expected) in [
        (DisplayField::Model, "模型(思考强度)"),
        (DisplayField::UpstreamModel, "上游回复的模型"),
        (DisplayField::Protocol, "接口类型"),
        (DisplayField::Agent, "agent"),
        (DisplayField::Tokens, "词元"),
        (DisplayField::Cache, "缓存命中率"),
        (DisplayField::Cost, "估算费用"),
    ] {
        assert_eq!(field.label(), expected);
    }
}

#[test]
fn every_display_field_has_a_label_and_a_stable_order() {
    assert_eq!(DisplayField::ALL.len(), 17);
    for field in DisplayField::ALL {
        assert!(!field.label().is_empty());
    }
    assert_eq!(DisplayField::ALL[0], DisplayField::Model);
    // Round trips through the name the settings file uses.
    let json = serde_json::to_string(&DisplayField::ProviderKey).expect("serializable");
    assert_eq!(json, "\"providerKey\"");
}

#[test]
fn endpoint_host_names_an_unnamed_connection() {
    assert_eq!(endpoint_host("http://127.0.0.1:3425"), "127.0.0.1:3425");
    assert_eq!(endpoint_host("http://localhost:3425/"), "localhost:3425");
    assert_eq!(
        endpoint_host("https://magpie.example/api"),
        "magpie.example"
    );
    assert_eq!(endpoint_host(""), "");
}

#[test]
fn the_window_is_kept_on_screen_and_above_the_minimum() {
    let screen = (0, 0, 1280, 720);
    let placed = clamp_to_virtual_screen(
        WindowState {
            x: -500,
            y: 900,
            width: 100,
            height: 100,
        },
        screen,
        1.0,
    );
    let (min_w, min_h) = min_window_size(1.0);
    // A size above the floor is kept; only the origin is pulled back.
    assert_eq!(placed.width, 100.max(min_w));
    assert_eq!(placed.height, 100.max(min_h));
    assert_eq!(placed.x, 0);
    assert_eq!(placed.y, 720 - placed.height);

    let too_big = clamp_to_virtual_screen(
        WindowState {
            x: 10,
            y: 10,
            width: 4000,
            height: 4000,
        },
        screen,
        1.0,
    );
    assert_eq!(too_big.width, 1280);
    assert_eq!(too_big.height, 720);
}

#[test]
fn an_environment_override_is_trimmed_and_optional() {
    // A variable that is not set yields nothing rather than an empty key.
    assert!(key_from_env("MAGPIE_PANEL_TEST_UNSET_KEY_VAR").is_none());
    assert!(key_from_env("").is_none());
}

#[test]
fn logical_window_sizes_scale_with_the_monitor() {
    let (min_w, min_h) = min_window_size(2.0);
    assert_eq!(min_w, 640);
    assert_eq!(min_h, 160);
}
