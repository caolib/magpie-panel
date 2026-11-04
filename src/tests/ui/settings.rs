use super::*;
use crate::config::{Account, AliasKind, Config, NameAlias};

fn app() -> App {
    let config = Config {
        accounts: vec![Account {
            id: "a1".into(),
            name: "本机".into(),
            endpoint: "http://127.0.0.1:3425".into(),
        }],
        ..Config::default()
    };
    App::new(config)
}

#[test]
fn every_tab_has_a_button_and_the_current_one_is_selected() {
    let app = app();
    let mut state = State::default();
    for tab in [
        Tab::Appearance,
        Tab::Fields,
        Tab::Accounts,
        Tab::Providers,
        Tab::Mappings,
    ] {
        state.tab = tab;
        let l = Layout::new(&app, &state, WIDTH, HEIGHT);
        assert_eq!(
            l.controls
                .iter()
                .filter(|c| matches!(c.action, Action::Tab(_)))
                .count(),
            5
        );
        assert!(
            l.controls.iter().any(|c| c.action == Action::Tab(tab)),
            "{tab:?} tab missing"
        );
        let button = l
            .controls
            .iter()
            .find(|c| c.action == Action::Tab(tab))
            .expect("tab button");
        assert_eq!(button.selected, tab == state.tab);
    }
}

#[test]
fn the_footer_offers_a_confirm_step_only_while_one_is_pending() {
    let app = app();
    let mut state = State::default();
    let l = Layout::new(&app, &state, WIDTH, HEIGHT);
    assert!(l.controls.iter().any(|c| c.action == Action::Close));
    assert!(!l.controls.iter().any(|c| c.action == Action::Confirm));

    state.pending = Some(Action::ClearKeys);
    let l = Layout::new(&app, &state, WIDTH, HEIGHT);
    assert!(l.controls.iter().any(|c| c.action == Action::Confirm));
    assert!(l.controls.iter().any(|c| c.action == Action::Cancel));
    // Everything else is disabled while the question is up.
    let fields = l
        .controls
        .iter()
        .find(|c| c.action == Action::Tab(Tab::Fields))
        .expect("tab still drawn");
    assert!(
        l.hit(&state, fields.rect.x + 2.0, fields.rect.y + 2.0)
            .is_none()
    );
}

#[test]
fn clicking_a_pending_confirmation_is_the_only_way_through() {
    let app = app();
    let state = State {
        pending: Some(Action::RemoveAccount("a1".into())),
        ..State::default()
    };
    let l = Layout::new(&app, &state, WIDTH, HEIGHT);
    let cancel = l
        .controls
        .iter()
        .find(|c| c.action == Action::Cancel)
        .expect("cancel");
    assert_eq!(
        l.hit(&state, cancel.rect.x + 2.0, cancel.rect.y + 2.0),
        Some(Action::Cancel)
    );
}

#[test]
fn the_fields_tab_lists_all_columns_in_normalized_order_including_hidden_ones() {
    let mut app = app();
    app.config.field_order = vec![
        DisplayField::CreatedAt,
        DisplayField::UpstreamModel,
        DisplayField::CreatedAt,
    ];
    app.config.hidden_fields = vec![DisplayField::CreatedAt, DisplayField::Provider];
    let state = State {
        tab: Tab::Fields,
        ..State::default()
    };
    for (width, height) in [(WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT)] {
        let l = Layout::new(&app, &state, width, height);
        let fields: Vec<_> = l
            .controls
            .iter()
            .filter_map(|c| match c.action {
                Action::Field(field) => Some((field, c)),
                _ => None,
            })
            .collect();
        assert_eq!(fields.len(), DisplayField::ALL.len());
        assert_eq!(
            fields.iter().map(|(field, _)| *field).collect::<Vec<_>>(),
            app.config.ordered_fields()
        );
        assert_eq!(fields[0].0, DisplayField::CreatedAt);
        assert_eq!(fields[1].0, DisplayField::UpstreamModel);
        for (index, (field, checkbox)) in fields.iter().enumerate() {
            assert!(checkbox.body && checkbox.check);
            assert_eq!(checkbox.selected, app.config.shows(*field));
            assert_eq!(checkbox.rect.x, 24.0);
            assert_eq!(checkbox.rect.y, 144.0 + index as f32 * 36.0);
            for offset in [-1, 1] {
                let button = l
                    .controls
                    .iter()
                    .find(|c| c.action == Action::MoveField(*field, offset))
                    .expect("move button for every column");
                assert!(button.body && !button.check && !button.danger);
                assert_eq!(button.label, if offset == -1 { "上移" } else { "下移" });
                assert_eq!(button.rect.y, checkbox.rect.y);
                assert!(button.rect.x >= checkbox.rect.x + checkbox.rect.w);
                assert!(button.rect.x + button.rect.w <= width - 24.0);
            }
        }
    }
}

