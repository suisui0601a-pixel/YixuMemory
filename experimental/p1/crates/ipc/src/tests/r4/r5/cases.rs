use super::super::cases::{establish_broker, queue, state};
use super::*;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command as Process, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const OTHER: Id = id(43);
const REV_A: Id = id(10);
const REV_B: Id = id(11);
const NEXT: Id = id(12);
const SOURCE: Id = id(20);
const NEW_SOURCE: Id = id(21);
const PROOF: Id = id(22);
const NEW_PROOF: Id = id(23);
const KEY_A: Id = id(51);
const COPY_A: Id = id(52);
const BACKUP_A: Id = id(53);
const KEY_B: Id = id(54);

const fn id(n: u8) -> Id {
    [n; 16]
}

fn object(id: Id) -> ObjectRef {
    ObjectRef {
        id,
        generation: if id == NEXT { 2 } else { 1 },
    }
}

fn make(path: &Path) -> Closure {
    let node = |id, kind, parent| Node {
        id,
        kind,
        generation: object(id).generation,
        parent,
    };
    let a_payload = id(30);
    let b_payload = id(31);
    let next_payload = id(32);
    let evidence_payload = id(33);
    Closure::create(
        path,
        &[
            node(TARGET, Kind::Memory, None),
            node(OTHER, Kind::Memory, None),
            node(REV_A, Kind::Revision, Some(TARGET)),
            node(NEXT, Kind::Revision, Some(TARGET)),
            node(REV_B, Kind::Revision, Some(OTHER)),
            node(SOURCE, Kind::Source, None),
            node(NEW_SOURCE, Kind::Source, None),
            node(PROOF, Kind::Evidence, Some(SOURCE)),
            node(NEW_PROOF, Kind::Evidence, Some(NEW_SOURCE)),
            node(a_payload, Kind::Payload, Some(REV_A)),
            node(b_payload, Kind::Payload, Some(REV_B)),
            node(next_payload, Kind::Payload, Some(NEXT)),
            node(evidence_payload, Kind::Payload, Some(PROOF)),
            node(KEY_A, Kind::Key, Some(a_payload)),
            node(COPY_A, Kind::Copy, Some(a_payload)),
            node(BACKUP_A, Kind::Backup, Some(a_payload)),
            node(KEY_B, Kind::Key, Some(b_payload)),
            node(id(55), Kind::Copy, Some(b_payload)),
            node(id(56), Kind::Key, Some(next_payload)),
            node(id(57), Kind::Copy, Some(next_payload)),
            node(id(58), Kind::Key, Some(evidence_payload)),
            node(id(59), Kind::Copy, Some(evidence_payload)),
        ],
        &[(REV_A, PROOF), (REV_B, PROOF), (NEXT, NEW_PROOF)],
        &[(TARGET, REV_A), (OTHER, REV_B)],
    )
    .unwrap()
}

fn fresh() -> (tempfile::TempDir, Closure) {
    let dir = tempfile::tempdir().unwrap();
    let realm = make(&dir.path().join("closure.sqlite"));
    (dir, realm)
}

fn schedule(realm: &mut Closure, request: u8, command: Command) -> i64 {
    realm.close(SCOPE, id(request), command).unwrap()
}

fn apply(realm: &mut Closure, event: i64) {
    realm.broker.make_effective(event).unwrap();
    realm.apply(event).unwrap();
}

fn observed(realm: &mut Closure, reference: Id, effect: Effect) -> Result<()> {
    realm.observe_effect(cleanup::observe(
        realm.broker.core.incarnation,
        object(reference),
        effect,
    ))
}

fn scalar(db: &Sqlite, query: &str) -> i64 {
    db.query_row(query, [], |row| row.get(0)).unwrap()
}

fn historical(realm: &Closure, revision: Id) -> bool {
    realm
        .graph
        .revision_is_eligible(&realm.broker.db, revision)
        .unwrap()
}

fn inspect_target(realm: &mut Closure, channel: u8, epoch: u64) -> Result<Option<Armed>> {
    let (connection, session) = establish_broker(&mut realm.broker, channel, epoch);
    let query = request(session, channel, 2, SCOPE, Some(TARGET)).encode();
    realm.broker.receive(connection, &query, 1).unwrap();
    realm.arm(1)
}

