//! Panel chrome and the table half of the list.
//!
//! The panel has one layout: a table with one row per request. This module
//! draws the header (filter chips, query box, settings button) and owns the
//! hit-test geometry shared by painting and input; `ui::table` draws the rows.

use crate::config::{Account, DisplayField};
use crate::model::{self, Filter, FilterMask, Row};
use crate::theme::{self, Painter};
use crate::ui::layout::{Metrics, Rect};
use crate::ui::table;

/// Colours that mark which connection a row came from.
const ACCOUNT_COLORS: [u32; 4] = [theme::MAUVE, theme::ORANGE, theme::CYAN, 0xFFB0_B8C4];

/// The colour of one connection's tag.
pub(crate) fn account_color(accounts: &[Account], id: &str) -> u32 {
    accounts
        .iter()
        .position(|a| a.id == id)
        .map(|i| ACCOUNT_COLORS[i % ACCOUNT_COLORS.len()])
        .unwrap_or(theme::TEXT_DIM)
}

/// Authored at 96 DPI; multiplied by `Metrics::scale` at draw time.
/// Header filter chips: one character plus padding.
const FILTER_W: f32 = 24.0;
const FILTER_H: f32 = 20.0;
const FILTER_GAP: f32 = 4.0;
/// The settings button, one chip wide so the header reads as one row.
const SETTINGS_W: f32 = 24.0;
const SETTINGS_H: f32 = 20.0;
/// Header query box, sitting just left of the settings button.
const QUERY_BOX_W: f32 = 150.0;
const QUERY_BOX_H: f32 = 20.0;
/// Chip order on the header, left to right.
const FILTERS: [Filter; 5] = [
    Filter::All,
    Filter::Completed,
    Filter::Failed,
    Filter::Live,
    Filter::Retried,
];

/// Colour palette for provider and model names.
const NAME_PALETTE: [u32; 9] = [
    theme::CYAN,
    theme::ORANGE,
    0xFFFF_6BC4,
    0xFF4F_E0C0,
    0xFFB9_8CFF,
    0xFFFF_B454,
    0xFF8D_E85B,
    0xFFE0_6BFF,
    0xFFFF_6B6B,
];

/// Hand the palette out to `names`, which are listed newest first, so the
/// oldest name on the page keeps the default light blue.
fn rank_palette(names: &mut [(&str, u32)]) {
    let count = names.len();
    for (i, (_, color)) in names.iter_mut().enumerate() {
        *color = NAME_PALETTE[(count - 1 - i) % NAME_PALETTE.len()];
    }
}

/// Map each distinct provider in `rows` (newest first) to a palette colour.
pub(crate) fn provider_palette(rows: &[Row]) -> Vec<(&str, u32)> {
    let mut map: Vec<(&str, u32)> = Vec::new();
    for row in rows {
        if let Some(provider) = row.provider.as_deref()
            && !map.iter().any(|(n, _)| *n == provider)
        {
            map.push((provider, 0));
        }
    }
    rank_palette(&mut map);
    map
}

/// Map each distinct served model in `rows` (newest first) to a palette colour.
pub(crate) fn model_palette(rows: &[Row]) -> Vec<(&str, u32)> {
    let mut map: Vec<(&str, u32)> = Vec::new();
    for row in rows {
        let model = row.served_model();
        if !map.iter().any(|(n, _)| *n == model) {
            map.push((model, 0));
        }
    }
    rank_palette(&mut map);
    map
}

/// Everything the list needs besides the rows themselves.
pub struct ListView<'a> {
    pub rows: &'a [Row],
    /// Unix seconds, used to render relative ages.
    pub now: i64,
    pub scroll: f32,
    pub hover: Option<usize>,
    pub selected: Option<usize>,
    pub status_text: Option<(String, bool)>,
    /// Saved connections; more than one means every row is tagged with its own.
    pub accounts: &'a [Account],
    /// A connection did not answer the last poll: the settings button turns red.
    pub settings_alert: bool,
    pub pinned: bool,
    /// Selected status-filter bits; the header chips highlight against this.
    pub filter: FilterMask,
    /// Table columns the user switched off.
    pub hidden_fields: &'a [DisplayField],
    /// The text the list was narrowed to, if the header box was submitted.
    pub query: Option<&'a str>,
    /// What is typed into the header box.
    pub query_draft: &'a str,
    /// The header box holds the keyboard, so it draws a caret.
    pub query_editing: bool,
    pub query_caret: bool,
}

impl ListView<'_> {
    /// Whether one column should be drawn.
    pub fn shows(&self, field: DisplayField) -> bool {
        !self.hidden_fields.contains(&field)
    }
}