#[test]
fn the_fields_tab_offers_a_separate_live_first_checkbox() {
    let mut app = app();
    let mut state = State {
        tab: Tab::Fields,
        ..State::default()
    };
    for enabled in [false, true] {
        app.config.live_first = enabled;
        for (width, height) in [(WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT)] {
            let l = Layout::new(&app, &state, width, height);
            let toggle = l
                .controls
                .iter()
                .find(|c| c.action == Action::LiveFirst)
                .expect("live-first checkbox");
            assert!(toggle.body && toggle.check);
            assert_eq!(toggle.selected, enabled);
            assert!(toggle.label.ends_with("进行中请求置顶"));
            let show_all = l
                .controls
                .iter()
                .find(|c| c.action == Action::ShowAllFields)
                .expect("show all columns");
            assert_eq!(toggle.rect.y, show_all.rect.y);
            assert!(toggle.rect.x >= show_all.rect.x + show_all.rect.w);
            assert!(toggle.rect.x + toggle.rect.w <= width - 24.0);
            assert_eq!(
                l.hit(&state, toggle.rect.x + 2.0, TOP + toggle.rect.y + 2.0),
                Some(Action::LiveFirst)
            );
            state.focus = Some(Action::ShowAllFields);
            l.focus_next(&mut state, false);
            assert_eq!(state.focus, Some(Action::LiveFirst));
            assert!(l.max_scroll() > 0.0);
        }
    }
}

#[test]
fn the_fields_tab_offers_a_separate_model_grouping_checkbox() {
    let mut app = app();
    for enabled in [false, true] {
        app.config.group_by_model = enabled;
        for (width, height) in [(WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT)] {
            let mut state = State {
                tab: Tab::Fields,
                ..State::default()
            };
            let l = Layout::new(&app, &state, width, height);
            let toggle = l
                .controls
                .iter()
                .find(|c| c.action == Action::GroupByModel)
                .expect("model grouping checkbox");
            assert!(toggle.body && toggle.check && !toggle.danger);
            assert_eq!(toggle.selected, enabled);
            assert_eq!(
                toggle.label.trim_start_matches(['☑', '☐', ' ']),
                "按模型分组"
            );
            let show_all = l
                .controls
                .iter()
                .find(|c| c.action == Action::ShowAllFields)
                .unwrap();
            let live_first = l
                .controls
                .iter()
                .find(|c| c.action == Action::LiveFirst)
                .unwrap();
            assert!(toggle.rect.y >= show_all.rect.y + show_all.rect.h);
            assert!(toggle.rect.y >= live_first.rect.y + live_first.rect.h);
            assert!(toggle.rect.x >= 24.0 && toggle.rect.x + toggle.rect.w <= width - 24.0);
            let rect = l.screen_rect(toggle, state.scroll);
            assert_eq!(
                l.hit(&state, rect.x + 2.0, rect.y + 2.0),
                Some(Action::GroupByModel)
            );
            state.focus = Some(Action::LiveFirst);
            l.focus_next(&mut state, false);
            assert_eq!(state.focus, Some(Action::GroupByModel));
            l.focus_next(&mut state, false);
            assert_eq!(state.focus, Some(Action::ResetFieldOrder));
            l.focus_next(&mut state, false);
            assert_eq!(state.focus, Some(Action::Field(DisplayField::Model)));
            l.focus_next(&mut state, true);
            assert_eq!(state.focus, Some(Action::ResetFieldOrder));
            l.focus_next(&mut state, true);
            assert_eq!(state.focus, Some(Action::GroupByModel));
            assert!(l.max_scroll() > 0.0);
            let body: Vec<_> = l.controls.iter().filter(|c| c.body).collect();
            for (i, control) in body.iter().enumerate() {
                let r = control.rect;
                assert!(r.x >= 24.0 && r.x + r.w <= width - 24.0);
                assert!(r.y + r.h <= l.content_height);
                for other in &body[i + 1..] {
                    let s = other.rect;
                    assert!(
                        r.x + r.w <= s.x
                            || s.x + s.w <= r.x
                            || r.y + r.h <= s.y
                            || s.y + s.h <= r.y,
                        "{:?} overlaps {:?}",
                        control.action,
                        other.action
                    );
                }
            }
        }
    }
    for tab in [
        Tab::Appearance,
        Tab::Accounts,
        Tab::Providers,
        Tab::Mappings,
    ] {
        let state = State {
            tab,
            ..State::default()
        };
        let l = Layout::new(&app, &state, WIDTH, HEIGHT);
        assert!(l.controls.iter().all(|c| c.action != Action::GroupByModel));
    }
}

