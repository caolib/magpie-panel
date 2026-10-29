//! Error and retry details: one record per block, its request fields on the
//! first line and the wrapped log on full-width lines below. The worker adds
//! matched trace attempts or an incomplete ledger summary.

use windows::Win32::Graphics::GdiPlus::RectF;

use crate::format as fmt;
use crate::model::{
    Attempt, DetailSource, ExecutionDetail, Row, apply_aliases, display_model_name,
};
use crate::theme::{self, Fonts, Painter};
use crate::ui::layout::Rect;

pub const POPUP_W: f32 = 720.0;
pub const POPUP_H: f32 = 400.0;
pub const MIN_W: f32 = 420.0;
pub const MIN_H: f32 = 220.0;

const PAD: f32 = 10.0;
const HEADER_H: f32 = 30.0;
const FOOTER_H: f32 = 32.0;
const ROW_H: f32 = 17.0;
const CELL_PAD: f32 = 4.0;
const COLUMN_GAP: f32 = 8.0;
const BUTTON_H: f32 = 22.0;
const CLOSE_W: f32 = 22.0;
const COPY_W: f32 = 52.0;
/// A log line longer than this wraps instead of widening the popup.
pub const MAX_LOG_W: f32 = 480.0;
const HEADERS: [&str; 5] = ["状态码", "供应商", "模型", "类型", "错误日志"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Copy,
    Close,
}

impl Button {
    pub fn label(self) -> &'static str {
        match self {
            Button::Copy => "复制",
            Button::Close => "×",
        }
    }
}

/// Geometry in device pixels; widths never depend on error text.
#[derive(Debug, Clone, Copy)]
pub struct Popup {
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

impl Popup {
    pub fn new(width: f32, height: f32, scale: f32) -> Self {
        Self {
            width,
            height,
            scale: scale.max(0.25),
        }
    }

    pub fn pad(&self) -> f32 {
        PAD * self.scale
    }

    pub fn header_h(&self) -> f32 {
        HEADER_H * self.scale
    }

    pub fn footer_h(&self) -> f32 {
        FOOTER_H * self.scale
    }

    pub fn row_h(&self) -> f32 {
        ROW_H * self.scale
    }

    pub fn content_top(&self) -> f32 {
        self.header_h()
    }

    pub fn content_view_h(&self) -> f32 {
        (self.height - self.content_top() - self.footer_h()).max(0.0)
    }
}

pub fn button_rect(m: &Popup, button: Button) -> Rect {
    let s = m.scale;
    match button {
        Button::Close => Rect {
            x: m.width - m.pad() - CLOSE_W * s,
            y: (m.header_h() - CLOSE_W * s) / 2.0,
            w: CLOSE_W * s,
            h: CLOSE_W * s,
        },
        Button::Copy => Rect {
            x: m.width - m.pad() - COPY_W * s,
            y: m.height - (m.footer_h() + BUTTON_H * s) / 2.0,
            w: COPY_W * s,
            h: BUTTON_H * s,
        },
    }
}

pub fn hit_button(m: &Popup, x: f32, y: f32) -> Option<Button> {
    [Button::Close, Button::Copy]
        .into_iter()
        .find(|b| button_rect(m, *b).contains(x, y))
}

pub struct Entry {
    pub code: String,
    pub provider: String,
    pub model: String,
    /// Short failure taxonomy (`fail`, `error_type`), kept apart from the log
    /// prose so the log column stays readable.
    pub kind: String,
    pub log: String,
    color: u32,
}

impl Entry {
    fn from_attempt(attempt: &Attempt, config: &crate::config::Config) -> Self {
        let provider = attempt
            .provider
            .as_deref()
            .map(|p| apply_aliases(p, &config.provider_aliases));
        let provider = match (provider, attempt.provider_key.as_deref()) {
            (Some(p), Some(k)) => format!("{p}({k})"),
            (Some(p), None) => p,
            (None, Some(k)) => format!("({k})"),
            _ => fmt::BLANK.into(),
        };
        let kinds: Vec<_> = [attempt.fail.as_deref(), attempt.error_type.as_deref()]
            .into_iter()
            .flatten()
            .filter(|text| !text.trim().is_empty())
            .collect();
        let reason = attempt
            .error
            .as_deref()
            .filter(|text| !text.trim().is_empty());
        let failed = attempt.status_code.is_some_and(|s| s >= 400)
            || !kinds.is_empty()
            || reason.is_some()
            || attempt.handed_on;
        Self {
            code: attempt
                .status_code
                .filter(|s| (100..600).contains(s))
                .map(|s| s.to_string())
                .unwrap_or_else(|| fmt::BLANK.into()),
            provider,
            model: if attempt.model.is_empty() {
                fmt::BLANK.into()
            } else {
                display_model_name(&attempt.model, &config.model_aliases)
            },
            kind: kinds.join(" · "),
            log: match reason {
                Some(error) => error.to_string(),
                // Without a type the missing reason has to be stated here.
                None if kinds.is_empty() && failed => "记录未提供错误原因".into(),
                None => fmt::BLANK.into(),
            },
            color: if failed {
                theme::RED
            } else if attempt.done == Some(true)
                && attempt.status_code.is_some_and(|s| (200..400).contains(&s))
            {
                theme::GREEN
            } else {
                theme::TEXT_DIM
            },
        }
    }