#[test]
fn legacy_dispatch_cannot_omit_the_r5_dependency_binding() {
    let (_dir, mut realm) = fresh();
    queue(&mut realm.broker, 11, 1);
    assert!(matches!(realm.broker.arm(1), Err(Failure::Blocked)));
    assert!(realm.broker.blocked);
    assert_eq!(
        scalar(&realm.broker.db, "SELECT count(*) FROM release_intent"),
        0
    );
}

#[test]
fn release_settlement_orders_overlapping_security_changes_and_deduplicates() {
    let (_dir, mut realm) = fresh();
    let (connection, _) = queue(&mut realm.broker, 11, 1);
    let armed = realm.arm(1).unwrap().unwrap();
    let revoke = schedule(&mut realm, 1, Command::Revoke);
    let delete = schedule(&mut realm, 2, Command::Delete(object(TARGET)));
    assert_eq!(schedule(&mut realm, 1, Command::Revoke), revoke);
    assert!(matches!(
        realm.close(SCOPE, id(1), Command::Delete(object(TARGET))),
        Err(Failure::Conflict)
    ));
    assert!(matches!(
        realm.broker.connect([18; 32], 1),
        Err(Failure::Blocked)
    ));
    realm.broker.disconnect(connection).unwrap();
    assert_eq!(state(&realm.broker.db, 1), "Unknown");
    assert!(realm.arm(1).unwrap().is_none());
    let terminal = executor::complete(armed, true);
    realm.broker.observe(&terminal).unwrap();
    assert!(matches!(
        realm.broker.make_effective(revoke),
        Err(Failure::Blocked)
    ));
    realm.broker.settle(terminal).unwrap();
    assert!(matches!(
        realm.broker.make_effective(delete),
        Err(Failure::Conflict)
    ));
    apply(&mut realm, revoke);
    assert!(matches!(
        realm.broker.connect([18; 32], 1),
        Err(Failure::Blocked)
    ));
    apply(&mut realm, delete);
    assert_eq!(
        scalar(&realm.broker.db, "SELECT count(*) FROM security_event"),
        2
    );
    assert_eq!(scalar(&realm.broker.db, "SELECT epoch FROM authority"), 9);
    assert_eq!(scalar(&realm.broker.db, "SELECT count(*) FROM receipt"), 1);
    assert_eq!(schedule(&mut realm, 1, Command::Revoke), revoke);
    assert!(matches!(realm.apply(revoke), Err(Failure::Conflict)));
}

#[test]
fn memory_delete_preserves_shared_source_and_requires_distinct_cleanup_receipts() {
    let (dir, mut realm) = fresh();
    let event = schedule(&mut realm, 1, Command::Delete(object(TARGET)));
    apply(&mut realm, event);
    assert!(!historical(&realm, REV_A));
    assert!(!historical(&realm, NEXT));
    assert!(historical(&realm, REV_B));
    let db = &realm.broker.db;
    assert_eq!(
        scalar(
            db,
            "SELECT count(*) FROM r5_tombstone WHERE object IN (SELECT id FROM r5_object WHERE kind IN ('Source','Evidence'))"
        ),
        0
    );
    let status = progress(db, event).unwrap();
    assert_eq!(
        status,
        Progress {
            state: "Applied".into(),
            pending: 5,
            observed_unsettled: 0,
            keys: 0,
            copies: 0,
            backups: 0
        }
    );
    assert!(matches!(
        realm.settle_effect(object(KEY_A)),
        Err(Failure::Conflict)
    ));
    assert!(matches!(
        realm.observe_effect(cleanup::observe(
            id(99),
            object(KEY_A),
            Effect::KeyUnavailable
        )),
        Err(Failure::Conflict)
    ));
    assert!(matches!(
        realm.observe_effect(cleanup::observe(
            realm.broker.core.incarnation,
            ObjectRef {
                id: KEY_A,
                generation: 2
            },
            Effect::KeyUnavailable
        )),
        Err(Failure::Conflict)
    ));
    for (key, copy) in [(KEY_A, COPY_A), (id(56), id(57))] {
        observed(&mut realm, key, Effect::KeyUnavailable).unwrap();
        assert_eq!(
            progress(&realm.broker.db, event)
                .unwrap()
                .observed_unsettled,
            1
        );
        realm.settle_effect(object(key)).unwrap();
        realm.settle_effect(object(key)).unwrap();
        observed(&mut realm, copy, Effect::CiphertextRemoved).unwrap();
        realm.settle_effect(object(copy)).unwrap();
    }
    let before_backup = progress(&realm.broker.db, event).unwrap();
    assert_eq!(
        (
            before_backup.pending,
            before_backup.keys,
            before_backup.copies,
            before_backup.backups
        ),
        (1, 2, 2, 0)
    );
    observed(&mut realm, BACKUP_A, Effect::BackupExpired).unwrap();
    realm.settle_effect(object(BACKUP_A)).unwrap();
    let status = progress(&realm.broker.db, event).unwrap();
    assert_eq!(status.pending, 0);
    assert_eq!(status.backups, 1);
    let path = dir.path().join("closure.sqlite");
    let recovery = Recovery::inspect(&path).unwrap();
    assert_eq!(recovery.progress(event).unwrap(), status);
    assert!(RecoveryBlocked::inspect(&path).is_err());
    assert!(
        recovery
            .blocked
            .db
            .execute("DELETE FROM r5_receipt", [])
            .is_err()
    );
}