#[test]
fn reset_order_is_a_separate_button_and_showing_all_keeps_the_order() {
    let mut app = app();
    app.config.field_order = DisplayField::ALL.into_iter().rev().collect();
    let order = app.config.ordered_fields();
    let hidden = app.config.hidden_fields.clone();
    let mut state = State {
        tab: Tab::Fields,
        ..State::default()
    };
    let l = Layout::new(&app, &state, MIN_WIDTH, MIN_HEIGHT);
    let reset = l
        .controls
        .iter()
        .find(|c| c.action == Action::ResetFieldOrder)
        .expect("reset order button");
    assert_eq!(reset.label, "恢复默认顺序");
    assert!(reset.body && !reset.check && !reset.danger && !reset.selected);
    let show_all = l
        .controls
        .iter()
        .find(|c| c.action == Action::ShowAllFields)
        .unwrap();
    assert_ne!(reset.action, show_all.action);
    l.focus_action(&mut state, Action::ResetFieldOrder);
    let rect = l.screen_rect(reset, state.scroll);
    assert_eq!(
        l.hit(&state, rect.x + 2.0, rect.y + 2.0),
        Some(Action::ResetFieldOrder)
    );
    assert_eq!(app.config.hidden_fields, hidden);
    assert_eq!(app.config.ordered_fields(), order);

    app.config.hidden_fields.clear();
    let l = Layout::new(&app, &state, MIN_WIDTH, MIN_HEIGHT);
    let fields: Vec<_> = l
        .controls
        .iter()
        .filter_map(|c| match c.action {
            Action::Field(field) => {
                assert!(c.selected);
                Some(field)
            }
            _ => None,
        })
        .collect();
    assert_eq!(fields, order);
    assert_eq!(app.config.ordered_fields(), order);
}

#[test]
fn only_the_first_up_and_last_down_buttons_are_disabled() {
    let mut app = app();
    for reverse in [false, true] {
        app.config.field_order = if reverse {
            DisplayField::ALL.into_iter().rev().collect()
        } else {
            Vec::new()
        };
        let fields = app.config.ordered_fields();
        for (width, height) in [(WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT)] {
            let mut state = State {
                tab: Tab::Fields,
                ..State::default()
            };
            let l = Layout::new(&app, &state, width, height);
            for (index, field) in fields.iter().copied().enumerate() {
                for offset in [-1, 1] {
                    let action = Action::MoveField(field, offset);
                    let button = l.controls.iter().find(|c| c.action == action).unwrap();
                    let enabled =
                        (offset != -1 || index > 0) && (offset != 1 || index + 1 < fields.len());
                    assert_eq!(l.enabled(&state, button), enabled);
                    l.focus_action(&mut state, Action::Field(field));
                    let scroll = state.scroll;
                    let rect = l.screen_rect(button, state.scroll);
                    assert_eq!(
                        l.hit(&state, rect.x + 2.0, rect.y + 2.0),
                        enabled.then_some(action.clone())
                    );
                    l.focus_action(&mut state, action.clone());
                    assert_eq!(
                        state.focus,
                        Some(if enabled {
                            action
                        } else {
                            Action::Field(field)
                        })
                    );
                    assert_eq!(state.scroll, scroll);
                }
            }
            assert_eq!(
                l.controls.iter().filter(|c| !l.enabled(&state, c)).count(),
                2
            );

            state.pending = Some(Action::ClearKeys);
            let l = Layout::new(&app, &state, width, height);
            assert!(l.controls.iter().all(|c| {
                l.enabled(&state, c) == matches!(c.action, Action::Confirm | Action::Cancel)
            }));
            l.focus_action(&mut state, Action::Cancel);
            l.focus_next(&mut state, false);
            assert_eq!(state.focus, Some(Action::Confirm));
            l.focus_next(&mut state, false);
            assert_eq!(state.focus, Some(Action::Cancel));
        }
    }
}

