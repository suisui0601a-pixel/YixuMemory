use rusqlite::{Connection, params};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Barrier},
    time::{Duration, Instant},
};
use tempfile::{TempDir, tempdir};
use yixu_domain::{Error, EvidenceEnvelope, Id, RevisionEnvelope, Scope};
use yixu_ports::{MemoryRepository, MetadataTransaction, UnitOfWork};
use yixu_sqlite_test_support::{APPLICATION_ID, MetadataLab, MetadataTx, SCHEMA_VERSION};

fn id(n: u64) -> Id {
    let mut bytes = [0; 16];
    bytes[6] = 0x40;
    bytes[8] = 0x80;
    bytes[8..].copy_from_slice(&(n | (0x80_u64 << 56)).to_be_bytes());
    Id::from_bytes(bytes)
}
fn scope() -> Scope {
    Scope {
        tenant: id(1),
        space: id(3),
    }
}
fn raw(path: &Path) -> Connection {
    let c = Connection::open(path).unwrap();
    c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=1000;")
        .unwrap();
    c
}
fn lab(path: &Path) -> MetadataLab {
    MetadataLab::open(path, Duration::from_millis(120)).unwrap()
}
fn count(path: &Path, table: &str) -> i64 {
    // Table names are hard-coded by these tests, never a public query interface.
    raw(path)
        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
struct Fixture {
    _dir: TempDir,
    path: PathBuf,
    db: MetadataLab,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempdir().unwrap();
        let path = dir.path().join("synthetic.sqlite");
        let mut db = lab(&path);
        db.create_tenant(id(1)).unwrap();
        let mut tx = db.begin(id(1)).unwrap();
        tx.add_principal(id(2)).unwrap();
        tx.add_personal_space(id(3), id(2)).unwrap();
        tx.add_personal_space(id(4), id(2)).unwrap();
        tx.commit().unwrap();
        db.create_tenant(id(5)).unwrap();
        let mut tx = db.begin(id(5)).unwrap();
        tx.add_principal(id(6)).unwrap();
        tx.add_personal_space(id(7), id(6)).unwrap();
        tx.commit().unwrap();
        // Explicit blocked placeholders only. There are no content bytes or real key refs.
        // These rows validate SQL relations, NOT production Payload admission or encryption.
        let c = raw(&path);
        for (n, purpose) in [
            (10, "content"),
            (11, "locator"),
            (12, "content"),
            (13, "content"),
            (14, "content"),
        ] {
            c.execute("INSERT INTO payload VALUES(?1,?2,?3,?4,NULL,'sensitive',1,?5,?6,'metadata-fixture-not-encrypted',?7,1,'blocked')",params![id(1).to_string(),id(3).to_string(),id(n).to_string(),purpose,id(100+n).to_string(),id(200+n).to_string(),id(300+n).to_string()]).unwrap();
        }
        Self {
            _dir: dir,
            path,
            db,
        }
    }
    fn graph(&mut self) -> u64 {
        let mut tx = self.db.begin(id(1)).unwrap();
        let seq = tx.knowledge_seq();
        tx.append_system_event(id(3), id(20), id(2)).unwrap();
        tx.add_evidence_envelope(evidence(id(21), id(20))).unwrap();
        tx.add_blocked_memory_envelope(id(3), id(30), id(31))
            .unwrap();
        tx.add_revision_envelope(revision(id(30), id(31), 1, None))
            .unwrap();
        tx.link_evidence(id(3), id(31), id(21)).unwrap();
        tx.enqueue_metadata(id(3), id(40), id(30), 1).unwrap();
        tx.commit().unwrap();
        seq
    }
}
fn evidence(e: Id, event: Id) -> EvidenceEnvelope {
    EvidenceEnvelope {
        scope: scope(),
        id: e,
        event,
        payload: id(10),
        locator: id(11),
    }
}
fn revision(memory: Id, rev: Id, number: u64, parent: Option<Id>) -> RevisionEnvelope {
    RevisionEnvelope {
        scope: scope(),
        id: rev,
        memory,
        number,
        parent,
        primary_evidence: id(21),
        content_payload: id(if number == 1 { 12 } else { 13 }),
        valid_from_ms: 0,
        valid_to_ms: None,
    }
}
fn second(tx: &mut MetadataTx<'_>) -> Result<(), Error> {
    tx.add_revision_envelope(revision(id(30), id(32), 2, Some(id(31))))?;
    tx.link_evidence(id(3), id(32), id(21))?;
    tx.cas_current(id(3), id(30), 1, id(32))?;
    tx.enqueue_metadata(id(3), id(41), id(30), 2)
}
fn assert_fk(result: rusqlite::Result<usize>) {
    match result {
        Err(rusqlite::Error::SqliteFailure(e, _)) => {
            assert_eq!(e.extended_code, rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY)
        }
        other => panic!("expected real FK error, got {other:?}"),
    }
}