    fn from_row(row: &Row, config: &crate::config::Config) -> Self {
        Self::from_attempt(
            &Attempt {
                model: row.model.clone(),
                status_code: row.status_code,
                error: row.error.clone(),
                error_type: row.error_type.clone(),
                provider: row.provider.clone(),
                provider_key: row.provider_key.clone(),
                handed_on: row.handed_on,
                done: Some(row.status != crate::model::Status::Processing),
                ..Default::default()
            },
            config,
        )
    }

    fn cells(&self) -> [&str; 5] {
        [
            &self.code,
            &self.provider,
            &self.model,
            &self.kind,
            &self.log,
        ]
    }
}

pub struct Doc {
    pub rows: Vec<Entry>,
    /// Loading, missing data and incomplete history stay explicit, not extra columns.
    pub notice: String,
}

impl Doc {
    pub fn with_detail(
        raw: &Row,
        detail: &ExecutionDetail,
        config: &crate::config::Config,
    ) -> Self {
        let mut rows: Vec<_> = detail
            .attempts
            .iter()
            .map(|a| Entry::from_attempt(a, config))
            .collect();
        // Show the selected ledger row immediately, or retain a reason the trace
        // did not contain. Do not repeat a final summary already in the attempts.
        let missing_reason = [raw.error.as_deref(), raw.error_type.as_deref()]
            .into_iter()
            .flatten()
            .filter(|e| !e.trim().is_empty())
            .any(|reason| {
                !detail.attempts.iter().any(|a| {
                    [
                        a.error.as_deref(),
                        a.error_type.as_deref(),
                        a.fail.as_deref(),
                    ]
                    .contains(&Some(reason))
                })
            });
        let selected_recorded = detail.attempts.iter().any(|a| {
            a.status_code == raw.status_code
                && (a.model == raw.model || a.model.is_empty())
                && raw
                    .provider
                    .as_deref()
                    .is_none_or(|provider| a.provider.as_deref().is_none_or(|p| p == provider))
                && raw
                    .provider_key
                    .as_deref()
                    .is_none_or(|key| a.provider_key.as_deref().is_none_or(|k| k == key))
        });
        if rows.is_empty() || missing_reason || !selected_recorded {
            rows.push(Entry::from_row(raw, config));
        }
        if let Some(error) = detail
            .route_error
            .as_deref()
            .filter(|e| !e.trim().is_empty())
            && !rows.iter().any(|r| r.log.contains(error))
        {
            let mut entry = Entry::from_row(raw, config);
            entry.code = detail
                .route_status
                .filter(|s| (100..600).contains(s))
                .map(|s| s.to_string())
                .unwrap_or_else(|| fmt::BLANK.into());
            // A route-level reason cannot identify one supplier/model attempt.
            entry.provider = fmt::BLANK.into();
            entry.model = fmt::BLANK.into();
            entry.kind = fmt::BLANK.into();
            entry.log = error.into();
            entry.color = theme::RED;
            rows.push(entry);
        }
        let notice = match detail.source {
            DetailSource::Trace
                if !detail.notice.contains("不完整") && !detail.notice.contains("未结束") =>
            {
                String::new()
            }
            _ => detail.notice.clone(),
        };
        Self { rows, notice }
    }
}

/// Copy keeps each error's original whitespace and line breaks.
pub fn plain_text(doc: &Doc) -> String {
    let mut out = String::new();
    if !doc.notice.is_empty() {
        out.push_str(&doc.notice);
        out.push('\n');
    }
    for (index, row) in doc.rows.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        for (label, value) in HEADERS.into_iter().zip(row.cells()) {
            out.push_str(label);
            out.push(':');
            if !value.is_empty() {
                out.push(' ');
                out.push_str(value);
            }
            out.push('\n');
        }
    }
    out
}