#[test]
fn fields_keyboard_navigation_follows_order_skips_endpoints_and_scrolls_rows_into_view() {
    let mut app = app();
    app.config.field_order = DisplayField::ALL.into_iter().rev().collect();
    let fields = app.config.ordered_fields();
    let mut actions = vec![
        Action::ShowAllFields,
        Action::LiveFirst,
        Action::GroupByModel,
        Action::ResetFieldOrder,
    ];
    for (index, field) in fields.iter().copied().enumerate() {
        actions.push(Action::Field(field));
        if index > 0 {
            actions.push(Action::MoveField(field, -1));
        }
        if index + 1 < fields.len() {
            actions.push(Action::MoveField(field, 1));
        }
    }
    actions.push(Action::Close);
    for (width, height) in [(WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT)] {
        let mut state = State {
            tab: Tab::Fields,
            focus: Some(Action::Tab(Tab::Mappings)),
            ..State::default()
        };
        let l = Layout::new(&app, &state, width, height);
        for action in &actions {
            l.focus_next(&mut state, false);
            assert_eq!(state.focus.as_ref(), Some(action));
            let control = l.controls.iter().find(|c| &c.action == action).unwrap();
            let rect = l.screen_rect(control, state.scroll);
            assert!(l.enabled(&state, control));
            if control.body {
                assert!(rect.y >= TOP && rect.y + rect.h <= height - FOOTER);
            }
            assert_eq!(
                l.hit(&state, rect.x + 2.0, rect.y + 2.0).as_ref(),
                Some(action)
            );
        }
        assert!(state.scroll > 0.0);
        for action in actions.iter().rev().skip(1) {
            l.focus_next(&mut state, true);
            assert_eq!(state.focus.as_ref(), Some(action));
            let control = l.controls.iter().find(|c| &c.action == action).unwrap();
            let rect = l.screen_rect(control, state.scroll);
            assert!(rect.y >= TOP && rect.y + rect.h <= height - FOOTER);
        }
        l.focus_next(&mut state, true);
        assert_eq!(state.focus, Some(Action::Tab(Tab::Mappings)));
    }
}

#[test]
fn moving_a_field_keeps_button_focus_and_falls_back_to_its_checkbox_at_endpoints() {
    for (field, offset, endpoint) in [
        (DisplayField::Model, 1, false),
        (DisplayField::CreatedAt, -1, false),
        (DisplayField::UpstreamModel, -1, true),
        (DisplayField::Account, 1, true),
    ] {
        for (width, height) in [(WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT)] {
            let mut app = app();
            let action = Action::MoveField(field, offset);
            let mut state = State {
                tab: Tab::Fields,
                focus: Some(action.clone()),
                hover: Some(action.clone()),
                pressed: Some(action.clone()),
                ..State::default()
            };
            assert!(app.config.move_field(field, offset));
            let l = Layout::new(&app, &state, width, height);
            l.focus_moved_field(&mut state, field, offset);
            let expected = if endpoint {
                Action::Field(field)
            } else {
                action
            };
            assert_eq!(state.focus, Some(expected.clone()));
            assert!(state.hover.is_none() && state.pressed.is_none());
            let control = l.controls.iter().find(|c| c.action == expected).unwrap();
            assert!(l.enabled(&state, control));
            let rect = l.screen_rect(control, state.scroll);
            assert!(rect.y >= TOP && rect.y + rect.h <= height - FOOTER);
            assert_eq!(l.hit(&state, rect.x + 2.0, rect.y + 2.0), Some(expected));
            if matches!(field, DisplayField::CreatedAt | DisplayField::Account) {
                assert!(state.scroll > 0.0);
            }
        }
    }
}

#[test]
fn fields_scrolling_clips_hits_and_keeps_disabled_moves_inert() {
    let app = app();
    for (width, height) in [(WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT)] {
        let mut state = State {
            tab: Tab::Fields,
            ..State::default()
        };
        let l = Layout::new(&app, &state, width, height);
        let close = l
            .controls
            .iter()
            .find(|c| c.action == Action::Close)
            .unwrap();
        for step in 0..=20 {
            state.scroll = l.max_scroll() * step as f32 / 20.0;
            assert_eq!(l.screen_rect(close, state.scroll).y, close.rect.y);
            for control in l.controls.iter().filter(|c| c.body) {
                let rect = l.screen_rect(control, state.scroll);
                let (x, y) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
                if y >= TOP && y < height - FOOTER && l.enabled(&state, control) {
                    assert_eq!(l.hit(&state, x, y), Some(control.action.clone()));
                } else {
                    assert_ne!(l.hit(&state, x, y), Some(control.action.clone()));
                }
            }
        }
    }
}

