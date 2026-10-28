//! Panel state: what is on screen and how user input mutates it.
//!
//! Scroll clamping lives in the UI layer because it depends on the live window
//! metrics; this module only records intent.

use crate::config::{Account, Config, HiddenProvider, Stored};
use crate::model::{self, Filter, FilterMask, Row};
use crate::ui::login::LoginForm;
use crate::worker::{AccountIssue, Update};

/// Which surface the panel is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    List,
    Login,
}

/// Which connection the form is editing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginTarget {
    /// A new connection: the name field starts empty.
    Add,
    /// An existing connection being edited.
    Account(String),
}

/// A "test this connection" round trip the user asked for from the settings
/// window, and what came back.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub id: String,
    pub state: ProbeState,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeState {
    /// Waiting for the worker's answer.
    Running,
    /// The connection answered; the number is its total request count.
    Ok(i64),
    /// The connection could not be reached.
    Failed(String),
}

pub struct App {
    pub view: View,
    pub config: Config,
    /// The list the UI renders, rebuilt on every update.
    pub rows: Vec<Row>,
    raw_rows: Vec<Row>,
    /// Selected status-filter bits, applied locally in `rebuild`.
    pub filter: FilterMask,
    /// Text the list is narrowed to, after the header box is submitted.
    pub query: Option<String>,
    /// What is currently typed in the header box, submitted or not.
    pub query_draft: String,
    /// The header box holds the keyboard.
    pub query_editing: bool,
    /// Caret visibility while `query_editing`; toggled by the blink timer.
    pub query_caret: bool,
    /// Unclamped scroll offset in pixels; the UI clamps against the metrics.
    pub scroll: f32,
    pub hover: Option<usize>,
    pub selected: Option<usize>,
    /// Transient status line; `true` marks an error presentation.
    pub status: Option<(String, bool)>,
    pub login: Option<LoginForm>,
    /// Saved connections, mirroring `config.accounts`.
    pub accounts: Vec<Account>,
    /// Connections that answered the last poll.
    pub open_accounts: Vec<String>,
    /// Connections that did not, with the reason.
    pub issues: Vec<AccountIssue>,
    /// The last connection test, if one has been run.
    pub probe: Option<Probe>,
    /// Gateway keys in memory, mirroring the encrypted blob.
    pub stored: Stored,
}

impl App {
    pub fn new(config: Config) -> Self {
        App {
            view: View::List,
            accounts: config.accounts.clone(),
            rows: Vec::new(),
            raw_rows: Vec::new(),
            filter: model::FILTER_NONE,
            query: None,
            query_draft: String::new(),
            query_editing: false,
            query_caret: true,
            scroll: 0.0,
            hover: None,
            selected: None,
            status: None,
            login: None,
            open_accounts: Vec::new(),
            issues: Vec::new(),
            probe: None,
            stored: Stored::default(),
            config,
        }
    }

    /// Show the connection form for one connection (or for a new one).
    pub fn open_login(&mut self, message: Option<String>, target: LoginTarget) {
        let form = match &target {
            LoginTarget::Account(id) => self.config.account(id).map(LoginForm::for_account),
            LoginTarget::Add => None,
        };
        let mut form = form.unwrap_or_else(|| LoginForm::adding(&self.config.endpoint));
        if let LoginTarget::Account(id) = &target
            && let Some(account) = self.config.account(id)
        {
            form.key = self.stored.key_for(&account.id).unwrap_or_default();
        }
        form.error = message;
        self.login = Some(form);
        self.view = View::Login;
        self.scroll = 0.0;
        self.hover = None;
    }

    pub fn is_login(&self) -> bool {
        self.view == View::Login
    }