pub struct View<'a> {
    pub doc: &'a Doc,
    pub scroll: f32,
    pub hover: Option<Button>,
}

struct Visual {
    rect: Rect,
    text: String,
    color: u32,
}

struct TableLayout {
    visuals: Vec<Visual>,
    bands: Vec<Rect>,
    height: f32,
}

/// The first line's four fields at their own text width: the widest single
/// line of each column's cells plus the cell padding.
fn natural_widths(doc: &Doc, scale: f32, measure: impl Fn(&str) -> f32) -> [f32; 4] {
    let mut widths = [0.0f32; 4];
    for row in &doc.rows {
        for (index, text) in row.cells()[..4].iter().enumerate() {
            let widest = text.split('\n').map(&measure).fold(0.0f32, f32::max);
            widths[index] = widths[index].max(widest);
        }
    }
    for width in &mut widths {
        *width += CELL_PAD * 2.0 * scale;
    }
    widths
}

/// Widest log line, capped: past the cap the log wraps rather than widening the
/// popup further.
fn log_width(doc: &Doc, scale: f32, measure: impl Fn(&str) -> f32) -> f32 {
    doc.rows
        .iter()
        .flat_map(|row| row.log.split('\n'))
        .map(&measure)
        .fold(0.0f32, f32::max)
        .min(MAX_LOG_W * scale)
}

/// The widths the first line's columns are drawn at. A window narrower than
/// they need shrinks all four together; wider windows leave the spare room
/// blank, so a short table does not stretch.
fn fitted_widths(natural: [f32; 4], width: f32, scale: f32) -> [f32; 4] {
    let available = (width - PAD * 2.0 * scale - COLUMN_GAP * 3.0 * scale).max(0.0);
    let total: f32 = natural.iter().sum();
    if total <= available {
        return natural;
    }
    let keep = (available / total).max(0.0);
    natural.map(|w| (w * keep).max(1.0))
}

/// Column rectangles for the first line, one per field.
fn column_rects(m: &Popup, widths: &[f32; 4]) -> [Rect; 4] {
    let gap = COLUMN_GAP * m.scale;
    let mut x = m.pad();
    widths.map(|w| {
        let rect = Rect {
            x,
            y: 0.0,
            w,
            h: ROW_H * m.scale,
        };
        x += w + gap;
        rect
    })
}

/// Client width the document asks for: the first-line fields, or the longest
/// log line up to its cap, floored at `MIN_W`.
fn width_of(natural: &[f32; 4], log: f32, m: &Popup) -> f32 {
    let columns = m.pad() * 2.0 + natural.iter().sum::<f32>() + COLUMN_GAP * 3.0 * m.scale;
    let log = m.pad() * 2.0 + log;
    columns.max(log).max(MIN_W * m.scale)
}