#[test]
fn the_appearance_tab_offers_opacity_themes_and_fonts_without_overlapping_inputs() {
    let mut app = app();
    app.config.opacity = 73;
    for (width, height) in [(WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT)] {
        let mut state = State::default();
        assert_eq!(state.tab, Tab::Appearance);
        let l = Layout::new(&app, &state, width, height);
        let tab = l
            .controls
            .iter()
            .find(|c| c.action == Action::Tab(Tab::Appearance))
            .expect("appearance tab");
        assert_eq!(tab.label, "外观");
        let body: Vec<_> = l.controls.iter().filter(|c| c.body).collect();
        assert_eq!(body.len(), 8);
        assert_eq!(body[0].action, Action::OpacityInput);
        assert_eq!(body[1].action, Action::SaveOpacity);
        assert_eq!(body[1].label, "保存");
        assert_eq!(body[0].rect.y, 32.0);
        assert_eq!(body[1].rect.y, body[0].rect.y);
        for (control, theme) in body[2..6].iter().zip(NameColorTheme::ALL) {
            assert_eq!(control.action, Action::NameColorTheme(theme));
        }
        assert_eq!(body[6].action, Action::FontSearch);
        assert_eq!(body[7].action, Action::FontFamily(String::new()));
        assert_eq!(l.labels.len(), 8);
        assert_eq!(l.labels[0].text, "面板透明度 · 当前 73%");
        assert_eq!(l.labels[0].rect.y, 0.0);
        assert_eq!(
            l.labels[1].text,
            format!("输入 {OPACITY_MIN}–{OPACITY_MAX} 之间的数值(可含小数),点保存生效")
        );
        assert_eq!(l.labels[2].text, "名称配色");
        assert!(l.labels[2].heading);
        assert_eq!(l.labels[3].text, "仅影响模型名与供应商");
        assert!(!l.labels[3].heading);
        assert_eq!(l.labels[4].text, "当前字体 · 自动选择默认字体");
        for (i, control) in body.iter().enumerate() {
            let r = control.rect;
            assert!(r.x >= 24.0 && r.x + r.w <= width - 24.0);
            assert!(r.y + r.h <= l.content_height);
            for other in &body[i + 1..] {
                let s = other.rect;
                assert!(
                    r.x + r.w <= s.x || s.x + s.w <= r.x || r.y + r.h <= s.y || s.y + s.h <= r.y,
                    "{:?} overlaps {:?}",
                    control.action,
                    other.action
                );
            }
        }
        assert!(l.edit_box_rect(&Action::OpacityInput, 0.0).is_some());
        assert_eq!(
            l.edit_box_rect(&Action::FontSearch, 0.0).is_some(),
            height == HEIGHT
        );
        l.focus_action(&mut state, Action::FontSearch);
        let search = l
            .edit_box_rect(&Action::FontSearch, state.scroll)
            .expect("focused font search is visible, including in a short window");
        assert!(search.y >= TOP && search.y + search.h <= height - FOOTER);
        assert_eq!(l.max_scroll() > 0.0, height == MIN_HEIGHT);
    }
}

#[test]
fn name_color_theme_buttons_form_one_row_with_exactly_one_selection() {
    let mut app = app();
    let state = State::default();
    for theme in NameColorTheme::ALL {
        app.config.name_color_theme = theme;
        for (width, height) in [(WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT)] {
            let l = Layout::new(&app, &state, width, height);
            let buttons: Vec<_> = l
                .controls
                .iter()
                .filter(|c| matches!(c.action, Action::NameColorTheme(_)))
                .collect();
            assert_eq!(buttons.len(), 4);
            assert_eq!(buttons.iter().filter(|c| c.selected).count(), 1);
            for (index, selected) in NameColorTheme::ALL.into_iter().enumerate() {
                let c = buttons[index];
                assert_eq!(c.action, Action::NameColorTheme(selected));
                assert_eq!(c.label, selected.label());
                assert_eq!(c.selected, selected == theme);
                // Selected buttons use the existing filled/outlined style, not checkboxes.
                assert!(c.body && !c.check && !c.danger);
                assert_eq!(c.rect.y, buttons[0].rect.y);
                if index > 0 {
                    let previous = buttons[index - 1].rect;
                    assert!(c.rect.x > previous.x + previous.w);
                }
                let r = l.screen_rect(c, 0.0);
                assert!(r.y >= TOP && r.y + r.h <= height - FOOTER);
                assert_eq!(
                    l.hit(&state, r.x + r.w / 2.0, r.y + r.h / 2.0),
                    Some(Action::NameColorTheme(selected))
                );
            }
        }
    }
    for tab in [Tab::Fields, Tab::Accounts, Tab::Providers, Tab::Mappings] {
        let state = State {
            tab,
            ..State::default()
        };
        let l = Layout::new(&app, &state, WIDTH, HEIGHT);
        assert!(
            l.controls
                .iter()
                .all(|c| !matches!(c.action, Action::NameColorTheme(_)))
        );
    }
}

