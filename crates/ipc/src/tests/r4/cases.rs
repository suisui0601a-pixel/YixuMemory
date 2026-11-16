use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn fixture_db() -> (tempfile::TempDir, Broker) {
    let dir = tempfile::tempdir().unwrap();
    let broker = create(&dir.path().join("synthetic.sqlite")).unwrap();
    (dir, broker)
}

fn establish_broker(broker: &mut Broker, channel: u8, epoch: u64) -> (Connection, Id) {
    let channel = [channel; 32];
    let connection = broker.connect(channel, 1).unwrap();
    let h = hello(SCOPE, PRINCIPAL, VERSION, epoch);
    let super::super::Outcome::Reply(challenge) =
        broker.receive(connection, &h.encode(), 1).unwrap()
    else {
        panic!("expected challenge");
    };
    let auth = proof(&broker.core, &h, &challenge, channel);
    broker.receive(connection, &auth.encode(), 1).unwrap();
    assert_eq!(broker.core.state(connection), State::Established);
    (connection, challenge.session)
}

fn queue(broker: &mut Broker, channel: u8, id: u8) -> (Connection, Id) {
    let (connection, sid) = establish_broker(broker, channel, 7);
    let request = request(sid, id, 2, SCOPE, Some(TARGET));
    assert_eq!(
        broker.receive(connection, &request.encode(), 1).unwrap(),
        super::super::Outcome::Queued
    );
    (connection, sid)
}