/// Client width the document asks for. Callers clamp it to the monitor.
pub fn desired_width(m: &Popup, doc: &Doc, measure: impl Fn(&str) -> f32) -> f32 {
    width_of(
        &natural_widths(doc, m.scale, &measure),
        log_width(doc, m.scale, &measure),
        m,
    )
}

/// The same row-height calculation drives painting and pure geometry tests.
fn layout(
    m: &Popup,
    doc: &Doc,
    widths: &[f32; 4],
    wrap: impl Fn(&str, f32) -> Vec<String>,
) -> TableLayout {
    let mut visuals = Vec::new();
    let mut bands = Vec::new();
    let row_h = ROW_H * m.scale;
    let cell_pad = CELL_PAD * m.scale;
    let content_w = (m.width - m.pad() * 2.0).max(1.0);
    let mut y = cell_pad;
    if !doc.notice.is_empty() {
        for text in wrap(&doc.notice, content_w) {
            visuals.push(Visual {
                rect: Rect {
                    x: m.pad(),
                    y,
                    w: content_w,
                    h: row_h,
                },
                text,
                color: theme::TEXT_FAINT,
            });
            y += row_h;
        }
        y += cell_pad;
    }
    let columns = column_rects(m, widths);
    for row in &doc.rows {
        let head = row.cells()[..4]
            .iter()
            .zip(columns)
            .map(|(text, rect)| {
                let inner = (rect.w - cell_pad * 2.0).max(1.0);
                (
                    wrap(text, inner),
                    Rect {
                        x: rect.x + cell_pad,
                        w: inner,
                        ..rect
                    },
                )
            })
            .collect::<Vec<_>>();
        let head_lines = head
            .iter()
            .map(|(lines, _)| lines.len())
            .max()
            .unwrap_or(1)
            .max(1);
        // A record without a log stays one line tall.
        let log_lines = if row.log.is_empty() {
            Vec::new()
        } else {
            wrap(&row.log, content_w)
        };
        let height = (head_lines + log_lines.len()) as f32 * row_h + cell_pad * 2.0;
        bands.push(Rect {
            x: m.pad(),
            y,
            w: content_w,
            h: height,
        });
        for (index, (lines, column)) in head.into_iter().enumerate() {
            let color = match index {
                0 | 3 => row.color,
                1 => theme::CYAN,
                _ => theme::TEXT,
            };
            for (line, text) in lines.into_iter().enumerate() {
                visuals.push(Visual {
                    rect: Rect {
                        y: y + cell_pad + line as f32 * row_h,
                        ..column
                    },
                    text,
                    color,
                });
            }
        }
        let log_top = y + cell_pad + head_lines as f32 * row_h;
        for (line, text) in log_lines.into_iter().enumerate() {
            visuals.push(Visual {
                rect: Rect {
                    x: m.pad(),
                    y: log_top + line as f32 * row_h,
                    w: content_w,
                    h: row_h,
                },
                text,
                color: row.color,
            });
        }
        y += height;
    }
    TableLayout {
        visuals,
        bands,
        height: y + cell_pad,
    }
}

/// What one frame measured: the height the document needs, and the client
/// width it asks for (the caller resizes when the two differ).
#[derive(Debug, Clone, Copy, Default)]
pub struct Frame {
    pub height: f32,
    pub width: f32,
}

