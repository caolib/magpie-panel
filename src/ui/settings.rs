//! Settings content and geometry, shared by painting, pointer and keyboard input.

use crate::app::{App, ProbeState};
use crate::config::{AliasKind, DisplayField, HiddenProvider, OPACITY_MAX, OPACITY_MIN};
use crate::font_catalog::InstalledFont;
use crate::theme::{self, Fonts, Painter};
use crate::ui::layout::Rect;

pub const WIDTH: f32 = 560.0;
pub const HEIGHT: f32 = 650.0;
pub const MIN_WIDTH: f32 = 460.0;
pub const MIN_HEIGHT: f32 = 420.0;
const TOP: f32 = 112.0;
const FOOTER: f32 = 70.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Appearance,
    Fields,
    Accounts,
    Providers,
    Mappings,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Tab(Tab),
    Field(DisplayField),
    ShowAllFields,
    /// The opacity cell, covered by a native edit box that types the number.
    OpacityInput,
    /// Apply what the opacity box holds; unparsable input reverts.
    SaveOpacity,
    FontFamily(String),
    FontSearch,
    AddAccount,
    Login(String),
    /// Ask the worker to prove one connection still answers.
    TestAccount(String),
    RemoveAccount(String),
    /// Forget every stored gateway key, keeping the connections themselves.
    ClearKeys,
    /// Hide or show one (connection, provider) pair.
    Provider(HiddenProvider),
    ClearProviders,
    /// Switch which display name the mapping form edits.
    AliasKind(AliasKind),
    /// The match cell, covered by a native edit box.
    AliasFromInput,
    /// The short form cell, also a native edit box.
    AliasToInput,
    /// Take both boxes' text and add (or update) the mapping of the active kind.
    AddAlias,
    /// Drop the mapping at this index of one kind.
    RemoveAlias(AliasKind, usize),
    Close,
    Confirm,
    Cancel,
}

#[derive(Default)]
pub struct State {
    pub tab: Tab,
    /// Logical pixels, independent of the panel's font-size setting.
    pub scroll: f32,
    pub hover: Option<Action>,
    pub focus: Option<Action>,
    pub pressed: Option<Action>,
    pub pending: Option<Action>,
    pub font_names: Vec<InstalledFont>,
    pub font_matches: Vec<usize>,
    pub font_query: String,
    /// Which alias list the mapping form edits.
    pub alias_kind: AliasKind,
}

impl State {
    pub fn font_focused(&self) -> bool {
        self.tab == Tab::Appearance
            && self.pending.is_none()
            && matches!(self.focus, Some(Action::FontSearch | Action::FontFamily(_)))
    }

    pub fn set_font_query(&mut self, query: String) {
        let lower = query.to_lowercase();
        self.font_matches = self
            .font_names
            .iter()
            .enumerate()
            .filter_map(|(i, font)| font.matches(&lower).then_some(i))
            .collect();
        self.font_query = query;
        self.scroll = 0.0;
        self.hover = None;
        self.pressed = None;
    }
}

pub struct Control {
    pub rect: Rect,
    pub label: String,
    pub action: Action,
    pub selected: bool,
    pub danger: bool,
    /// Content controls scroll; header and footer controls stay in place.
    pub body: bool,
    /// A checkbox row: label left-aligned, state carried by the ☑/☐ prefix.
    pub check: bool,
}

pub struct Layout {
    pub width: f32,
    pub height: f32,
    pub controls: Vec<Control>,
    labels: Vec<Label>,
    pub content_height: f32,
    content_top: f32,
}

struct Label {
    rect: Rect,
    text: String,
    heading: bool,
    /// Overrides the default text colour, for labels that carry a verdict
    /// (green when a connection answers, red when it does not).
    color: Option<u32>,
}