#[test]
fn appearance_scrolling_keeps_theme_hits_and_native_inputs_inside_the_body() {
    let app = app();
    let mut state = State {
        font_names: (0..30)
            .map(|i| crate::font_catalog::InstalledFont::named(&format!("Font {i}")))
            .collect(),
        ..State::default()
    };
    state.set_font_query(String::new());
    for (width, height) in [(WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT)] {
        let l = Layout::new(&app, &state, width, height);
        let close = l
            .controls
            .iter()
            .find(|c| c.action == Action::Close)
            .unwrap();
        for step in 0..=20 {
            state.scroll = l.max_scroll() * step as f32 / 20.0;
            assert_eq!(l.screen_rect(close, state.scroll).y, close.rect.y);
            for control in l.controls.iter().filter(|c| c.body) {
                let r = l.screen_rect(control, state.scroll);
                let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                if y >= TOP && y < height - FOOTER {
                    assert_eq!(l.hit(&state, x, y), Some(control.action.clone()));
                } else {
                    assert_ne!(l.hit(&state, x, y), Some(control.action.clone()));
                }
                if matches!(control.action, Action::OpacityInput | Action::FontSearch) {
                    assert_eq!(
                        l.edit_box_rect(&control.action, state.scroll).is_some(),
                        r.y >= TOP && r.y + r.h <= height - FOOTER
                    );
                }
            }
        }
        l.focus_action(&mut state, Action::NameColorTheme(NameColorTheme::Classic));
        assert!(!state.font_focused());
        let selected = l
            .controls
            .iter()
            .find(|c| Some(&c.action) == state.focus.as_ref())
            .unwrap();
        let rect = l.screen_rect(selected, state.scroll);
        assert!(rect.y >= TOP && rect.y + rect.h <= height - FOOTER);
        assert_eq!(l.hit(&state, rect.x + 2.0, rect.y + 2.0), state.focus);
        l.focus_action(&mut state, Action::OpacityInput);
        assert!(
            l.edit_box_rect(&Action::OpacityInput, state.scroll)
                .is_some()
        );
    }
}

#[test]
fn no_settings_tab_or_footer_repeats_context_menu_operations() {
    let mut app = app();
    app.config.row_limit = 20;
    app.config.font_size = 24.0;
    app.config.pin_position = true;
    app.config.always_on_top = true;
    app.config.always_on_bottom = true;
    let removed_labels = [
        "显示与窗口",
        "显示数量",
        "显示行数",
        "字号",
        "固定窗口位置",
        "普通窗口",
        "置顶",
        "置底",
        "刷新",
        "退出",
    ];
    for tab in [
        Tab::Appearance,
        Tab::Fields,
        Tab::Accounts,
        Tab::Providers,
        Tab::Mappings,
    ] {
        for pending in [
            None,
            Some(Action::ClearKeys),
            Some(Action::RemoveAccount("a1".into())),
        ] {
            let mut state = State {
                tab,
                pending,
                font_names: vec![crate::font_catalog::InstalledFont::named("Arial")],
                ..State::default()
            };
            state.set_font_query(String::new());
            for (width, height) in [(WIDTH, HEIGHT), (MIN_WIDTH, MIN_HEIGHT)] {
                let l = Layout::new(&app, &state, width, height);
                for removed in removed_labels {
                    assert!(
                        l.controls.iter().all(|c| {
                            c.label.trim_start_matches(['☑', '☐', ' ']) != removed
                        }),
                        "{tab:?} still has a {removed} control"
                    );
                    assert!(
                        l.labels
                            .iter()
                            .filter(|label| label.heading)
                            .all(|label| !label.text.contains(removed)),
                        "{tab:?} still has a {removed} section"
                    );
                }
                let footer: Vec<_> = l
                    .controls
                    .iter()
                    .filter(|c| !c.body && !matches!(c.action, Action::Tab(_) | Action::FontSearch))
                    .map(|c| c.label.as_str())
                    .collect();
                assert_eq!(
                    footer,
                    if state.pending.is_some() {
                        vec!["取消", "确认操作"]
                    } else {
                        vec!["完成"]
                    },
                    "{tab:?} footer changed"
                );
            }
        }
    }
}