#[test]
fn fresh_migration_is_strict_wal_and_foreign_keys_are_really_on() {
    let f = Fixture::new();
    assert!(f.db.foreign_keys_enabled().unwrap());
    let c = raw(&f.path);
    assert_eq!(
        c.pragma_query_value(None, "application_id", |r| r.get::<_, i64>(0))
            .unwrap(),
        APPLICATION_ID
    );
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        SCHEMA_VERSION
    );
    assert_eq!(
        c.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0))
            .unwrap(),
        "wal"
    );
    assert_eq!(
        c.query_row(
            "SELECT strict FROM pragma_table_list WHERE name='revision'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_fk(c.execute(
        "INSERT INTO principal VALUES(?1,?2,'user','active')",
        params![id(999).to_string(), id(998).to_string()],
    ));
    f.db.integrity_check().unwrap();
    println!("real SQLite {}", f.db.sqlite_version());
}
#[test]
fn first_memory_revision_cycle_is_atomic_and_durable() {
    let mut f = Fixture::new();
    let seq = f.graph();
    assert_eq!(seq, 2);
    assert_eq!(f.db.memory_version(scope(), id(30)).unwrap(), 1);
    let reopened = lab(&f.path);
    reopened.integrity_check().unwrap();
    assert_eq!(reopened.memory_version(scope(), id(30)).unwrap(), 1);
    assert_eq!(reopened.knowledge_seq(id(1)).unwrap(), 2);
    for (table, n) in [
        ("memory", 1),
        ("revision", 1),
        ("revision_evidence", 1),
        ("outbox_job", 1),
    ] {
        assert_eq!(count(&f.path, table), n);
    }
}
#[test]
fn evidence_cannot_reference_event_in_foreign_tenant() {
    let mut f = Fixture::new();
    let mut tx = f.db.begin(id(5)).unwrap();
    tx.append_system_event(id(7), id(22), id(6)).unwrap();
    tx.commit().unwrap();
    let mut tx = f.db.begin(id(1)).unwrap();
    assert_eq!(
        tx.add_evidence_envelope(evidence(id(21), id(22))),
        Err(Error::Constraint)
    );
    drop(tx);
    assert_eq!(count(&f.path, "evidence"), 0);
    let c = raw(&f.path);
    assert_fk(c.execute(
        "INSERT INTO evidence VALUES(?1,?2,?3,?4,?5,?6,'user_statement',0)",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(21).to_string(),
            id(22).to_string(),
            id(10).to_string(),
            id(11).to_string()
        ],
    ));
}
#[test]
fn evidence_cannot_reference_event_in_foreign_space() {
    let mut f = Fixture::new();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.append_system_event(id(4), id(22), id(2)).unwrap();
    tx.commit().unwrap();
    let mut tx = f.db.begin(id(1)).unwrap();
    assert_eq!(
        tx.add_evidence_envelope(evidence(id(21), id(22))),
        Err(Error::Constraint)
    );
}
#[test]
fn evidence_cannot_reference_payload_in_foreign_space() {
    let mut f = Fixture::new();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.append_system_event(id(3), id(20), id(2)).unwrap();
    tx.commit().unwrap();
    let c = raw(&f.path);
    c.execute("INSERT INTO payload VALUES(?1,?2,?3,'content',NULL,'sensitive',1,?4,?5,'metadata-fixture-not-encrypted',?6,1,'blocked')",params![id(1).to_string(),id(4).to_string(),id(15).to_string(),id(115).to_string(),id(215).to_string(),id(315).to_string()]).unwrap();
    let mut tx = f.db.begin(id(1)).unwrap();
    let mut e = evidence(id(21), id(20));
    e.payload = id(15);
    assert_eq!(tx.add_evidence_envelope(e), Err(Error::Constraint));
}
#[test]
fn transaction_scope_cannot_be_changed_by_envelope() {
    let mut f = Fixture::new();
    let mut tx = f.db.begin(id(1)).unwrap();
    let mut e = evidence(id(21), id(20));
    e.scope.tenant = id(5);
    assert_eq!(tx.add_evidence_envelope(e), Err(Error::Constraint));
}
#[test]
fn revision_without_evidence_fails_at_commit_and_rolls_back_everything() {
    let mut f = Fixture::new();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.add_blocked_memory_envelope(id(3), id(30), id(31))
        .unwrap();
    tx.add_revision_envelope(revision(id(30), id(31), 1, None))
        .unwrap();
    tx.enqueue_metadata(id(3), id(40), id(30), 1).unwrap();
    assert_eq!(tx.commit(), Err(Error::Constraint));
    for table in ["memory", "revision", "revision_evidence", "outbox_job"] {
        assert_eq!(count(&f.path, table), 0);
    }
    assert_eq!(f.db.knowledge_seq(id(1)).unwrap(), 1);
    // A failed deferred COMMIT must not leave a live transaction locking the next attempt.
    f.graph();
    f.db.integrity_check().unwrap();
}
#[test]
fn incomplete_current_pointer_cannot_be_committed() {
    let mut f = Fixture::new();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.add_blocked_memory_envelope(id(3), id(30), id(31))
        .unwrap();
    assert_eq!(tx.commit(), Err(Error::Constraint));
    assert_eq!(count(&f.path, "memory"), 0);
}
#[test]
fn current_revision_of_another_memory_is_rejected() {
    let mut f = Fixture::new();
    f.graph();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.add_blocked_memory_envelope(id(3), id(35), id(31))
        .unwrap();
    assert_eq!(tx.commit(), Err(Error::Constraint));
    assert_eq!(count(&f.path, "memory"), 1);
}
#[test]
fn current_revision_of_another_space_is_rejected() {
    let mut f = Fixture::new();
    f.graph();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.add_blocked_memory_envelope(id(4), id(35), id(31))
        .unwrap();
    assert_eq!(tx.commit(), Err(Error::Constraint));
}
#[test]
fn parent_must_be_immediately_previous_revision_of_same_memory() {
    let mut f = Fixture::new();
    f.graph();
    for (number, parent) in [(2, Some(id(999))), (3, Some(id(31)))] {
        let mut tx = f.db.begin(id(1)).unwrap();
        let mut v = revision(id(30), id(32), number, parent);
        v.content_payload = id(13);
        tx.add_revision_envelope(v).unwrap();
        tx.link_evidence(id(3), id(32), id(21)).unwrap();
        tx.cas_current(id(3), id(30), 1, id(32)).unwrap();
        assert_eq!(tx.commit(), Err(Error::Constraint));
    }
    assert_eq!(count(&f.path, "revision"), 1);
}