    /// How one connection is doing, for the menu: `None` when it answered the
    /// last poll, otherwise a short label for what is wrong.
    pub fn account_state(&self, id: &str) -> Option<&'static str> {
        self.issues.iter().find(|i| i.id == id)?;
        Some("连接失败")
    }

    /// Whether any connection failed its last poll.
    pub fn any_issue(&self) -> bool {
        !self.issues.is_empty()
    }

    /// Start tracking a test for one connection, replacing any previous result.
    pub fn begin_probe(&mut self, id: &str) {
        self.probe = Some(Probe {
            id: id.to_string(),
            state: ProbeState::Running,
        });
    }

    /// Apply everything the worker produced.
    pub fn apply_updates(&mut self, updates: Vec<Update>) {
        for update in updates {
            match update {
                Update::Snapshot { rows } => {
                    self.raw_rows = rows;
                    self.rebuild();
                    self.status = self.status_line();
                }
                Update::Accounts { open, broken } => {
                    self.open_accounts = open;
                    self.issues = broken;
                    self.status = self.status_line();
                }
                // Popup-owned output is intercepted by the UI before this.
                Update::Detail { .. } => {}
                Update::Probe { id, result } => {
                    // A late answer for a connection the user has since tested
                    // again is dropped rather than painted over.
                    if let Some(probe) = self.probe.as_mut().filter(|p| p.id == id) {
                        probe.state = match result {
                            Ok(total) => ProbeState::Ok(total),
                            Err(message) => ProbeState::Failed(message),
                        };
                    }
                }
            }
        }
    }

    /// What the status line should say after a poll: a broken connection first,
    /// otherwise a submitted query that came back empty.
    fn status_line(&self) -> Option<(String, bool)> {
        if let Some(issue) = self.issues.first() {
            return Some((format!("连接 {} {}", issue.name, issue.message), true));
        }
        self.query
            .as_ref()
            .filter(|_| self.rows.is_empty())
            .map(|_| ("未找到匹配记录".into(), true))
    }

    /// Apply filters and aliases, newest first, capped at the configured row
    /// count.
    pub fn rebuild(&mut self) {
        let mut rows: Vec<_> = self
            .raw_rows
            .iter()
            .map(|row| row.for_display(&self.config))
            .collect();

        let hidden = &self.config.hidden_providers;
        if !hidden.is_empty() {
            rows.retain(|row| {
                let provider = row.provider_id.as_deref().or(row.provider.as_deref());
                !provider.is_some_and(|provider| {
                    hidden
                        .iter()
                        .any(|h| h.account_id == row.account_id && h.provider == provider)
                })
            });
        }

        if self.filter != model::FILTER_NONE {
            rows.retain(|row| model::passes(self.filter, row));
        }

        if let Some(query) = self.query.as_deref() {
            let needle = query.trim().to_lowercase();
            if !needle.is_empty() {
                rows.retain(|row| row_matches(row, &needle));
            }
        }

        rows.sort_by_key(|row| std::cmp::Reverse(row.age_key()));
        rows.truncate(self.config.row_limit.max(1) as usize);
        self.rows = rows;
        if self.selected.is_some_and(|sel| sel >= self.rows.len()) {
            self.selected = None;
        }
    }

    pub fn raw_rows(&self) -> &[Row] {
        &self.raw_rows
    }

    /// The provider filters the settings window offers: every (connection,
    /// provider) pair seen in the fetched rows, plus the ones already hidden —
    /// which by definition are absent from those rows and still have to be
    /// listed so they can be shown again.
    pub fn provider_filters(&self) -> Vec<HiddenProvider> {
        let mut out: Vec<HiddenProvider> = self.config.hidden_providers.clone();
        for row in &self.raw_rows {
            let Some(provider) = row.provider_id.as_deref().or(row.provider.as_deref()) else {
                continue;
            };
            if row.account_id.is_empty() {
                continue;
            }
            let entry = HiddenProvider {
                account_id: row.account_id.clone(),
                provider: provider.to_string(),
            };
            if !out.contains(&entry) {
                out.push(entry);
            }
        }
        out.sort_by(|a, b| (&a.account_id, &a.provider).cmp(&(&b.account_id, &b.provider)));
        out
    }

    /// Apply a chip click: 全部 clears the mask, a status chip toggles its bit.
    /// Returns whether the mask changed.
    pub fn click_filter(&mut self, chip: Filter) -> bool {
        let next = if chip == Filter::All {
            model::FILTER_NONE
        } else {
            self.filter ^ chip.mask()
        };
        if next == self.filter {
            return false;
        }
        self.filter = next;
        self.rebuild();
        true
    }

    /// Apply the header box: an empty box clears the query.
    pub fn apply_query(&mut self) {
        let trimmed = self.query_draft.trim();
        self.query = (!trimmed.is_empty()).then(|| trimmed.to_string());
        self.scroll = 0.0;
        self.rebuild();
        self.status = self.query.as_ref().map(|_| ("筛选中…".into(), false));
    }

    /// Persist anything that changed while running.
    pub fn save(&self) {
        self.config.save();
    }
}

/// Whether a row mentions `needle` in any field the header box searches.
fn row_matches(row: &Row, needle: &str) -> bool {
    let fields = [
        Some(row.model.as_str()),
        Some(row.requested_model.as_str()),
        row.served_model.as_deref(),
        row.provider.as_deref(),
        row.provider_id.as_deref(),
        row.agent.as_deref(),
        row.host.as_deref(),
        row.endpoint.as_deref(),
        Some(row.account_name.as_str()),
        row.error.as_deref(),
        row.rid.as_deref(),
        row.session.as_deref(),
    ];
    fields
        .into_iter()
        .flatten()
        .any(|value| value.to_lowercase().contains(needle))
}

#[cfg(test)]
#[path = "tests/app.rs"]
mod tests;