impl Layout {
    pub fn new(app: &App, state: &State, width: f32, height: f32) -> Self {
        let mut l = Self {
            width,
            height,
            controls: Vec::new(),
            labels: Vec::new(),
            content_height: 0.0,
            content_top: TOP,
        };
        let tab_w = (width - 56.0) / 5.0;
        for (i, (tab, label)) in [
            (Tab::Appearance, "外观".to_string()),
            (Tab::Fields, "属性显示".to_string()),
            (Tab::Accounts, format!("连接 · {}", app.accounts.len())),
            (Tab::Providers, "供应商过滤".to_string()),
            (Tab::Mappings, "名称映射".to_string()),
        ]
        .into_iter()
        .enumerate()
        {
            l.button(
                Rect {
                    x: 20.0 + i as f32 * (tab_w + 4.0),
                    y: 66.0,
                    w: tab_w,
                    h: 32.0,
                },
                label,
                Action::Tab(tab),
                state.tab == tab,
                false,
                false,
            );
        }
        match state.tab {
            Tab::Appearance => l.appearance(app, state),
            Tab::Fields => l.fields(app),
            Tab::Accounts => l.accounts(app),
            Tab::Providers => l.providers(app),
            Tab::Mappings => l.mappings(app, state),
        }
        let y = height - 40.0;
        if state.pending.is_some() {
            l.button(
                Rect {
                    x: width - 194.0,
                    y,
                    w: 82.0,
                    h: 28.0,
                },
                "取消",
                Action::Cancel,
                false,
                false,
                false,
            );
            l.button(
                Rect {
                    x: width - 104.0,
                    y,
                    w: 84.0,
                    h: 28.0,
                },
                "确认操作",
                Action::Confirm,
                false,
                true,
                false,
            );
        } else {
            l.button(
                Rect {
                    x: width - 104.0,
                    y,
                    w: 84.0,
                    h: 28.0,
                },
                "完成",
                Action::Close,
                true,
                false,
                false,
            );
        }
        l
    }

    fn button(
        &mut self,
        rect: Rect,
        label: impl Into<String>,
        action: Action,
        selected: bool,
        danger: bool,
        body: bool,
    ) {
        self.controls.push(Control {
            rect,
            label: label.into(),
            action,
            selected,
            danger,
            body,
            check: false,
        });
    }

    /// A two-column checkbox row: `checked` means the cell is shown.
    fn checkbox(&mut self, rect: Rect, label: impl Into<String>, action: Action, checked: bool) {
        self.controls.push(Control {
            rect,
            label: format!("{}  {}", if checked { "☑" } else { "☐" }, label.into()),
            action,
            selected: checked,
            danger: false,
            body: true,
            check: true,
        });
    }

    /// Lay checkbox rows out in `columns` columns.
    fn check_grid(&mut self, y: f32, rows: Vec<(String, Action, bool)>, columns: usize) {
        let w = (self.width - 48.0 - (columns - 1) as f32 * 6.0) / columns as f32;
        for (i, (label, action, checked)) in rows.into_iter().enumerate() {
            self.checkbox(
                Rect {
                    x: 24.0 + (i % columns) as f32 * (w + 6.0),
                    y: y + (i / columns) as f32 * 36.0,
                    w,
                    h: 30.0,
                },
                label,
                action,
                checked,
            );
        }
    }

    fn label(&mut self, y: f32, text: impl Into<String>, heading: bool) {
        self.label_colored(y, text, heading, None);
    }

    fn label_colored(
        &mut self,
        y: f32,
        text: impl Into<String>,
        heading: bool,
        color: Option<u32>,
    ) {
        self.labels.push(Label {
            rect: Rect {
                x: 24.0,
                y,
                w: self.width - 48.0,
                h: 24.0,
            },
            text: text.into(),
            heading,
            color,
        });
    }

    fn appearance(&mut self, app: &App, state: &State) {
        self.label(
            0.0,
            format!("面板透明度 · 当前 {}%", app.config.opacity),
            true,
        );
        // This cell is a native edit box; the control below only carries its
        // geometry so painting and Tab order agree.
        self.controls.push(Control {
            rect: Rect {
                x: 24.0,
                y: 32.0,
                w: 120.0,
                h: 30.0,
            },
            label: String::new(),
            action: Action::OpacityInput,
            selected: false,
            danger: false,
            body: true,
            check: false,
        });
        self.button(
            Rect {
                x: 152.0,
                y: 32.0,
                w: 80.0,
                h: 30.0,
            },
            "保存",
            Action::SaveOpacity,
            false,
            false,
            true,
        );
        self.label(
            70.0,
            format!("输入 {OPACITY_MIN}–{OPACITY_MAX} 之间的数值(可含小数),点保存生效"),
            false,
        );
        self.fonts(app, state, 106.0);
    }

