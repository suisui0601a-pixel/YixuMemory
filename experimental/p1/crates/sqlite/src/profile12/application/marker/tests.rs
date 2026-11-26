//! Real SQLite/NTFS metadata fixtures; no DEK, body, DPAPI, Native output or Ready recovery.
use super::*;
use crate::profile12::tests::seed_with_binding;
use std::path::PathBuf;
struct TestClock;
impl Clock for TestClock {
    fn now(&self) -> Result<i64> {
        Ok(100)
    }
}
fn id(n: u8) -> Id {
    [n; 16]
}
fn actor() -> Actor {
    Actor {
        scope: [1; 48],
        principal: id(9),
    }
}
struct Case {
    app: Application,
    health: Arc<OwnerHealth>,
    db: Connection,
    path: PathBuf,
    _dir: tempfile::TempDir,
}
impl Case {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s3.sqlite");
        for n in ["markers", "payload", "backups"] {
            std::fs::create_dir(dir.path().join(n)).unwrap();
        }
        let io = MarkerStore::open(
            &dir.path().join("markers"),
            &dir.path().join("payload"),
            &dir.path().join("backups"),
        )
        .unwrap();
        let mut db = Connection::open(&path).unwrap();
        let permit = OwnerHealth::initialize_fresh(&mut db, &io).unwrap();
        db.execute(
            "INSERT INTO runtime VALUES(1,?1,?2,?3,?4,?5,?6,7,0,0,0)",
            params![
                id(2),
                id(1),
                id(3),
                id(4),
                io.binding().unwrap(),
                crate::profile12::schema_hash()
            ],
        )
        .unwrap();
        for (n, op) in [10u8, 20, 30].into_iter().enumerate() {
            let member = Member {
                operation: id(op),
                scope: [1; 48],
                principal: id(9),
                request: id(op + 6),
                memory: id(op + 1),
                revision: id(op + 2),
                payload: id(op + 3),
                key: id(op + 4),
                evidence: id(op + 5),
                evidence_gen: 1,
                generation: 1,
                incarnation: id(3),
                epoch: 7 + n as i64,
                dispatch: [0; 32],
                source: id(if op == 30 { 70 } else { 60 }),
                source_gen: 1,
                parent: None,
            };
            seed_with_binding(
                &db,
                op,
                if op == 30 { 70 } else { 60 },
                member.original_dispatch(id(2)).unwrap(),
            );
        }
        let app = Application::attach(
            Connection::open(&path).unwrap(),
            id(2),
            id(1),
            id(3),
            RealmOrder::new(),
            Arc::new(TestClock),
        )
        .unwrap();
        let health = OwnerHealth::bind_fresh(&app, io, permit).unwrap();
        Self {
            app,
            health,
            db,
            path,
            _dir: dir,
        }
    }
    fn deleted(&mut self) -> i64 {
        let event = self
            .app
            .close(actor(), id(90), Target::Memory(id(11)))
            .unwrap();
        self.app.fence(actor(), event).unwrap();
        self.app.apply(actor(), event).unwrap();
        event
    }
    fn work(&self) -> Work {
        self.health
            .load(&self.app)
            .unwrap()
            .into_iter()
            .find(|w| w.member.operation == id(10))
            .unwrap()
    }
    fn marker_path(&self, w: &Work) -> PathBuf {
        self.path.parent().unwrap().join("markers").join(&w.name)
    }
    fn prime(&self, observed: bool, settled: bool) -> [u8; 12] {
        let w = self.work();
        // Known-success fixture publication through the actual native Publisher.
        // Tests can access private constructors; ordinary applications cannot.
        let pin = Publisher(&self.health)
            .publish(PublishPermit {
                work: w.clone(),
                incarnation: id(3),
                roots: self.health.roots.clone(),
            })
            .unwrap();
        let identity = pin.identity();
        self.db
            .execute(
                "UPDATE deletion_work SET marker_identity=?1 WHERE operation=?2",
                params![identity, id(10)],
            )
            .unwrap();
        if observed {
            self.db
                .execute(
                    "INSERT INTO deletion_observation VALUES(?1,'KeyUnavailable',1)",
                    [id(10)],
                )
                .unwrap();
        }
        if settled {
            self.db
                .execute(
                    "INSERT INTO deletion_receipt VALUES(?1,'KeyUnavailable',1)",
                    [id(10)],
                )
                .unwrap();
        }
        self.health.io.verify(&mut { pin }).unwrap();
        identity
    }
    fn blocked(&self) -> i64 {
        self.db
            .query_row("SELECT blocked FROM runtime", [], |r| r.get(0))
            .unwrap()
    }
    fn snapshot_except_health(&self) -> Vec<(String, Vec<String>)> {
        let mut names = self
            .db
            .prepare("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
            .unwrap();
        let tables = names
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        tables.into_iter().map(|name|{
            let query=if name=="runtime" {"SELECT rowid,id,installation,realm,incarnation,owner_nonce,store_identity,schema_hash,epoch,knowledge_seq,clean FROM runtime ORDER BY rowid".to_string()}else{format!("SELECT rowid,* FROM {name} ORDER BY rowid")};
            let mut q=self.db.prepare(&query).unwrap();let count=q.column_count();
            let rows=q.query_map([],|r|Ok((0..count).map(|i|format!("{:?}",r.get_ref(i).unwrap())).collect::<Vec<_>>().join("|"))).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap();(name,rows)
        }).collect()
    }
    fn denied_after_latch(&mut self) {
        let callbacks = AtomicI64::new(0);
        assert!(
            self.app
                .transaction(|_, _| {
                    callbacks.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(callbacks.load(Ordering::SeqCst), 0);
        // Another actual SQLite connection shares the same trusted host gate.
        // It cannot evade a non-durable latch by attaching again.
        assert!(
            Application::attach(
                Connection::open(&self.path).unwrap(),
                id(2),
                id(1),
                id(3),
                self.app.order.clone(),
                Arc::new(TestClock),
            )
            .is_err()
        );
        assert!(self.app.admit_metadata(actor(), id(31)).is_err());
        assert!(
            self.app
                .close(actor(), id(91), Target::Source(id(70)))
                .is_err()
        );
        assert!(self.app.reconcile().is_err());
        assert!(self.health.clean_stop_allowed(&self.app).is_err());
        assert_eq!(
            self.db
                .query_row("SELECT count(*) FROM stop_receipt", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        println!(
            "S3_HEALTH report={:?} metadata_callbacks=0 clean_stop=false current_owner_blocked=true",
            self.health.report()
        );
    }
}

#[test]
fn prewrite_failure_is_known_not_committed_and_never_restores_owner() {
    let mut c = Case::new();
    let before = c.snapshot_except_health();
    c.health
        .block_with(&c.app, Cause::StorageUnknown, BlockFault::BeforeWrite);
    assert_eq!(
        c.health.report(),
        Report::NotConfirmed(Unconfirmed::KnownNotCommitted)
    );
    assert_eq!(c.blocked(), 0);
    c.denied_after_latch();
    assert_eq!(before, c.snapshot_except_health());
}

#[test]
fn poisoned_order_latches_owner_before_rejecting_new_commands() {
    let mut c = Case::new();
    let gate = c.app.order.gate.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = gate.lock().unwrap();
            panic!("S3 synthetic ordering fault; no content or native operation");
        })
        .join()
        .is_err()
    );
    let before = c.snapshot_except_health();
    assert_eq!(
        c.app.admit_metadata(actor(), id(31)),
        Err(Error::CapabilityUnavailable)
    );
    assert_eq!(c.health.report(), Report::Durable);
    assert_eq!(c.blocked(), 1);
    c.denied_after_latch();
    assert_eq!(before, c.snapshot_except_health());
}

#[test]
fn real_sharing_error_is_not_absence_and_cannot_issue_publish_permit() {
    let mut c = Case::new();
    c.deleted();
    c.prime(true, false);
    let w = c.work();
    let writer = std::fs::OpenOptions::new()
        .write(true)
        .open(c.marker_path(&w))
        .unwrap();
    let error = match c.health.io.observe(&w.name) {
        Err(e) => e,
        Ok(_) => panic!("conflicting write handle admitted"),
    };
    println!(
        "S3_NATIVE observation_sharing_error={:?} absent=false",
        error.raw_os_error()
    );
    assert!(matches!(error.raw_os_error(), Some(5 | 32)));
    let before = c.snapshot_except_health();
    assert_eq!(c.app.settle_marker(id(10)), Err(Error::ResourceUnavailable));
    assert_eq!(c.health.publish_calls.load(Ordering::SeqCst), 0);
    assert_eq!(c.blocked(), 1);
    assert_eq!(before, c.snapshot_except_health());
    drop(writer);
    c.denied_after_latch();
}

#[test]
fn ordinary_authorization_denial_rolls_back_without_fabricating_storage_fault() {
    let mut c = Case::new();
    let before = c.snapshot_except_health();
    let mut wrong = actor();
    wrong.principal = id(88);
    assert_eq!(
        c.app.admit_metadata(wrong, id(31)),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(c.health.report(), Report::Ready);
    assert_eq!(c.blocked(), 0);
    assert_eq!(before, c.snapshot_except_health());
    c.app.admit_metadata(actor(), id(31)).unwrap();
}

#[test]
fn schema_fault_latches_before_error_but_cannot_sign_trusted_durable_record() {
    let mut c = Case::new();
    c.db.execute_batch("CREATE TABLE unexpected_s3_object(id INTEGER)")
        .unwrap();
    assert!(c.app.admit_metadata(actor(), id(31)).is_err());
    assert_eq!(
        c.health.report(),
        Report::NotConfirmed(Unconfirmed::UntrustedSchema)
    );
    assert_eq!(c.blocked(), 0);
    c.denied_after_latch();
}

#[test]
fn full_marker_protocol_uses_real_pin_and_settled_replay_never_publishes_again() {
    let mut c = Case::new();
    c.deleted();
    let before = c.snapshot_except_health();
    assert_eq!(c.app.settle_marker(id(10)), Ok(State::Settled));
    let w = c.work();
    let pin = c.health.io.observe(&w.name).unwrap().unwrap();
    let identity = pin.identity();
    drop(pin);
    assert_eq!(c.health.publish_calls.load(Ordering::SeqCst), 1);
    assert_eq!(c.app.settle_marker(id(10)), Ok(State::Settled));
    assert_eq!(c.health.publish_calls.load(Ordering::SeqCst), 1);
    c.app.reconcile().unwrap();
    c.app.admit_metadata(actor(), id(31)).unwrap();
    let pin = c.health.io.observe(&w.name).unwrap().unwrap();
    assert_eq!(identity, pin.identity());
    assert_eq!(
        c.db.query_row("SELECT knowledge_seq FROM runtime", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_ne!(c.snapshot_except_health(), before); // Only intended lifecycle rows advance.
    assert_eq!(c.blocked(), 0);
    assert_eq!(c.health.report(), Report::Ready);
    println!(
        "S3_PHASE identity_committed -> observation_committed -> receipt_committed -> Settled; replay_publish_calls=1 FileId={identity:?}"
    );
}
#[test]
fn recorded_observation_without_receipt_missing_marker_blocks_all_host_connections() {
    let mut c = Case::new();
    c.deleted();
    let identity = c.prime(true, false);
    let w = c.work();
    assert_eq!(w.ledger.identity, Some(identity));
    assert_eq!(w.ledger.observation, Some(1));
    assert_eq!(w.ledger.receipt, None);
    c.app.reconcile().unwrap();
    assert_eq!(c.blocked(), 0);
    std::fs::remove_file(c.marker_path(&w)).unwrap();
    let before = c.snapshot_except_health();
    assert_eq!(c.app.admit_metadata(actor(), id(31)), Err(Error::Integrity));
    assert_eq!(c.health.report(), Report::Durable);
    assert_eq!(c.blocked(), 1);
    assert_eq!(c.health.publish_calls.load(Ordering::SeqCst), 0);
    assert!(c.health.io.observe(&w.name).unwrap().is_none());
    c.denied_after_latch();
    assert_eq!(before, c.snapshot_except_health());
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM deletion_receipt", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
#[test]
fn published_and_recorded_markers_register_or_settle_without_republication() {
    for recorded in [false, true] {
        let mut c = Case::new();
        c.deleted();
        let w = c.work();
        let pin = Publisher(&c.health)
            .publish(PublishPermit {
                work: w.clone(),
                incarnation: id(3),
                roots: c.health.roots.clone(),
            })
            .unwrap();
        let identity = pin.identity();
        if recorded {
            c.db.execute(
                "UPDATE deletion_work SET marker_identity=?1 WHERE operation=?2",
                params![identity, id(10)],
            )
            .unwrap();
        }
        drop(pin);
        assert_eq!(c.app.settle_marker(id(10)), Ok(State::Settled));
        assert_eq!(c.health.publish_calls.load(Ordering::SeqCst), 0);
        assert_eq!(c.work().ledger.identity, Some(identity));
        c.app.reconcile().unwrap();
    }
}
#[test]
fn settled_receipt_still_checks_marker_fileid_content_and_presence() {
    for damage in ["missing", "identity", "content"] {
        let mut c = Case::new();
        c.deleted();
        let old = c.prime(true, true);
        let w = c.work();
        match damage {
            "missing" => std::fs::remove_file(c.marker_path(&w)).unwrap(),
            "identity" => {
                std::fs::remove_file(c.marker_path(&w)).unwrap();
                let new = c
                    .health
                    .io
                    .publish_once("pending-replacement", &w.name, &w.bytes)
                    .unwrap();
                assert_ne!(old, new.identity());
            }
            _ => std::fs::write(c.marker_path(&w), b"CORRUPTED-SYNTHETIC-MARKER").unwrap(),
        }
        let before = c.snapshot_except_health();
        assert_eq!(c.app.admit_metadata(actor(), id(31)), Err(Error::Integrity));
        assert_eq!(c.health.report(), Report::Durable);
        c.denied_after_latch();
        assert_eq!(before, c.snapshot_except_health());
        println!(
            "S3_NATIVE settled_fault={damage} shared_block=true publisher_calls=0 receipt_not_rewritten=true"
        );
    }
}
#[test]
fn unknown_marker_is_rejected_before_domain_work_and_never_cleaned() {
    let mut c = Case::new();
    std::fs::write(
        c.path.parent().unwrap().join("markers/revoked-unknown"),
        b"UNKNOWN-METADATA",
    )
    .unwrap();
    let before = c.snapshot_except_health();
    assert_eq!(c.app.admit_metadata(actor(), id(31)), Err(Error::Integrity));
    c.denied_after_latch();
    assert_eq!(before, c.snapshot_except_health());
    assert!(
        c.path
            .parent()
            .unwrap()
            .join("markers/revoked-unknown")
            .exists()
    );
}
#[test]
fn readonly_block_storage_failure_latches_memory_and_retry_does_not_restore_ready() {
    let mut c = Case::new();
    c.deleted();
    c.prime(true, false);
    let w = c.work();
    std::fs::remove_file(c.marker_path(&w)).unwrap();
    c.app.db.execute_batch("PRAGMA query_only=ON").unwrap();
    let before = c.snapshot_except_health();
    assert_eq!(c.app.admit_metadata(actor(), id(31)), Err(Error::Integrity));
    assert_eq!(
        c.health.report(),
        Report::NotConfirmed(Unconfirmed::KnownNotCommitted)
    );
    assert_eq!(c.blocked(), 0);
    assert!(c.app.db.is_autocommit());
    c.denied_after_latch();
    assert_eq!(before, c.snapshot_except_health());
    c.app.db.execute_batch("PRAGMA query_only=OFF").unwrap();
    assert_eq!(c.health.retry_block_record(&c.app), Report::Durable);
    assert_eq!(c.blocked(), 1);
    c.denied_after_latch();
}
#[test]
fn unknown_block_commit_is_not_reported_durable_even_when_update_is_visible() {
    let mut c = Case::new();
    c.health.block_with(
        &c.app,
        Cause::CommitUnknown,
        BlockFault::AfterCommitReplyLost,
    );
    assert_eq!(c.blocked(), 1);
    assert_eq!(
        c.health.report(),
        Report::NotConfirmed(Unconfirmed::CommitUnknown)
    );
    c.denied_after_latch();
    assert_eq!(c.health.retry_block_record(&c.app), Report::Durable);
    c.denied_after_latch();
    println!(
        "S3_SIMULATED block_COMMIT_reply_lost persisted_bit=1 initial_durable=false retry_durable=true admission_restored=false"
    );
}
#[test]
fn wrong_incarnation_and_unsettled_connection_never_sign_durable() {
    let mut c = Case::new();
    c.db.execute("UPDATE runtime SET incarnation=?1", [id(88)])
        .unwrap();
    c.health.block(&c.app, Cause::MetadataFault);
    assert_eq!(
        c.health.report(),
        Report::NotConfirmed(Unconfirmed::WrongOwner)
    );
    assert_eq!(c.blocked(), 0);
    c.denied_after_latch();
    let mut c = Case::new();
    c.app.db.execute_batch("BEGIN IMMEDIATE").unwrap();
    c.health.block(&c.app, Cause::CommitUnknown);
    assert_eq!(
        c.health.report(),
        Report::NotConfirmed(Unconfirmed::Unsettled)
    );
    assert!(!c.app.db.is_autocommit());
    c.denied_after_latch();
    assert!(!c.app.db.is_autocommit());
    c.app.db.execute_batch("ROLLBACK").unwrap(); // Fixture teardown only; health stays blocked.
    assert_ne!(c.health.report(), Report::Ready);
}
#[test]
fn busy_storage_and_begin_failure_block_before_command_callback() {
    let mut c = Case::new();
    c.app
        .db
        .busy_timeout(std::time::Duration::from_millis(10))
        .unwrap();
    c.db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let raw = c.app.db.execute_batch("BEGIN IMMEDIATE").unwrap_err();
    println!("S3_SQLITE real_busy={raw:?}");
    assert_eq!(
        raw.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    let callbacks = AtomicI64::new(0);
    assert_eq!(
        c.app.transaction(|_, _| {
            callbacks.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Err(Error::Database)
    );
    assert_eq!(callbacks.load(Ordering::SeqCst), 0);
    assert_eq!(
        c.health.report(),
        Report::NotConfirmed(Unconfirmed::KnownNotCommitted)
    );
    c.db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(c.health.retry_block_record(&c.app), Report::Durable);
    c.denied_after_latch();
}
#[test]
fn per_phase_response_loss_never_replays_and_preserves_committed_phase() {
    for stop in [
        Point::Published,
        Point::IdentityCommitted,
        Point::ObservationCommitted,
        Point::ReceiptCommitted,
    ] {
        let mut c = Case::new();
        c.deleted();
        assert!(
            c.app
                .settle_marker_with_hooks(
                    id(10),
                    Hooks {
                        stop: Some(stop),
                        ..Hooks::default()
                    }
                )
                .is_err()
        );
        assert!(c.app.db.is_autocommit());
        assert_eq!(c.health.publish_calls.load(Ordering::SeqCst), 1);
        let identity: i64 =
            c.db.query_row(
                "SELECT marker_identity IS NOT NULL FROM deletion_work",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let obs: i64 =
            c.db.query_row("SELECT count(*) FROM deletion_observation", [], |r| {
                r.get(0)
            })
            .unwrap();
        let receipt: i64 =
            c.db.query_row("SELECT count(*) FROM deletion_receipt", [], |r| r.get(0))
                .unwrap();
        assert_eq!(identity, i64::from(stop != Point::Published));
        assert_eq!(
            obs,
            i64::from(matches!(
                stop,
                Point::ObservationCommitted | Point::ReceiptCommitted
            ))
        );
        assert_eq!(receipt, i64::from(stop == Point::ReceiptCommitted));
        c.denied_after_latch();
        assert!(c.app.settle_marker(id(10)).is_err());
        assert_eq!(c.health.publish_calls.load(Ordering::SeqCst), 1);
        println!(
            "S3_SIMULATED phase_stop={} identity={identity} observation={obs} receipt={receipt} retry_body=false",
            stop as u8
        );
    }
}
#[test]
fn real_deferred_commit_failure_rolls_back_identity_and_quarantines_owner() {
    let mut c = Case::new();
    c.deleted();
    let before = c.snapshot_except_health();
    assert!(
        c.app
            .settle_marker_with_hooks(
                id(10),
                Hooks {
                    deferred_failure: true,
                    ..Hooks::default()
                }
            )
            .is_err()
    );
    assert!(c.app.db.is_autocommit());
    assert_eq!(before, c.snapshot_except_health());
    assert_eq!(c.blocked(), 1);
    c.denied_after_latch();
    assert_eq!(c.health.publish_calls.load(Ordering::SeqCst), 1);
    assert_eq!(*c.health.cause.lock().unwrap(), Some(Cause::CommitUnknown));
}
#[test]
fn concurrent_connections_share_marker_health_and_one_publication() {
    let mut c = Case::new();
    c.deleted();
    let mut other = Application::attach(
        Connection::open(&c.path).unwrap(),
        id(2),
        id(1),
        id(3),
        c.app.order.clone(),
        Arc::new(TestClock),
    )
    .unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let b = barrier.clone();
    let join = std::thread::spawn(move || {
        b.wait();
        other.settle_marker(id(10))
    });
    barrier.wait();
    assert_eq!(c.app.settle_marker(id(10)), Ok(State::Settled));
    assert_eq!(join.join().unwrap(), Ok(State::Settled));
    assert_eq!(c.health.publish_calls.load(Ordering::SeqCst), 1);
    assert_eq!(c.health.report(), Report::Ready);
    c.app.reconcile().unwrap();
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM deletion_receipt", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn later_source_deletion_reuses_original_canonical_marker_without_republishing() {
    let mut c = Case::new();
    let original = c.deleted();
    assert_eq!(c.app.settle_marker(id(10)), Ok(State::Settled));
    let before = c.work();
    let event = c
        .app
        .close(actor(), id(91), Target::Source(id(60)))
        .unwrap();
    c.app.fence(actor(), event).unwrap();
    c.app.apply(actor(), event).unwrap();
    assert_ne!(event, original);
    let after = c.work();
    assert_eq!(after.event, original);
    assert_eq!(after.bytes, before.bytes);
    assert_eq!(after.ledger.identity, before.ledger.identity);
    assert_eq!(c.app.settle_marker(id(10)), Ok(State::Settled));
    assert_eq!(c.health.publish_calls.load(Ordering::SeqCst), 1);
    assert_eq!(c.app.settle_marker(id(20)), Ok(State::Settled));
    assert_eq!(c.health.publish_calls.load(Ordering::SeqCst), 2);
    c.app.reconcile().unwrap();
    c.app.admit_metadata(actor(), id(31)).unwrap();
    crate::profile12::attest(&c.app.db).unwrap();
    assert!(crate::profile12::foreign_keys_safe(&c.app.db).unwrap());
}
#[test]
fn no_fresh_token_for_existing_database_and_no_successful_clean_stop_issuer() {
    let c = Case::new();
    let io = MarkerStore::open(
        &c.path.parent().unwrap().join("markers"),
        &c.path.parent().unwrap().join("payload"),
        &c.path.parent().unwrap().join("backups"),
    )
    .unwrap();
    let mut reopened = Connection::open(&c.path).unwrap();
    assert!(OwnerHealth::initialize_fresh(&mut reopened, &io).is_err());
    assert!(c.health.clean_stop_allowed(&c.app).is_err());
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM stop_receipt", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.db.query_row("SELECT count(*) FROM resume_receipt", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
}