#[test]
fn source_deletion_reaches_every_dependent_and_only_fresh_revision_can_read() {
    let (_dir, mut realm) = fresh();
    queue(&mut realm.broker, 11, 1);
    let scene = schedule(&mut realm, 1, Command::Delete(object(SOURCE)));
    assert!(realm.arm(1).unwrap().is_none());
    apply(&mut realm, scene);
    assert!(!historical(&realm, REV_A));
    assert!(!historical(&realm, REV_B));
    assert!(historical(&realm, NEXT));
    assert_eq!(
        scalar(&realm.broker.db, "SELECT source_ok FROM authority"),
        0
    );
    assert_eq!(
        scalar(&realm.broker.db, "SELECT count(*) FROM release_intent"),
        0
    );
    assert!(matches!(
        inspect_target(&mut realm, 12, 8),
        Err(Failure::Denied)
    ));
    let revision = schedule(
        &mut realm,
        2,
        Command::Revise {
            expected: object(REV_A),
            next: object(NEXT),
        },
    );
    apply(&mut realm, revision);
    assert_eq!(current_revision(&realm.broker.db, TARGET).unwrap(), NEXT);
    assert_eq!(
        scalar(&realm.broker.db, "SELECT source_ok FROM authority"),
        1
    );
    assert!(!historical(&realm, REV_A));
    assert!(!historical(&realm, REV_B));
    let armed = inspect_target(&mut realm, 13, 9).unwrap().unwrap();
    let completed = executor::complete(armed, false);
    realm.broker.observe(&completed).unwrap();
    realm.broker.settle(completed).unwrap();
    let overlapping = schedule(&mut realm, 3, Command::Delete(object(PROOF)));
    apply(&mut realm, overlapping);
    assert_eq!(
        progress(&realm.broker.db, scene).unwrap().pending,
        progress(&realm.broker.db, overlapping).unwrap().pending
    );
    observed(&mut realm, KEY_B, Effect::KeyUnavailable).unwrap();
    realm.settle_effect(object(KEY_B)).unwrap();
    assert_eq!(progress(&realm.broker.db, scene).unwrap().keys, 1);
    assert_eq!(progress(&realm.broker.db, overlapping).unwrap().keys, 1);
    let stale = Command::Revise {
        expected: object(REV_A),
        next: object(NEXT),
    };
    let stale = schedule(&mut realm, 4, stale);
    realm.broker.make_effective(stale).unwrap();
    assert!(matches!(realm.apply(stale), Err(Failure::Blocked)));
    assert_eq!(current_revision(&realm.broker.db, TARGET).unwrap(), NEXT);
    assert!(realm.broker.blocked);
}

