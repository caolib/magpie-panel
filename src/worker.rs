//! Background polling of every saved Magpie gateway.
//!
//! All network work happens off the UI thread. Results come back as plain
//! values that the UI applies from its own timer, so painting never blocks on a
//! socket and the panel stays responsive when a gateway is slow or unreachable.
//!
//! Each poll is two reads: the usage log (finished requests) and the
//! concurrency feed (live ones). Finished rows keep Magpie's own semantics;
//! live rows are merged in front by start time.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use crate::client::Client;
use crate::config::{Config, Stored};
use crate::model::{DetailTicket, ExecutionDetail, Row};
use tracing::warn;

/// Upper bound on a usage query before the panel gives up on it.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
/// Longest sleep, so queued commands are noticed promptly without spinning.
const TICK: Duration = Duration::from_millis(200);
/// Rows fetched per poll before the local status filter and row limit are
/// applied. Filtering locally needs a window larger than what is shown.
const MIN_FETCH: i64 = 100;
const MAX_FETCH: i64 = 500;

pub enum Update {
    Detail {
        ticket: Box<DetailTicket>,
        detail: ExecutionDetail,
    },
    /// A fresh page of requests, merged across every connection.
    Snapshot { rows: Vec<Row> },
    /// Which connections answered and which are broken, sent when that changes.
    Accounts {
        open: Vec<String>,
        broken: Vec<AccountIssue>,
    },
    /// Answer to `TestAccount`: the connection's total request count, or why it
    /// could not be reached.
    Probe {
        id: String,
        result: Result<i64, String>,
    },
}

/// One connection that failed to answer, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountIssue {
    pub id: String,
    pub name: String,
    pub message: String,
}

/// One saved Magpie gateway, as the worker polls it.
#[derive(Clone)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub endpoint: String,
    pub key: Option<String>,
}

impl Target {
    pub fn from_account(account: &crate::config::Account, stored: &Stored) -> Self {
        Target {
            id: account.id.clone(),
            name: account.name.clone(),
            endpoint: account.endpoint.clone(),
            key: stored.key_for(&account.id),
        }
    }

    /// The environment override wins over the stored key.
    fn with_env(mut self, env_key: Option<&str>) -> Self {
        if let Some(key) = env_key.map(str::trim).filter(|key| !key.is_empty()) {
            self.key = Some(key.to_string());
        }
        self
    }
}

/// Signals the worker thread accepts.
#[derive(Clone)]
pub enum Command {
    FetchDetail {
        ticket: Box<DetailTicket>,
        row: Box<Row>,
    },
    CancelDetail,
    /// Replace the whole polled set: connections were added, removed or edited.
    SetTargets(Vec<Target>),
    RefreshNow,
    /// Adopt a new row count (the UI derives it from the window height).
    SetRowLimit(i64),
    /// Read one connection's total, purely to report whether it works.
    TestAccount {
        account: String,
    },
    Shutdown,
}