/// Draw one frame. Returns the client width the table wants when that differs
/// from the one the window has, so the caller can resize.
pub fn draw(p: &Painter, fonts: &theme::Fonts, m: Metrics, v: &ListView) -> Option<f32> {
    p.fill_rect(0.0, 0.0, m.width, m.height, theme::BG);
    draw_header(p, fonts, m, v);
    let wanted = table::draw(p, fonts, m, v).map(|w| w.max(header_min_width(m)));
    draw_edge(p, m);
    wanted
}

/// Width the header's controls need so nothing overlaps.
pub fn header_min_width(m: Metrics) -> f32 {
    let s = m.scale;
    let chips = FILTERS.len() as f32 * FILTER_W + (FILTERS.len() - 1) as f32 * FILTER_GAP;
    (2.0 + chips + 8.0 + QUERY_BOX_W + 6.0 + SETTINGS_W) * s + m.pad() * 2.0
}

/// One-pixel outline so the panel separates from whatever is behind it.
fn draw_edge(p: &Painter, m: Metrics) {
    let c = theme::with_alpha(theme::BORDER, 0xE0);
    let t = m.scale.max(1.0);
    p.fill_rect(0.0, 0.0, m.width, t, c);
    p.fill_rect(0.0, m.height - t, m.width, t, c);
    p.fill_rect(0.0, 0.0, t, m.height, c);
    p.fill_rect(m.width - t, 0.0, t, m.height, c);
}

pub fn settings_button(m: Metrics) -> Rect {
    Rect {
        x: m.width - m.pad() - SETTINGS_W * m.scale,
        y: (m.header_h - SETTINGS_H * m.scale) / 2.0,
        w: SETTINGS_W * m.scale,
        h: SETTINGS_H * m.scale,
    }
}

/// The tail of `text` that fits in `width`.
fn fitting_tail<'a>(p: &Painter, font: &theme::DualFont, text: &'a str, width: f32) -> &'a str {
    if p.dual_measure(font, text) <= width {
        return text;
    }
    let mut start = 0;
    for (i, _) in text.char_indices() {
        if p.dual_measure(font, &text[i..]) <= width {
            start = i;
            break;
        }
        start = i;
    }
    &text[start..]
}

/// The header's query box, immediately left of the settings button.
pub fn query_box_rect(m: Metrics) -> Rect {
    let settings = settings_button(m);
    let s = m.scale;
    Rect {
        x: settings.x - 6.0 * s - QUERY_BOX_W * s,
        y: (m.header_h - QUERY_BOX_H * s) / 2.0,
        w: QUERY_BOX_W * s,
        h: QUERY_BOX_H * s,
    }
}

fn draw_header(p: &Painter, fonts: &theme::Fonts, m: Metrics, v: &ListView) {
    let s = m.scale;
    let y = 6.0 * s;

    let settings = settings_button(m);
    let (settings_fill, settings_text) = if v.settings_alert {
        (theme::with_alpha(theme::RED, 0x38), theme::RED)
    } else {
        (theme::CARD_HOVER, theme::TEXT_DIM)
    };
    p.round_rect(
        settings.x,
        settings.y,
        settings.w,
        settings.h,
        3.0 * s,
        settings_fill,
        true,
    );
    p.dual_text(
        &fonts.small,
        "设",
        settings.x,
        settings.y,
        settings.w,
        settings.h,
        settings_text,
        theme::ALIGN_CENTER,
    );

    let box_ = query_box_rect(m);
    p.round_rect(
        box_.x,
        box_.y,
        box_.w,
        box_.h,
        3.0 * s,
        theme::INPUT_BG,
        true,
    );
    p.round_rect(
        box_.x,
        box_.y,
        box_.w,
        box_.h,
        3.0 * s,
        theme::INPUT_BORDER,
        false,
    );
    let pad = 5.0 * s;
    let inner_w = (box_.w - pad * 2.0).max(0.0);
    let placeholder = v.query_draft.is_empty() && !v.query_editing;
    let visible = if placeholder {
        "模型 / 应用"
    } else {
        fitting_tail(p, &fonts.small, v.query_draft, inner_w)
    };
    p.dual_text(
        &fonts.small,
        visible,
        box_.x + pad,
        box_.y,
        inner_w,
        box_.h,
        if placeholder {
            theme::TEXT_FAINT
        } else {
            theme::TEXT
        },
        theme::ALIGN_NEAR,
    );
    if v.query_editing && v.query_caret {
        let tw = p.dual_measure(&fonts.small, visible).min(inner_w);
        let cx = box_.x + pad + tw;
        p.fill_rect(
            cx,
            box_.y + 4.0 * s,
            s.max(1.0),
            box_.h - 8.0 * s,
            theme::TEXT,
        );
    }

    // Lock icon when window position is pinned.
    let sub_x = if v.pinned {
        let lx = m.pad() + 2.0;
        let ly = y + 4.0 * s;
        let c = theme::TEXT_DIM;
        let bw = 7.0 * s;
        p.round_rect(lx + 1.5 * s, ly, 4.0 * s, 5.0 * s, 2.0 * s, c, false);
        p.round_rect(lx, ly + 3.0 * s, bw, 5.0 * s, 1.5 * s, c, true);
        m.pad() + 2.0 + bw + 6.0 * s
    } else {
        m.pad() + 2.0
    };

    let chip_y = (m.header_h - FILTER_H * s) / 2.0;
    let chip_text_h = 14.0 * s;
    let mut x = sub_x;
    for f in FILTERS {
        let selected = if f == Filter::All {
            v.filter == model::FILTER_NONE
        } else {
            v.filter & f.mask() != 0
        };
        if selected {
            p.round_rect(
                x,
                chip_y,
                FILTER_W * s,
                FILTER_H * s,
                3.0 * s,
                theme::CARD_HOVER,
                true,
            );
        }
        let label = f.label();
        let tw = p.dual_measure(&fonts.small, label);
        let color = if selected {
            match f {
                Filter::All => theme::WHITE,
                Filter::Completed => theme::GREEN,
                Filter::Failed => theme::RED,
                Filter::Live => theme::BLUE,
                Filter::Retried => theme::YELLOW,
            }
        } else {
            theme::TEXT_DIM
        };
        p.dual_text(
            &fonts.small,
            label,
            x + (FILTER_W * s - tw) / 2.0,
            chip_y + (FILTER_H * s - chip_text_h) / 2.0,
            tw,
            chip_text_h,
            color,
            theme::ALIGN_NEAR,
        );
        x += FILTER_W * s + FILTER_GAP * s;
    }

    let sub_right = box_.x - 8.0 * s;
    let sub_w = (sub_right - x).max(0.0);
    let (text, color) = match &v.status_text {
        Some((message, true)) => (message.clone(), theme::RED),
        Some((message, false)) => (message.clone(), theme::GREEN),
        None => (String::new(), theme::TEXT_FAINT),
    };
    p.dual_text(
        &fonts.small,
        &text,
        x,
        y,
        sub_w,
        18.0 * s,
        color,
        theme::ALIGN_NEAR,
    );

    p.fill_rect(0.0, m.header_h - 1.0, m.width, 1.0, theme::SEPARATOR);
}

