use super::*;

#[test]
fn only_opacity_uses_decimal_input_rules() {
    assert!(numeric_id(OPACITY_ID));
    for id in [ID, ALIAS_FROM_ID, ALIAS_TO_ID, KEY_INDEX_ID] {
        assert!(!numeric_id(id), "{id} must not use opacity input rules");
    }
    for ch in ['0', '9', '.', '\u{8}'] {
        assert!(numeric_char(ch as usize));
    }
    for ch in ['a', '-', '中'] {
        assert!(!numeric_char(ch as usize));
    }
}

#[test]
fn only_font_search_forwards_arrow_keys_to_the_font_list() {
    for key in [VK_DOWN.0, VK_UP.0] {
        assert!(font_navigation_key(ID, key));
        for id in [OPACITY_ID, ALIAS_FROM_ID, ALIAS_TO_ID] {
            assert!(!font_navigation_key(id, key));
        }
    }
    assert!(!font_navigation_key(ID, VK_RETURN.0));
}

#[test]
fn name_mapping_boxes_remain_free_text_for_chinese_input() {
    for id in [ALIAS_FROM_ID, ALIAS_TO_ID] {
        assert!(!numeric_id(id));
        assert!(!integer_id(id));
    }
}
