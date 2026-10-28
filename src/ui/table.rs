//! Table layout: one row per request, one column per visible cell.
//!
//! Every column is sized to its widest text, so nothing is truncated, and the
//! width the table needs is reported back to the caller, which widens the
//! window to match.

use windows::Win32::Graphics::GdiPlus::RectF;

use crate::config::DisplayField;
use crate::format as fmt;
use crate::model::{self, Protocol, Row};
use crate::theme::{self, Painter};
use crate::ui::layout::Metrics;
use crate::ui::panel::{self, ListView};

/// Horizontal padding inside a cell, on each side.
const CELL_HPAD: f32 = 1.0;

/// Headroom added to the rectangle a cell is drawn in: GDI+ trims text the
/// moment the layout rectangle is narrower than the measured text.
const CELL_SLACK: f32 = 0.5;

/// The status cell is a dot, not a word.
const STATUS_DOT: &str = "●";

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
        for field in DisplayField::ALL {
            if !v.shows(field) {
                continue;
            }
            let keep = match field {
                DisplayField::Retry => v.rows.iter().any(has_retry),
                DisplayField::Error => v.rows.iter().any(|row| error_code(row).is_some()),
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
                DisplayField::Provider => row.provider_with_key(),
                // Only reachable through an old config; the key now rides in
                // the provider cell.
                DisplayField::ProviderKey => text(row.provider_key.as_deref()),
                DisplayField::Tokens => {
                    (row.total_tokens() > 0).then(|| fmt::tokens_compact(row.total_tokens()))
                }
                // Errors stay compact even if the failed attempt used cache.
                DisplayField::Cache if row.status == model::Status::Failed => error_code(row),
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
        models: &[(&str, u32)],
    ) -> u32 {
        if self.value(row, v.now).is_none() {
            return theme::status_color(row.status);
        }
        match self {
            Column::Status => theme::status_color(row.status),
            Column::Field(field) => match field {
                DisplayField::Model => models
                    .iter()
                    .find(|(name, _)| *name == row.served_model())
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
                DisplayField::Cache if row.status == model::Status::Failed => theme::RED,
                DisplayField::Cache => match row.cache_hit_rate() {
                    Some(_) if row.cache_hit_is_low() => theme::RED,
                    Some(_) => theme::GREEN,
                    None => theme::RED,
                },
                DisplayField::CacheWrite | DisplayField::Speed => theme::TEXT_DIM,
                DisplayField::Retry => theme::RED,
                DisplayField::Cost => theme::YELLOW,
                DisplayField::Latency | DisplayField::Completion => theme::TEXT_DIM,
                DisplayField::Source => theme::TEXT_FAINT,
                DisplayField::Error => theme::RED,
                DisplayField::Account => panel::account_color(v.accounts, &row.account_id),
                DisplayField::CreatedAt => theme::TEXT_FAINT,
            },
        }
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
        .filter(|code| (100..600).contains(code) && row.status == model::Status::Failed)
        .map(|code| code.to_string())
}

/// Width of every column: the widest cell under it, plus the padding on both
/// sides. Pure so the fit can be exercised without a device context.
fn column_widths(
    rows: &[Row],
    columns: &[Column],
    now: i64,
    hpad: f32,
    measure: impl Fn(&str) -> f32,
) -> Vec<f32> {
    columns
        .iter()
        .map(|column| {
            let mut width = 0.0f32;
            for row in rows {
                width = width.max(measure(&column.text(row, now)));
            }
            width + hpad * 2.0
        })
        .collect()
}

/// Draw the table and report the client width that fits it, or `None` when
/// there is nothing to size.
pub(super) fn draw(p: &Painter, fonts: &theme::Fonts, m: Metrics, v: &ListView) -> Option<f32> {
    if v.rows.is_empty() {
        panel::draw_empty(p, fonts, m, v);
        return None;
    }

    let hpad = CELL_HPAD * m.scale;
    let columns = Column::all(v);
    let widths = column_widths(v.rows, &columns, v.now, hpad, |text| {
        p.dual_measure(&fonts.small, text)
    });

    let providers = panel::provider_palette(v.rows);
    let models = panel::model_palette(v.rows);

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
            p.dual_text(
                &fonts.small,
                &column.text(row, v.now),
                x + hpad,
                rect.y,
                width - hpad * 2.0 + CELL_SLACK * m.scale,
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
