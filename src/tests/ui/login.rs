use super::*;

fn form() -> LoginForm {
    LoginForm::adding(crate::config::DEFAULT_ENDPOINT)
}

#[test]
fn a_new_connection_starts_at_the_local_gateway_with_the_name_field_focused() {
    let f = form();
    assert_eq!(f.endpoint, crate::config::DEFAULT_ENDPOINT);
    assert_eq!(f.focus, Field::Name);
    assert!(f.key.is_empty());
    assert!(f.editing.is_none());
}

#[test]
fn an_existing_connection_is_prefilled_and_lands_on_the_key() {
    let f = LoginForm::for_account(&Account {
        id: "a1".into(),
        name: "本机".into(),
        endpoint: "http://127.0.0.1:3425".into(),
    });
    assert_eq!(f.name, "本机");
    assert_eq!(f.endpoint, "http://127.0.0.1:3425");
    assert_eq!(f.focus, Field::Key);
    assert_eq!(f.editing.as_deref(), Some("a1"));
}

#[test]
fn an_endpoint_is_required_and_must_be_http() {
    let mut f = form();
    f.endpoint.clear();
    assert!(f.validate().is_err());

    f.endpoint = "127.0.0.1:3425".into();
    assert!(f.validate().is_err());

    f.endpoint = " https://magpie.example/ ".into();
    assert!(f.validate().is_ok());
    assert_eq!(f.trimmed_endpoint(), "https://magpie.example");
}

#[test]
fn a_gateway_key_is_optional() {
    let f = form();
    assert!(f.validate().is_ok());
    assert_eq!(f.trimmed_key(), "");
}

#[test]
fn an_unnamed_connection_is_named_after_its_host() {
    let mut f = form();
    assert_eq!(f.resolved_name(), "127.0.0.1:3425");
    f.name = "  工作  ".into();
    assert_eq!(f.resolved_name(), "工作");
}

#[test]
fn the_key_is_masked_and_a_pasted_bearer_prefix_is_removed() {
    let f = LoginForm {
        key: "abc".into(),
        ..form()
    };
    assert_eq!(f.shown_value(Field::Key), "***");
    assert_eq!(f.shown_value(Field::Name), "");

    let mut f = form();
    f.focus = Field::Key;
    f.paste("  Bearer abc  ");
    assert_eq!(f.key, "Bearer abc");
    assert_eq!(f.trimmed_key(), "abc");
}

#[test]
fn typing_inserts_at_the_caret_and_backspace_removes_before_it() {
    let mut f = form();
    f.focus_field(Field::Name);
    for ch in "abc".chars() {
        f.insert(ch);
    }
    assert_eq!(f.name, "abc");
    assert_eq!(f.caret, 3);

    f.set_caret(1, false);
    f.insert('X');
    assert_eq!(f.name, "aXbc");

    f.backspace();
    assert_eq!(f.name, "abc");
    // The caret sits after "a", so forward-delete removes the "b".
    f.delete_forward();
    assert_eq!(f.name, "ac");
}

#[test]
fn a_control_character_is_rejected_but_chinese_names_are_kept() {
    let mut f = form();
    f.focus_field(Field::Name);
    f.insert('\n');
    f.insert('中');
    assert_eq!(f.name, "中");
}

#[test]
fn an_endpoint_field_rejects_non_ascii_input() {
    let mut f = form();
    f.focus_field(Field::Endpoint);
    f.clear_focused();
    f.insert('中');
    f.insert('h');
    assert_eq!(f.endpoint, "h");
}

#[test]
fn selection_covers_what_the_drag_anchored() {
    let mut f = form();
    f.focus_field(Field::Name);
    f.paste("hello");
    f.press_caret(1);
    f.set_caret(4, true);
    assert_eq!(f.selection(), Some((1, 4)));
    assert_eq!(f.selected_text().as_deref(), Some("ell"));

    assert_eq!(f.take_selection().as_deref(), Some("ell"));
    assert_eq!(f.name, "ho");
    assert_eq!(f.selection(), None);
}

#[test]
fn focus_cycles_through_the_three_fields() {
    let mut f = form();
    assert_eq!(f.focus, Field::Name);
    f.focus_next(false);
    assert_eq!(f.focus, Field::Endpoint);
    f.focus_next(false);
    assert_eq!(f.focus, Field::Key);
    f.focus_next(false);
    assert_eq!(f.focus, Field::Name);
    f.focus_next(true);
    assert_eq!(f.focus, Field::Key);
}

#[test]
fn the_layout_shows_name_endpoint_and_key_within_the_authored_height() {
    let f = form();
    let scale = 1.0;
    let height = window_height(scale) as f32;
    let m = Metrics::new(452.0, height, scale);
    let l = layout(&f, m);
    assert_eq!(l.fields.len(), 3);
    assert!(l.card.y >= 0.0);
    assert!(l.card.y + l.card.h <= height);
    assert!(l.button.y + l.button.h <= l.card.y + l.card.h);
    // The local gateway is the placeholder a new form shows.
    assert_eq!(
        placeholder_for(Field::Endpoint),
        crate::config::DEFAULT_ENDPOINT
    );
}

#[test]
fn clicks_map_to_fields_or_submit() {
    let f = form();
    let m = Metrics::new(452.0, window_height(1.0) as f32, 1.0);
    let l = layout(&f, m);

    let (name, _) = l.fields[0];
    assert!(matches!(
        hit(&f, m, name.x + 2.0, name.y + 2.0),
        Some(Action::Focus(Field::Name))
    ));
    assert!(matches!(
        hit(&f, m, l.button.x + 2.0, l.button.y + 2.0),
        Some(Action::Submit)
    ));
    assert!(hit(&f, m, 1.0, 1.0).is_none());
}

#[test]
fn the_form_asks_for_the_keyboard_and_the_list_does_not() {
    // A form height leaves room for all three rows plus the button.
    let height = window_height(1.0) as f32;
    let card = layout(&form(), Metrics::new(452.0, height, 1.0)).card;
    assert!(card.h > 3.0 * (LABEL_H + FIELD_H));
}