pub fn draw(p: &Painter, fonts: &Fonts, m: &Popup, v: &View) -> Frame {
    p.fill_rect(0.0, 0.0, m.width, m.height, theme::BG);
    let edge = m.scale.max(1.0);
    let border = theme::with_alpha(theme::BORDER, 0xE0);
    p.fill_rect(0.0, 0.0, m.width, edge, border);
    p.fill_rect(0.0, m.height - edge, m.width, edge, border);
    p.fill_rect(0.0, 0.0, edge, m.height, border);
    p.fill_rect(m.width - edge, 0.0, edge, m.height, border);
    p.dual_text(
        &fonts.body,
        "错误日志",
        m.pad(),
        5.0 * m.scale,
        m.width - m.pad() * 2.0 - CLOSE_W * m.scale,
        ROW_H * m.scale,
        theme::TEXT,
        theme::ALIGN_NEAR,
    );
    p.fill_rect(
        m.pad(),
        m.content_top() - edge,
        m.width - m.pad() * 2.0,
        edge,
        theme::SEPARATOR,
    );
    for button in [Button::Close, Button::Copy] {
        let rect = button_rect(m, button);
        let hovered = v.hover == Some(button);
        if button == Button::Copy || hovered {
            p.round_rect(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                4.0 * m.scale,
                if hovered {
                    theme::CARD_HOVER
                } else {
                    theme::CARD
                },
                true,
            );
        }
        let label = button.label();
        let w = p.dual_measure(&fonts.small, label);
        p.dual_text(
            &fonts.small,
            label,
            rect.x + (rect.w - w) / 2.0,
            rect.y + (rect.h - ROW_H * m.scale) / 2.0,
            w,
            ROW_H * m.scale,
            if hovered {
                theme::WHITE
            } else {
                theme::TEXT_DIM
            },
            theme::ALIGN_NEAR,
        );
    }
    let measure = |text: &str| p.dual_measure(&fonts.small, text);
    let natural = natural_widths(v.doc, m.scale, measure);
    let width = width_of(&natural, log_width(v.doc, m.scale, measure), m);
    let widths = fitted_widths(natural, m.width, m.scale);
    let table = layout(m, v.doc, &widths, |text, inner| {
        p.wrap(&fonts.small, text, inner)
    });
    let top = m.content_top();
    let view_h = m.content_view_h();
    p.clip(RectF {
        X: 0.0,
        Y: top,
        Width: m.width,
        Height: view_h,
    });
    for (index, band) in table.bands.iter().enumerate() {
        let y = top + band.y - v.scroll;
        if y + band.h < top || y > top + view_h {
            continue;
        }
        if index % 2 == 0 {
            p.fill_rect(band.x, y, band.w, band.h, theme::CARD);
        }
        p.fill_rect(
            band.x,
            y + band.h - m.scale,
            band.w,
            m.scale,
            theme::SEPARATOR,
        );
    }
    for visual in &table.visuals {
        let rect = visual.rect;
        let y = top + rect.y - v.scroll;
        if y + rect.h < top || y > top + view_h {
            continue;
        }
        p.dual_text(
            &fonts.small,
            &visual.text,
            rect.x,
            y,
            rect.w,
            rect.h,
            visual.color,
            theme::ALIGN_NEAR,
        );
    }
    p.reset_clip();
    draw_scrollbar(p, m, table.height, v.scroll);
    Frame {
        height: table.height,
        width,
    }
}

fn draw_scrollbar(p: &Painter, m: &Popup, content_h: f32, scroll: f32) {
    let view_h = m.content_view_h();
    let max_scroll = content_h - view_h;
    if max_scroll <= 1.0 {
        return;
    }
    let s = m.scale;
    let x = m.width - 4.5 * s;
    let w = 3.0 * s;
    let thumb_h = (view_h / content_h * view_h).max(24.0 * s).min(view_h);
    let offset = (scroll / max_scroll).clamp(0.0, 1.0) * (view_h - thumb_h);
    p.round_rect(x, m.content_top(), w, view_h, w / 2.0, theme::CARD, true);
    p.round_rect(
        x,
        m.content_top() + offset,
        w,
        thumb_h,
        w / 2.0,
        theme::TEXT_FAINT,
        true,
    );
}

#[cfg(test)]
#[path = "../tests/ui/detail.rs"]
mod tests;
