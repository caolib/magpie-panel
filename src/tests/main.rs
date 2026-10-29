use super::*;

#[test]
fn window_level_menu_commands_select_all_three_modes() {
    for (id, level) in [
        (MENU_NORMAL, WindowLevel::Normal),
        (MENU_TOPMOST, WindowLevel::Topmost),
        (MENU_BOTTOMMOST, WindowLevel::Bottommost),
    ] {
        assert!(is_menu_command(id));
        assert_eq!(menu_window_level(id), Some(level));
    }
    assert_eq!(menu_window_level(MENU_PIN_POSITION), None);
    assert_eq!(menu_window_level(MENU_REFRESH), None);
    assert_eq!(menu_window_level(0), None);
    assert!(!is_menu_command(0));
}