#[test]
fn appearance_keyboard_focus_reaches_opacity_fonts_and_done() {
    let app = app();
    let mut state = State {
        focus: Some(Action::Tab(Tab::Mappings)),
        ..State::default()
    };
    let l = Layout::new(&app, &state, MIN_WIDTH, MIN_HEIGHT);
    for action in [
        Action::OpacityInput,
        Action::SaveOpacity,
        Action::NameColorTheme(NameColorTheme::Classic),
        Action::NameColorTheme(NameColorTheme::Nord),
        Action::NameColorTheme(NameColorTheme::OneDark),
        Action::NameColorTheme(NameColorTheme::Dracula),
        Action::FontSearch,
        Action::FontFamily(String::new()),
        Action::Close,
    ] {
        l.focus_next(&mut state, false);
        assert_eq!(state.focus, Some(action.clone()));
        if action == Action::FontSearch || action == Action::OpacityInput {
            assert!(l.edit_box_rect(&action, state.scroll).is_some());
        }
    }
    l.focus_next(&mut state, true);
    assert_eq!(state.focus, Some(Action::FontFamily(String::new())));
}

#[test]
fn appearance_keeps_search_current_font_and_font_family_selection() {
    let mut app = app();
    app.config.font_family = "Arial".into();
    app.config.cjk_font_family = "Microsoft YaHei".into();
    let mut state = State {
        tab: Tab::Appearance,
        font_names: vec![crate::font_catalog::InstalledFont::named("Arial")],
        ..State::default()
    };
    state.set_font_query(String::new());
    let l = Layout::new(&app, &state, WIDTH, HEIGHT);
    assert!(
        l.labels
            .iter()
            .any(|label| label.heading && label.text == "当前字体 · Arial · Microsoft YaHei")
    );
    assert!(l.edit_box_rect(&Action::FontSearch, 0.0).is_some());
    let body: Vec<_> = l
        .controls
        .iter()
        .filter(|c| matches!(c.action, Action::FontFamily(_)))
        .collect();
    assert_eq!(body.len(), 2);
    assert_eq!(body[0].label, "使用默认字体");
    assert_eq!(body[0].action, Action::FontFamily(String::new()));
    assert_eq!(body[1].label, "Arial");
    assert_eq!(body[1].action, Action::FontFamily("Arial".into()));
    assert!(body[1].selected);
}

#[test]
fn font_search_focus_returns_from_a_scrolled_list_and_survives_filtering() {
    let app = app();
    let mut state = State {
        font_names: (0..30)
            .map(|i| crate::font_catalog::InstalledFont::named(&format!("Font {i}")))
            .collect(),
        ..State::default()
    };
    state.set_font_query(String::new());
    let l = Layout::new(&app, &state, MIN_WIDTH, MIN_HEIGHT);
    l.focus_action(&mut state, Action::FontFamily("Font 29".into()));
    assert!(state.scroll > 0.0);
    assert!(l.edit_box_rect(&Action::FontSearch, state.scroll).is_none());
    l.focus_action(&mut state, Action::FontSearch);
    assert!(state.font_focused());
    assert!(l.edit_box_rect(&Action::FontSearch, state.scroll).is_some());
    state.set_font_query("Font 29".into());
    let l = Layout::new(&app, &state, MIN_WIDTH, MIN_HEIGHT);
    // EN_CHANGE restores visibility after filtering resets the scroll.
    l.focus_action(&mut state, Action::FontSearch);
    assert!(l.edit_box_rect(&Action::FontSearch, state.scroll).is_some());
    l.move_font_focus(&mut state, false);
    assert_eq!(state.focus, Some(Action::FontFamily("Font 29".into())));
    let control = l
        .controls
        .iter()
        .find(|c| Some(&c.action) == state.focus.as_ref())
        .unwrap();
    let rect = l.screen_rect(control, state.scroll);
    assert!(rect.y >= l.content_top && rect.y + rect.h <= l.height - FOOTER);
    l.focus_action(&mut state, Action::OpacityInput);
    assert!(!state.font_focused());
    assert!(
        l.edit_box_rect(&Action::OpacityInput, state.scroll)
            .is_some()
    );
    state.focus = Some(Action::FontSearch);
    state.pending = Some(Action::ClearKeys);
    assert!(!state.font_focused());
}

#[test]
fn the_accounts_tab_lists_each_connection_with_its_keys() {
    let mut app = app();
    app.stored.set_key("a1", "k");
    let state = State {
        tab: Tab::Accounts,
        ..State::default()
    };
    let l = Layout::new(&app, &state, WIDTH, HEIGHT);
    assert!(
        l.controls
            .iter()
            .any(|c| c.action == Action::TestAccount("a1".into()))
    );
    assert!(
        l.controls
            .iter()
            .any(|c| c.action == Action::Login("a1".into()))
    );
    assert!(
        l.controls
            .iter()
            .any(|c| c.action == Action::RemoveAccount("a1".into()))
    );
    assert!(l.controls.iter().any(|c| c.action == Action::ClearKeys));
    assert!(l.controls.iter().any(|c| c.action == Action::AddAccount));
}

