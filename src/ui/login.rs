//! In-panel connection form.
//!
//! Deliberately not a native dialog: a modal message loop would pump messages
//! meant for the panel and fight the always-on-top, no-activate window. The
//! form renders with the same primitives as the table and keeps all input
//! inside the panel's own message handling.

use crate::config::Account;
use crate::theme::{self, Painter};
use crate::ui::layout::{Metrics, Rect};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    /// Name the connection is listed under.
    Name,
    /// Where the gateway listens, e.g. `http://127.0.0.1:3425`.
    Endpoint,
    /// Gateway key. Only needed when the gateway is reached over the network;
    /// a loopback gateway answers without one.
    Key,
}

#[derive(Clone)]
pub struct LoginForm {
    /// Account management returns to the settings window instead of exiting.
    pub return_to_settings: bool,
    /// The connection being edited, or `None` when this is a new one.
    pub editing: Option<String>,
    pub name: String,
    pub endpoint: String,
    /// The gateway key as typed. Empty means "no key".
    pub key: String,
    pub focus: Field,
    /// Caret position, in characters, within the focused field.
    pub caret: usize,
    /// Where the current selection was dragged from, in characters. `None`
    /// when the caret sits alone; the pair is normalised wherever it is read.
    pub anchor: Option<usize>,
    pub error: Option<String>,
    pub busy: bool,
    /// Blink phase for the caret, flipped by the UI timer.
    pub caret_on: bool,
}

/// The byte offset of the `chars`-th character, saturating at the end.
fn byte_at(s: &str, chars: usize) -> usize {
    s.char_indices()
        .nth(chars)
        .map(|(offset, _)| offset)
        .unwrap_or(s.len())
}

impl LoginForm {
    /// A form for a new connection: the endpoint the panel already knows is
    /// filled in, and the name field waits for a label.
    pub fn adding(endpoint: &str) -> Self {
        LoginForm {
            return_to_settings: false,
            editing: None,
            name: String::new(),
            endpoint: endpoint.to_string(),
            key: String::new(),
            focus: Field::Name,
            caret: 0,
            anchor: None,
            error: None,
            busy: false,
            caret_on: true,
        }
    }

    /// A form for an existing connection: its name and endpoint are prefilled
    /// so re-saving only means touching the gateway key.
    pub fn for_account(account: &Account) -> Self {
        LoginForm {
            return_to_settings: false,
            editing: Some(account.id.clone()),
            name: account.name.clone(),
            endpoint: account.endpoint.clone(),
            key: String::new(),
            focus: Field::Key,
            caret: 0,
            anchor: None,
            error: None,
            busy: false,
            caret_on: true,
        }
    }

    /// The value a field holds.
    pub fn value(&self, field: Field) -> &str {
        match field {
            Field::Name => &self.name,
            Field::Endpoint => &self.endpoint,
            Field::Key => &self.key,
        }
    }