#[test]
fn nullable_parent_number_must_not_bypass_parent_fk_or_check() {
    let mut f = Fixture::new();
    f.graph();
    let c = raw(&f.path);
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    let result = c.execute(
        "INSERT INTO revision VALUES(?1,?2,?3,?4,2,?5,NULL,?6,?7,'user_assertion',0,NULL,0,2)",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(32).to_string(),
            id(30).to_string(),
            id(999).to_string(),
            id(21).to_string(),
            id(13).to_string()
        ],
    );
    assert!(
        result.is_err(),
        "SQL NULL must not turn parent CHECK into UNKNOWN and skip the composite FK"
    );
}
#[test]
fn parent_and_valid_time_shape_checks_are_real() {
    let mut f = Fixture::new();
    f.graph();
    for (number, parent, end) in [
        (1, Some(id(31)), None),
        (2, None, None),
        (2, Some(id(31)), Some(0)),
    ] {
        let mut tx = f.db.begin(id(1)).unwrap();
        let mut v = revision(id(30), id(32), number, parent);
        v.valid_to_ms = end;
        assert_eq!(tx.add_revision_envelope(v), Err(Error::Constraint));
    }
}
#[test]
fn old_current_pointer_is_not_silently_left_after_append() {
    let mut f = Fixture::new();
    f.graph();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.add_revision_envelope(revision(id(30), id(32), 2, Some(id(31))))
        .unwrap();
    tx.link_evidence(id(3), id(32), id(21)).unwrap();
    assert_eq!(tx.commit(), Err(Error::Constraint));
    assert_eq!(count(&f.path, "revision"), 1);
}
#[test]
fn dropped_transaction_removes_graph_and_does_not_consume_sequence() {
    let mut f = Fixture::new();
    {
        let mut tx = f.db.begin(id(1)).unwrap();
        assert_eq!(tx.knowledge_seq(), 2);
        tx.append_system_event(id(3), id(20), id(2)).unwrap();
        tx.add_blocked_memory_envelope(id(3), id(30), id(31))
            .unwrap();
    }
    assert_eq!(count(&f.path, "event"), 0);
    assert_eq!(count(&f.path, "memory"), 0);
    assert_eq!(f.db.knowledge_seq(id(1)).unwrap(), 1);
    assert_eq!(f.graph(), 2);
}
#[test]
fn two_clients_same_cas_version_only_one_can_commit() {
    let mut f = Fixture::new();
    f.graph();
    let barrier = Arc::new(Barrier::new(2));
    let mut joins = Vec::new();
    for _ in 0..2 {
        let path = f.path.clone();
        let gate = barrier.clone();
        joins.push(std::thread::spawn(move || {
            let mut db = lab(&path);
            assert_eq!(db.memory_version(scope(), id(30)).unwrap(), 1);
            gate.wait();
            let mut tx = db.begin(id(1))?;
            // CAS first so the loser is a semantic conflict, not a duplicate Revision PK.
            tx.cas_current(id(3), id(30), 1, id(32))?;
            tx.add_revision_envelope(revision(id(30), id(32), 2, Some(id(31))))?;
            tx.link_evidence(id(3), id(32), id(21))?;
            tx.enqueue_metadata(id(3), id(41), id(30), 2)?;
            tx.commit()
        }));
    }
    let results: Vec<_> = joins.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(Error::CasConflict))
            .count(),
        1
    );
    assert_eq!(f.db.memory_version(scope(), id(30)).unwrap(), 2);
    assert_eq!(count(&f.path, "revision"), 2);
    assert_eq!(count(&f.path, "outbox_job"), 2);
    assert_eq!(f.db.knowledge_seq(id(1)).unwrap(), 3);
}
#[test]
fn begin_immediate_blocks_other_writer_and_timeout_then_recovers() {
    let mut f = Fixture::new();
    let mut other = lab(&f.path);
    let tx = f.db.begin(id(1)).unwrap();
    let start = Instant::now();
    assert!(matches!(other.begin(id(1)), Err(Error::Busy)));
    assert!(start.elapsed() >= Duration::from_millis(80));
    assert!(start.elapsed() < Duration::from_secs(3));
    drop(tx);
    other.begin(id(1)).unwrap().commit().unwrap();
    assert_eq!(other.knowledge_seq(id(1)).unwrap(), 2);
}
#[test]
fn sequence_is_committed_order_and_separate_per_tenant() {
    let mut f = Fixture::new();
    let mut other = lab(&f.path);
    let mut a = f.db.begin(id(1)).unwrap();
    assert_eq!(a.knowledge_seq(), 2);
    a.append_system_event(id(3), id(20), id(2)).unwrap();
    a.commit().unwrap();
    let b = other.begin(id(1)).unwrap();
    assert_eq!(b.knowledge_seq(), 3);
    drop(b);
    let c = other.begin(id(1)).unwrap();
    assert_eq!(c.knowledge_seq(), 3);
    c.commit().unwrap();
    assert_eq!(f.db.knowledge_seq(id(5)).unwrap(), 1);
    assert_eq!(f.db.knowledge_seq(id(1)).unwrap(), 3);
}
#[test]
fn migration_repeated_execution_has_no_effect_on_data_or_sequence() {
    let mut f = Fixture::new();
    f.graph();
    for _ in 0..3 {
        let reopened = lab(&f.path);
        reopened.integrity_check().unwrap();
        assert_eq!(reopened.knowledge_seq(id(1)).unwrap(), 2);
    }
    assert_eq!(count(&f.path, "migration_record"), 1);
    assert_eq!(count(&f.path, "revision"), 1);
}
#[test]
fn migration_checksum_conflict_is_rejected() {
    let f = Fixture::new();
    raw(&f.path)
        .execute("UPDATE migration_record SET checksum='tampered'", [])
        .unwrap();
    assert!(matches!(
        MetadataLab::open(&f.path, Duration::from_millis(50)),
        Err(Error::MigrationChecksum)
    ));
}
#[test]
fn newer_schema_version_is_rejected_without_downgrade() {
    let f = Fixture::new();
    raw(&f.path).pragma_update(None, "user_version", 2).unwrap();
    assert!(matches!(
        MetadataLab::open(&f.path, Duration::from_millis(50)),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(
        raw(&f.path)
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}
#[test]
fn unknown_migration_record_is_rejected() {
    let f = Fixture::new();
    raw(&f.path)
        .execute(
            "INSERT INTO migration_record VALUES(2,'unknown','applied')",
            [],
        )
        .unwrap();
    assert!(matches!(
        MetadataLab::open(&f.path, Duration::from_millis(50)),
        Err(Error::IncompatibleSchema)
    ));
}
#[test]
fn unknown_database_is_never_overwritten_or_adopted() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("unrelated.sqlite");
    let c = raw(&path);
    c.execute_batch("CREATE TABLE sentinel(n INTEGER); INSERT INTO sentinel VALUES(7);")
        .unwrap();
    assert!(matches!(
        MetadataLab::open(&path, Duration::from_millis(50)),
        Err(Error::IncompatibleSchema)
    ));
    assert_eq!(
        c.query_row("SELECT n FROM sentinel", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        7
    );
    assert_eq!(count(&path, "sqlite_master"), 1);
}
#[test]
fn migration_ddl_is_atomic_when_mid_script_fails() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("migration-failure.sqlite");
    let mut c = raw(&path);
    {
        let tx = c
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .unwrap();
        tx.execute_batch("CREATE TABLE phase_one(n INTEGER);")
            .unwrap();
        assert!(tx.execute_batch("THIS IS NOT SQL;").is_err());
    }
    assert_eq!(count(&path, "sqlite_master"), 0);
    lab(&path).integrity_check().unwrap();
}
#[test]
fn event_and_revision_are_immutable_at_sql_boundary() {
    let mut f = Fixture::new();
    f.graph();
    let c = raw(&f.path);
    for sql in [
        "UPDATE event SET occurred_at_ms=1",
        "DELETE FROM event",
        "UPDATE revision SET valid_from_ms=1",
        "DELETE FROM revision",
        "DELETE FROM revision_evidence",
    ] {
        assert!(c.execute(sql, []).is_err());
    }
    assert_eq!(count(&f.path, "revision"), 1);
}
#[test]
fn idempotency_and_outbox_uniqueness_are_real() {
    let mut f = Fixture::new();
    f.graph();
    let c = raw(&f.path);
    c.execute(
        "INSERT INTO idempotency_record VALUES(?1,?2,?3,?4,'metadata_append',?5,?6,?7,2592000000)",
        params![
            id(1).to_string(),
            id(50).to_string(),
            id(2).to_string(),
            id(3).to_string(),
            vec![1u8; 32],
            vec![2u8; 32],
            id(30).to_string()
        ],
    )
    .unwrap();
    assert!(c.execute("INSERT INTO idempotency_record SELECT tenant_id,?1,actor_id,space_id,operation,key_lookup,request_digest,result_memory_id,expires_at_ms FROM idempotency_record",params![id(51).to_string()]).is_err());
    let mut tx = f.db.begin(id(1)).unwrap();
    assert_eq!(
        tx.enqueue_metadata(id(3), id(41), id(30), 1),
        Err(Error::Constraint)
    );
}
#[test]
fn synthetic_cannot_enable_sql_content_state_or_runtime_stage() {
    let mut f = Fixture::new();
    f.graph();
    let c = raw(&f.path);
    assert!(
        c.execute(
            "UPDATE payload SET declared_classification='synthetic',state='available'",
            []
        )
        .is_err()
    );
    assert!(c.execute("UPDATE memory SET state='active'", []).is_err());
    let runtime = yixu_application::NoContentRuntime::default();
    let tables_before = count(&f.path, "payload");
    assert_eq!(
        runtime.stage(
            scope(),
            yixu_domain::Classification::Synthetic,
            b"never-persist-this-synthetic-body"
        ),
        Err(Error::CapabilityUnavailable)
    );
    assert_eq!(count(&f.path, "payload"), tables_before);
    for entry in std::fs::read_dir(f.path.parent().unwrap()).unwrap() {
        let bytes = std::fs::read(entry.unwrap().path()).unwrap();
        assert!(
            !bytes
                .windows(b"never-persist-this-synthetic-body".len())
                .any(|b| b == b"never-persist-this-synthetic-body")
        );
    }
}
#[test]
fn cas_failure_after_partial_append_rolls_back_entire_attempt() {
    let mut f = Fixture::new();
    f.graph();
    let mut tx = f.db.begin(id(1)).unwrap();
    second(&mut tx).unwrap();
    tx.commit().unwrap();
    let before = f.db.knowledge_seq(id(1)).unwrap();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.append_system_event(id(3), id(22), id(2)).unwrap();
    assert_eq!(
        tx.cas_current(id(3), id(30), 1, id(32)),
        Err(Error::CasConflict)
    );
    drop(tx);
    assert_eq!(f.db.knowledge_seq(id(1)).unwrap(), before);
    assert_eq!(count(&f.path, "event"), 1);
}

#[test]
fn swallowed_statement_failure_still_poisons_whole_uow() {
    let mut f = Fixture::new();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.append_system_event(id(3), id(20), id(2)).unwrap();
    assert_eq!(
        tx.append_system_event(id(3), id(20), id(2)),
        Err(Error::Constraint)
    );
    assert_eq!(tx.commit(), Err(Error::Constraint));
    assert_eq!(count(&f.path, "event"), 0);
    assert_eq!(f.db.knowledge_seq(id(1)).unwrap(), 1);
}
#[test]
fn foreign_space_revision_evidence_link_is_rejected_by_composite_fk() {
    let mut f = Fixture::new();
    f.graph();
    let c = raw(&f.path);
    assert_fk(c.execute(
        "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,'context')",
        params![
            id(1).to_string(),
            id(4).to_string(),
            id(31).to_string(),
            id(21).to_string()
        ],
    ));
}
#[test]
fn revision_content_cannot_be_reused_as_shared_source() {
    let mut f = Fixture::new();
    f.graph();
    let mut tx = f.db.begin(id(1)).unwrap();
    let mut e = evidence(id(22), id(20));
    e.payload = id(12);
    assert_eq!(tx.add_evidence_envelope(e), Err(Error::Constraint));
    assert_eq!(tx.commit(), Err(Error::Constraint));
}
#[test]
fn legal_shared_evidence_and_independent_revision_payloads_survive() {
    let mut f = Fixture::new();
    f.graph();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.add_blocked_memory_envelope(id(3), id(35), id(36))
        .unwrap();
    let mut v = revision(id(35), id(36), 1, None);
    v.content_payload = id(14);
    tx.add_revision_envelope(v).unwrap();
    tx.link_evidence(id(3), id(36), id(21)).unwrap();
    tx.commit().unwrap();
    assert_eq!(count(&f.path, "memory"), 2);
    assert_eq!(count(&f.path, "evidence"), 1);
    assert_eq!(count(&f.path, "revision_evidence"), 2);
    f.db.integrity_check().unwrap();
}
#[test]
fn aggregate_cas_version_is_not_revision_number() {
    let mut f = Fixture::new();
    f.graph();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.cas_current(id(3), id(30), 1, id(31)).unwrap();
    tx.commit().unwrap();
    let mut tx = f.db.begin(id(1)).unwrap();
    tx.add_revision_envelope(revision(id(30), id(32), 2, Some(id(31))))
        .unwrap();
    tx.link_evidence(id(3), id(32), id(21)).unwrap();
    tx.cas_current(id(3), id(30), 2, id(32)).unwrap();
    tx.commit().unwrap();
    assert_eq!(f.db.memory_version(scope(), id(30)).unwrap(), 3);
    assert_eq!(
        raw(&f.path)
            .query_row("SELECT max(number) FROM revision", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

fn deletion_fixture(c: &Connection) {
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    c.execute("INSERT INTO deletion_request(tenant_id,space_id,id,operation,requested_scope,effective_scope,target_type,target_id,memory_id,status,graph_cut_off,closure_ref,closure_version,policy_epoch,deletion_generation,requested_by,coverage) VALUES(?1,?2,?3,'memory_graph','space','space','memory',?4,?4,'blocked',2,?5,1,1,1,?6,'pending')",params![id(1).to_string(),id(3).to_string(),id(70).to_string(),id(30).to_string(),id(71).to_string(),id(2).to_string()]).unwrap();
    c.execute(
        "INSERT INTO deletion_closure VALUES(?1,?2,?3,?4,1)",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(70).to_string(),
            id(71).to_string()
        ],
    )
    .unwrap();
    c.execute(
        "INSERT INTO copy_manifest VALUES(?1,?2,?3,?4,1,'payload','unknown')",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(72).to_string(),
            id(12).to_string()
        ],
    )
    .unwrap();
    c.execute_batch("COMMIT").unwrap();
}
#[test]
fn deletion_scope_cannot_be_broadened_or_completed_by_a_flag() {
    let mut f = Fixture::new();
    f.graph();
    let c = raw(&f.path);
    deletion_fixture(&c);
    for sql in [
        "UPDATE deletion_request SET requested_scope='tenant'",
        "UPDATE deletion_request SET operation='source_all_copies'",
        "UPDATE deletion_request SET status='completed',coverage='space_graph_complete',receipt_set_ref='00000000-0000-4000-8000-000000000001'",
    ] {
        assert!(c.execute(sql, []).is_err());
    }
    assert_eq!(
        c.query_row("SELECT status FROM deletion_request", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "blocked"
    );
}
#[test]
fn per_copy_receipts_require_exact_store_and_generations() {
    let mut f = Fixture::new();
    f.graph();
    let c = raw(&f.path);
    deletion_fixture(&c);
    for (store, seal, delete, closure) in [
        ("backup", 1, 1, 1),
        ("payload", 2, 1, 1),
        ("payload", 1, 2, 1),
        ("payload", 1, 1, 2),
    ] {
        assert_fk(c.execute(
            "INSERT INTO erasure_receipt VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'failed',NULL)",
            params![
                id(1).to_string(),
                id(73).to_string(),
                id(3).to_string(),
                id(70).to_string(),
                store,
                id(72).to_string(),
                seal,
                delete,
                closure
            ],
        ));
    }
    c.execute(
        "INSERT INTO erasure_receipt VALUES(?1,?2,?3,?4,'payload',?5,1,1,1,'failed',NULL)",
        params![
            id(1).to_string(),
            id(73).to_string(),
            id(3).to_string(),
            id(70).to_string(),
            id(72).to_string()
        ],
    )
    .unwrap();
    assert!(
        c.execute("UPDATE erasure_receipt SET status='verified'", [])
            .is_err()
    );
    assert_eq!(count(&f.path, "erasure_receipt"), 1);
}
#[test]
fn invalidation_rows_cannot_be_cancelled() {
    let mut f = Fixture::new();
    f.graph();
    let c = raw(&f.path);
    deletion_fixture(&c);
    c.execute(
        "INSERT INTO revision_invalidation VALUES(?1,?2,?3,?4,?5,1,1)",
        params![
            id(1).to_string(),
            id(74).to_string(),
            id(3).to_string(),
            id(31).to_string(),
            id(70).to_string()
        ],
    )
    .unwrap();
    assert!(c.execute("DELETE FROM revision_invalidation", []).is_err());
    assert!(
        c.execute("UPDATE revision_invalidation SET deletion_generation=2", [])
            .is_err()
    );
}

#[cfg(feature = "fault-injection")]
fn crash_child(committed: bool) {
    use std::io::{BufRead, BufReader};
    let f = Fixture::new();
    drop(f.db); // Child is the sole long-lived connection when terminated.
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_foundation-fault"));
    command.arg(&f.path);
    if committed {
        command.arg("commit");
    }
    let mut child = command
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut marker = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut marker)
        .unwrap();
    assert_eq!(
        marker.trim(),
        if committed {
            "COMMITTED"
        } else {
            "UNCOMMITTED"
        }
    );
    {
        let check = raw(&f.path);
        check.busy_timeout(Duration::from_millis(50)).unwrap();
        let locked = check.execute_batch("BEGIN IMMEDIATE");
        if committed {
            locked.unwrap();
            check.execute_batch("ROLLBACK").unwrap();
        } else {
            assert!(
                locked.is_err(),
                "Uncommitted child must still own the actual SQLite writer lock"
            );
        }
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let reopened = lab(&f.path);
    reopened.integrity_check().unwrap();
    assert_eq!(count(&f.path, "event"), i64::from(committed));
    assert_eq!(
        reopened.knowledge_seq(id(1)).unwrap(),
        if committed { 2 } else { 1 }
    );
}
#[test]
#[cfg(feature = "fault-injection")]
fn killed_process_before_commit_leaves_no_partial_data() {
    crash_child(false);
}
#[test]
#[cfg(feature = "fault-injection")]
fn killed_process_after_commit_preserves_committed_data() {
    crash_child(true);
}