#[test]
fn the_provider_tab_offers_one_row_per_seen_pair() {
    let mut app = app();
    let record: crate::model::UsageRecord =
        serde_json::from_str(r#"{"provider":"openai","model":"gpt-5"}"#).expect("record");
    let row = crate::model::Row::from_record(&record).with_account("a1", "本机");
    app.apply_updates(vec![crate::worker::Update::Snapshot { rows: vec![row] }]);
    let state = State {
        tab: Tab::Providers,
        ..State::default()
    };
    let l = Layout::new(&app, &state, WIDTH, HEIGHT);
    let entry = HiddenProvider {
        account_id: "a1".into(),
        provider: "openai".into(),
    };
    assert!(
        l.controls
            .iter()
            .any(|c| c.action == Action::Provider(entry.clone()))
    );
    assert!(
        l.controls
            .iter()
            .any(|c| c.action == Action::ClearProviders)
    );
}

#[test]
fn the_mappings_tab_lists_the_active_kinds_aliases() {
    let mut app = app();
    app.config.model_aliases = vec![NameAlias {
        from: "deepseek".into(),
        to: "ds".into(),
    }];
    let state = State {
        tab: Tab::Mappings,
        ..State::default()
    };
    let l = Layout::new(&app, &state, WIDTH, HEIGHT);
    for action in [
        Action::AliasFromInput,
        Action::AliasToInput,
        Action::AddAlias,
        Action::RemoveAlias(AliasKind::Model, 0),
    ] {
        assert!(l.controls.iter().any(|c| c.action == action), "{action:?}");
    }
}

#[test]
fn the_mappings_tab_switches_kinds_and_lists_that_kinds_entries() {
    let mut app = app();
    app.config.provider_aliases = vec![NameAlias {
        from: "Anthropic".into(),
        to: "A社".into(),
    }];
    let mut state = State {
        tab: Tab::Mappings,
        ..State::default()
    };
    let l = Layout::new(&app, &state, WIDTH, HEIGHT);
    for kind in AliasKind::ALL {
        assert!(
            l.controls
                .iter()
                .any(|c| c.action == Action::AliasKind(kind)),
            "{kind:?} selector missing"
        );
    }
    // The model list is empty, so the provider row is not shown.
    assert!(
        !l.controls
            .iter()
            .any(|c| matches!(c.action, Action::RemoveAlias(..)))
    );

    state.alias_kind = AliasKind::Provider;
    let l = Layout::new(&app, &state, WIDTH, HEIGHT);
    assert!(
        l.controls
            .iter()
            .any(|c| c.action == Action::RemoveAlias(AliasKind::Provider, 0))
    );
}

#[test]
fn native_edit_boxes_are_placed_over_their_cells() {
    let mut app = app();
    app.config.model_aliases = vec![NameAlias {
        from: "a".into(),
        to: "b".into(),
    }];
    let state = State {
        tab: Tab::Mappings,
        ..State::default()
    };
    let l = Layout::new(&app, &state, WIDTH, HEIGHT);
    let rect = l
        .edit_box_rect(&Action::AliasFromInput, 0.0)
        .expect("visible box");
    let control = l
        .controls
        .iter()
        .find(|c| c.action == Action::AliasFromInput)
        .expect("control");
    assert_eq!(rect.y, control.rect.y + l.content_top);
}

#[test]
fn a_short_window_scrolls_its_body_but_keeps_the_footer() {
    let app = app();
    let mut state = State {
        tab: Tab::Fields,
        ..State::default()
    };
    let l = Layout::new(&app, &state, WIDTH, 420.0);
    assert!(l.max_scroll() > 0.0);
    let save = l
        .controls
        .iter()
        .find(|c| c.action == Action::Close)
        .expect("close");
    let rect = l.screen_rect(save, l.max_scroll());
    assert_eq!(rect.y, save.rect.y);
    assert!(!save.body);

    state.scroll = l.max_scroll();
    let scrolled = l.screen_rect(
        l.controls
            .iter()
            .find(|c| c.action == Action::Field(DisplayField::Model))
            .expect("field"),
        state.scroll,
    );
    assert!(scrolled.y < l.content_top);
}

#[test]
fn the_font_search_box_filters_installed_faces() {
    let mut state = State {
        font_names: vec![
            crate::font_catalog::InstalledFont::named("Arial"),
            crate::font_catalog::InstalledFont::named("Microsoft YaHei 微软雅黑"),
        ],
        ..State::default()
    };
    state.set_font_query("雅黑".into());
    assert_eq!(state.font_matches, vec![1]);
    state.set_font_query("".into());
    assert_eq!(state.font_matches.len(), 2);
}