/// The placeholder for a list with nothing to show.
pub(crate) fn draw_empty(p: &Painter, fonts: &theme::Fonts, m: Metrics, v: &ListView) {
    let label = if v.query.is_some() {
        "未找到匹配记录"
    } else {
        "等待数据…"
    };
    p.dual_text(
        &fonts.body,
        label,
        0.0,
        m.rows_top() + m.rows_viewport_h() / 2.0 - 10.0 * m.scale,
        m.width,
        20.0 * m.scale,
        theme::TEXT_FAINT,
        theme::ALIGN_CENTER,
    );
}

/// The header chip for `f`, laid out exactly as `draw_header` draws them.
fn chip_rect(m: Metrics, v: &ListView, f: Filter) -> Rect {
    let s = m.scale;
    let mut x = if v.pinned {
        m.pad() + 2.0 + 7.0 * s + 6.0 * s
    } else {
        m.pad() + 2.0
    };
    for g in FILTERS {
        if g == f {
            return Rect {
                x,
                y: (m.header_h - FILTER_H * s) / 2.0,
                w: FILTER_W * s,
                h: FILTER_H * s,
            };
        }
        x += FILTER_W * s + FILTER_GAP * s;
    }
    unreachable!("FILTERS covers every Filter variant")
}

/// Which filter chip a point in viewport space is over. Pinned mode is
/// display-only: chips are drawn but never clickable.
pub fn hit_filter(m: Metrics, v: &ListView, x: f32, y: f32) -> Option<Filter> {
    if v.pinned || y < 0.0 || y > m.header_h {
        return None;
    }
    for f in FILTERS {
        let r = chip_rect(m, v, f);
        if x >= r.x && x <= r.x + r.w {
            return Some(f);
        }
    }
    None
}

/// Hit-test a point in viewport space.
pub fn hit_row(m: Metrics, v: &ListView, x: f32, y: f32) -> Option<usize> {
    if x < m.pad() || x > m.width - m.pad() {
        return None;
    }
    m.row_at(y, v.scroll, v.rows.len())
}

/// The row a point is over whose detail can be opened, if any. Only rows with
/// something to explain take a click; everything else stays a drag handle.
pub fn hit_error_row(m: Metrics, v: &ListView, x: f32, y: f32) -> Option<usize> {
    if v.pinned {
        return None;
    }
    let index = hit_row(m, v, x, y)?;
    v.rows.get(index).filter(|r| r.is_error()).map(|_| index)
}

#[cfg(test)]
#[path = "../tests/ui/panel.rs"]
mod tests;
