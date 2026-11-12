//! Table layout: one row per request, one column per visible cell.
//!
//! Columns fit their widest text, except the supplier cell, which has a
//! density-scaled cap and ellipsis for long account names. The table reports
//! the width it needs so the caller can fit the window.

use windows::Win32::Graphics::GdiPlus::RectF;

use crate::config::DisplayField;
use crate::format as fmt;
use crate::model::{self, Protocol, Row};
use crate::theme::{self, Painter};
use crate::ui::layout::Metrics;
use crate::ui::logo::LogoCache;
use crate::ui::panel::{self, ListView};

/// Horizontal padding inside a cell, on each side.
const CELL_HPAD: f32 = 1.0;

/// Headroom added to the rectangle a cell is drawn in: GDI+ trims text the
/// moment the layout rectangle is narrower than the measured text.
const CELL_SLACK: f32 = 0.5;

/// Full supplier column width at base density, including horizontal padding.
const PROVIDER_MAX_WIDTH: f32 = 180.0;

/// The status cell is a dot, not a word.
const STATUS_DOT: &str = "●";

/// Latency thresholds in milliseconds: past a minute the whole cell turns
/// yellow, past two minutes red.
const LATENCY_SLOW_MS: i64 = 60_000;
const LATENCY_CRITICAL_MS: i64 = 120_000;

/// One table column: the status dot, followed by one column per visible cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    Status,
    Field(DisplayField),
}

impl Column {
    /// Every column of the table, in draw order. Columns with nothing to show
    /// in the current rows are dropped.
    fn all(v: &ListView) -> Vec<Column> {
        let mut out = vec![Column::Status];
        for &field in &v.field_order {
            if !v.shows(field) {
                continue;
            }
            let keep = match field {
                // One caller for every row: the header carries the name
                // instead, and the column stops taking width.
                DisplayField::Agent => v.single_agent().is_none(),
                DisplayField::Retry => v.rows.iter().any(has_retry),
                // The cache cell already carries the status code of a failed
                // request, so the error column only earns its width when the
                // cache column is switched off.
                DisplayField::Error => {
                    !v.shows(DisplayField::Cache)
                        && v.rows.iter().any(|row| error_code(row).is_some())
                }
                DisplayField::UpstreamModel => v.rows.iter().any(|row| row.served_model.is_some()),
                _ => true,
            };
            if keep {
                out.push(Column::Field(field));
            }
        }
        out
    }

    /// The cell's value, or `None` when the row has none for this column.
    fn value(self, row: &Row, now: i64) -> Option<String> {
        match self {
            Column::Status => Some(STATUS_DOT.into()),
            Column::Field(field) => match field {
                DisplayField::Model => Some(row.display_model()),
                DisplayField::UpstreamModel => row
                    .served_model
                    .as_deref()
                    .map(model::short_model_name)
                    .map(str::to_string),
                DisplayField::Protocol => protocol_text(row),
                DisplayField::Agent => text(row.agent.as_deref()),
                DisplayField::Provider => row.provider_label(),
                // Only reachable through an old config; the key now rides in
                // the provider cell.
                DisplayField::ProviderKey => text(row.provider_key.as_deref()),
                DisplayField::Tokens => {
                    (row.total_tokens() > 0).then(|| fmt::tokens_compact(row.total_tokens()))
                }
                // Errors stay compact even if the failed attempt used cache.
                DisplayField::Cache if error_code(row).is_some() => error_code(row),
                DisplayField::Cache => row.cache_hit_rate().map(|rate| format!("{rate:.2}%")),
                DisplayField::CacheWrite => (row.write_cached_tokens > 0)
                    .then(|| format!("写 {}", fmt::tokens_compact(row.write_cached_tokens))),
                DisplayField::Speed => row.tps().map(|tps| format!("{tps:.0} tps")),
                DisplayField::Retry => retry_text(row),
                DisplayField::Cost => row.total_cost.map(fmt::cost),
                DisplayField::Latency => fmt::latency(row.latency_ms, row.first_token_ms),
                DisplayField::Completion => completion_text(row),
                DisplayField::Source => source_text(row),
                DisplayField::Error => error_code(row),
                DisplayField::Account => text(Some(&row.account_name)),
                DisplayField::CreatedAt => match row.started_at.as_deref() {
                    Some(iso) if crate::time::parse_unix(iso).is_some() => {
                        Some(fmt::relative_time(Some(iso), now))
                    }
                    _ => None,
                },
            },
        }
    }

    fn text(self, row: &Row, now: i64) -> String {
        self.value(row, now).unwrap_or_default()
    }