    fn fields(&mut self, app: &App) {
        let hidden = app.config.hidden_fields.len();
        self.label(0.0, format!("属性显示 · 已隐藏 {hidden} 项"), true);
        self.label(
            26.0,
            "勾选显示对应列，取消勾选隐藏；首次默认显示常用字段",
            false,
        );
        self.button(
            Rect {
                x: 24.0,
                y: 62.0,
                w: 144.0,
                h: 30.0,
            },
            "全部显示",
            Action::ShowAllFields,
            false,
            false,
            true,
        );
        let rows = DisplayField::ALL
            .into_iter()
            .map(|field| {
                (
                    field.label().to_string(),
                    Action::Field(field),
                    app.config.shows(field),
                )
            })
            .collect();
        self.check_grid(108.0, rows, 2);
        let grid_rows = DisplayField::ALL.len().div_ceil(2);
        self.content_height = 108.0 + grid_rows as f32 * 36.0 + 12.0;
    }

    fn accounts(&mut self, app: &App) {
        self.label(0.0, "Magpie 连接", true);
        self.label(26.0, "多个网关的请求会合并显示", false);
        self.button(
            Rect {
                x: self.width - 120.0,
                y: 0.0,
                w: 96.0,
                h: 30.0,
            },
            "添加连接",
            Action::AddAccount,
            false,
            false,
            true,
        );
        let mut y = 68.0;
        if app.accounts.is_empty() {
            self.label(y, "暂无连接，点击「添加连接」连接 Magpie", false);
            y += 44.0;
        }
        for account in &app.accounts {
            self.label(y, &account.name, true);
            self.label(y + 22.0, &account.endpoint, false);
            // A test result, while it is the freshest thing known about the
            // connection, stands in for the poll's own verdict.
            let probe = app.probe.as_ref().filter(|p| p.id == account.id);
            let (status, status_color) = match probe.map(|p| &p.state) {
                Some(ProbeState::Running) => ("正在测试连接…".to_string(), None),
                Some(ProbeState::Ok(total)) => {
                    (format!("连接正常 · 共 {total} 条请求"), Some(theme::GREEN))
                }
                Some(ProbeState::Failed(message)) => {
                    (format!("测试失败 · {message}"), Some(theme::RED))
                }
                None => (
                    app.account_state(&account.id)
                        .unwrap_or_else(|| {
                            if app.open_accounts.contains(&account.id) {
                                "已连接"
                            } else {
                                "等待连接"
                            }
                        })
                        .to_string(),
                    None,
                ),
            };
            let has_key = app.stored.key_for(&account.id).is_some();
            let detail = format!(
                "{status} · {}",
                if has_key {
                    "已保存密钥"
                } else {
                    "本机免密钥"
                }
            );
            self.label_colored(y + 44.0, detail, false, status_color);
            for label in self.labels.iter_mut().rev().take(3) {
                label.rect.w -= 208.0;
            }
            self.button(
                Rect {
                    x: self.width - 224.0,
                    y,
                    w: 96.0,
                    h: 30.0,
                },
                "测试连接",
                Action::TestAccount(account.id.clone()),
                false,
                false,
                true,
            );
            self.button(
                Rect {
                    x: self.width - 120.0,
                    y,
                    w: 96.0,
                    h: 30.0,
                },
                "编辑",
                Action::Login(account.id.clone()),
                false,
                false,
                true,
            );
            self.button(
                Rect {
                    x: self.width - 120.0,
                    y: y + 38.0,
                    w: 96.0,
                    h: 28.0,
                },
                "删除连接",
                Action::RemoveAccount(account.id.clone()),
                false,
                true,
                true,
            );
            y += 92.0;
        }
        self.label(y, "网关密钥", true);
        self.label(
            y + 24.0,
            "密钥由 Windows 加密保存；本机网关不需要密钥",
            false,
        );
        self.button(
            Rect {
                x: 24.0,
                y: y + 56.0,
                w: 170.0,
                h: 32.0,
            },
            "清除保存的密钥",
            Action::ClearKeys,
            false,
            true,
            true,
        );
        self.content_height = y + 108.0;
    }