#[test]
fn stale_grant_and_incorrect_generation_block_before_state_can_revive() {
    let (_dir, mut realm) = fresh();
    let revoke = schedule(&mut realm, 1, Command::Revoke);
    let grant = schedule(&mut realm, 2, Command::Grant { expected_epoch: 7 });
    apply(&mut realm, revoke);
    realm.broker.make_effective(grant).unwrap();
    assert!(matches!(realm.apply(grant), Err(Failure::Blocked)));
    assert_eq!(
        scalar(&realm.broker.db, "SELECT grant_ok FROM authority"),
        0
    );
    assert_eq!(
        progress(&realm.broker.db, grant).unwrap().state,
        "EffectiveFence"
    );

    let (_dir, mut realm) = fresh();
    assert!(matches!(
        realm.close(
            SCOPE,
            id(1),
            Command::RetireKey(ObjectRef {
                id: KEY_A,
                generation: 2
            })
        ),
        Err(Failure::Blocked)
    ));
    assert_eq!(
        scalar(&realm.broker.db, "SELECT count(*) FROM security_event"),
        0
    );
    assert!(realm.broker.blocked);
    assert_eq!(scalar(&realm.broker.db, "SELECT blocked FROM runtime"), 1);
}

#[test]
fn other_authority_changes_share_the_order_and_old_scope_is_denied() {
    let (_dir, mut realm) = fresh();
    for (i, command) in [
        Command::Grant { expected_epoch: 7 },
        Command::DisablePrincipal,
        Command::DisableTenant,
        Command::DisableSpace,
        Command::InvalidateSource(object(SOURCE)),
        Command::RetireKey(object(KEY_A)),
        Command::RetireCopy(object(COPY_A)),
    ]
    .into_iter()
    .enumerate()
    {
        let event = schedule(&mut realm, u8::try_from(i).unwrap() + 1, command);
        apply(&mut realm, event);
    }
    assert_eq!(scalar(&realm.broker.db, "SELECT epoch FROM authority"), 14);
    assert_eq!(
        scalar(&realm.broker.db, "SELECT principal_ok FROM authority"),
        0
    );
    assert_eq!(
        scalar(&realm.broker.db, "SELECT space_ok FROM authority"),
        0
    );
    assert!(!historical(&realm, REV_A));
    assert_eq!(
        scalar(&realm.broker.db, "SELECT count(*) FROM r5_outbox"),
        1
    );
    assert!(matches!(
        realm.close(
            Scope {
                space: ZERO,
                ..SCOPE
            },
            id(9),
            Command::Revoke
        ),
        Err(Failure::Protocol(Error::Binding))
    ));
    assert!(realm.broker.blocked);
}

#[test]
fn apply_failure_rollback_is_visible_as_fence_from_second_connection() {
    let (dir, mut realm) = fresh();
    let mut observer = Sqlite::open(dir.path().join("closure.sqlite")).unwrap();
    observer.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    let event = schedule(&mut realm, 1, Command::Delete(object(TARGET)));
    realm.broker.make_effective(event).unwrap();
    assert!(
        observer
            .execute("UPDATE authority SET grant_ok=0", [])
            .is_err()
    );
    assert!(
        observer
            .execute("UPDATE r5_current SET revision=?1", [NEXT])
            .is_err()
    );
    let partial = observer
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    partial
        .execute("INSERT INTO r5_apply_context VALUES(?1)", [event])
        .unwrap();
    partial
        .execute("UPDATE authority SET grant_ok=0", [])
        .unwrap();
    assert!(partial.commit().is_err());
    assert_eq!(scalar(&observer, "SELECT grant_ok FROM authority"), 1);
    realm
        .broker
        .db
        .execute_batch(
            "CREATE TEMP TRIGGER fail_r5 BEFORE INSERT ON r5_applied
        BEGIN SELECT RAISE(ABORT,'synthetic fault'); END;",
        )
        .unwrap();
    assert!(matches!(realm.apply(event), Err(Failure::Database(_))));
    assert_eq!(progress(&observer, event).unwrap().state, "EffectiveFence");
    assert_eq!(
        (
            scalar(&observer, "SELECT epoch FROM authority"),
            scalar(&observer, "SELECT count(*) FROM r5_tombstone"),
            scalar(&observer, "SELECT count(*) FROM r5_outbox")
        ),
        (7, 0, 0)
    );
    assert_eq!(scalar(&observer, "SELECT blocked FROM runtime"), 1);
}