fn counts(db: &Sqlite) -> (i64, i64, i64, i64) {
    db.query_row(
        "SELECT (SELECT count(*) FROM release_intent), (SELECT count(*) FROM observation),
         (SELECT count(*) FROM receipt), (SELECT count(*) FROM audit)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .unwrap()
}

fn state(db: &Sqlite, operation: i64) -> String {
    db.query_row(
        "SELECT state FROM release_intent WHERE operation=?1",
        [operation],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn current_database_authority_is_separate_from_ipc() {
    for update in [
        "grant_ok=0",
        "principal_ok=0",
        "space_ok=0",
        "source_ok=0",
        "key_ok=0",
        "copy_ok=0",
        "deletion_gen=1",
        "epoch=8",
        "expiry=1",
    ] {
        let (_dir, mut broker) = fixture_db();
        queue(&mut broker, 11, 1);
        // Fixture fault models a stale session or a stale in-memory epoch cache.
        broker
            .db
            .execute(&format!("UPDATE authority SET {update}"), [])
            .unwrap();
        assert!(matches!(broker.arm(1), Err(Failure::Denied)));
        assert_eq!(counts(&broker.db), (0, 0, 0, 0));
        assert!(broker.core.active.is_none());
    }
}

#[test]
fn observation_and_receipt_commit_separately() {
    let (dir, mut broker) = fixture_db();
    queue(&mut broker, 11, 1);
    let armed = broker.arm(1).unwrap().unwrap();
    assert_eq!(counts(&broker.db), (1, 0, 0, 1));
    let (op, binding) = (armed.operation, armed.binding);
    let completion = executor::complete(armed, false);
    broker.observe(&completion).unwrap();
    assert_eq!(counts(&broker.db), (1, 1, 0, 2));
    assert!(broker.arm(1).unwrap().is_none());
    assert_eq!(
        broker.settle(completion).unwrap(),
        executor::Outcome::Accepted
    );
    assert_eq!(counts(&broker.db), (1, 1, 1, 3));
    assert_eq!(state(&broker.db, op), "Settled");
    assert!(broker.active.is_none() && broker.core.active.is_none());
    assert!(
        broker
            .db
            .execute("UPDATE receipt SET outcome='other'", [])
            .is_err()
    );
    assert!(broker.db.execute("DELETE FROM receipt", []).is_err());
    let replay = executor::complete(
        Armed {
            operation: op,
            binding,
        },
        false,
    );
    assert!(matches!(broker.settle(replay), Err(Failure::Conflict)));
    let path = dir.path().join("synthetic.sqlite");
    assert!(matches!(create(&path), Err(Failure::Conflict)));
    let reopened = RecoveryBlocked::inspect(&path).unwrap();
    assert_eq!(reopened.unresolved, 0);
    assert_eq!(
        reopened.receipt(op, &binding).unwrap().as_deref(),
        Some("SyntheticAccepted")
    );
    assert!(reopened.receipt(op, &[0; 32]).unwrap().is_none());
}

#[test]
fn mismatched_completions_cannot_settle() {
    let (_dir, mut broker) = fixture_db();
    let (_other_dir, mut other) = fixture_db();
    queue(&mut broker, 11, 1);
    queue(&mut other, 12, 1);
    other
        .db
        .execute("UPDATE authority SET key_gen=2", [])
        .unwrap();
    let a = broker.arm(1).unwrap().unwrap();
    let b = other.arm(1).unwrap().unwrap();
    assert_eq!(a.operation, b.operation);
    let mismatched = executor::complete(b, false);
    assert!(matches!(
        broker.observe(&mismatched),
        Err(Failure::Conflict)
    ));
    assert!(matches!(broker.settle(mismatched), Err(Failure::Conflict)));
    assert_eq!(counts(&broker.db), (1, 0, 0, 1));
    broker.observe(&executor::complete(a, true)).unwrap();
}

#[test]
fn receipt_failure_rolls_back_and_blocks() {
    let (_dir, mut broker) = fixture_db();
    queue(&mut broker, 11, 1);
    let done = executor::complete(broker.arm(1).unwrap().unwrap(), false);
    broker.observe(&done).unwrap();
    broker
        .db
        .execute_batch(
            "CREATE TEMP TRIGGER fail_settlement BEFORE INSERT ON audit
         WHEN NEW.event='Settled' BEGIN SELECT RAISE(ABORT,'fixture fault'); END;",
        )
        .unwrap();
    assert!(matches!(broker.settle(done), Err(Failure::Database(_))));
    assert_eq!(counts(&broker.db), (1, 1, 0, 2));
    assert_eq!(state(&broker.db, 1), "Armed");
    assert!(broker.blocked && broker.active.is_some() && broker.core.active.is_some());
}

#[test]
fn closing_is_ordered_across_all_security_changes() {
    for kind in [
        Change::Grant,
        Change::Revocation,
        Change::Delete,
        Change::Revision,
        Change::Source,
        Change::Key,
        Change::Copy,
        Change::Principal,
        Change::Space,
    ] {
        let (_dir, mut broker) = fixture_db();
        queue(&mut broker, 11, 1);
        let a = broker.arm(1).unwrap().unwrap();
        let first = broker.close_for(SCOPE, kind).unwrap();
        let second = broker.close_for(SCOPE, Change::Revision).unwrap();
        assert!(matches!(
            broker.make_effective(first),
            Err(Failure::Blocked)
        ));
        assert!(broker.arm(1).unwrap().is_none());
        assert_eq!(state(&broker.db, 1), "CancelRequested");
        let done = executor::complete(a, true);
        broker.observe(&done).unwrap();
        assert!(matches!(
            broker.make_effective(first),
            Err(Failure::Blocked)
        ));
        assert_eq!(broker.settle(done).unwrap(), executor::Outcome::Cancelled);
        assert!(matches!(
            broker.make_effective(second),
            Err(Failure::Conflict)
        ));
        broker.make_effective(first).unwrap();
        assert!(matches!(broker.connect([12; 32], 1), Err(Failure::Blocked)));
        broker.apply(first).unwrap();
        assert!(matches!(
            broker.make_effective(first),
            Err(Failure::Conflict)
        ));
        assert!(matches!(broker.connect([12; 32], 1), Err(Failure::Blocked)));
        broker.make_effective(second).unwrap();
        broker.apply(second).unwrap();
        assert_eq!(broker.epoch.get(), Some(9));
    }
}

#[test]
fn delete_before_dispatch_drops_queued_work_and_epoch_refuses_old_reconnect() {
    let (_dir, mut broker) = fixture_db();
    let (connection, _) = queue(&mut broker, 11, 1);
    let event = broker.close_for(SCOPE, Change::Delete).unwrap();
    assert_eq!(
        broker.core.request_state(connection, [1; 16]),
        Some(RequestState::Dropped)
    );
    assert!(broker.arm(1).unwrap().is_none());
    broker.make_effective(event).unwrap();
    broker.apply(event).unwrap();
    let old = broker.connect([12; 32], 1).unwrap();
    let h = initial_hello();
    let super::super::Outcome::Reply(r) = broker.receive(old, &h.encode(), 1).unwrap() else {
        panic!("expected challenge");
    };
    let a = proof(&broker.core, &h, &r, [12; 32]);
    assert!(matches!(
        broker.receive(old, &a.encode(), 1),
        Err(Failure::Protocol(Error::Denied))
    ));
    assert_eq!(counts(&broker.db), (0, 0, 0, 0));
}

#[test]
fn revoke_cancel_and_disconnect_hold_budget_until_settlement() {
    for cancel_first in [false, true] {
        let (_dir, mut broker) = fixture_db();
        let (connection, sid) = queue(&mut broker, 11, 1);
        let a = broker.arm(1).unwrap().unwrap();
        for channel in 12..19 {
            broker.connect([channel; 32], 1).unwrap();
        }
        assert!(matches!(
            broker.connect([19; 32], 1),
            Err(Failure::Protocol(Error::Backpressure))
        ));
        let cancel = frame(Kind::Cancel, sid, [1; 16], 3, &[]).encode();
        if cancel_first {
            assert_eq!(
                broker.receive(connection, &cancel, 1).unwrap(),
                super::super::Outcome::CancelRequested
            );
        }
        let e = broker.close_for(SCOPE, Change::Revocation).unwrap();
        if !cancel_first {
            assert!(matches!(
                broker.receive(connection, &cancel, 1),
                Err(Failure::Blocked)
            ));
        }
        broker.disconnect(connection).unwrap();
        assert_eq!(state(&broker.db, 1), "Unknown");
        assert_eq!(broker.core.sessions.len(), MAX_CONNECTIONS);
        assert!(matches!(broker.make_effective(e), Err(Failure::Blocked)));
        let done = executor::complete(a, true);
        broker.observe(&done).unwrap();
        broker.settle(done).unwrap();
        assert_eq!(broker.core.sessions.len(), MAX_CONNECTIONS - 1);
        broker.make_effective(e).unwrap();
        broker.apply(e).unwrap();
    }
}

#[test]
fn invalid_security_scope_first_blocks() {
    let (_dir, mut broker) = fixture_db();
    queue(&mut broker, 11, 1);
    assert!(matches!(
        broker.close_for(
            Scope {
                tenant: ZERO,
                ..SCOPE
            },
            Change::Delete
        ),
        Err(Failure::Protocol(Error::Binding))
    ));
    assert!(broker.blocked && broker.core.realm_blocked);
    assert_eq!(
        broker
            .db
            .query_row("SELECT blocked FROM runtime", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(matches!(broker.arm(1), Err(Failure::Blocked)));
    assert_eq!(counts(&broker.db), (0, 0, 0, 0));
}

const STAGE: &str = "YIXU_R4_TEST_STAGE";
const PATH: &str = "YIXU_R4_TEST_PATH";

fn checkpoint() {
    println!("R4_CHECKPOINT");
    std::io::stdout().flush().unwrap();
    let _ = std::io::stdin().read(&mut [0]);
}

#[test]
fn child_checkpoint() {
    let Ok(stage) = std::env::var(STAGE) else {
        return;
    };
    let path = std::path::PathBuf::from(std::env::var_os(PATH).unwrap());
    assert!(matches!(
        stage.as_str(),
        "before_arm_commit" | "armed" | "observed" | "settled"
    ));
    let mut broker = create(&path).unwrap();
    queue(&mut broker, 11, 1);
    if stage == "before_arm_commit" {
        broker.arm_before_commit(1, checkpoint).unwrap();
        return;
    }
    let a = broker.arm(1).unwrap().unwrap();
    if stage != "armed" {
        let done = executor::complete(a, false);
        broker.observe(&done).unwrap();
        if stage == "settled" {
            broker.settle(done).unwrap();
        }
    }
    checkpoint();
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn process_loss_and_read_only_reconciliation() {
    for (stage, expected) in [
        ("before_arm_commit", (0, 0, 0, 0)),
        ("armed", (1, 0, 0, 1)),
        ("observed", (1, 1, 0, 2)),
        ("settled", (1, 1, 1, 3)),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.sqlite");
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::r4::cases::child_checkpoint",
                    "--nocapture",
                ])
                .env(STAGE, stage)
                .env(PATH, &path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = child.0.stdout.take().unwrap();
        let (send, receive) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if line.unwrap().contains("R4_CHECKPOINT") {
                    send.send(()).unwrap();
                    break;
                }
            }
        });
        receive.recv_timeout(Duration::from_secs(15)).unwrap();
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        reader.join().unwrap();
        // In rollback-journal mode a hot journal may require a separate SQLite
        // rollback before read-only inspection. The fixture performs only that
        // database-engine recovery; it does not acquire a Broker owner.
        let recovery = Sqlite::open(&path).unwrap();
        let _: i64 = recovery
            .query_row("SELECT count(*) FROM runtime", [], |r| r.get(0))
            .unwrap();
        drop(recovery);
        let reopened = RecoveryBlocked::inspect(&path).unwrap();
        assert_eq!(counts(&reopened.db), expected);
        assert_eq!(reopened.unresolved, expected.0 - expected.2);
        assert!(matches!(create(&path), Err(Failure::Conflict)));
        if stage == "settled" {
            let binding: [u8; 32] = reopened
                .db
                .query_row("SELECT binding FROM receipt", [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                reopened.receipt(1, &binding).unwrap().as_deref(),
                Some("SyntheticAccepted")
            );
        }
    }
}