    fn value_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Name => &mut self.name,
            Field::Endpoint => &mut self.endpoint,
            Field::Key => &mut self.key,
        }
    }

    /// Whether the field shows its value as asterisks.
    pub fn masked(field: Field) -> bool {
        field == Field::Key
    }

    /// The value as drawn: the key becomes a run of asterisks.
    pub fn shown_value(&self, field: Field) -> String {
        let value = self.value(field);
        if Self::masked(field) {
            "*".repeat(value.chars().count())
        } else {
            value.to_string()
        }
    }

    /// Characters in the focused field.
    fn focused_len(&self) -> usize {
        self.value(self.focus).chars().count()
    }

    /// Move focus. Landing on a different field puts the caret at its end,
    /// which is where typing is expected to continue.
    pub fn focus_field(&mut self, field: Field) {
        if self.focus != field {
            self.focus = field;
            self.caret = self.focused_len();
            self.anchor = None;
        }
        self.caret_on = true;
    }

    /// The selection as an ordered (start, end) pair in characters, when the
    /// two ends are actually apart.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        let len = self.focused_len();
        let caret = self.caret.min(len);
        let anchor = anchor.min(len);
        (anchor != caret).then(|| (anchor.min(caret), anchor.max(caret)))
    }

    /// Place the caret, optionally dragging the selection anchor along with it
    /// (shift + arrows, or a mouse drag).
    pub fn set_caret(&mut self, index: usize, extend: bool) {
        let len = self.focused_len();
        let index = index.min(len);
        let previous = self.caret.min(len);
        if extend {
            self.anchor.get_or_insert(previous);
        } else {
            self.anchor = None;
        }
        self.caret = index;
        self.caret_on = true;
    }

    pub fn move_caret(&mut self, delta: isize, extend: bool) {
        let len = self.focused_len() as isize;
        let next = (self.caret as isize + delta).clamp(0, len) as usize;
        self.set_caret(next, extend);
    }

    /// Home (start = false) and End (start = true).
    pub fn caret_to_edge(&mut self, end: bool, extend: bool) {
        let len = self.focused_len();
        self.set_caret(if end { len } else { 0 }, extend);
    }

    /// Drop the caret where a mouse press landed and mark that spot as the
    /// start of a possible drag. A press that never moves leaves no selection
    /// behind, since the two ends coincide.
    pub fn press_caret(&mut self, index: usize) {
        self.set_caret(index, false);
        self.anchor = Some(self.caret);
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.focused_len();
        self.caret_on = true;
    }

    /// Forget a selection whose ends sit on the same spot; a click that never
    /// became a drag must not leave a zero-width anchor behind.
    pub fn collapse_selection(&mut self) {
        if self.selection().is_none() {
            self.anchor = None;
        }
    }

    /// The selected text, for the clipboard.
    pub fn selected_text(&self) -> Option<String> {
        let (lo, hi) = self.selection()?;
        let value = self.value(self.focus);
        Some(value[byte_at(value, lo)..byte_at(value, hi)].to_string())
    }

    /// Cut: remove the selection and return what was removed. `None` means
    /// there was no selection to take.
    pub fn take_selection(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.delete_selection();
        Some(text)
    }

    /// Delete the selected characters, leaving the caret where they started.
    /// Returns whether there was a selection to delete.
    fn delete_selection(&mut self) -> bool {
        let Some((lo, hi)) = self.selection() else {
            return false;
        };
        let field = self.focus;
        let text = self.value_mut(field);
        let (start, end) = (byte_at(text, lo), byte_at(text, hi));
        text.replace_range(start..end, "");
        self.caret = lo;
        self.anchor = None;
        self.caret_on = true;
        true
    }

    /// The endpoint as the worker should call it: trimmed, no trailing slash.
    pub fn trimmed_endpoint(&self) -> String {
        self.endpoint.trim().trim_end_matches('/').to_string()
    }

    /// The gateway key as stored: whitespace and a `Bearer ` prefix removed.
    /// An empty result means the connection needs no key.
    pub fn trimmed_key(&self) -> String {
        self.key
            .trim()
            .trim_start_matches("Bearer ")
            .trim()
            .to_string()
    }

    /// The name to save under: what the user typed, or the endpoint's host when
    /// the box was left empty.
    pub fn resolved_name(&self) -> String {
        let name = self.name.trim();
        if name.is_empty() {
            crate::config::endpoint_host(&self.trimmed_endpoint())
        } else {
            name.to_string()
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        let endpoint = self.trimmed_endpoint();
        if endpoint.is_empty() {
            return Err("请填写 Magpie 地址".into());
        }
        if !endpoint.starts_with("http://") && !endpoint.starts_with("https://") {
            return Err("地址需要以 http:// 或 https:// 开头".into());
        }
        Ok(())
    }

    /// The text field currently receiving keystrokes.
    fn focused_text(&mut self) -> &mut String {
        self.value_mut(self.focus)
    }

    /// Maximum characters the focused field accepts.
    fn char_limit(&self) -> usize {
        match self.focus {
            Field::Key => 512,
            Field::Name => 64,
            Field::Endpoint => 256,
        }
    }

    /// Whether `ch` is allowed in the currently focused text field.
    ///
    /// The endpoint is URL-shaped ASCII. The name and the key accept any
    /// printable Unicode (everything except control characters), so a Chinese
    /// label or an unusual key is not silently dropped.
    fn accepts(&self, ch: char) -> bool {
        match self.focus {
            Field::Name | Field::Key => !ch.is_control(),
            Field::Endpoint => ch.is_ascii_graphic() || ch == ' ',
        }
    }

    /// Type one character at the caret, replacing any selection. The caret
    /// advances with the text, the way an edit control behaves.
    pub fn insert(&mut self, ch: char) {
        if !self.accepts(ch) {
            return;
        }
        self.delete_selection();
        let limit = self.char_limit();
        let caret = self.caret.min(self.focused_len());
        let mut placed = false;
        let text = self.focused_text();
        if text.chars().count() < limit {
            let at = byte_at(text, caret);
            text.insert(at, ch);
            placed = true;
        }
        if placed {
            self.caret = caret + 1;
        }
        self.caret_on = true;
        self.error = None;
    }

    /// Insert a pasted string at the caret, replacing any selection, filtered
    /// to characters the field can hold.
    pub fn paste(&mut self, raw: &str) {
        let cleaned: String = raw.chars().filter(|c| self.accepts(*c)).collect();
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            return;
        }
        self.delete_selection();
        let limit = self.char_limit();
        let caret = self.caret.min(self.focused_len());
        let room = limit.saturating_sub(self.focused_len());
        let taken: String = cleaned.chars().take(room).collect();
        if taken.is_empty() {
            return;
        }
        let text = self.focused_text();
        let at = byte_at(text, caret);
        text.insert_str(at, &taken);
        self.caret = caret + taken.chars().count();
        self.caret_on = true;
        self.error = None;
    }

    /// Empty the focused field (Ctrl+X with nothing selected).
    pub fn clear_focused(&mut self) {
        self.focused_text().clear();
        self.caret = 0;
        self.anchor = None;
        self.caret_on = true;
        self.error = None;
    }

    /// Delete the selection, or the character before the caret when there is
    /// none.
    pub fn backspace(&mut self) {
        if self.delete_selection() {
            self.error = None;
            return;
        }
        let caret = self.caret.min(self.focused_len());
        if caret == 0 {
            return;
        }
        let text = self.focused_text();
        let (start, end) = (byte_at(text, caret - 1), byte_at(text, caret));
        text.replace_range(start..end, "");
        self.caret = caret - 1;
        self.caret_on = true;
        self.error = None;
    }

    /// Delete the selection, or the character under the caret when there is
    /// none.
    pub fn delete_forward(&mut self) {
        if self.delete_selection() {
            self.error = None;
            return;
        }
        let caret = self.caret.min(self.focused_len());
        if caret >= self.focused_len() {
            return;
        }
        let text = self.focused_text();
        let (start, end) = (byte_at(text, caret), byte_at(text, caret + 1));
        text.replace_range(start..end, "");
        self.caret_on = true;
        self.error = None;
    }

    pub fn focus_next(&mut self, backwards: bool) {
        const ORDER: [Field; 3] = [Field::Name, Field::Endpoint, Field::Key];
        let index = ORDER.iter().position(|f| *f == self.focus).unwrap_or(0);
        let len = ORDER.len();
        let next = if backwards {
            (index + len - 1) % len
        } else {
            (index + 1) % len
        };
        self.focus_field(ORDER[next]);
    }
}