#[test]
fn competing_writer_cannot_skip_terminal_or_hold_security_transaction_open() {
    let (dir, mut realm) = fresh();
    queue(&mut realm.broker, 11, 1);
    let _armed = realm.arm(1).unwrap().unwrap();
    let event = schedule(&mut realm, 1, Command::Revoke);
    let mut other = Sqlite::open(dir.path().join("closure.sqlite")).unwrap();
    other
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=15;")
        .unwrap();
    assert!(
        other
            .execute(
                "UPDATE security_event SET state='EffectiveFence' WHERE id=?1",
                [event]
            )
            .is_err()
    );
    let lock = other
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    realm
        .broker
        .db
        .busy_timeout(Duration::from_millis(15))
        .unwrap();
    assert!(matches!(
        realm.close(SCOPE, id(2), Command::Revoke),
        Err(Failure::Database(_))
    ));
    assert!(realm.broker.blocked);
    lock.rollback().unwrap();
    assert_eq!(scalar(&other, "SELECT count(*) FROM r5_command"), 1);
    assert_eq!(progress(&other, event).unwrap().state, "Closing");
}

#[test]
fn unobserved_ciphertext_cleanup_and_foreign_receipts_do_not_complete() {
    let (_dir, mut realm) = fresh();
    let event = schedule(&mut realm, 1, Command::Delete(object(TARGET)));
    apply(&mut realm, event);
    assert!(matches!(
        observed(&mut realm, COPY_A, Effect::CiphertextRemoved),
        Err(Failure::Database(_))
    ));
    assert_eq!(progress(&realm.broker.db, event).unwrap().pending, 5);
    assert!(realm.broker.blocked);
}

#[test]
fn cleanup_receipt_failure_leaves_observation_and_pending_work() {
    let (_dir, mut realm) = fresh();
    let event = schedule(&mut realm, 1, Command::RetireKey(object(KEY_A)));
    apply(&mut realm, event);
    observed(&mut realm, KEY_A, Effect::KeyUnavailable).unwrap();
    realm
        .broker
        .db
        .execute_batch(
            "CREATE TEMP TRIGGER fail_cleanup BEFORE INSERT ON r5_receipt
         BEGIN SELECT RAISE(ABORT,'synthetic fault'); END;",
        )
        .unwrap();
    assert!(matches!(
        realm.settle_effect(object(KEY_A)),
        Err(Failure::Database(_))
    ));
    let status = progress(&realm.broker.db, event).unwrap();
    assert_eq!(
        (status.pending, status.observed_unsettled, status.keys),
        (1, 1, 0)
    );
    assert!(realm.broker.blocked);
}

#[test]
fn old_backup_has_read_only_diagnostics_and_no_admission() {
    let (dir, mut realm) = fresh();
    let path = dir.path().join("closure.sqlite");
    let older = dir.path().join("backup.sqlite");
    realm
        .broker
        .db
        .execute("VACUUM INTO ?1", [older.to_str().unwrap()])
        .unwrap();
    let event = schedule(&mut realm, 1, Command::Delete(object(SOURCE)));
    apply(&mut realm, event);
    let old = Recovery::inspect(&older).unwrap();
    assert!(old.progress(event).is_err());
    assert!(
        old.blocked
            .db
            .execute("UPDATE authority SET grant_ok=1", [])
            .is_err()
    );
    let current = Recovery::inspect(&path).unwrap();
    assert_eq!(current.progress(event).unwrap().state, "Applied");
    assert!(matches!(
        create_with(&older, |_| Ok(())),
        Err(Failure::Conflict)
    ));
}

const STAGE: &str = "YIXU_R5_TEST_STAGE";
const PATH: &str = "YIXU_R5_TEST_PATH";

#[test]
fn incomplete_security_event_blocks_dispatch_and_reopen_diagnostics() {
    let (dir, mut realm) = fresh();
    queue(&mut realm.broker, 11, 1);
    let event = realm
        .broker
        .transact(|tx| insert_change(tx, SCOPE, Change::Revocation))
        .unwrap();
    assert!(realm.arm(1).unwrap().is_none());
    assert!(matches!(
        realm.broker.make_effective(event),
        Err(Failure::Database(_))
    ));
    assert!(realm.broker.blocked);
    assert_eq!(scalar(&realm.broker.db, "SELECT epoch FROM authority"), 7);
    assert_eq!(
        scalar(&realm.broker.db, "SELECT count(*) FROM release_intent"),
        0
    );
    assert!(matches!(
        Recovery::inspect(&dir.path().join("closure.sqlite")),
        Err(Failure::Blocked)
    ));
}