    fn fonts(&mut self, app: &App, state: &State, top: f32) {
        let current = match (
            app.config.font_family.is_empty(),
            app.config.cjk_font_family.is_empty(),
        ) {
            (true, true) => "自动选择默认字体".to_string(),
            (false, true) => app.config.font_family.clone(),
            (true, false) => format!("默认 · {}", app.config.cjk_font_family),
            (false, false) => format!(
                "{} · {}",
                app.config.font_family, app.config.cjk_font_family
            ),
        };
        self.label(top, format!("当前字体 · {current}"), true);
        self.label(
            top + 24.0,
            "搜索本地字体（Ctrl+F），支持中文和英文名称",
            false,
        );
        self.button(
            Rect {
                x: 24.0,
                y: top + 48.0,
                w: self.width - 48.0,
                h: 32.0,
            },
            "",
            Action::FontSearch,
            false,
            false,
            true,
        );
        self.label(
            top + 84.0,
            format!(
                "匹配 {} / {} 款字体 · 点击或按 Enter 应用",
                state.font_matches.len(),
                state.font_names.len()
            ),
            false,
        );
        self.button(
            Rect {
                x: 24.0,
                y: top + 112.0,
                w: self.width - 48.0,
                h: 32.0,
            },
            "使用默认字体",
            Action::FontFamily(String::new()),
            app.config.font_family.is_empty(),
            false,
            true,
        );
        let mut y = top + 154.0;
        for &index in &state.font_matches {
            let font = &state.font_names[index];
            self.button(
                Rect {
                    x: 24.0,
                    y,
                    w: self.width - 48.0,
                    h: 32.0,
                },
                &font.name,
                Action::FontFamily(font.name.clone()),
                app.config.font_family == font.name,
                false,
                true,
            );
            y += 38.0;
        }
        if state.font_matches.is_empty() {
            self.label(y, "没有匹配的本地字体，请换一个名称搜索", false);
            y += 32.0;
        }
        self.content_height = y + 12.0;
    }

    /// Screen-space cell of a native edit box, given the body scroll offset.
    /// A body cell follows the content top and the scroll, and stays hidden
    /// while it is not fully inside the viewport — a child window cannot be
    /// clipped to the body the way painting is.
    pub fn edit_box_rect(&self, action: &Action, scroll: f32) -> Option<Rect> {
        let control = self.controls.iter().find(|c| c.action == *action)?;
        let rect = self.screen_rect(control, scroll);
        let visible = !control.body
            || (rect.y >= self.content_top && rect.y + rect.h <= self.height - FOOTER);
        visible.then_some(rect)
    }

    fn providers(&mut self, app: &App) {
        self.label(
            0.0,
            format!(
                "供应商过滤 · 已隐藏 {} 项",
                app.config.hidden_providers.len()
            ),
            true,
        );
        self.label(
            26.0,
            "选中后隐藏该连接下此供应商的请求；按连接分别记录",
            false,
        );
        self.button(
            Rect {
                x: 24.0,
                y: 62.0,
                w: 144.0,
                h: 30.0,
            },
            "清除供应商过滤",
            Action::ClearProviders,
            false,
            false,
            true,
        );
        let filters = app.provider_filters();
        if filters.is_empty() {
            self.label(112.0, "暂无可过滤供应商，连接网关并刷新请求后会显示", false);
        }
        let mut y = 108.0;
        for entry in filters {
            let name = app
                .config
                .account(&entry.account_id)
                .map(|a| a.name.as_str())
                .unwrap_or(&entry.account_id);
            let hidden = app
                .config
                .hides_provider(&entry.account_id, &entry.provider);
            self.button(
                Rect {
                    x: 24.0,
                    y,
                    w: self.width - 48.0,
                    h: 36.0,
                },
                format!(
                    "{}  {name} · {}",
                    if hidden { "☑" } else { "☐" },
                    entry.provider
                ),
                Action::Provider(entry),
                hidden,
                false,
                true,
            );
            y += 44.0;
        }
        self.content_height = (y + 16.0).max(160.0);
    }