/// Geometry of the form, recomputed per paint.
pub struct LoginLayout {
    pub card: Rect,
    /// (rect, label) for name, endpoint and gateway key.
    pub fields: Vec<(Rect, &'static str)>,
    pub button: Rect,
    pub error_y: f32,
}

/// Authored at 96 DPI; scaled by `Metrics::scale` in `layout`.
const FIELD_H: f32 = 34.0;
const LABEL_H: f32 = 18.0;
const FIELD_GAP: f32 = 14.0;
/// Title and subtitle, down to the first field label.
const HEAD_H: f32 = 56.0;
/// Everything below the last field: hint line, button and error line.
const TAIL_H: f32 = 116.0;
/// Breathing room kept above and below the card.
const CARD_MARGIN: f32 = 16.0;
const FIELD_COUNT: f32 = 3.0;

/// Height that fits the form with its margins, in device pixels.
pub fn window_height(scale: f32) -> i32 {
    let s = scale.max(0.5);
    let card_h = (HEAD_H + TAIL_H) + FIELD_COUNT * (LABEL_H + FIELD_H + FIELD_GAP);
    ((card_h + CARD_MARGIN * 2.0) * s).ceil() as i32
}

pub fn layout(_form: &LoginForm, m: Metrics) -> LoginLayout {
    let s = m.scale;
    let (field_h, label_h, field_gap) = (FIELD_H * s, LABEL_H * s, FIELD_GAP * s);

    let card_w = (m.width - m.pad() * 2.0).min(404.0 * s).max(280.0 * s);
    let card_x = (m.width - card_w) / 2.0;
    let inner_x = card_x + 18.0 * s;
    let inner_w = card_w - 36.0 * s;

    let card_h = (HEAD_H + TAIL_H) * s + FIELD_COUNT * (label_h + field_h + field_gap);
    let card_y = ((m.height - card_h) / 2.0).max(CARD_MARGIN * s);
    let card = Rect {
        x: card_x,
        y: card_y,
        w: card_w,
        h: card_h,
    };

    let mut y = card_y + HEAD_H * s;
    let mut fields = Vec::with_capacity(3);
    for label in [
        "名称  (可留空)",
        "Magpie 地址",
        "网关密钥  (本机访问可留空)",
    ] {
        fields.push((
            Rect {
                x: inner_x,
                y: y + label_h,
                w: inner_w,
                h: field_h,
            },
            label,
        ));
        y += label_h + field_h + field_gap;
    }

    let button = Rect {
        x: inner_x,
        y: y + 26.0 * s,
        w: inner_w,
        h: 36.0 * s,
    };

    LoginLayout {
        card,
        fields,
        button,
        error_y: y + 68.0 * s,
    }
}

/// One text row of the form: what it holds and where the caret sits.
struct FieldView<'a> {
    label: &'static str,
    value: &'a str,
    placeholder: &'static str,
    masked: bool,
    /// Caret position in characters; only drawn for the focused row.
    caret: usize,
    /// Normalised selection (start, end) in characters, when one is active.
    selection: Option<(usize, usize)>,
}