fn checkpoint() {
    println!("R5_CHECKPOINT");
    std::io::stdout().flush().unwrap();
    let _ = std::io::stdin().read(&mut [0]);
}

#[test]
fn child_closure() {
    let Ok(stage) = std::env::var(STAGE) else {
        return;
    };
    let path = std::path::PathBuf::from(std::env::var_os(PATH).unwrap());
    let mut realm = make(&path);
    let command = if stage == "revoke-closing" {
        Command::Revoke
    } else {
        Command::Delete(object(TARGET))
    };
    if stage == "unknown" {
        let (connection, _) = queue(&mut realm.broker, 11, 1);
        realm.arm(1).unwrap().unwrap();
        realm.broker.disconnect(connection).unwrap();
    }
    let event = schedule(&mut realm, 1, command);
    match stage.as_str() {
        "revoke-closing" | "unknown" => {}
        "delete-fenced" => realm.broker.make_effective(event).unwrap(),
        "delete-before-commit" => {
            realm.broker.make_effective(event).unwrap();
            realm.apply_before_commit(event, checkpoint).unwrap();
            return;
        }
        "delete-applied" => apply(&mut realm, event),
        "key-observed" | "key-settled" => {
            apply(&mut realm, event);
            observed(&mut realm, KEY_A, Effect::KeyUnavailable).unwrap();
            if stage == "key-settled" {
                realm.settle_effect(object(KEY_A)).unwrap();
            }
        }
        _ => panic!("unexpected checkpoint"),
    }
    checkpoint();
}

#[test]
fn process_exit_preserves_security_phase_outbox_observation_and_receipt() {
    use super::super::cases::OwnedChild;
    for (stage, expected, pending, observed, keys, unresolved) in [
        ("revoke-closing", "Closing", 0, 0, 0, 0),
        ("delete-fenced", "EffectiveFence", 0, 0, 0, 0),
        ("delete-before-commit", "EffectiveFence", 0, 0, 0, 0),
        ("delete-applied", "Applied", 5, 0, 0, 0),
        ("key-observed", "Applied", 5, 1, 0, 0),
        ("key-settled", "Applied", 4, 0, 1, 0),
        ("unknown", "Closing", 0, 0, 0, 1),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("closure.sqlite");
        let mut child = OwnedChild(
            Process::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::r4::r5::cases::child_closure",
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
        let (sender, receiver) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if line.unwrap().contains("R5_CHECKPOINT") {
                    sender.send(()).unwrap();
                    break;
                }
            }
        });
        receiver.recv_timeout(Duration::from_secs(15)).unwrap();
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        reader.join().unwrap();
        // SQLite may need to remove a hot rollback journal before a read-only
        // connection can check the state. This opens no broker or cleanup capability.
        let recovery = Sqlite::open(&path).unwrap();
        assert_eq!(scalar(&recovery, "SELECT count(*) FROM runtime"), 1);
        drop(recovery);
        let outcome = Recovery::inspect(&path).unwrap();
        let view = outcome.progress(1).unwrap();
        let command = if stage == "revoke-closing" {
            Command::Revoke
        } else {
            Command::Delete(object(TARGET))
        };
        assert_eq!(
            outcome.reconcile(SCOPE, id(1), command).unwrap().as_ref(),
            Some(&view)
        );
        assert!(matches!(
            outcome.reconcile(SCOPE, id(1), Command::DisableSpace),
            Err(Failure::Conflict)
        ));
        assert_eq!(outcome.reconcile(SCOPE, id(2), command).unwrap(), None);
        assert_eq!(
            (
                view.state.as_str(),
                view.pending,
                view.observed_unsettled,
                view.keys,
                outcome.blocked.unresolved
            ),
            (expected, pending, observed, keys, unresolved)
        );
        assert_eq!(
            scalar(&outcome.blocked.db, "SELECT count(*) FROM r5_receipt"),
            keys
        );
        assert_eq!(
            scalar(
                &outcome.blocked.db,
                "SELECT count(*) FROM pragma_integrity_check WHERE integrity_check='ok'"
            ),
            1
        );
        assert!(matches!(
            create_with(&path, |_| Ok(())),
            Err(Failure::Conflict)
        ));
        println!(
            "R5 stage={stage} state={expected} pending={pending} observed_unsettled={observed} key_receipts={keys} unresolved_release={unresolved}"
        );
    }
}
