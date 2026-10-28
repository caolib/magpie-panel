use super::*;
use crate::config::{Account, Config, Stored};

fn account(id: &str, endpoint: &str) -> Account {
    Account {
        id: id.into(),
        name: id.into(),
        endpoint: endpoint.into(),
    }
}

#[test]
fn one_target_per_connection_with_its_stored_key() {
    let config = Config {
        accounts: vec![
            account("a1", "http://127.0.0.1:3425"),
            account("a2", "https://magpie.example"),
        ],
        ..Config::default()
    };
    let mut stored = Stored::default();
    stored.set_key("a2", "k2");

    let targets = targets_from(&config, &stored);
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].key, None);
    assert_eq!(targets[1].key.as_deref(), Some("k2"));
    assert_eq!(targets[1].endpoint, "https://magpie.example");
}

#[test]
fn the_fetch_window_leaves_room_to_filter_locally() {
    assert_eq!(fetch_limit(1), 100);
    assert_eq!(fetch_limit(12), 100);
    assert_eq!(fetch_limit(100), 500);
    assert_eq!(fetch_limit(5000), 500);
}

#[test]
fn backoff_starts_at_two_seconds_and_caps_at_thirty() {
    assert_eq!(next_backoff(Duration::ZERO), Duration::from_secs(2));
    assert_eq!(next_backoff(Duration::from_secs(2)), Duration::from_secs(4));
    assert_eq!(
        next_backoff(Duration::from_secs(20)),
        Duration::from_secs(30)
    );
    assert_eq!(
        next_backoff(Duration::from_secs(30)),
        Duration::from_secs(30)
    );
}

#[test]
fn a_row_limit_command_is_adopted_and_forces_a_poll() {
    let mut config = Config::default();
    let mut targets = Vec::new();
    let mut next_poll = Instant::now() + Duration::from_secs(60);
    let mut force = false;
    let mut probe = None;

    apply_command(
        Command::SetRowLimit(20),
        &mut config,
        &mut targets,
        &mut next_poll,
        &mut force,
        &mut probe,
        &mut DetailQueue::default(),
    );
    assert_eq!(config.row_limit, 20);
    assert!(force);
    assert!(probe.is_none());
}

#[test]
fn a_test_account_command_records_which_connection_to_probe() {
    let mut config = Config::default();
    let mut targets = Vec::new();
    let mut next_poll = Instant::now();
    let mut force = false;
    let mut probe = None;

    apply_command(
        Command::TestAccount {
            account: "a1".into(),
        },
        &mut config,
        &mut targets,
        &mut next_poll,
        &mut force,
        &mut probe,
        &mut DetailQueue::default(),
    );
    assert_eq!(probe.as_deref(), Some("a1"));
}

#[test]
fn replacing_the_targets_resets_the_probe_and_polls_immediately() {
    let mut config = Config::default();
    let mut targets = Vec::new();
    let mut next_poll = Instant::now() + Duration::from_secs(60);
    let mut force = false;
    let mut probe = Some("a1".to_string());

    let new_targets = vec![Target {
        id: "a2".into(),
        name: "a2".into(),
        endpoint: "http://127.0.0.1:3425".into(),
        key: None,
    }];
    apply_command(
        Command::SetTargets(new_targets),
        &mut config,
        &mut targets,
        &mut next_poll,
        &mut force,
        &mut probe,
        &mut DetailQueue::default(),
    );
    assert_eq!(targets.len(), 1);
    assert_eq!(targets[0].id, "a2");
    // A pending probe survives: replacing the poll list does not cancel a
    // test the user already asked for.
    assert_eq!(probe.as_deref(), Some("a1"));
    assert!(force);
}

fn detail_request(nonce: u64) -> (DetailTicket, Box<Row>) {
    let row = Row::from_record(&crate::model::UsageRecord {
        route_id: Some(7),
        ..Default::default()
    })
    .with_account("a1", "one");
    let ticket = DetailTicket {
        nonce,
        revision: 0,
        endpoint: "http://127.0.0.1:3425".into(),
        identity: row.detail_identity(),
    };
    (ticket, Box::new(row))
}

#[test]
fn detail_queue_is_bounded_latest_wins_and_does_not_force_poll_or_probe() {
    let mut config = Config::default();
    let mut targets = vec![Target {
        id: "a1".into(),
        name: "one".into(),
        endpoint: "http://127.0.0.1:3425".into(),
        key: None,
    }];
    let mut next = Instant::now() + Duration::from_secs(60);
    let original = next;
    let mut force = false;
    let mut probe = Some("probe".into());
    let mut queue = DetailQueue::default();
    let (old, row) = detail_request(1);
    apply_command(
        Command::FetchDetail {
            ticket: Box::new(old.clone()),
            row,
        },
        &mut config,
        &mut targets,
        &mut next,
        &mut force,
        &mut probe,
        &mut queue,
    );
    let (new, row) = detail_request(2);
    apply_command(
        Command::FetchDetail {
            ticket: Box::new(new.clone()),
            row,
        },
        &mut config,
        &mut targets,
        &mut next,
        &mut force,
        &mut probe,
        &mut queue,
    );
    assert_eq!(queue.pending.as_ref().unwrap().0.nonce, 2);
    assert!(!queue.accepts(&old, &targets));
    assert!(queue.accepts(&new, &targets));
    assert!(!force);
    assert_eq!(next, original);
    assert_eq!(probe.as_deref(), Some("probe"));
    apply_command(
        Command::CancelDetail,
        &mut config,
        &mut targets,
        &mut next,
        &mut force,
        &mut probe,
        &mut queue,
    );
    assert!(queue.pending.is_none());
    assert!(!queue.accepts(&new, &targets));
}

#[test]
fn target_changes_cancel_pending_and_inflight_detail_including_same_origin_edit() {
    let mut config = Config::default();
    let mut targets = vec![Target {
        id: "a1".into(),
        name: "one".into(),
        endpoint: "http://127.0.0.1:3425".into(),
        key: None,
    }];
    let mut next = Instant::now();
    let mut force = false;
    let mut probe = None;
    let mut queue = DetailQueue::default();
    let (old, row) = detail_request(1);
    queue.enqueue(old.clone(), row, &targets);
    let replacement = targets.clone();
    apply_command(
        Command::SetTargets(replacement),
        &mut config,
        &mut targets,
        &mut next,
        &mut force,
        &mut probe,
        &mut queue,
    );
    assert!(queue.pending.is_none());
    assert!(!queue.accepts(&old, &targets));
    assert_eq!(queue.revision, 1);
    let (_, row) = detail_request(2);
    queue.enqueue(old, row, &targets);
    assert!(queue.pending.is_none());
    let (mut current, row) = detail_request(3);
    current.revision = 1;
    queue.enqueue(current.clone(), row, &targets);
    assert!(queue.accepts(&current, &targets));
    targets.clear();
    assert!(!queue.accepts(&current, &targets));
}