    /// The display-name mapping tab: one form, three name kinds (model,
    /// provider, caller). The form boxes are native edit controls placed by
    /// `sync_edit_boxes`, the same way the opacity field works.
    fn mappings(&mut self, app: &App, state: &State) {
        self.label(
            0.0,
            format!(
                "名称映射 · 模型 {} · 供应商 {} · 发起方 {}",
                app.config.model_aliases.len(),
                app.config.provider_aliases.len(),
                app.config.agent_aliases.len()
            ),
            true,
        );
        self.label(
            26.0,
            "匹配不区分大小写，逐条全部替换；简写留空即删除匹配",
            false,
        );
        // Kind selector; each button carries its own count so the empty lists
        // are visible without switching.
        let gap = 8.0;
        let w = (self.width - 48.0 - gap * 2.0) / 3.0;
        for (index, kind) in AliasKind::ALL.into_iter().enumerate() {
            self.button(
                Rect {
                    x: 24.0 + index as f32 * (w + gap),
                    y: 56.0,
                    w,
                    h: 30.0,
                },
                format!("{} · {}", kind.label(), app.config.aliases(kind).len()),
                Action::AliasKind(kind),
                state.alias_kind == kind,
                false,
                true,
            );
        }
        let kind = state.alias_kind;
        let hint = match kind {
            AliasKind::Model => "作用于请求模型、实际模型和声明模型，如 deepseek → ds、flash → f",
            AliasKind::Provider => "作用于表格与详情里的供应商名，如 Anthropic → A社",
            AliasKind::Agent => "作用于表格与详情里的发起方（应用）名，如 Claude Code → CC",
        };
        self.label(94.0, hint, false);
        // The two native edit cells; the controls only carry geometry so
        // painting and Tab order agree.
        self.controls.push(Control {
            rect: Rect {
                x: 24.0,
                y: 122.0,
                w: 170.0,
                h: 30.0,
            },
            label: String::new(),
            action: Action::AliasFromInput,
            selected: false,
            danger: false,
            body: true,
            check: false,
        });
        self.controls.push(Control {
            rect: Rect {
                x: 202.0,
                y: 122.0,
                w: 130.0,
                h: 30.0,
            },
            label: String::new(),
            action: Action::AliasToInput,
            selected: false,
            danger: false,
            body: true,
            check: false,
        });
        self.button(
            Rect {
                x: self.width - 114.0,
                y: 122.0,
                w: 90.0,
                h: 30.0,
            },
            "添加",
            Action::AddAlias,
            false,
            false,
            true,
        );
        let aliases = app.config.aliases(kind);
        let mut y = 168.0;
        if aliases.is_empty() {
            self.label(
                y,
                format!(
                    "暂无{}映射；输入后点「添加」，重复添加会更新简写",
                    kind.label()
                ),
                false,
            );
            y += 38.0;
        }
        for (index, alias) in aliases.iter().enumerate() {
            let to = if alias.to.is_empty() {
                "空"
            } else {
                alias.to.as_str()
            };
            self.label(y + 5.0, format!("{} → {}", alias.from, to), false);
            if let Some(label) = self.labels.last_mut() {
                label.rect.w -= 110.0;
            }
            self.button(
                Rect {
                    x: self.width - 94.0,
                    y,
                    w: 70.0,
                    h: 28.0,
                },
                "删除",
                Action::RemoveAlias(kind, index),
                false,
                true,
                true,
            );
            y += 38.0;
        }
        self.content_height = (y + 12.0).max(160.0);
    }

    pub fn view_height(&self) -> f32 {
        (self.height - self.content_top - FOOTER).max(0.0)
    }

