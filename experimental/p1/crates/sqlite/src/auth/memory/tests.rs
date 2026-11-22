use super::*;
use std::{
    sync::{
        Arc, Barrier,
        atomic::{AtomicI64, Ordering},
    },
    thread,
};
use tempfile::{TempDir, tempdir};
use yixu_application::memory::MemoryApplication;
use yixu_sqlite_test_support::{SyntheticSource, blocked_payload};

#[derive(Clone)]
struct TestClock(Arc<AtomicI64>);
impl Clock for TestClock {
    fn utc_millis(&self) -> Result<i64> {
        Ok(self.0.load(Ordering::SeqCst))
    }
    fn monotonic_ticks(&self) -> u64 {
        0
    }
}
fn id(n: u8) -> Id {
    Id::from_bytes([n; 16])
}
fn scope() -> Scope {
    Scope {
        tenant: id(1),
        space: id(3),
    }
}
fn source() -> SyntheticSource {
    SyntheticSource {
        event: id(20),
        evidence: id(21),
        body: id(10),
        locator: id(11),
    }
}
fn create(key: u8, payload: u8) -> MemoryWrite {
    MemoryWrite {
        scope: scope(),
        target: MemoryTarget::Create,
        evidence: vec![id(21)],
        primary_evidence: id(21),
        content_payload: id(payload),
        idempotency: id(key),
    }
}
fn revise(key: u8, payload: u8, receipt: MemoryReceipt) -> MemoryWrite {
    MemoryWrite {
        target: MemoryTarget::Revise {
            memory: receipt.memory,
            expected_version: receipt.version,
        },
        ..create(key, payload)
    }
}
struct Fixture {
    _dir: TempDir,
    path: std::path::PathBuf,
    db: AuthDatabase<TestClock>,
    clock: TestClock,
    authority: BootstrapAuthority,
}
impl Fixture {
    fn empty() -> Self {
        let dir = tempdir().unwrap();
        let path = dir.path().join("m1.sqlite");
        assert!(
            path.to_string_lossy().starts_with("C:"),
            "SQLite acceptance must run on C: NTFS"
        );
        let clock = TestClock(Arc::new(AtomicI64::new(100)));
        let mut db = AuthDatabase::open_profile(&path, clock.clone(), 4).unwrap();
        let authority = db.bootstrap(id(90)).unwrap();
        for (tenant, principal, subject, space) in
            [(id(1), id(2), id(4), id(3)), (id(5), id(6), id(8), id(7))]
        {
            db.administer(
                &authority,
                AdminChange::Tenant {
                    tenant,
                    principal,
                    subject,
                    space,
                },
            )
            .unwrap();
        }
        for capability in ["memory.append", "memory.revise"] {
            db.administer(
                &authority,
                AdminChange::MemoryGrant {
                    scope: scope(),
                    principal: id(2),
                    capability,
                    enabled: true,
                    expires_at_ms: 1000,
                },
            )
            .unwrap();
        }
        for payload in 12..=18 {
            blocked_payload(&db.connection, scope(), id(payload), false).unwrap();
        }
        Self {
            _dir: dir,
            path,
            db,
            clock,
            authority,
        }
    }
    fn new() -> Self {
        let mut f = Self::empty();
        let tx =
            f.db.connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
        tx.execute(
            "UPDATE tenant SET knowledge_seq=1 WHERE id=?1",
            [id(1).to_string()],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO commit_record VALUES(?1,1,0)",
            [id(1).to_string()],
        )
        .unwrap();
        source().seed(&tx, scope(), id(2), 1).unwrap();
        tx.commit().unwrap();
        f
    }
    fn context(&self) -> AuthContext {
        self.db
            .identify(TrustedIdentity { subject: id(4) }, id(3))
            .unwrap()
    }
    fn open(&self) -> AuthDatabase<TestClock> {
        AuthDatabase::open_profile(&self.path, self.clock.clone(), 4).unwrap()
    }
    fn count(&self, table: &str) -> i64 {
        self.db
            .connection
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
    fn seq(&self) -> i64 {
        self.db
            .connection
            .query_row(
                "SELECT knowledge_seq FROM tenant WHERE id=?1",
                [id(1).to_string()],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn apply(&mut self, c: MemoryWrite) -> Result<MemoryReceipt> {
        let ctx = self.context();
        self.db.apply_memory(Some(&ctx), c)
    }
    fn grant(&mut self, capability: &'static str, enabled: bool) {
        self.db
            .administer(
                &self.authority,
                AdminChange::MemoryGrant {
                    scope: scope(),
                    principal: id(2),
                    capability,
                    enabled,
                    expires_at_ms: 1000,
                },
            )
            .unwrap();
    }
    fn snapshot(&self) -> Vec<i64> {
        [
            "memory",
            "revision",
            "revision_evidence",
            "memory_retry",
            "memory_audit",
            "outbox_job",
            "commit_record",
        ]
        .into_iter()
        .map(|t| self.count(t))
        .chain([self.seq()])
        .collect()
    }
    fn second_principal(&mut self, other_space: bool) -> AuthContext {
        self.db
            .connection
            .execute(
                "INSERT INTO principal VALUES(?1,?2,'user','active')",
                params![id(1).to_string(), id(22).to_string()],
            )
            .unwrap();
        self.db
            .connection
            .execute(
                "INSERT INTO identity_binding VALUES(?1,?2,?3,1)",
                params![id(24).to_string(), id(1).to_string(), id(22).to_string()],
            )
            .unwrap();
        let space = if other_space { id(23) } else { id(3) };
        if other_space {
            self.db
                .administer(
                    &self.authority,
                    AdminChange::CreateSpace {
                        scope: Scope {
                            tenant: id(1),
                            space,
                        },
                        owner: id(22),
                    },
                )
                .unwrap();
        }
        for capability in ["memory.append", "memory.revise"] {
            self.db
                .administer(
                    &self.authority,
                    AdminChange::MemoryGrant {
                        scope: Scope {
                            tenant: id(1),
                            space,
                        },
                        principal: id(22),
                        capability,
                        enabled: true,
                        expires_at_ms: 1000,
                    },
                )
                .unwrap();
        }
        self.db
            .identify(TrustedIdentity { subject: id(24) }, space)
            .unwrap()
    }
}
#[test]
fn atomic_source_memory_revision_graph_has_real_fks_and_no_admission() {
    let mut f = Fixture::empty();
    f.db.source_fixture = Some(source());
    let ctx = f.context();
    let mut app = MemoryApplication::assemble(f.open());
    // Fixture can be installed only on the private internal assembly, not via Application DTO.
    let first = f.db.apply_memory(Some(&ctx), create(40, 12)).unwrap();
    assert_eq!(
        (first.version, first.revision_number, first.knowledge_seq),
        (1, 1, 1)
    );
    for table in [
        "event",
        "evidence",
        "memory",
        "revision",
        "revision_evidence",
        "memory_retry",
        "memory_audit",
        "outbox_job",
    ] {
        assert_eq!(f.count(table), 1, "{table}");
    }
    let replay = app.execute(Some(&ctx), create(40, 12)).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.memory, first.memory);
    let c = &f.db.connection;
    assert_eq!(
        c.pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        c.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        c.query_row("SELECT state FROM memory", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "source_blocked"
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM payload WHERE state<>'blocked'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    println!(
        "real SQLite {}; database on C: NTFS; blocked graph seq=1",
        rusqlite::version()
    );
}
#[test]
fn missing_evidence_and_foreign_primary_do_not_commit() {
    let mut f = Fixture::new();
    let baseline = f.snapshot();
    for evidence in [vec![], vec![id(21), id(21)], vec![id(99)]] {
        let mut c = create(40, 12);
        c.evidence = evidence;
        assert_eq!(f.apply(c), Err(Error::InvalidInput));
        assert_eq!(f.snapshot(), baseline);
    }
    let mut c = create(40, 12);
    c.primary_evidence = id(99);
    assert_eq!(f.apply(c), Err(Error::InvalidInput));
    assert_eq!(f.snapshot(), baseline);
}
#[test]
fn absent_reference_is_rejected_before_sql_constraint_feedback() {
    let mut f = Fixture::new();
    let baseline = f.snapshot();
    let mut c = create(40, 12);
    c.evidence = vec![id(99)];
    c.primary_evidence = id(99);
    assert_eq!(f.apply(c), Err(Error::AuthorizationDenied));
    assert_eq!(f.snapshot(), baseline);
}
#[test]
fn contiguous_revision_chain_and_immutable_history() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let b = f.apply(revise(41, 13, a)).unwrap();
    let c = f.apply(revise(42, 14, b)).unwrap();
    assert_eq!((c.version, c.revision_number), (3, 3));
    assert_eq!(f.count("revision"), 3);
    let parent: (String, i64) =
        f.db.connection
            .query_row(
                "SELECT parent_revision_id,parent_number FROM revision WHERE id=?1",
                [c.revision.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
    assert_eq!(parent, (b.revision.to_string(), 2));
    assert!(
        f.db.connection
            .execute(
                "UPDATE revision SET number=99 WHERE id=?1",
                [a.revision.to_string()]
            )
            .is_err()
    );
    assert!(
        f.db.connection
            .execute("DELETE FROM revision WHERE id=?1", [a.revision.to_string()])
            .is_err()
    );
    assert!(
        f.db.connection
            .execute(
                "DELETE FROM revision_evidence WHERE revision_id=?1",
                [a.revision.to_string()]
            )
            .is_err()
    );
}
#[test]
fn aggregate_version_is_not_revision_number() {
    let mut f = Fixture::new();
    let mut a = f.apply(create(40, 12)).unwrap();
    // Privileged synthetic fixture simulates an independent aggregate metadata version bump.
    f.db.connection
        .execute(
            "UPDATE memory SET version=7 WHERE id=?1",
            [a.memory.to_string()],
        )
        .unwrap();
    a.version = 7;
    let b = f.apply(revise(41, 13, a)).unwrap();
    assert_eq!((b.version, b.revision_number), (8, 2));
}
#[test]
fn stale_cas_leaves_no_revision_retry_outbox_or_sequence() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    f.apply(revise(41, 13, a)).unwrap();
    let baseline = f.snapshot();
    assert_eq!(f.apply(revise(42, 14, a)), Err(Error::CasConflict));
    assert_eq!(f.snapshot(), baseline);
}
#[test]
fn two_connections_same_version_have_exactly_one_winner() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let ctx = f.context();
    let gate = Arc::new(Barrier::new(3));
    let joins: Vec<_> = (0..2)
        .map(|n| {
            let mut db = f.open();
            let ctx = ctx.clone();
            let gate = gate.clone();
            thread::spawn(move || {
                gate.wait();
                db.apply_memory(Some(&ctx), revise(41 + n, 13 + n, a))
            })
        })
        .collect();
    gate.wait();
    let results: Vec<_> = joins.into_iter().map(|j| j.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(Error::CasConflict))
            .count(),
        1
    );
    assert_eq!(f.count("revision"), 2);
    assert_eq!(f.count("memory_retry"), 2);
    assert_eq!(f.seq(), 3);
}
#[test]
fn late_sql_failure_rolls_back_entire_fresh_source_graph() {
    let mut f = Fixture::empty();
    f.db.source_fixture = Some(source());
    let baseline = f.snapshot();
    f.db.connection.execute_batch("CREATE TEMP TRIGGER fail_retry BEFORE INSERT ON main.memory_retry BEGIN SELECT RAISE(ABORT,'test-fault'); END;").unwrap();
    assert_eq!(f.apply(create(40, 12)), Err(Error::Constraint));
    assert_eq!(f.snapshot(), baseline);
    assert_eq!(f.count("event"), 0);
    assert_eq!(f.count("evidence"), 0);
    assert_eq!(f.count("payload"), 7);
    f.db.connection
        .execute_batch("DROP TRIGGER temp.fail_retry")
        .unwrap();
    f.db.source_fixture = Some(source());
    assert_eq!(f.apply(create(40, 12)).unwrap().knowledge_seq, 1);
}
#[test]
fn late_revision_failure_preserves_old_current_and_history() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let baseline = f.snapshot();
    f.db.connection.execute_batch("CREATE TEMP TRIGGER fail_audit BEFORE INSERT ON main.memory_audit BEGIN SELECT RAISE(ABORT,'test-fault'); END;").unwrap();
    assert_eq!(f.apply(revise(41, 13, a)), Err(Error::Constraint));
    assert_eq!(f.snapshot(), baseline);
    let current: String =
        f.db.connection
            .query_row("SELECT current_revision_id FROM memory", [], |r| r.get(0))
            .unwrap();
    assert_eq!(current, a.revision.to_string());
}
#[test]
fn reopen_response_loss_replays_without_new_business_facts() {
    let mut f = Fixture::new();
    let ctx = f.context();
    let a = f.apply(create(40, 12)).unwrap();
    let baseline = f.snapshot();
    let mut reopened = f.open();
    let b = reopened.apply_memory(Some(&ctx), create(40, 12)).unwrap();
    assert!(b.replayed);
    assert_eq!(
        (a.memory, a.revision, a.knowledge_seq),
        (b.memory, b.revision, b.knowledge_seq)
    );
    assert_eq!(f.snapshot(), baseline);
    let mut changed = create(40, 13);
    assert_eq!(f.apply(changed.clone()), Err(Error::IdempotencyConflict));
    changed.target = MemoryTarget::Revise {
        memory: a.memory,
        expected_version: 1,
    };
    assert_eq!(f.apply(changed), Err(Error::IdempotencyConflict));
}
#[test]
fn revision_replay_returns_original_even_after_newer_revision() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let command = revise(41, 13, a);
    let b = f.apply(command.clone()).unwrap();
    f.apply(revise(42, 14, b)).unwrap();
    let baseline = f.snapshot();
    let replay = f.apply(command).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.revision, b.revision);
    assert_eq!(replay.version, 2);
    assert_eq!(f.snapshot(), baseline);
}
#[test]
fn same_key_other_principal_cannot_read_first_receipt() {
    let mut f = Fixture::new();
    let b = f.second_principal(false);
    let a = f.apply(create(40, 12)).unwrap();
    let other = f.db.apply_memory(Some(&b), create(40, 13)).unwrap();
    assert_ne!(a.memory, other.memory);
    assert!(!other.replayed);
    assert_eq!(
        f.db.apply_memory(Some(&b), create(40, 12)),
        Err(Error::IdempotencyConflict)
    );
}
#[test]
fn reordered_evidence_is_same_canonical_request_and_shared_sources_survive() {
    let mut f = Fixture::new();
    f.db.connection.execute("INSERT INTO evidence SELECT tenant_id,space_id,?1,event_id,payload_id,locator_payload_id,source_kind,captured_at_ms FROM evidence",[id(25).to_string()]).unwrap();
    let mut c = create(40, 12);
    c.evidence.push(id(25));
    let a = f.apply(c.clone()).unwrap();
    c.evidence.reverse();
    assert!(f.apply(c).unwrap().replayed);
    let b = f.apply(create(41, 13)).unwrap();
    assert_ne!(a.memory, b.memory);
    assert_eq!(f.count("evidence"), 2);
    assert_eq!(f.count("event"), 1);
    assert_eq!(f.count("revision_evidence"), 3);
}
#[test]
fn source_payload_or_used_revision_payload_cannot_be_aliased() {
    let mut f = Fixture::new();
    assert_eq!(f.apply(create(40, 10)), Err(Error::ResourceUnavailable));
    f.apply(create(40, 12)).unwrap();
    let baseline = f.snapshot();
    assert_eq!(f.apply(create(41, 12)), Err(Error::ResourceUnavailable));
    assert_eq!(f.snapshot(), baseline);
}
#[test]
fn missing_grant_metadata_only_grant_and_wrong_scope_deny() {
    let mut f = Fixture::new();
    f.grant("memory.append", false);
    f.db.administer(
        &f.authority,
        AdminChange::Grant {
            scope: scope(),
            principal: id(2),
            enabled: true,
            expires_at_ms: 1000,
        },
    )
    .unwrap();
    assert_eq!(f.apply(create(40, 12)), Err(Error::AuthorizationDenied));
    assert_eq!(
        f.db.apply_memory(None, create(40, 12)),
        Err(Error::AuthorizationDenied)
    );
    f.grant("memory.append", true);
    let ctx = f.context();
    let mut c = create(40, 12);
    c.scope.space = id(7);
    assert_eq!(
        f.db.apply_memory(Some(&ctx), c),
        Err(Error::AuthorizationDenied)
    );
}
#[test]
fn append_grant_never_implies_revise_grant() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    f.grant("memory.revise", false);
    assert_eq!(f.apply(revise(41, 13, a)), Err(Error::AuthorizationDenied));
}
#[test]
fn revoked_context_and_old_idempotency_key_are_denied() {
    let mut f = Fixture::new();
    let ctx = f.context();
    f.db.apply_memory(Some(&ctx), create(40, 12)).unwrap();
    let baseline = f.snapshot();
    f.grant("memory.append", false);
    assert_eq!(
        f.db.apply_memory(Some(&ctx), create(40, 12)),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(f.snapshot(), baseline);
    assert_eq!(f.apply(create(41, 13)), Err(Error::AuthorizationDenied));
}
#[test]
fn expiry_at_final_check_rolls_back_and_retry_cannot_bypass() {
    let mut f = Fixture::new();
    let clock = f.clock.clone();
    let baseline = f.snapshot();
    f.db.before_commit = Some(Box::new(move || {
        clock.0.store(1000, Ordering::SeqCst);
    }));
    assert_eq!(f.apply(create(40, 12)), Err(Error::AuthorizationDenied));
    assert_eq!(f.snapshot(), baseline);
    assert_eq!(f.apply(create(40, 12)), Err(Error::AuthorizationDenied));
}
#[test]
fn disabled_principal_cannot_write_with_old_context() {
    let mut f = Fixture::new();
    let ctx = f.context();
    f.db.administer(
        &f.authority,
        AdminChange::Principal {
            tenant: id(1),
            principal: id(2),
            enabled: false,
        },
    )
    .unwrap();
    assert_eq!(
        f.db.apply_memory(Some(&ctx), create(40, 12)),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(f.count("memory"), 0);
}
#[test]
fn foreign_and_absent_evidence_payload_and_memory_are_indistinguishable() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let b = f.second_principal(true);
    let other = b.scope;
    for payload in [60, 61] {
        blocked_payload(&f.db.connection, other, id(payload), false).unwrap();
    }
    // Use independent opaque fixture IDs for B's legal source, preserving the global PK.
    let own = SyntheticSource {
        event: id(50),
        evidence: id(51),
        body: id(52),
        locator: id(53),
    };
    own.seed(&f.db.connection, other, id(22), 1).unwrap();
    let mut base = create(41, 60);
    base.scope = other;
    base.evidence = vec![id(51)];
    base.primary_evidence = id(51);
    for foreign in [id(21), id(99)] {
        let mut c = base.clone();
        c.evidence = vec![foreign];
        c.primary_evidence = foreign;
        assert_eq!(
            f.db.apply_memory(Some(&b), c),
            Err(Error::AuthorizationDenied)
        );
    }
    // These references are checked only inside B's Space; A's occupied payload is not visible.
    for foreign in [id(10), id(98)] {
        let mut c = base.clone();
        c.content_payload = foreign;
        assert_eq!(
            f.db.apply_memory(Some(&b), c),
            Err(Error::AuthorizationDenied)
        );
    }
    for foreign in [a.memory, id(97)] {
        let mut c = base.clone();
        c.target = MemoryTarget::Revise {
            memory: foreign,
            expected_version: 1,
        };
        assert_eq!(
            f.db.apply_memory(Some(&b), c),
            Err(Error::AuthorizationDenied)
        );
    }
    assert!(f.db.apply_memory(Some(&b), base).is_ok());
}
#[test]
fn cross_tenant_context_and_source_reference_are_denied() {
    let mut f = Fixture::new();
    let other = Scope {
        tenant: id(5),
        space: id(7),
    };
    f.db.administer(
        &f.authority,
        AdminChange::MemoryGrant {
            scope: other,
            principal: id(6),
            capability: "memory.append",
            enabled: true,
            expires_at_ms: 1000,
        },
    )
    .unwrap();
    let b =
        f.db.identify(TrustedIdentity { subject: id(8) }, id(7))
            .unwrap();
    let ctx = f.context();
    let mut c = create(40, 12);
    c.scope = other;
    assert_eq!(
        f.db.apply_memory(Some(&ctx), c.clone()),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(
        f.db.apply_memory(Some(&b), c),
        Err(Error::AuthorizationDenied)
    );
}
#[test]
fn old_profile_three_refuses_memory_operations_and_profiles_do_not_upgrade() {
    let d = tempdir().unwrap();
    let p = d.path().join("v3.sqlite");
    let clock = TestClock(Arc::new(AtomicI64::new(100)));
    let mut old = AuthDatabase::open(&p, clock.clone()).unwrap();
    assert_eq!(
        old.apply_memory(None, create(40, 12)),
        Err(Error::CapabilityUnavailable)
    );
    drop(old);
    assert!(matches!(
        AuthDatabase::open_profile(&p, clock, 4),
        Err(Error::IncompatibleSchema)
    ));
    let f = Fixture::new();
    assert!(matches!(
        AuthDatabase::open(&f.path, f.clock.clone()),
        Err(Error::IncompatibleSchema)
    ));
}
#[test]
fn m1_schema_drift_still_fails_closed_on_reopen() {
    for sql in [
        "DROP TRIGGER payload_no_admission",
        "DROP TABLE memory_audit",
        "CREATE TABLE unexpected(x INTEGER)",
    ] {
        let f = Fixture::new();
        f.db.connection.execute_batch(sql).unwrap();
        assert!(matches!(
            AuthDatabase::open_profile(&f.path, f.clock.clone(), 4),
            Err(Error::IncompatibleSchema)
        ));
    }
}
#[test]
fn formal_memory_runtime_and_content_operations_remain_unavailable() {
    let mut app = MemoryApplication::assemble(yixu_application::NoContentRuntime::default());
    assert_eq!(
        app.execute(None, create(40, 12)),
        Err(Error::CapabilityUnavailable)
    );
    let f = Fixture::new();
    let runtime = yixu_application::NoContentRuntime::default();
    assert!(matches!(
        runtime.hydrate(scope(), id(12)),
        Err(Error::CapabilityUnavailable)
    ));
    assert_eq!(
        runtime.erase(scope(), id(12)),
        Err(Error::CapabilityUnavailable)
    );
    assert_eq!(runtime.restore(scope()), Err(Error::CapabilityUnavailable));
    assert_eq!(f.count("memory"), 0);
}

// Deliberately bypass orchestration ONLY in private tests to exercise SQLite's constraints.
fn raw_revision(
    f: &mut Fixture,
    memory: Id,
    number: i64,
    parent: Option<Id>,
    parent_number: Option<i64>,
    primary: Id,
) -> rusqlite::Result<()> {
    let tx =
        f.db.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO revision VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'user_assertion',100,NULL,100,2)",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(99).to_string(),
            memory.to_string(),
            number,
            parent.map(|p| p.to_string()),
            parent_number,
            primary.to_string(),
            id(14).to_string()
        ],
    )?;
    tx.execute(
        "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,'supports')",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(99).to_string(),
            id(21).to_string()
        ],
    )?;
    tx.commit()
}
#[test]
fn sqlite_rejects_primary_without_matching_revision_evidence_row() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let baseline = f.snapshot();
    // Evidence exists globally and in this Space, but not as a relation of the NEW Revision.
    f.db.connection.execute("INSERT INTO evidence SELECT tenant_id,space_id,?1,event_id,payload_id,locator_payload_id,source_kind,captured_at_ms FROM evidence",[id(25).to_string()]).unwrap();
    assert!(raw_revision(&mut f, a.memory, 2, Some(a.revision), Some(1), id(25)).is_err());
    assert_eq!(f.snapshot(), baseline);
}
#[test]
fn sqlite_rejects_current_revision_of_another_memory_at_commit() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let b = f.apply(create(41, 13)).unwrap();
    let baseline = f.snapshot();
    let tx =
        f.db.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
    tx.execute(
        "UPDATE memory SET current_revision_id=?1 WHERE id=?2",
        params![b.revision.to_string(), a.memory.to_string()],
    )
    .unwrap();
    assert!(tx.commit().is_err());
    assert_eq!(f.snapshot(), baseline);
}
#[test]
fn sqlite_rejects_parent_from_another_memory_at_commit() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let b = f.apply(create(41, 13)).unwrap();
    let baseline = f.snapshot();
    assert!(raw_revision(&mut f, a.memory, 2, Some(b.revision), Some(1), id(21)).is_err());
    assert_eq!(f.snapshot(), baseline);
}
#[test]
fn sqlite_rejects_number_gap_wrong_parent_number_and_null_bypass() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let baseline = f.snapshot();
    for (number, parent_number) in [(3, Some(2)), (2, Some(0)), (2, None)] {
        assert!(
            raw_revision(
                &mut f,
                a.memory,
                number,
                Some(a.revision),
                parent_number,
                id(21)
            )
            .is_err()
        );
        assert_eq!(f.snapshot(), baseline);
    }
}
#[test]
fn sqlite_rejects_revision_with_no_evidence_even_when_memory_exists() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let baseline = f.snapshot();
    let tx =
        f.db.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
    tx.execute(
        "INSERT INTO revision VALUES(?1,?2,?3,?4,2,?5,1,?6,?7,'user_assertion',100,NULL,100,2)",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(99).to_string(),
            a.memory.to_string(),
            a.revision.to_string(),
            id(21).to_string(),
            id(14).to_string()
        ],
    )
    .unwrap();
    assert!(tx.commit().is_err());
    assert_eq!(f.snapshot(), baseline);
}
#[test]
fn ordinary_dto_has_no_temporal_conflict_or_source_reactivation_path() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    f.db.connection
        .execute(
            "UPDATE memory SET state='deletion_pending' WHERE id=?1",
            [a.memory.to_string()],
        )
        .unwrap();
    let baseline = f.snapshot();
    assert_eq!(
        f.apply(revise(41, 13, a)),
        Err(Error::CapabilityUnavailable)
    );
    assert_eq!(f.apply(create(40, 12)), Err(Error::CapabilityUnavailable));
    assert_eq!(f.snapshot(), baseline);
    assert!(
        f.db.connection
            .execute(
                "UPDATE memory SET state='active' WHERE id=?1",
                [a.memory.to_string()]
            )
            .is_err()
    );
    assert!(
        f.db.connection
            .execute(
                "UPDATE payload SET state='available' WHERE id=?1",
                [id(12).to_string()]
            )
            .is_err()
    );
}
#[test]
fn grant_revoke_and_memory_write_serialize_on_actual_sqlite_writer_lock() {
    use std::sync::mpsc;
    // Revoke takes the write lock first: a queued old request cannot commit after it.
    let mut f = Fixture::new();
    let ctx = f.context();
    let mut admin = f.open();
    let mut writer = f.open();
    let authority = BootstrapAuthority {
        realm: f.authority.realm,
    };
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (attempt_tx, attempt_rx) = mpsc::channel();
    admin.before_commit = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let revoke = thread::spawn(move || {
        admin.administer(
            &authority,
            AdminChange::MemoryGrant {
                scope: scope(),
                principal: id(2),
                capability: "memory.append",
                enabled: false,
                expires_at_ms: 1000,
            },
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let write = thread::spawn(move || {
        attempt_tx.send(()).unwrap();
        writer.apply_memory(Some(&ctx), create(40, 12))
    });
    attempt_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    release_tx.send(()).unwrap();
    revoke.join().unwrap().unwrap();
    assert_eq!(write.join().unwrap(), Err(Error::AuthorizationDenied));
    assert_eq!(f.count("memory"), 0);
    assert_eq!(f.seq(), 1);
    // Write takes the lock first: it may commit once BEFORE revoke, never after revoke commits.
    f.grant("memory.append", true);
    let ctx = f.context();
    let mut writer = f.open();
    let mut admin = f.open();
    let authority = BootstrapAuthority {
        realm: f.authority.realm,
    };
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    writer.before_commit = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let owned = ctx.clone();
    let write = thread::spawn(move || writer.apply_memory(Some(&owned), create(41, 12)));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let revoke = thread::spawn(move || {
        admin.administer(
            &authority,
            AdminChange::MemoryGrant {
                scope: scope(),
                principal: id(2),
                capability: "memory.append",
                enabled: false,
                expires_at_ms: 1000,
            },
        )
    });
    release_tx.send(()).unwrap();
    write.join().unwrap().unwrap();
    revoke.join().unwrap().unwrap();
    assert_eq!(
        f.db.apply_memory(Some(&ctx), create(41, 12)),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(f.count("memory"), 1);
}
#[test]
fn concurrent_identical_idempotency_commands_create_one_fact() {
    let f = Fixture::new();
    let ctx = f.context();
    let gate = Arc::new(Barrier::new(3));
    let joins: Vec<_> = (0..2)
        .map(|_| {
            let mut db = f.open();
            let ctx = ctx.clone();
            let gate = gate.clone();
            thread::spawn(move || {
                gate.wait();
                db.apply_memory(Some(&ctx), create(40, 12))
            })
        })
        .collect();
    gate.wait();
    let results: Vec<_> = joins
        .into_iter()
        .map(|j| j.join().unwrap().unwrap())
        .collect();
    assert_eq!(results[0].memory, results[1].memory);
    assert_eq!(results.iter().filter(|r| r.replayed).count(), 1);
    assert_eq!(f.count("memory"), 1);
    assert_eq!(f.count("revision"), 1);
    assert_eq!(f.count("memory_retry"), 1);
    assert_eq!(f.seq(), 2);
}
struct ScriptedIds(std::collections::VecDeque<Result<Id>>);
impl ResourceIds for ScriptedIds {
    fn next(&mut self) -> Result<Id> {
        self.0
            .pop_front()
            .unwrap_or(Err(Error::ResourceUnavailable))
    }
}
#[test]
fn server_id_collision_is_retried_and_exhaustion_has_no_partial_effects() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    f.db.ids = Box::new(ScriptedIds(
        vec![
            Ok(a.memory),
            Ok(id(70)),
            Ok(a.revision),
            Ok(id(71)),
            Ok(id(72)),
        ]
        .into(),
    ));
    let b = f.apply(create(41, 13)).unwrap();
    assert_eq!((b.memory, b.revision), (id(70), id(71)));
    let baseline = f.snapshot();
    f.db.ids = Box::new(ScriptedIds(vec![Ok(a.memory); 8].into()));
    assert_eq!(f.apply(create(42, 14)), Err(Error::ResourceUnavailable));
    assert_eq!(f.snapshot(), baseline);
    f.db.ids = Box::new(ScriptedIds(vec![Err(Error::Database)].into()));
    assert_eq!(f.apply(create(42, 14)), Err(Error::ResourceUnavailable));
    assert_eq!(f.snapshot(), baseline);
    // Committed receipt replay never consults failed entropy or allocates another resource.
    assert!(f.apply(create(41, 13)).unwrap().replayed);
}

#[test]
fn recorded_deletion_and_invalidation_never_reactivate_or_replay_old_revision() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let tx =
        f.db.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
    tx.execute("INSERT INTO deletion_request(tenant_id,space_id,id,operation,requested_scope,effective_scope,target_type,target_id,memory_id,status,graph_cut_off,closure_ref,closure_version,policy_epoch,deletion_generation,requested_by,coverage) VALUES(?1,?2,?3,'memory_graph','space','space','memory',?4,?4,'blocked',2,?5,1,1,1,?6,'pending')",params![id(1).to_string(),id(3).to_string(),id(80).to_string(),a.memory.to_string(),id(81).to_string(),id(2).to_string()]).unwrap();
    tx.execute(
        "INSERT INTO deletion_closure VALUES(?1,?2,?3,?4,1)",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(80).to_string(),
            id(81).to_string()
        ],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO revision_invalidation VALUES(?1,?2,?3,?4,?5,1,1)",
        params![
            id(1).to_string(),
            id(82).to_string(),
            id(3).to_string(),
            a.revision.to_string(),
            id(80).to_string()
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    let baseline = f.snapshot();
    for c in [create(40, 12), create(41, 13), revise(42, 14, a)] {
        assert_eq!(f.apply(c), Err(Error::CapabilityUnavailable));
        assert_eq!(f.snapshot(), baseline);
    }
    assert_eq!(f.count("revision_invalidation"), 1);
}

#[test]
fn legal_same_ids_and_same_key_in_different_tenants_remain_independent() {
    let mut f = Fixture::new();
    let a = f.apply(create(40, 12)).unwrap();
    let other = Scope {
        tenant: id(5),
        space: id(7),
    };
    f.db.administer(
        &f.authority,
        AdminChange::MemoryGrant {
            scope: other,
            principal: id(6),
            capability: "memory.append",
            enabled: true,
            expires_at_ms: 1000,
        },
    )
    .unwrap();
    let tx = f.db.connection.transaction().unwrap();
    tx.execute(
        "UPDATE tenant SET knowledge_seq=1 WHERE id=?1",
        [id(5).to_string()],
    )
    .unwrap();
    tx.execute(
        "INSERT INTO commit_record VALUES(?1,1,0)",
        [id(5).to_string()],
    )
    .unwrap();
    source().seed(&tx, other, id(6), 1).unwrap();
    blocked_payload(&tx, other, id(12), false).unwrap();
    tx.commit().unwrap();
    let b =
        f.db.identify(TrustedIdentity { subject: id(8) }, id(7))
            .unwrap();
    f.db.ids = Box::new(ScriptedIds(
        vec![Ok(a.memory), Ok(a.revision), Ok(id(70))].into(),
    ));
    let mut command = create(40, 12);
    command.scope = other;
    let result = f.db.apply_memory(Some(&b), command).unwrap();
    assert!(!result.replayed);
    assert_eq!(result.memory, a.memory);
    assert_eq!(result.revision, a.revision);
    assert_eq!(f.count("memory"), 2);
    assert_eq!(f.count("memory_retry"), 2);
    assert_eq!(
        f.db.connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        0
    );
}