    /// The colour the cell is drawn with. A cell with no value keeps the row's
    /// status colour, so a missing reading still reads as "not yet".
    fn color(
        self,
        row: &Row,
        v: &ListView,
        providers: &[(&str, u32)],
        models: &[(panel::ModelColorKey<'_>, u32)],
    ) -> u32 {
        if self.value(row, v.now).is_none() {
            return theme::status_color(row.status);
        }
        match self {
            Column::Status => theme::status_color(row.status),
            Column::Field(field) => match field {
                DisplayField::Model => models
                    .iter()
                    .find(|(key, _)| *key == panel::model_color_key(row))
                    .map(|(_, color)| *color)
                    .unwrap_or(theme::TEXT),
                DisplayField::UpstreamModel | DisplayField::Protocol => theme::TEXT_DIM,
                DisplayField::Agent => theme::TEXT_FAINT,
                DisplayField::Provider => providers
                    .iter()
                    .find(|(name, _)| Some(*name) == row.provider.as_deref())
                    .map(|(_, color)| *color)
                    .unwrap_or(theme::CYAN),
                DisplayField::ProviderKey => theme::TEXT_FAINT,
                DisplayField::Tokens => theme::TEXT_DIM,
                DisplayField::Cache if error_code(row).is_some() => theme::RED,
                DisplayField::Cache => match row.cache_hit_rate() {
                    Some(_) if row.cache_hit_is_low() => theme::RED,
                    Some(_) => theme::GREEN,
                    None => theme::RED,
                },
                DisplayField::CacheWrite | DisplayField::Speed => theme::TEXT_DIM,
                DisplayField::Retry => theme::RED,
                DisplayField::Cost => theme::YELLOW,
                DisplayField::Latency => latency_color(row),
                DisplayField::Completion => theme::TEXT_DIM,
                DisplayField::Source => theme::TEXT_FAINT,
                DisplayField::Error => theme::RED,
                DisplayField::Account => panel::account_color(v.accounts, &row.account_id),
                DisplayField::CreatedAt => theme::TEXT_FAINT,
            },
        }
    }
}

/// The whole latency cell takes one colour, keyed on the first-token reading:
/// that is the wait an operator feels. A request still in flight has no first
/// token yet, so its running total stands in.
fn latency_color(row: &Row) -> u32 {
    let ms = row.first_token_ms.or(row.latency_ms).unwrap_or(0);
    if ms >= LATENCY_CRITICAL_MS {
        theme::RED
    } else if ms >= LATENCY_SLOW_MS {
        theme::YELLOW
    } else {
        theme::TEXT_DIM
    }
}

fn has_retry(row: &Row) -> bool {
    row.tries > 1 || row.handed_on
}

fn retry_text(row: &Row) -> Option<String> {
    if row.handed_on {
        return Some("交".into());
    }
    (row.tries > 1).then(|| format!("{}×", row.tries - 1))
}

fn text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// `mess → chat`, or a single protocol name.
fn protocol_text(row: &Row) -> Option<String> {
    match row.protocol() {
        Protocol::Same(name) | Protocol::Single(name) => Some(name),
        Protocol::Converted { from, to } => Some(format!("{from} → {to}")),
        Protocol::Unknown => None,
    }
}

/// `出 1.2k 思 340`: completion tokens, with the reasoning share called out.
fn completion_text(row: &Row) -> Option<String> {
    if row.completion_tokens <= 0 {
        return None;
    }
    let mut label = format!("出 {}", fmt::tokens_compact(row.completion_tokens));
    if row.reasoning_tokens > 0 {
        label.push_str(&format!(
            " 思 {}",
            fmt::tokens_compact(row.reasoning_tokens)
        ));
    }
    Some(label)
}

/// `/v1/messages · host`: the client endpoint and the host it was sent to.
fn source_text(row: &Row) -> Option<String> {
    match (&row.endpoint, &row.host) {
        (Some(endpoint), Some(host)) => Some(format!("{endpoint} · {host}")),
        (Some(endpoint), None) => Some(endpoint.clone()),
        (None, Some(host)) => Some(host.clone()),
        (None, None) => None,
    }
}

/// Error messages belong in the popup; only the code affects table width.
fn error_code(row: &Row) -> Option<String> {
    row.status_code
        .filter(|code| {
            (100..600).contains(code) && (row.status == model::Status::Failed || row.live)
        })
        .map(|code| code.to_string())
}

/// Ellipsize only at UTF-8 character boundaries. Measure the entire candidate
/// because mixed Latin/CJK font runs have their own overhang allowances.
pub(crate) fn ellipsize(text: &str, max_width: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(text) <= max_width {
        return text.into();
    }
    let suffix = if text.ends_with(')') { "…)" } else { "…" };
    if measure(suffix) > max_width {
        return if measure("…") <= max_width {
            "…".into()
        } else {
            String::new()
        };
    }
    let boundaries: Vec<_> = text.char_indices().map(|(index, _)| index).collect();
    let (mut low, mut high) = (0, boundaries.len());
    while low < high {
        let mid = (low + high) / 2;
        let candidate = format!("{}{}", &text[..boundaries[mid]], suffix);
        if measure(&candidate) <= max_width {
            low = mid + 1;
        } else {
            high = mid;
        }
    }
    let end = low.checked_sub(1).map_or(0, |index| boundaries[index]);
    format!("{}{}", &text[..end], suffix)
}

/// Width of every column: widest cell plus padding, with a supplier cap.
/// Pure so the fit can be exercised without a device context.
fn column_widths(
    rows: &[Row],
    columns: &[Column],
    now: i64,
    scale: f32,
    measure: impl Fn(&str) -> f32,
) -> Vec<f32> {
    columns
        .iter()
        .map(|column| {
            let mut width = 0.0f32;
            for row in rows {
                let text = column.text(row, now);
                let mut cell = measure(&text);
                if *column == Column::Field(DisplayField::Model) && row.logo.is_some() {
                    // The mark and its gap are part of the cell's width, so the
                    // text keeps its own breathing room.
                    cell += (theme::ICON_SIZE + theme::ICON_GAP) * scale;
                }
                width = width.max(cell);
            }
            let padded = width + CELL_HPAD * scale * 2.0;
            if *column == Column::Field(DisplayField::Provider) {
                padded.min(PROVIDER_MAX_WIDTH * scale)
            } else {
                padded
            }
        })
        .collect()
}

/// Draw the table and report the client width that fits it, or `None` when
/// there is nothing to size.
pub(super) fn draw(
    p: &Painter,
    fonts: &theme::Fonts,
    m: Metrics,
    v: &ListView,
    logos: &LogoCache,
) -> Option<f32> {
    if v.rows.is_empty() {
        panel::draw_empty(p, fonts, m, v);
        return None;
    }

    let hpad = CELL_HPAD * m.scale;
    let columns = Column::all(v);
    let widths = column_widths(v.rows, &columns, v.now, m.scale, |text| {
        p.dual_measure(&fonts.small, text)
    });

    let providers = panel::provider_palette(v.rows, v.name_color_theme);
    let models = panel::model_palette(v.rows, v.name_color_theme);

    let area = m.rows_area();
    p.clip(RectF {
        X: 0.0,
        Y: area.y,
        Width: area.w,
        Height: area.h,
    });

    for index in 0..v.rows.len() {
        let rect = m.row_rect(index, v.scroll);
        if rect.y + rect.h < area.y || rect.y > area.y + area.h {
            continue;
        }
        let row = &v.rows[index];
        if v.hover == Some(index) || v.selected == Some(index) {
            p.fill_rect(rect.x, rect.y, rect.w, rect.h, theme::CARD_HOVER);
        }
        let mut x = rect.x;
        for (column, width) in columns.iter().zip(&widths) {
            let text = column.text(row, v.now);
            let text = if *column == Column::Field(DisplayField::Provider) {
                ellipsize(&text, (width - hpad * 2.0).max(0.0), |s| {
                    p.dual_measure(&fonts.small, s)
                })
            } else {
                text
            };
            let mut text_x = x + hpad;
            let mut text_w = width - hpad * 2.0;
            if *column == Column::Field(DisplayField::Model)
                && let Some(logo) = row.logo.as_ref()
            {
                // Whole device pixels for the square and the picture inside it:
                // a fractional offset makes the mark sit a pixel off centre.
                let side = (theme::ICON_SIZE * m.scale).round();
                let left = text_x.round();
                let top = (rect.y + (rect.h - side) / 2.0).round();
                if let Some(fill) = logo.bg {
                    p.round_rect(
                        left,
                        top,
                        side,
                        side,
                        (theme::ICON_RADIUS * m.scale).round(),
                        fill,
                        true,
                    );
                }
                if let Some(image) = logos.get(&logo.source) {
                    let inset = if logo.bg.is_some() {
                        (theme::ICON_INSET * m.scale).round()
                    } else {
                        0.0
                    };
                    p.draw_image_fit(
                        image,
                        left + inset,
                        top + inset,
                        side - inset * 2.0,
                        side - inset * 2.0,
                    );
                }
                let advance = (theme::ICON_SIZE + theme::ICON_GAP) * m.scale;
                text_x += advance;
                text_w -= advance;
            }
            p.dual_text(
                &fonts.small,
                &text,
                text_x,
                rect.y,
                (text_w + CELL_SLACK * m.scale).max(0.0),
                rect.h,
                column.color(row, v, &providers, &models),
                theme::ALIGN_NEAR,
            );
            x += width;
        }
        p.fill_rect(rect.x, rect.y + rect.h - 1.0, rect.w, 1.0, theme::SEPARATOR);
    }
    p.reset_clip();

    Some(m.pad() * 2.0 + widths.iter().sum::<f32>())
}

#[cfg(test)]
#[path = "../tests/ui/table.rs"]
mod tests;