    pub fn max_scroll(&self) -> f32 {
        (self.content_height - self.view_height()).max(0.0)
    }

    fn screen_rect(&self, control: &Control, scroll: f32) -> Rect {
        let mut rect = control.rect;
        if control.body {
            rect.y += self.content_top - scroll.clamp(0.0, self.max_scroll());
        }
        rect
    }

    pub fn hit(&self, state: &State, x: f32, y: f32) -> Option<Action> {
        self.controls
            .iter()
            .find(|c| {
                self.available(state, c)
                    && (!c.body || (y >= self.content_top && y < self.height - FOOTER))
                    && self.screen_rect(c, state.scroll).contains(x, y)
            })
            .map(|c| c.action.clone())
    }

    fn available(&self, state: &State, control: &Control) -> bool {
        state.pending.is_none() || matches!(control.action, Action::Confirm | Action::Cancel)
    }

    pub fn focus_next(&self, state: &mut State, backwards: bool) {
        let controls: Vec<_> = self
            .controls
            .iter()
            .filter(|c| self.available(state, c))
            .collect();
        if controls.is_empty() {
            return;
        }
        let index = controls
            .iter()
            .position(|c| Some(&c.action) == state.focus.as_ref());
        let next = match index {
            Some(i) if backwards => (i + controls.len() - 1) % controls.len(),
            Some(i) => (i + 1) % controls.len(),
            None if backwards => controls.len() - 1,
            None => 0,
        };
        self.focus_action(state, controls[next].action.clone());
    }

    pub fn focus_action(&self, state: &mut State, action: Action) {
        let Some(control) = self
            .controls
            .iter()
            .find(|c| c.action == action && self.available(state, c))
        else {
            return;
        };
        state.focus = Some(action);
        if control.body {
            state.scroll = state
                .scroll
                .min(control.rect.y)
                .max(control.rect.y + control.rect.h - self.view_height())
                .clamp(0.0, self.max_scroll());
        }
    }

    pub fn move_font_focus(&self, state: &mut State, backwards: bool) {
        let fonts: Vec<_> = self
            .controls
            .iter()
            .filter(|c| matches!(c.action, Action::FontFamily(_)))
            .collect();
        if fonts.is_empty() {
            return;
        }
        let current = fonts
            .iter()
            .position(|c| Some(&c.action) == state.focus.as_ref());
        let index = match current {
            Some(i) if backwards => i.saturating_sub(1),
            Some(i) => (i + 1).min(fonts.len() - 1),
            None => usize::from(fonts.len() > 1),
        };
        self.focus_action(state, fonts[index].action.clone());
    }