pub struct Worker {
    commands: Sender<Command>,
    updates: Receiver<Update>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Worker {
    pub fn spawn(config: Config, targets: Vec<Target>) -> Self {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<Command>();
        let (upd_tx, upd_rx) = std::sync::mpsc::channel::<Update>();

        let handle = std::thread::Builder::new()
            .name("magpie-panel-poll".into())
            .spawn(move || run(config, targets, cmd_rx, upd_tx))
            .ok();

        Worker {
            commands: cmd_tx,
            updates: upd_rx,
            handle,
        }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    /// Non-blocking drain of everything produced since the last call.
    pub fn drain(&self) -> Vec<Update> {
        let mut out = Vec::new();
        while let Ok(update) = self.updates.try_recv() {
            out.push(update);
        }
        out
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// What one poll cycle produced.
#[derive(Default)]
struct Cycle {
    rows: Vec<Row>,
    /// Connections that answered.
    open: Vec<String>,
    /// Connections that failed, with a short reason.
    broken: Vec<AccountIssue>,
}

/// The worker's poll list, one target per saved connection.
pub fn targets_from(config: &Config, stored: &Stored) -> Vec<Target> {
    // The environment key is a single secret, so it stands in for the first
    // connection: exporting a fresh key is how a headless setup refreshes it.
    let env_key = crate::config::key_from_env(&config.gateway_key_env_var);
    config
        .accounts
        .iter()
        .enumerate()
        .map(|(index, account)| {
            let mut target = Target::from_account(account, stored);
            if index == 0 {
                target = target.with_env(env_key.as_deref());
            }
            target
        })
        .collect()
}

fn next_backoff(previous: Duration) -> Duration {
    if previous.is_zero() {
        Duration::from_secs(2)
    } else {
        (previous * 2).min(Duration::from_secs(30))
    }
}

/// Rows to ask the gateway for: enough to filter locally, never past the cap.
fn fetch_limit(row_limit: i64) -> i64 {
    (row_limit.max(1) * 8).clamp(MIN_FETCH, MAX_FETCH)
}

#[derive(Default)]
struct DetailQueue {
    revision: u64,
    latest: Option<u64>,
    pending: Option<(DetailTicket, Box<Row>)>,
}

impl DetailQueue {
    fn enqueue(&mut self, ticket: DetailTicket, row: Box<Row>, targets: &[Target]) {
        if ticket.revision == self.revision
            && ticket.identity == row.detail_identity()
            && targets
                .iter()
                .any(|t| t.id == ticket.identity.account_id && t.endpoint == ticket.endpoint)
        {
            self.latest = Some(ticket.nonce);
            self.pending = Some((ticket, row));
        }
    }
    fn cancel(&mut self) {
        self.latest = None;
        self.pending = None;
    }
    fn accepts(&self, ticket: &DetailTicket, targets: &[Target]) -> bool {
        self.latest == Some(ticket.nonce)
            && ticket.revision == self.revision
            && targets
                .iter()
                .any(|t| t.id == ticket.identity.account_id && t.endpoint == ticket.endpoint)
    }
}

fn apply_command(
    command: Command,
    config: &mut Config,
    targets: &mut Vec<Target>,
    next_poll: &mut Instant,
    force: &mut bool,
    pending_probe: &mut Option<String>,
    details: &mut DetailQueue,
) -> bool {
    match command {
        Command::Shutdown => return true,
        Command::FetchDetail { ticket, row } => details.enqueue(*ticket, row, targets),
        Command::CancelDetail => details.cancel(),
        Command::RefreshNow => {
            *next_poll = Instant::now();
            *force = true;
        }
        Command::SetRowLimit(limit) => {
            config.row_limit = limit;
            *next_poll = Instant::now();
            *force = true;
        }
        Command::SetTargets(next) => {
            details.revision = details.revision.wrapping_add(1);
            details.cancel();
            *targets = next;
            *next_poll = Instant::now();
            *force = true;
        }
        Command::TestAccount { account } => *pending_probe = Some(account),
    }
    false
}

fn run(
    mut config: Config,
    mut targets: Vec<Target>,
    commands: Receiver<Command>,
    updates: Sender<Update>,
) {
    let client = Client::new(REQUEST_TIMEOUT);
    // A single bounded detail lane reuses the configured Agent. Slow detail
    // reads cannot postpone poll/probe work; no per-click thread spawning.
    let (detail_tx, detail_rx) =
        std::sync::mpsc::sync_channel::<(DetailTicket, Box<Row>, Target)>(1);
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    let detail_client = client.clone();
    let detail_started = std::thread::Builder::new()
        .name("magpie-panel-detail".into())
        .spawn(move || {
            while let Ok((ticket, row, target)) = detail_rx.recv() {
                let detail = detail_client.detail(&target.endpoint, target.key.as_deref(), &row);
                if result_tx.send((ticket, detail)).is_err() {
                    break;
                }
            }
        })
        .is_ok();
    let mut detail_busy = false;

    let mut backoff = Duration::ZERO;
    let mut force = true;
    let mut pending_probe: Option<String> = None;
    let mut details = DetailQueue::default();
    let mut next_poll = Instant::now();
    // Last reported connection health, so `Update::Accounts` only goes out when
    // something actually changed.
    let mut reported: Option<(Vec<String>, Vec<AccountIssue>)> = None;

    loop {
        let mut shutdown = false;
        while let Ok(command) = commands.try_recv() {
            if apply_command(
                command,
                &mut config,
                &mut targets,
                &mut next_poll,
                &mut force,
                &mut pending_probe,
                &mut details,
            ) {
                shutdown = true;
            }
        }
        if shutdown {
            return;
        }

        // Served inline: the user is watching the settings window for the
        // answer, and the test is a single request.
        if let Some(account) = pending_probe.take() {
            let result = match targets.iter().find(|t| t.id == account) {
                Some(target) => client
                    .usage(&target.endpoint, target.key.as_deref(), "all", 1)
                    .map(|page| page.total.unwrap_or(0))
                    .map_err(|e| e.message()),
                None => Err("该连接已不存在".to_string()),
            };
            if let Err(message) = &result {
                warn!("测试连接失败 ({}): {}", account, message);
            }
            let _ = updates.send(Update::Probe {
                id: account,
                result,
            });
        }

        if force || Instant::now() >= next_poll {
            force = false;
            let mut cycle = Cycle::default();

            for target in targets.iter() {
                match client.usage(
                    &target.endpoint,
                    target.key.as_deref(),
                    &config.period,
                    fetch_limit(config.row_limit),
                ) {
                    Ok(page) => {
                        let mut rows: Vec<Row> = page
                            .rows
                            .iter()
                            .map(Row::from_record)
                            .map(|row| row.with_account(&target.id, &target.name))
                            .collect();
                        // The live feed is best-effort: a restricted key is
                        // refused it, and that is not a connection failure.
                        match client.concurrency(&target.endpoint, target.key.as_deref()) {
                            Ok(active) => {
                                rows.extend(
                                    active
                                        .iter()
                                        .map(Row::from_active)
                                        .map(|row| row.with_account(&target.id, &target.name)),
                                );
                            }
                            Err(err) => {
                                warn!("读取实时请求失败 ({}): {}", target.name, err.message());
                            }
                        }
                        cycle.rows.extend(rows);
                        cycle.open.push(target.id.clone());
                    }
                    Err(err) => {
                        warn!(
                            "轮询 Magpie 请求失败 (连接 {}): {}",
                            target.name,
                            err.message()
                        );
                        cycle.broken.push(AccountIssue {
                            id: target.id.clone(),
                            name: target.name.clone(),
                            message: err.message(),
                        });
                    }
                }
            }

            // Recomputed every cycle: a busy gateway is polled faster, and
            // going idle must return to the slow cadence.
            let any_active = cycle.rows.iter().any(|r| r.status.is_active());
            if cycle.open.is_empty() {
                // Nothing answered. Grow the backoff so a down gateway cannot
                // produce a request storm, and keep whatever is on screen —
                // an empty replacement list would look like "no requests".
                if !targets.is_empty() {
                    backoff = next_backoff(backoff);
                }
            } else {
                backoff = Duration::ZERO;
                let _ = updates.send(Update::Snapshot {
                    rows: std::mem::take(&mut cycle.rows),
                });
            }

            let health = (cycle.open.clone(), cycle.broken.clone());
            if reported.as_ref() != Some(&health) {
                let _ = updates.send(Update::Accounts {
                    open: cycle.open,
                    broken: cycle.broken,
                });
                reported = Some(health);
            }

            let interval = if !backoff.is_zero() {
                backoff
            } else {
                let secs = if any_active {
                    config.active_poll_seconds
                } else {
                    config.poll_seconds
                };
                Duration::from_secs(secs.max(1))
            };
            next_poll = Instant::now() + interval;
        }

        while let Ok((ticket, detail)) = result_rx.try_recv() {
            detail_busy = false;
            if details.accepts(&ticket, &targets) {
                let _ = updates.send(Update::Detail {
                    ticket: Box::new(ticket),
                    detail,
                });
            }
        }
        if !detail_busy && let Some((ticket, row)) = details.pending.take() {
            if detail_started
                && let Some(target) = targets
                    .iter()
                    .find(|t| t.id == ticket.identity.account_id && t.endpoint == ticket.endpoint)
            {
                detail_busy = detail_tx
                    .try_send((ticket.clone(), row, target.clone()))
                    .is_ok();
            }
            if !detail_busy && details.accepts(&ticket, &targets) {
                let _ = updates.send(Update::Detail {
                    ticket: Box::new(ticket),
                    detail: ExecutionDetail::unavailable("后台详情线程不可用或连接已编辑"),
                });
            }
        }

        let wait = next_poll
            .saturating_duration_since(Instant::now())
            .clamp(Duration::from_millis(20), TICK);
        let deadline = Instant::now() + wait;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match commands.recv_timeout(remaining.min(TICK)) {
                Ok(command) => {
                    if apply_command(
                        command,
                        &mut config,
                        &mut targets,
                        &mut next_poll,
                        &mut force,
                        &mut pending_probe,
                        &mut details,
                    ) {
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

#[cfg(test)]
#[path = "tests/worker.rs"]
mod tests;