/// Where the value sits inside its box: the left edge of the text area and its
/// width, both in device pixels.
fn text_area(rect: Rect) -> (f32, f32) {
    let s = rect.h / FIELD_H;
    (rect.x + 10.0 * s, rect.w - 20.0 * s)
}

/// Pixel width of the first `chars` characters of `shown`.
fn width_up_to(p: &Painter, fonts: &theme::Fonts, shown: &str, chars: usize) -> f32 {
    p.dual_measure(&fonts.body, &shown[..byte_at(shown, chars)])
}

/// Space kept between the caret and the right edge of the box.
const CARET_MARGIN: f32 = 8.0;

/// How far the text is scrolled left so the caret stays in view. Long values
/// (a key can be wide) therefore show their tail, while the caret can still be
/// moved back to reveal the rest.
fn scroll_offset(p: &Painter, fonts: &theme::Fonts, shown: &str, caret: usize, text_w: f32) -> f32 {
    (width_up_to(p, fonts, shown, caret) + CARET_MARGIN - text_w).max(0.0)
}

/// The character boundary under `local_x`, given in pixels from the start of
/// the text. Binary search: a long key would make a linear scan of every
/// prefix far too expensive to run on a mouse move.
fn index_at_x(p: &Painter, fonts: &theme::Fonts, shown: &str, local_x: f32) -> usize {
    let total = shown.chars().count();
    let (mut lo, mut hi) = (0usize, total + 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if width_up_to(p, fonts, shown, mid) <= local_x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

/// Draw a rounded text input, with its selection and caret when focused.
fn text_field(
    p: &Painter,
    fonts: &theme::Fonts,
    rect: Rect,
    row: &FieldView,
    focused: bool,
    caret_on: bool,
) {
    p.dual_text(
        &fonts.small,
        row.label,
        rect.x,
        rect.y - rect.h * (LABEL_H / FIELD_H),
        rect.w,
        rect.h * (LABEL_H / FIELD_H),
        if focused {
            theme::MAUVE
        } else {
            theme::TEXT_DIM
        },
        theme::ALIGN_NEAR,
    );

    let s = rect.h / FIELD_H;
    p.round_rect(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        6.0 * s,
        theme::INPUT_BG,
        true,
    );
    p.round_rect(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        6.0 * s,
        if focused {
            theme::MAUVE
        } else {
            theme::INPUT_BORDER
        },
        false,
    );

    let (text_x, text_w) = text_area(rect);
    let shown = if row.masked {
        "*".repeat(row.value.chars().count())
    } else {
        row.value.to_string()
    };
    let caret = row.caret.min(shown.chars().count());
    // An unfocused row always draws from the start; the focused one scrolls
    // just enough to keep the caret (and the text being typed) in view.
    let offset = if focused {
        scroll_offset(p, fonts, &shown, caret, text_w)
    } else {
        0.0
    };

    // Everything inside the field is clipped to it. The value may be wider
    // than the box and is drawn from a scrolled origin, so without the clip it
    // would spill out of the card.
    p.clip(theme::RectF {
        X: rect.x + 1.0,
        Y: rect.y,
        Width: rect.w - 2.0,
        Height: rect.h,
    });

    if shown.is_empty() {
        p.dual_text(
            &fonts.body,
            row.placeholder,
            text_x,
            rect.y,
            text_w,
            rect.h,
            theme::TEXT_FAINT,
            theme::ALIGN_NEAR,
        );
    } else {
        let origin = text_x - offset;
        if let Some((lo, hi)) = row.selection {
            let x1 = origin + width_up_to(p, fonts, &shown, lo);
            let x2 = origin + width_up_to(p, fonts, &shown, hi);
            p.fill_rect(
                x1,
                rect.y + 5.0 * s,
                (x2 - x1).max(1.0),
                rect.h - 10.0 * s,
                theme::SELECTION,
            );
        }
        // The full measured width (plus slack for `MeasureString` rounding
        // under what `DrawString` needs) so nothing is trimmed into an
        // ellipsis; the clip above cuts whatever runs past the box.
        let full = p.dual_measure(&fonts.body, &shown) + 4.0;
        p.dual_text(
            &fonts.body,
            &shown,
            origin,
            rect.y,
            full,
            rect.h,
            theme::TEXT,
            theme::ALIGN_NEAR,
        );
    }

    if focused && caret_on {
        let caret_x =
            (text_x - offset + width_up_to(p, fonts, &shown, caret)).min(rect.x + rect.w - 6.0 * s);
        p.fill_rect(
            caret_x,
            rect.y + 7.0 * s,
            1.5 * s,
            rect.h - 14.0 * s,
            theme::MAUVE,
        );
    }

    p.reset_clip();
}

/// Move the caret to the character under a mouse position. `press` starts a
/// new selection anchor there (button down); otherwise the existing anchor is
/// kept and the selection grows towards the pointer (drag). Returns whether
/// anything about the form changed, so a drag that stays within one character
/// does not repaint.
pub fn place_caret(
    p: &Painter,
    fonts: &theme::Fonts,
    form: &mut LoginForm,
    m: Metrics,
    x: f32,
    y: f32,
    press: bool,
) -> bool {
    const FIELDS: [Field; 3] = [Field::Name, Field::Endpoint, Field::Key];
    let l = layout(form, m);
    let Some((field, rect)) = l
        .fields
        .iter()
        .enumerate()
        .find_map(|(i, (rect, _))| rect.contains(x, y).then_some((FIELDS[i], *rect)))
    else {
        return false;
    };
    let before = (form.focus, form.caret, form.anchor);
    // The origin the row was last painted with: an unfocused row draws from
    // its start, the focused one is scrolled to keep its caret in view.
    let shown = form.shown_value(field);
    let (text_x, text_w) = text_area(rect);
    let offset = if form.focus == field {
        scroll_offset(p, fonts, &shown, form.caret, text_w)
    } else {
        0.0
    };
    form.focus_field(field);
    let index = index_at_x(p, fonts, &shown, x - (text_x - offset));
    if press {
        form.press_caret(index);
    } else {
        form.set_caret(index, true);
    }
    before != (form.focus, form.caret, form.anchor)
}

/// The placeholder a field shows while empty.
fn placeholder_for(field: Field) -> &'static str {
    match field {
        Field::Name => "例如:本机、公司",
        Field::Endpoint => crate::config::DEFAULT_ENDPOINT,
        Field::Key => "本机网关无需填写",
    }
}

/// Whether a point is over one of the text rows. Answered from the rectangles
/// alone, which is what lets the window show an I-beam without a painter.
pub fn hit_text_field(form: &LoginForm, m: Metrics, x: f32, y: f32) -> bool {
    layout(form, m)
        .fields
        .iter()
        .any(|(rect, _)| rect.contains(x, y))
}

pub fn draw(p: &Painter, fonts: &theme::Fonts, form: &LoginForm, m: Metrics) {
    const FIELDS: [Field; 3] = [Field::Name, Field::Endpoint, Field::Key];
    let l = layout(form, m);
    let s = m.scale;
    let inset = 18.0 * s;

    p.fill_rect(0.0, 0.0, m.width, m.height, theme::BG);
    p.round_rect(
        l.card.x,
        l.card.y,
        l.card.w,
        l.card.h,
        10.0 * s,
        theme::CARD,
        true,
    );

    p.dual_text(
        &fonts.bold,
        if form.editing.is_some() {
            "编辑 Magpie 连接"
        } else {
            "添加 Magpie 连接"
        },
        l.card.x + inset,
        l.card.y + 12.0 * s,
        l.card.w - inset * 2.0,
        22.0 * s,
        theme::TEXT,
        theme::ALIGN_NEAR,
    );
    p.dual_text(
        &fonts.small,
        "可添加多个网关,请求合并显示;默认地址 http://127.0.0.1:3425",
        l.card.x + inset,
        l.card.y + 34.0 * s,
        l.card.w - inset * 2.0,
        18.0 * s,
        theme::TEXT_FAINT,
        theme::ALIGN_NEAR,
    );

    if form.return_to_settings {
        let r = back_button(form, m);
        p.dual_text(
            &fonts.small,
            "返回设置",
            r.x,
            r.y,
            r.w,
            r.h,
            theme::MAUVE,
            theme::ALIGN_FAR,
        );
    }

    for (i, (rect, label)) in l.fields.iter().enumerate() {
        let field = FIELDS[i];
        let focused = form.focus == field;
        let row = FieldView {
            label,
            value: form.value(field),
            placeholder: placeholder_for(field),
            masked: LoginForm::masked(field),
            caret: form.caret,
            // A selection only ever belongs to the focused row.
            selection: if focused { form.selection() } else { None },
        };
        text_field(p, fonts, *rect, &row, focused, form.caret_on);
    }

    p.dual_text(
        &fonts.small,
        "密钥只在本机加密保存,远程访问网关时才需要",
        l.button.x,
        l.button.y - 22.0 * s,
        l.button.w,
        16.0 * s,
        theme::TEXT_FAINT,
        theme::ALIGN_NEAR,
    );

    let enabled = !form.busy;
    p.round_rect(
        l.button.x,
        l.button.y,
        l.button.w,
        l.button.h,
        7.0 * s,
        if enabled { 0xFF2E3644 } else { 0xFF23272E },
        true,
    );
    p.dual_text(
        &fonts.bold,
        if form.busy { "保存中…" } else { "保存" },
        l.button.x,
        l.button.y,
        l.button.w,
        l.button.h,
        if enabled {
            theme::TEXT
        } else {
            theme::TEXT_FAINT
        },
        theme::ALIGN_CENTER,
    );

    if let Some(err) = &form.error {
        p.dual_text(
            &fonts.small,
            err,
            l.card.x + inset,
            l.error_y,
            l.card.w - inset * 2.0,
            20.0 * s,
            theme::RED,
            theme::ALIGN_NEAR,
        );
    }
}

/// Map a click to a form control.
pub fn hit(form: &LoginForm, m: Metrics, x: f32, y: f32) -> Option<Action> {
    const FIELDS: [Field; 3] = [Field::Name, Field::Endpoint, Field::Key];
    if form.return_to_settings && back_button(form, m).contains(x, y) {
        return Some(Action::Back);
    }
    let l = layout(form, m);
    if l.button.contains(x, y) {
        return Some(Action::Submit);
    }
    for (index, (rect, _)) in l.fields.iter().enumerate() {
        if rect.contains(x, y) {
            return Some(Action::Focus(FIELDS[index]));
        }
    }
    None
}

fn back_button(form: &LoginForm, m: Metrics) -> Rect {
    let card = layout(form, m).card;
    Rect {
        x: card.x + card.w - 104.0 * m.scale,
        y: card.y + 8.0 * m.scale,
        w: 86.0 * m.scale,
        h: 30.0 * m.scale,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Back,
    Submit,
    Focus(Field),
}

#[cfg(test)]
#[path = "../tests/ui/login.rs"]
mod tests;