    pub fn draw(&self, p: &Painter, fonts: &Fonts, state: &State, scale: f32) {
        let text = |rect: Rect, label: &str, heading: bool, color, align| {
            p.dual_text(
                if heading { &fonts.bold } else { &fonts.small },
                label,
                rect.x * scale,
                rect.y * scale,
                rect.w * scale,
                rect.h * scale,
                color,
                align,
            );
        };
        p.fill_rect(0.0, 0.0, self.width * scale, self.height * scale, theme::BG);
        text(
            Rect {
                x: 24.0,
                y: 14.0,
                w: self.width - 48.0,
                h: 24.0,
            },
            "面板设置",
            true,
            theme::TEXT,
            theme::ALIGN_NEAR,
        );
        text(
            Rect {
                x: 24.0,
                y: 14.0,
                w: self.width - 48.0,
                h: 24.0,
            },
            concat!("v", env!("CARGO_PKG_VERSION")),
            false,
            theme::TEXT_DIM,
            theme::ALIGN_FAR,
        );
        text(
            Rect {
                x: 24.0,
                y: 38.0,
                w: self.width - 48.0,
                h: 20.0,
            },
            "Magpie · 调整显示、管理连接与请求过滤",
            false,
            theme::TEXT_DIM,
            theme::ALIGN_NEAR,
        );
        p.clip(theme::RectF {
            X: 0.0,
            Y: self.content_top * scale,
            Width: self.width * scale,
            Height: self.view_height() * scale,
        });
        let scroll = state.scroll.clamp(0.0, self.max_scroll());
        for label in &self.labels {
            let mut r = label.rect;
            r.y += self.content_top - scroll;
            text(
                r,
                &label.text,
                label.heading,
                label.color.unwrap_or(if label.heading {
                    theme::TEXT
                } else {
                    theme::TEXT_DIM
                }),
                theme::ALIGN_NEAR,
            );
        }
        for body in [true, false] {
            if !body {
                p.reset_clip();
            }
            for c in self.controls.iter().filter(|c| c.body == body) {
                // Native edit boxes paint themselves over these cells.
                if matches!(
                    c.action,
                    Action::FontSearch
                        | Action::OpacityInput
                        | Action::AliasFromInput
                        | Action::AliasToInput
                ) {
                    continue;
                }
                let r = self.screen_rect(c, state.scroll);
                if body && (r.y + r.h <= self.content_top || r.y >= self.height - FOOTER) {
                    continue;
                }
                let enabled = self.available(state, c);
                let hover = enabled && state.hover.as_ref() == Some(&c.action);
                let focused = enabled && state.focus.as_ref() == Some(&c.action);
                // A checkbox row keeps the card surface: its state is already
                // legible from the ☑/☐ prefix, so a selected fill would only
                // add noise to a list of eighteen.
                let fill = if !c.check && c.selected {
                    theme::TAB_ON
                } else if hover {
                    theme::CARD_HOVER
                } else {
                    theme::CARD
                };
                p.round_rect(
                    r.x * scale,
                    r.y * scale,
                    r.w * scale,
                    r.h * scale,
                    5.0 * scale,
                    fill,
                    true,
                );
                p.round_rect(
                    r.x * scale,
                    r.y * scale,
                    r.w * scale,
                    r.h * scale,
                    5.0 * scale,
                    if focused {
                        theme::MAUVE
                    } else if c.selected && !c.check {
                        theme::INPUT_BORDER
                    } else {
                        theme::BORDER
                    },
                    false,
                );
                let color = if !enabled {
                    theme::TEXT_FAINT
                } else if c.danger {
                    theme::RED
                } else if c.check {
                    if c.selected {
                        theme::TEXT
                    } else {
                        theme::TEXT_FAINT
                    }
                } else if c.selected {
                    theme::MAUVE
                } else {
                    theme::TEXT
                };
                let (label_x, align) = if c.check {
                    (r.x + 10.0, theme::ALIGN_NEAR)
                } else {
                    (r.x + 5.0, theme::ALIGN_CENTER)
                };
                text(
                    Rect {
                        x: label_x,
                        w: r.w - 15.0,
                        ..r
                    },
                    &c.label,
                    false,
                    color,
                    align,
                );
            }
        }
        let footer_y = self.height - FOOTER;
        p.fill_rect(
            0.0,
            footer_y * scale,
            self.width * scale,
            scale,
            theme::BORDER,
        );
        let hint = match state.pending {
            Some(Action::RemoveAccount(_)) => "确认删除该连接及其本机密钥？此操作无法撤销。",
            Some(Action::ClearKeys) => "确认清除所有连接的网关密钥？远程网关将无法连接。",
            _ => "设置即时生效并自动保存 · 失焦或 Esc 关闭",
        };
        text(
            Rect {
                x: 24.0,
                y: footer_y + 4.0,
                w: self.width - 48.0,
                h: 22.0,
            },
            hint,
            false,
            theme::TEXT_DIM,
            theme::ALIGN_NEAR,
        );
        if self.max_scroll() > 0.0 {
            let view = self.view_height();
            let thumb = (view * view / self.content_height).max(24.0).min(view);
            let y = self.content_top + (view - thumb) * scroll / self.max_scroll();
            p.round_rect(
                (self.width - 7.0) * scale,
                y * scale,
                3.0 * scale,
                thumb * scale,
                1.5 * scale,
                theme::INPUT_BORDER,
                true,
            );
        }
    }
}

#[cfg(test)]
#[path = "../tests/ui/settings.rs"]
mod tests;
