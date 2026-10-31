use super::*;
mod reopen_tests;
use std::{
    sync::{
        Arc, Barrier,
        atomic::{AtomicI64, Ordering},
        mpsc,
    },
    thread,
};
use tempfile::{TempDir, tempdir};
use yixu_application::memory_m2::MemoryM2Application;
use yixu_ports::memory::{AuthorizedMemory, MemoryTarget, MemoryWrite};
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
fn known(from: i64, to: Option<i64>) -> Validity {
    Validity::Known {
        from: Bound::exact(from),
        to: to.map(Bound::exact),
    }
}
fn command(key: u8, payload: u8, validity: Validity) -> TemporalWrite {
    TemporalWrite {
        write: MemoryWrite {
            scope: scope(),
            target: MemoryTarget::Create,
            evidence: vec![id(21)],
            primary_evidence: id(21),
            content_payload: id(payload),
            idempotency: id(key),
        },
        validity,
    }
}
fn revise(key: u8, payload: u8, previous: MemoryReceipt, validity: Validity) -> TemporalWrite {
    let mut c = command(key, payload, validity);
    c.write.target = MemoryTarget::Revise {
        memory: previous.memory,
        expected_version: previous.version,
    };
    c
}
fn query(mode: QueryMode) -> MetadataQuery {
    MetadataQuery {
        scope: scope(),
        mode,
        as_of: None,
        valid_at: None,
        include_uncertain: false,
        evidence: None,
        assertion_kind: None,
        state: MemoryState::SourceBlocked,
        retrieval: Retrieval::Metadata,
        limit: 100,
        cursor: None,
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
    fn new() -> Self {
        let dir = tempdir().unwrap();
        let path = dir.path().join("m2.sqlite");
        assert!(path.to_string_lossy().starts_with("C:"));
        let clock = TestClock(Arc::new(AtomicI64::new(100)));
        let mut db = AuthDatabase::open_profile(&path, clock.clone(), 5).unwrap();
        db.query_tokens = Some(Arc::new(tokens::TokenAuthority::random().unwrap()));
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
            for capability in [
                "memory.append",
                "memory.revise",
                "memory.read",
                "memory.history",
                "memory.search",
            ] {
                db.administer(
                    &authority,
                    AdminChange::MemoryGrant {
                        scope: Scope { tenant, space },
                        principal,
                        capability,
                        enabled: true,
                        expires_at_ms: 10_000,
                    },
                )
                .unwrap();
            }
        }
        for n in 12..=19 {
            blocked_payload(&db.connection, scope(), id(n), false).unwrap();
        }
        let mut f = Self {
            _dir: dir,
            path,
            db,
            clock,
            authority,
        };
        f.source(scope(), id(2), 20, 21, 10, 11);
        f
    }
    fn source(
        &mut self,
        scope: Scope,
        principal: Id,
        event: u8,
        evidence: u8,
        body: u8,
        locator: u8,
    ) {
        let tx = self
            .db
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        tx.execute(
            "UPDATE tenant SET knowledge_seq=knowledge_seq+1 WHERE id=?1",
            [scope.tenant.to_string()],
        )
        .unwrap();
        let seq: i64 = tx
            .query_row(
                "SELECT knowledge_seq FROM tenant WHERE id=?1",
                [scope.tenant.to_string()],
                |r| r.get(0),
            )
            .unwrap();
        tx.execute(
            "INSERT INTO commit_record VALUES(?1,?2,100)",
            params![scope.tenant.to_string(), seq],
        )
        .unwrap();
        SyntheticSource {
            event: id(event),
            evidence: id(evidence),
            body: id(body),
            locator: id(locator),
        }
        .seed_at(&tx, scope, principal, seq, (10, 100))
        .unwrap();
        tx.commit().unwrap();
    }
    fn context(&self) -> AuthContext {
        self.db
            .identify(TrustedIdentity { subject: id(4) }, id(3))
            .unwrap()
    }
    fn open(&self) -> AuthDatabase<TestClock> {
        let mut db = AuthDatabase::open_profile(&self.path, self.clock.clone(), 5).unwrap();
        db.query_tokens = self.db.query_tokens.clone();
        db
    }
    fn write(&mut self, c: TemporalWrite) -> Result<MemoryReceipt> {
        let ctx = self.context();
        self.db.write_temporal(Some(&ctx), c)
    }
    fn read(&mut self, q: MetadataQuery) -> Result<MetadataPage> {
        let ctx = self.context();
        self.db.query_metadata(Some(&ctx), q)
    }
    fn conflict(&mut self, target: ConflictTarget, key: u8) -> Result<ConflictReceipt> {
        let ctx = self.context();
        self.db.write_conflict(
            Some(&ctx),
            ConflictWrite {
                scope: scope(),
                target,
                idempotency: id(key),
            },
        )
    }
    fn history(&mut self, id: Id, as_of: Option<KnowledgeToken>) -> Result<ConflictHistory> {
        let ctx = self.context();
        self.db.conflict_history(
            Some(&ctx),
            ConflictQuery {
                scope: scope(),
                conflict: id,
                as_of,
            },
        )
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
    fn grant(&mut self, capability: &'static str, enabled: bool) {
        self.db
            .administer(
                &self.authority,
                AdminChange::MemoryGrant {
                    scope: scope(),
                    principal: id(2),
                    capability,
                    enabled,
                    expires_at_ms: 10_000,
                },
            )
            .unwrap();
    }
    fn snapshot(&self) -> Vec<i64> {
        [
            "memory",
            "revision",
            "revision_time",
            "memory_retry",
            "outbox_job",
            "conflict",
            "conflict_decision",
            "conflict_retry",
            "conflict_outbox",
            "event",
            "commit_record",
        ]
        .into_iter()
        .map(|t| self.count(t))
        .chain([self.seq()])
        .collect()
    }
    fn pair(&mut self) -> (MemoryReceipt, MemoryReceipt) {
        let a = self.write(command(40, 12, known(0, Some(100)))).unwrap();
        self.source(scope(), id(2), 24, 25, 60, 61);
        let mut c = command(41, 13, known(0, Some(100)));
        c.write.evidence = vec![id(25)];
        c.write.primary_evidence = id(25);
        let b = self.write(c).unwrap();
        (a, b)
    }
}
#[test]
fn late_event_validity_and_acceptance_are_distinct_in_real_database() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, known(50, Some(80)))).unwrap();
    let rows = f.read(query(QueryMode::One(a.memory))).unwrap().rows;
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r.recorded_at_ms, 100);
    assert_eq!(r.evidence[0].occurred_at_ms, 10);
    assert_eq!(r.evidence[0].recorded_at_ms, 100);
    assert_eq!(r.validity, known(50, Some(80)));
    assert_eq!(r.knowledge_seq, 2);
    assert!(!r.content_available);
    assert_eq!(r.state, MemoryState::SourceBlocked);
    assert_eq!(f.db.connection.query_row("SELECT count(*) FROM pragma_table_info('revision') WHERE name IN ('valid_from_ms','valid_to_ms')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(
        f.db.connection
            .query_row(
                "SELECT accepted_placeholder_from_ms FROM revision",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        100
    );
    println!(
        "M2 actual bundled SQLite {}; C: NTFS; Profile 5; no content",
        rusqlite::version()
    );
}
#[test]
fn retrospective_correction_respects_both_knowledge_snapshots() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, known(50, Some(80)))).unwrap();
    let old = f.read(query(QueryMode::One(a.memory))).unwrap().snapshot;
    f.write(revise(41, 13, a, known(20, Some(80)))).unwrap();
    let mut q = query(QueryMode::One(a.memory));
    q.valid_at = Some(30);
    q.as_of = Some(old);
    assert!(f.read(q.clone()).unwrap().rows.is_empty());
    q.as_of = None;
    let rows = f.read(q).unwrap().rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].number, 2);
}
#[test]
fn future_and_half_open_interval_boundaries_are_exact() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, known(200, Some(300)))).unwrap();
    for (instant, count) in [(199, 0), (200, 1), (299, 1), (300, 0)] {
        let mut q = query(QueryMode::One(a.memory));
        q.valid_at = Some(instant);
        assert_eq!(f.read(q).unwrap().rows.len(), count);
    }
}
#[test]
fn unknown_is_not_forever_and_uncertainty_is_explicit() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    let mut q = query(QueryMode::One(a.memory));
    q.valid_at = Some(100);
    assert!(f.read(q.clone()).unwrap().rows.is_empty());
    q.include_uncertain = true;
    assert_eq!(
        f.read(q).unwrap().rows[0].temporal_match,
        Some(TemporalMatch::Unknown)
    );
    let uncertain = Validity::Known {
        from: Bound {
            earliest_ms: 50,
            latest_ms: 80,
            precision: Precision::Uncertain,
        },
        to: Some(Bound::exact(200)),
    };
    let b = f.write(command(41, 13, uncertain)).unwrap();
    let mut q = query(QueryMode::One(b.memory));
    q.valid_at = Some(60);
    assert!(f.read(q.clone()).unwrap().rows.is_empty());
    q.include_uncertain = true;
    assert_eq!(
        f.read(q).unwrap().rows[0].temporal_match,
        Some(TemporalMatch::Possible)
    );
}
#[test]
fn day_month_year_precision_survives_storage_without_clock_substitution() {
    let mut f = Fixture::new();
    for (index, precision) in [Precision::Day, Precision::Month, Precision::Year]
        .into_iter()
        .enumerate()
    {
        let v = Validity::Known {
            from: Bound {
                earliest_ms: 0,
                latest_ms: match precision {
                    Precision::Day => 86_399_999,
                    Precision::Month => 2_678_399_999,
                    Precision::Year => 31_535_999_999,
                    _ => unreachable!(),
                },
                precision,
            },
            to: None,
        };
        let a = f
            .write(command(40 + index as u8, 12 + index as u8, v))
            .unwrap();
        let row = f
            .read(query(QueryMode::One(a.memory)))
            .unwrap()
            .rows
            .remove(0);
        assert_eq!(row.validity, v);
        assert_ne!(row.validity, known(100, None));
    }
}
#[test]
fn invalid_or_empty_validity_never_consumes_sequence_or_rows() {
    let mut f = Fixture::new();
    let before = f.snapshot();
    for v in [
        known(5, Some(5)),
        known(8, Some(5)),
        Validity::Known {
            from: Bound {
                earliest_ms: 9,
                latest_ms: 8,
                precision: Precision::Year,
            },
            to: None,
        },
        Validity::Known {
            from: Bound {
                earliest_ms: 0,
                latest_ms: 9,
                precision: Precision::Millisecond,
            },
            to: None,
        },
    ] {
        assert_eq!(f.write(command(40, 12, v)), Err(Error::InvalidInput));
        assert_eq!(f.snapshot(), before);
    }
}
#[test]
fn wall_clock_can_move_without_reordering_authoritative_knowledge() {
    let mut f = Fixture::new();
    f.clock.0.store(200, Ordering::SeqCst);
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    f.clock.0.store(150, Ordering::SeqCst);
    let b = f.write(command(41, 13, Validity::Unknown)).unwrap();
    let rows = f.read(query(QueryMode::Current)).unwrap().rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].revision, a.revision);
    assert_eq!(rows[1].revision, b.revision);
    assert!(rows[0].recorded_at_ms > rows[1].recorded_at_ms);
    assert!(rows[0].knowledge_seq < rows[1].knowledge_seq);
}
#[test]
fn latest_then_validity_never_resurrects_superseded_revision() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, known(0, Some(100)))).unwrap();
    f.write(revise(41, 13, a, known(200, None))).unwrap();
    let mut q = query(QueryMode::One(a.memory));
    q.valid_at = Some(50);
    assert!(f.read(q.clone()).unwrap().rows.is_empty());
    q.mode = QueryMode::History(a.memory);
    let history = f.read(q).unwrap().rows;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].number, 1);
}
#[test]
fn immutable_version_projection_keeps_old_pages_identical_after_future_revisions() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    let initial = f.read(query(QueryMode::One(a.memory))).unwrap();
    f.write(revise(41, 13, a, known(0, None))).unwrap();
    let mut q = query(QueryMode::One(a.memory));
    q.as_of = Some(initial.snapshot.clone());
    assert_eq!(f.read(q).unwrap(), initial);
    assert_eq!(initial.rows[0].memory_version_at_revision, 1);
}
#[test]
fn aggregate_version_and_revision_number_remain_separate_in_reads() {
    let mut f = Fixture::new();
    let mut a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    f.db.connection
        .execute(
            "UPDATE memory SET version=7 WHERE id=?1",
            [a.memory.to_string()],
        )
        .unwrap();
    a.version = 7;
    f.write(revise(41, 13, a, Validity::Unknown)).unwrap();
    let row = f
        .read(query(QueryMode::One(a.memory)))
        .unwrap()
        .rows
        .remove(0);
    assert_eq!((row.memory_version_at_revision, row.number), (8, 2));
}
#[test]
fn stale_current_is_integrity_error_even_for_history_and_no_auto_repair() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    f.write(revise(41, 13, a, Validity::Unknown)).unwrap();
    f.db.connection
        .execute(
            "UPDATE memory SET current_revision_id=?1 WHERE id=?2",
            params![a.revision.to_string(), a.memory.to_string()],
        )
        .unwrap();
    for mode in [
        QueryMode::One(a.memory),
        QueryMode::History(a.memory),
        QueryMode::Current,
    ] {
        assert_eq!(f.read(query(mode)), Err(Error::Integrity));
    }
    assert_eq!(
        f.db.connection
            .query_row("SELECT current_revision_id FROM memory", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        a.revision.to_string()
    );
}
#[test]
fn wrong_memory_and_missing_current_are_detected_in_same_read_snapshot() {
    for missing in [false, true] {
        let mut f = Fixture::new();
        let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
        let b = f.write(command(41, 13, Validity::Unknown)).unwrap();
        f.db.connection
            .execute_batch("PRAGMA foreign_keys=OFF")
            .unwrap();
        f.db.connection
            .execute(
                "UPDATE memory SET current_revision_id=?1 WHERE id=?2",
                params![
                    if missing { id(250) } else { b.revision }.to_string(),
                    a.memory.to_string()
                ],
            )
            .unwrap();
        f.db.connection
            .execute_batch("PRAGMA foreign_keys=ON")
            .unwrap();
        assert_eq!(
            f.read(query(QueryMode::One(a.memory))),
            Err(Error::Integrity)
        );
    }
}
#[test]
fn missing_historical_revision_gap_is_detected_without_repair() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    let b = f.write(revise(41, 13, a, Validity::Unknown)).unwrap();
    f.write(revise(42, 14, b, Validity::Unknown)).unwrap();
    // Privileged corruption injection, not an application path. Restore exact trusted trigger definition.
    f.db.connection
        .execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER revision_immutable_delete;")
        .unwrap();
    f.db.connection
        .execute("DELETE FROM revision WHERE id=?1", [b.revision.to_string()])
        .unwrap();
    f.db.connection.execute_batch("CREATE TRIGGER revision_immutable_delete BEFORE DELETE ON revision BEGIN SELECT RAISE(ABORT,'immutable'); END; PRAGMA foreign_keys=ON;").unwrap();
    assert_eq!(
        f.read(query(QueryMode::History(a.memory))),
        Err(Error::Integrity)
    );
    assert_eq!(f.count("revision"), 2);
}
#[test]
fn absent_time_row_is_not_silently_filled_from_acceptance_time() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    f.db.connection
        .execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER revision_time_no_delete;")
        .unwrap();
    f.db.connection
        .execute(
            "DELETE FROM revision_time WHERE revision_id=?1",
            [a.revision.to_string()],
        )
        .unwrap();
    f.db.connection.execute_batch("CREATE TRIGGER revision_time_no_delete BEFORE DELETE ON revision_time BEGIN SELECT RAISE(ABORT,'immutable'); END; PRAGMA foreign_keys=ON;").unwrap();
    assert_eq!(
        f.read(query(QueryMode::One(a.memory))),
        Err(Error::Integrity)
    );
    assert_eq!(f.count("revision_time"), 0);
}
#[test]
fn stable_pagination_ignores_later_commits_and_checks_query_binding() {
    let mut f = Fixture::new();
    for i in 0..3 {
        f.write(command(40 + i, 12 + i, Validity::Unknown)).unwrap();
    }
    let mut q = query(QueryMode::Current);
    q.limit = 1;
    let first = f.read(q.clone()).unwrap();
    assert!(first.next_cursor.is_some());
    f.write(command(44, 15, Validity::Unknown)).unwrap();
    let mut rows = first.rows.clone();
    let mut cursor = first.next_cursor.clone();
    while let Some(c) = cursor {
        let mut next = q.clone();
        next.cursor = Some(c);
        let page = f.read(next).unwrap();
        assert_eq!(page.snapshot, first.snapshot);
        rows.extend(page.rows);
        cursor = page.next_cursor;
    }
    assert_eq!(rows.len(), 3);
    let mut bad = q.clone();
    bad.cursor = first.next_cursor.clone();
    bad.limit = 2;
    assert_eq!(f.read(bad), Err(Error::InvalidCursor));
    let mut bad = q.clone();
    bad.cursor = first.next_cursor.clone();
    bad.valid_at = Some(0);
    assert_eq!(f.read(bad), Err(Error::InvalidCursor));
    let current = f.read(query(QueryMode::Current)).unwrap();
    let mut bad = q;
    bad.cursor = first.next_cursor;
    bad.as_of = Some(current.snapshot);
    assert_eq!(f.read(bad), Err(Error::InvalidCursor));
}
#[test]
fn filters_are_scoped_and_deterministic() {
    let mut f = Fixture::new();
    f.write(command(40, 12, known(0, None))).unwrap();
    let mut q = query(QueryMode::Current);
    q.evidence = Some(id(21));
    q.assertion_kind = Some(AssertionKind::UserAssertion);
    assert_eq!(f.read(q.clone()).unwrap().rows.len(), 1);
    q.evidence = Some(id(250));
    assert!(f.read(q.clone()).unwrap().rows.is_empty());
    q.evidence = None;
    q.assertion_kind = Some(AssertionKind::ModelInference);
    assert!(f.read(q).unwrap().rows.is_empty());
}
#[test]
fn token_tampering_wrong_tenant_and_disabled_signing_authority_fail_closed() {
    let mut f = Fixture::new();
    f.write(command(40, 12, Validity::Unknown)).unwrap();
    let token = f.read(query(QueryMode::Current)).unwrap().snapshot;
    let mut q = query(QueryMode::Current);
    let mut changed = token.clone();
    changed.0[0] ^= 1;
    q.as_of = Some(changed);
    assert_eq!(f.read(q.clone()), Err(Error::InvalidToken));
    q.as_of = Some(KnowledgeToken(vec![]));
    assert_eq!(f.read(q), Err(Error::InvalidToken));
    let other =
        f.db.identify(TrustedIdentity { subject: id(8) }, id(7))
            .unwrap();
    let mut q = query(QueryMode::Current);
    q.scope = other.scope;
    q.as_of = Some(token);
    assert_eq!(
        f.db.query_metadata(Some(&other), q),
        Err(Error::InvalidToken)
    );
    f.db.query_tokens = None;
    assert_eq!(
        f.read(query(QueryMode::Current)),
        Err(Error::CapabilityUnavailable)
    );
}
#[test]
fn query_authentication_and_capability_are_not_implied_by_write_grant() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    assert_eq!(
        f.db.query_metadata(None, query(QueryMode::Current)),
        Err(Error::AuthorizationDenied)
    );
    for (capability, mode) in [
        ("memory.read", QueryMode::One(a.memory)),
        ("memory.history", QueryMode::History(a.memory)),
        ("memory.search", QueryMode::Current),
    ] {
        f.grant(capability, false);
        assert_eq!(f.read(query(mode)), Err(Error::AuthorizationDenied));
        f.grant(capability, true);
    }
}
#[test]
fn revoked_context_and_old_cursor_are_not_authority() {
    let mut f = Fixture::new();
    f.write(command(40, 12, Validity::Unknown)).unwrap();
    f.write(command(41, 13, Validity::Unknown)).unwrap();
    let mut q = query(QueryMode::Current);
    q.limit = 1;
    let page = f.read(q.clone()).unwrap();
    let old = f.context();
    f.grant("memory.search", false);
    assert_eq!(
        f.db.query_metadata(Some(&old), q.clone()),
        Err(Error::AuthorizationDenied)
    );
    f.grant("memory.search", true);
    q.cursor = page.next_cursor;
    assert_eq!(f.read(q), Err(Error::InvalidCursor));
}
#[test]
fn final_read_expiry_discards_materialized_rows_and_audit() {
    let mut f = Fixture::new();
    f.write(command(40, 12, Validity::Unknown)).unwrap();
    let clock = f.clock.clone();
    f.db.before_commit = Some(Box::new(move || {
        clock.0.store(10_000, Ordering::SeqCst);
    }));
    let before = f.count("metadata_read_audit");
    assert_eq!(
        f.read(query(QueryMode::Current)),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(f.count("metadata_read_audit"), before);
}
#[test]
fn cross_tenant_query_and_foreign_memory_never_expose_existing_ids() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    let b =
        f.db.identify(TrustedIdentity { subject: id(8) }, id(7))
            .unwrap();
    for target in [a.memory, id(250)] {
        let mut q = query(QueryMode::One(target));
        q.scope = b.scope;
        assert!(f.db.query_metadata(Some(&b), q).unwrap().rows.is_empty());
    }
    let mut q = query(QueryMode::Current);
    q.scope = b.scope;
    let a = f.context();
    assert_eq!(
        f.db.query_metadata(Some(&a), q),
        Err(Error::AuthorizationDenied)
    );
}
#[test]
fn unsupported_retrieval_never_creates_a_text_index() {
    let mut f = Fixture::new();
    for mode in [
        Retrieval::FullText,
        Retrieval::Bm25,
        Retrieval::Graph,
        Retrieval::Vector,
        Retrieval::TemporalScore,
    ] {
        let mut q = query(QueryMode::Current);
        q.retrieval = mode;
        assert_eq!(f.read(q), Err(Error::UnsupportedQuery));
    }
    assert_eq!(
        f.db.connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE sql LIKE '%VIRTUAL TABLE%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}
#[test]
fn pagination_limits_reject_without_partial_output() {
    let mut f = Fixture::new();
    for limit in [0, 101, u16::MAX] {
        let mut q = query(QueryMode::Current);
        q.limit = limit;
        assert_eq!(f.read(q), Err(Error::InvalidInput));
    }
}
#[test]
fn temporal_write_failure_and_digest_mismatch_preserve_all_old_invariants() {
    let mut f = Fixture::new();
    let before = f.snapshot();
    f.db.connection.execute_batch("CREATE TEMP TRIGGER fail_time BEFORE INSERT ON main.revision_time BEGIN SELECT RAISE(ABORT,'fault'); END;").unwrap();
    assert_eq!(
        f.write(command(40, 12, Validity::Unknown)),
        Err(Error::Constraint)
    );
    assert_eq!(f.snapshot(), before);
    f.db.connection
        .execute_batch("DROP TRIGGER temp.fail_time")
        .unwrap();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    assert_eq!(a.knowledge_seq, 2);
    assert!(
        f.write(command(40, 12, Validity::Unknown))
            .unwrap()
            .replayed
    );
    assert_eq!(
        f.write(command(40, 12, known(0, None))),
        Err(Error::IdempotencyConflict)
    );
}
#[test]
fn old_profiles_never_upgrade_or_reinterpret_m1_validity() {
    let d = tempdir().unwrap();
    let p = d.path().join("v4.sqlite");
    let clock = TestClock(Arc::new(AtomicI64::new(100)));
    let db = AuthDatabase::open_profile(&p, clock.clone(), 4).unwrap();
    drop(db);
    assert!(matches!(
        AuthDatabase::open_profile(&p, clock, 5),
        Err(Error::IncompatibleSchema)
    ));
    let f = Fixture::new();
    assert!(matches!(
        AuthDatabase::open_profile(&f.path, f.clock.clone(), 4),
        Err(Error::IncompatibleSchema)
    ));
}
#[test]
fn conflict_preserves_both_sources_and_implicit_overwrite_is_impossible() {
    let mut f = Fixture::new();
    let (a, b) = f.pair();
    let c = f
        .conflict(
            ConflictTarget::Create {
                left: a.revision,
                right: b.revision,
            },
            42,
        )
        .unwrap();
    assert_eq!(f.count("memory"), 2);
    assert_eq!(f.count("revision"), 2);
    assert_eq!(f.count("evidence"), 2);
    let mut q = query(QueryMode::Current);
    q.valid_at = Some(50);
    let rows = f.read(q).unwrap().rows;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.conflicts == vec![c.conflict]));
    let h = f.history(c.conflict, None).unwrap();
    assert_eq!(h.decisions.len(), 1);
    assert_eq!(h.decisions[0].status, ConflictStatus::Open);
    assert!(h.decisions[0].resolution_revision.is_none());
    assert_eq!(
        f.conflict(
            ConflictTarget::Create {
                left: b.revision,
                right: a.revision
            },
            43
        ),
        Err(Error::StateConflict)
    );
}
#[test]
fn conflict_resolution_requires_union_of_both_evidence_sets() {
    let mut f = Fixture::new();
    let (a, b) = f.pair();
    let c = f
        .conflict(
            ConflictTarget::Create {
                left: a.revision,
                right: b.revision,
            },
            42,
        )
        .unwrap();
    let before = f.snapshot();
    assert_eq!(
        f.conflict(
            ConflictTarget::Resolve {
                conflict: c.conflict,
                expected_version: 1,
                resolution_revision: a.revision
            },
            43
        ),
        Err(Error::InvalidInput)
    );
    assert_eq!(f.snapshot(), before);
}
#[test]
fn resolved_and_reopened_conflict_history_is_snapshot_correct() {
    let mut f = Fixture::new();
    let (a, b) = f.pair();
    let c = f
        .conflict(
            ConflictTarget::Create {
                left: a.revision,
                right: b.revision,
            },
            42,
        )
        .unwrap();
    let open_token = f.read(query(QueryMode::Current)).unwrap().snapshot;
    let mut r = revise(43, 14, a, known(0, Some(100)));
    r.write.evidence.push(id(25));
    let resolution = f.write(r).unwrap();
    let resolved = f
        .conflict(
            ConflictTarget::Resolve {
                conflict: c.conflict,
                expected_version: 1,
                resolution_revision: resolution.revision,
            },
            44,
        )
        .unwrap();
    assert_eq!(resolved.version, 2);
    assert_eq!(
        f.history(c.conflict, Some(open_token))
            .unwrap()
            .decisions
            .len(),
        1
    );
    let h = f.history(c.conflict, None).unwrap();
    assert_eq!(h.decisions.len(), 2);
    assert_eq!(h.decisions[1].status, ConflictStatus::Resolved);
    let resolved_token = h.snapshot;
    f.source(scope(), id(2), 64, 65, 66, 67);
    let mut r = revise(45, 15, resolution, known(0, Some(100)));
    r.write.evidence = vec![id(21), id(25), id(65)];
    let new_revision = f.write(r).unwrap();
    f.conflict(
        ConflictTarget::Reopen {
            conflict: c.conflict,
            expected_version: 2,
            new_revision: new_revision.revision,
        },
        46,
    )
    .unwrap();
    let h = f.history(c.conflict, None).unwrap();
    assert_eq!(h.decisions.len(), 3);
    assert_eq!(h.decisions[2].status, ConflictStatus::Open);
    assert_eq!(
        f.history(c.conflict, Some(resolved_token))
            .unwrap()
            .decisions
            .len(),
        2
    );
    assert_eq!(f.count("revision"), 4);
    let rows = f.read(query(QueryMode::Current)).unwrap().rows;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|r| r.conflicts.contains(&c.conflict)));
}
#[test]
fn conflict_reopen_without_new_source_is_refused() {
    let mut f = Fixture::new();
    let (a, b) = f.pair();
    let c = f
        .conflict(
            ConflictTarget::Create {
                left: a.revision,
                right: b.revision,
            },
            42,
        )
        .unwrap();
    let mut r = revise(43, 14, a, Validity::Unknown);
    r.write.evidence.push(id(25));
    let res = f.write(r).unwrap();
    f.conflict(
        ConflictTarget::Resolve {
            conflict: c.conflict,
            expected_version: 1,
            resolution_revision: res.revision,
        },
        44,
    )
    .unwrap();
    let before = f.snapshot();
    assert_eq!(
        f.conflict(
            ConflictTarget::Reopen {
                conflict: c.conflict,
                expected_version: 2,
                new_revision: res.revision
            },
            45
        ),
        Err(Error::InvalidInput)
    );
    assert_eq!(f.snapshot(), before);
}
#[test]
fn conflict_transaction_failure_replay_and_cas_are_real() {
    let mut f = Fixture::new();
    let (a, b) = f.pair();
    let target = ConflictTarget::Create {
        left: a.revision,
        right: b.revision,
    };
    let before = f.snapshot();
    f.db.connection.execute_batch("CREATE TEMP TRIGGER fail_conflict BEFORE INSERT ON main.conflict_outbox BEGIN SELECT RAISE(ABORT,'fault'); END;").unwrap();
    assert_eq!(f.conflict(target, 42), Err(Error::Constraint));
    assert_eq!(f.snapshot(), before);
    f.db.connection
        .execute_batch("DROP TRIGGER temp.fail_conflict")
        .unwrap();
    let c = f.conflict(target, 42).unwrap();
    let replay = f.conflict(target, 42).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.conflict, c.conflict);
    assert_eq!(replay.decision, c.decision);
    assert_eq!(
        f.conflict(
            ConflictTarget::Resolve {
                conflict: c.conflict,
                expected_version: 2,
                resolution_revision: a.revision
            },
            43
        ),
        Err(Error::CasConflict)
    );
    assert!(
        f.db.connection
            .execute("UPDATE conflict_decision SET status='resolved'", [])
            .is_err()
    );
}
#[test]
fn cross_tenant_and_nonexistent_conflict_endpoints_are_same_denial() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    let b =
        f.db.identify(TrustedIdentity { subject: id(8) }, id(7))
            .unwrap();
    for other in [a.revision, id(250)] {
        let c = ConflictWrite {
            scope: b.scope,
            target: ConflictTarget::Create {
                left: other,
                right: id(249),
            },
            idempotency: id(41),
        };
        assert_eq!(
            f.db.write_conflict(Some(&b), c),
            Err(Error::AuthorizationDenied)
        );
    }
    assert_eq!(f.count("conflict"), 0);
}
#[test]
fn source_blocked_and_production_content_guards_are_not_relaxed() {
    let mut f = Fixture::new();
    f.write(command(40, 12, Validity::Unknown)).unwrap();
    let ctx = f.context();
    let mut app = MemoryM2Application::assemble(f.open());
    assert!(
        !app.query(Some(&ctx), query(QueryMode::Current))
            .unwrap()
            .rows[0]
            .content_available
    );
    let mut formal = MemoryM2Application::assemble(yixu_application::NoContentRuntime::default());
    assert_eq!(
        formal.query(None, query(QueryMode::Current)),
        Err(Error::CapabilityUnavailable)
    );
    assert_eq!(
        formal.write(None, command(41, 13, Validity::Unknown)),
        Err(Error::CapabilityUnavailable)
    );
    assert_eq!(
        f.db.connection
            .query_row(
                "SELECT count(*) FROM payload WHERE state<>'blocked'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    assert!(
        f.db.connection
            .execute("UPDATE memory SET state='active'", [])
            .is_err()
    );
}

#[test]
fn read_publication_and_writer_share_one_sqlite_snapshot_and_lock() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    let ctx = f.context();
    let mut reader = f.open();
    let mut writer = f.open();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (attempt_tx, attempt_rx) = mpsc::channel();
    reader.before_commit = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let owned = ctx.clone();
    let read =
        thread::spawn(move || reader.query_metadata(Some(&owned), query(QueryMode::One(a.memory))));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let write = thread::spawn(move || {
        attempt_tx.send(()).unwrap();
        writer.write_temporal(Some(&ctx), revise(41, 13, a, Validity::Unknown))
    });
    attempt_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    release_tx.send(()).unwrap();
    let old = read.join().unwrap().unwrap();
    write.join().unwrap().unwrap();
    assert_eq!(old.rows.len(), 1);
    assert_eq!(old.rows[0].number, 1);
    let mut q = query(QueryMode::One(a.memory));
    q.as_of = Some(old.snapshot.clone());
    assert_eq!(f.read(q).unwrap(), old);
    assert_eq!(
        f.read(query(QueryMode::One(a.memory))).unwrap().rows[0].number,
        2
    );
}
#[test]
fn revoke_first_denies_queued_query_before_any_candidates_return() {
    let mut f = Fixture::new();
    f.write(command(40, 12, Validity::Unknown)).unwrap();
    let ctx = f.context();
    let mut admin = f.open();
    let mut reader = f.open();
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
                capability: "memory.search",
                enabled: false,
                expires_at_ms: 10_000,
            },
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let read = thread::spawn(move || {
        attempt_tx.send(()).unwrap();
        reader.query_metadata(Some(&ctx), query(QueryMode::Current))
    });
    attempt_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    release_tx.send(()).unwrap();
    revoke.join().unwrap().unwrap();
    assert_eq!(read.join().unwrap(), Err(Error::AuthorizationDenied));
    assert_eq!(f.count("metadata_read_audit"), 0);
}
#[test]
fn read_first_linearizes_before_queued_revocation_and_all_later_reads_deny() {
    let mut f = Fixture::new();
    f.write(command(40, 12, Validity::Unknown)).unwrap();
    let ctx = f.context();
    let owned = ctx.clone();
    let mut reader = f.open();
    let mut admin = f.open();
    let authority = BootstrapAuthority {
        realm: f.authority.realm,
    };
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    reader.before_commit = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let read =
        thread::spawn(move || reader.query_metadata(Some(&owned), query(QueryMode::Current)));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let revoke = thread::spawn(move || {
        admin.administer(
            &authority,
            AdminChange::MemoryGrant {
                scope: scope(),
                principal: id(2),
                capability: "memory.search",
                enabled: false,
                expires_at_ms: 10_000,
            },
        )
    });
    release_tx.send(()).unwrap();
    assert_eq!(read.join().unwrap().unwrap().rows.len(), 1);
    revoke.join().unwrap().unwrap();
    assert_eq!(
        f.db.query_metadata(Some(&ctx), query(QueryMode::Current)),
        Err(Error::AuthorizationDenied)
    );
}
#[test]
fn temporal_cas_two_connections_and_conflict_decision_cas_have_one_winner() {
    let mut f = Fixture::new();
    let (a, b) = f.pair();
    let ctx = f.context();
    let gate = Arc::new(Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|i| {
            let mut db = f.open();
            let ctx = ctx.clone();
            let gate = gate.clone();
            thread::spawn(move || {
                gate.wait();
                let mut r = revise(42 + i, 14 + i, a, Validity::Unknown);
                r.write.evidence.push(id(25));
                db.write_temporal(Some(&ctx), r)
            })
        })
        .collect();
    gate.wait();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(Error::CasConflict))
            .count(),
        1
    );
    let resolution = results.into_iter().find_map(Result::ok).unwrap();
    let c = f
        .conflict(
            ConflictTarget::Create {
                left: a.revision,
                right: b.revision,
            },
            45,
        )
        .unwrap();
    let gate = Arc::new(Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|i| {
            let mut db = f.open();
            let ctx = ctx.clone();
            let gate = gate.clone();
            thread::spawn(move || {
                gate.wait();
                db.write_conflict(
                    Some(&ctx),
                    ConflictWrite {
                        scope: scope(),
                        target: ConflictTarget::Resolve {
                            conflict: c.conflict,
                            expected_version: 1,
                            resolution_revision: resolution.revision,
                        },
                        idempotency: id(46 + i),
                    },
                )
            })
        })
        .collect();
    gate.wait();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(Error::CasConflict))
            .count(),
        1
    );
    assert_eq!(f.count("conflict_decision"), 2);
}
#[test]
fn cross_space_cursor_memory_and_conflict_id_oracles_stay_closed() {
    let mut f = Fixture::new();
    let (a, b) = f.pair();
    let c = f
        .conflict(
            ConflictTarget::Create {
                left: a.revision,
                right: b.revision,
            },
            42,
        )
        .unwrap();
    let mut q = query(QueryMode::Current);
    q.limit = 1;
    let page = f.read(q).unwrap();
    f.db.connection
        .execute(
            "INSERT INTO principal VALUES(?1,?2,'user','active')",
            params![id(1).to_string(), id(22).to_string()],
        )
        .unwrap();
    f.db.connection
        .execute(
            "INSERT INTO identity_binding VALUES(?1,?2,?3,1)",
            params![id(24).to_string(), id(1).to_string(), id(22).to_string()],
        )
        .unwrap();
    let other = Scope {
        tenant: id(1),
        space: id(23),
    };
    f.db.administer(
        &f.authority,
        AdminChange::CreateSpace {
            scope: other,
            owner: id(22),
        },
    )
    .unwrap();
    for capability in [
        "memory.read",
        "memory.search",
        "memory.history",
        "memory.revise",
    ] {
        f.db.administer(
            &f.authority,
            AdminChange::MemoryGrant {
                scope: other,
                principal: id(22),
                capability,
                enabled: true,
                expires_at_ms: 10_000,
            },
        )
        .unwrap();
    }
    let other_ctx =
        f.db.identify(TrustedIdentity { subject: id(24) }, other.space)
            .unwrap();
    for memory in [a.memory, id(250)] {
        let mut q = query(QueryMode::One(memory));
        q.scope = other;
        assert!(
            f.db.query_metadata(Some(&other_ctx), q)
                .unwrap()
                .rows
                .is_empty()
        );
    }
    let mut q = query(QueryMode::Current);
    q.scope = other;
    q.limit = 1;
    q.cursor = page.next_cursor;
    assert_eq!(
        f.db.query_metadata(Some(&other_ctx), q),
        Err(Error::InvalidCursor)
    );
    for conflict in [c.conflict, id(250)] {
        assert_eq!(
            f.db.conflict_history(
                Some(&other_ctx),
                ConflictQuery {
                    scope: other,
                    conflict,
                    as_of: None
                }
            ),
            Err(Error::AuthorizationDenied)
        );
    }
}
#[test]
fn disabled_principal_and_space_are_rechecked_on_reads() {
    for principal in [false, true] {
        let mut f = Fixture::new();
        f.write(command(40, 12, Validity::Unknown)).unwrap();
        let ctx = f.context();
        if principal {
            f.db.administer(
                &f.authority,
                AdminChange::Principal {
                    tenant: id(1),
                    principal: id(2),
                    enabled: false,
                },
            )
            .unwrap();
        } else {
            f.db.administer(
                &f.authority,
                AdminChange::Space {
                    scope: scope(),
                    enabled: false,
                },
            )
            .unwrap();
        }
        assert_eq!(
            f.db.query_metadata(Some(&ctx), query(QueryMode::Current)),
            Err(Error::AuthorizationDenied)
        );
        assert_eq!(f.count("metadata_read_audit"), 0);
    }
}
#[test]
fn stable_tie_break_and_page_order_do_not_depend_on_row_insertion_order() {
    let mut f = Fixture::new();
    f.write(command(40, 12, Validity::Unknown)).unwrap();
    let tx =
        f.db.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
    // Independent synthetic fixture: a second aggregate in the SAME committed knowledge batch.
    tx.execute(
        "INSERT INTO memory VALUES(?1,?2,?3,?4,1,'source_blocked',100)",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(70).to_string(),
            id(71).to_string()
        ],
    )
    .unwrap();
    tx.execute("INSERT INTO revision VALUES(?1,?2,?3,?4,1,NULL,NULL,?5,?6,'user_assertion',100,NULL,100,2)",params![id(1).to_string(),id(3).to_string(),id(71).to_string(),id(70).to_string(),id(21).to_string(),id(13).to_string()]).unwrap();
    insert_time(&tx, scope(), id(71), 1, Validity::Unknown).unwrap();
    tx.execute(
        "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,'supports')",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(71).to_string(),
            id(21).to_string()
        ],
    )
    .unwrap();
    tx.commit().unwrap();
    let rows = f.read(query(QueryMode::Current)).unwrap().rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].knowledge_seq, rows[1].knowledge_seq);
    assert!(rows[0].memory.to_string() < rows[1].memory.to_string());
    let mut q = query(QueryMode::Current);
    q.limit = 1;
    let first = f.read(q.clone()).unwrap();
    q.cursor = first.next_cursor;
    let second = f.read(q).unwrap();
    assert_eq!(
        [first.rows[0].clone(), second.rows[0].clone()].to_vec(),
        rows
    );
}
#[test]
fn contentless_fts_still_exposes_terms_and_cjk_tokenization_is_not_assumed() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE VIRTUAL TABLE diagnostic USING fts5(body,content=''); CREATE VIRTUAL TABLE vocabulary USING fts5vocab(diagnostic,'row'); INSERT INTO diagnostic(body) VALUES('忆序长期记忆检索');").unwrap();
    assert_eq!(
        c.query_row("SELECT body FROM diagnostic", [], |r| r
            .get::<_, Option<String>>(0))
            .unwrap(),
        None
    );
    let term: String = c
        .query_row("SELECT term FROM vocabulary", [], |r| r.get(0))
        .unwrap();
    assert_eq!(term, "忆序长期记忆检索");
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM diagnostic WHERE diagnostic MATCH '长期记忆'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    c.execute_batch("CREATE VIRTUAL TABLE trigrams USING fts5(body,tokenize='trigram');INSERT INTO trigrams(body) VALUES('忆序长期记忆检索');").unwrap();
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM trigrams WHERE trigrams MATCH '长期记'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM trigrams WHERE trigrams MATCH '长期'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}
#[test]
fn accepted_profile_three_four_checksums_are_byte_compatible_and_m2_requires_time() {
    let old4 =
        Sha256::digest(format!("{MIGRATION}\n{AUTH_DDL}\n{R1_DDL}\n{MEMORY_DDL}").as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
    assert_eq!(profile_checksum(4), old4);
    assert_eq!(
        profile_checksum(3),
        checksum()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    );
    let mut f = Fixture::new();
    assert_eq!(
        f.db.apply_memory(None, command(40, 12, Validity::Unknown).write),
        Err(Error::CapabilityUnavailable)
    );
    f.write(command(40, 12, Validity::Unknown)).unwrap();
    assert!(
        f.db.connection
            .execute("UPDATE revision_time SET known=1", [])
            .is_err()
    );
}
#[test]
fn startup_drift_and_runtime_foreign_keys_remain_independent_guards() {
    let f = Fixture::new();
    f.db.connection
        .execute_batch("DROP TRIGGER payload_no_admission")
        .unwrap();
    assert!(matches!(
        AuthDatabase::open_profile(&f.path, f.clock.clone(), 5),
        Err(Error::IncompatibleSchema)
    ));
    let mut g = Fixture::new();
    g.db.connection
        .execute_batch("PRAGMA foreign_keys=OFF")
        .unwrap();
    assert_eq!(g.read(query(QueryMode::Current)), Err(Error::Integrity));
}

#[test]
fn signed_future_snapshot_and_key_rotation_are_rejected() {
    let mut f = Fixture::new();
    let ctx = f.context();
    let key = f.db.query_tokens.clone().unwrap();
    let mut q = query(QueryMode::Current);
    q.as_of = Some(key.knowledge(&ctx, 99).unwrap());
    assert_eq!(f.read(q), Err(Error::InvalidToken));
    let token = f.read(query(QueryMode::Current)).unwrap().snapshot;
    f.db.query_tokens = Some(Arc::new(tokens::TokenAuthority::random().unwrap()));
    let mut q = query(QueryMode::Current);
    q.as_of = Some(token);
    assert_eq!(f.read(q), Err(Error::InvalidToken));
}
#[test]
fn sqlite_cannot_commit_revision_without_explicit_temporal_binding() {
    let mut f = Fixture::new();
    let before = f.snapshot();
    let tx =
        f.db.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
    tx.execute(
        "INSERT INTO memory VALUES(?1,?2,?3,?4,1,'source_blocked',100)",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(70).to_string(),
            id(71).to_string()
        ],
    )
    .unwrap();
    tx.execute("INSERT INTO revision VALUES(?1,?2,?3,?4,1,NULL,NULL,?5,?6,'user_assertion',100,NULL,100,1)",params![id(1).to_string(),id(3).to_string(),id(71).to_string(),id(70).to_string(),id(21).to_string(),id(13).to_string()]).unwrap();
    tx.execute(
        "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,'supports')",
        params![
            id(1).to_string(),
            id(3).to_string(),
            id(71).to_string(),
            id(21).to_string()
        ],
    )
    .unwrap();
    assert!(tx.commit().is_err());
    assert_eq!(f.snapshot(), before);
}
#[test]
fn space_budget_fails_closed_instead_of_returning_a_truncated_complete_page() {
    fn many_id(n: u64) -> Id {
        let mut bytes = [0; 16];
        bytes[8..].copy_from_slice(&n.to_be_bytes());
        Id::from_bytes(bytes)
    }
    let mut f = Fixture::new();
    let tx =
        f.db.connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
    for n in 0..1001 {
        let m = many_id(10000 + n);
        let r = many_id(20000 + n);
        let p = many_id(30000 + n);
        blocked_payload(&tx, scope(), p, false).unwrap();
        tx.execute(
            "INSERT INTO memory VALUES(?1,?2,?3,?4,1,'source_blocked',100)",
            params![
                id(1).to_string(),
                id(3).to_string(),
                m.to_string(),
                r.to_string()
            ],
        )
        .unwrap();
        tx.execute("INSERT INTO revision VALUES(?1,?2,?3,?4,1,NULL,NULL,?5,?6,'user_assertion',100,NULL,100,1)",params![id(1).to_string(),id(3).to_string(),r.to_string(),m.to_string(),id(21).to_string(),p.to_string()]).unwrap();
        insert_time(&tx, scope(), r, 1, Validity::Unknown).unwrap();
        tx.execute(
            "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,'supports')",
            params![
                id(1).to_string(),
                id(3).to_string(),
                r.to_string(),
                id(21).to_string()
            ],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    assert_eq!(
        f.read(query(QueryMode::Current)),
        Err(Error::ResourceUnavailable)
    );
    assert_eq!(f.count("metadata_read_audit"), 0);
}

#[test]
fn deletion_state_overrides_old_snapshot_and_cursor_without_reactivation() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    f.write(command(41, 13, Validity::Unknown)).unwrap();
    let mut q = query(QueryMode::Current);
    q.limit = 1;
    let page = f.read(q.clone()).unwrap();
    f.db.connection
        .execute(
            "UPDATE memory SET state='deletion_pending' WHERE id=?1",
            [a.memory.to_string()],
        )
        .unwrap();
    q.as_of = Some(page.snapshot.clone());
    assert_eq!(f.read(q.clone()), Err(Error::CapabilityUnavailable));
    q.cursor = page.next_cursor;
    assert_eq!(f.read(q), Err(Error::CapabilityUnavailable));
    assert_eq!(
        f.write(command(42, 14, Validity::Unknown)),
        Err(Error::CapabilityUnavailable)
    );
}
#[test]
fn evidence_filter_never_falls_back_to_a_superseded_revision() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    f.source(scope(), id(2), 24, 25, 60, 61);
    let mut r = revise(41, 13, a, Validity::Unknown);
    r.write.evidence = vec![id(25)];
    r.write.primary_evidence = id(25);
    f.write(r).unwrap();
    let mut q = query(QueryMode::One(a.memory));
    q.evidence = Some(id(21));
    assert!(f.read(q.clone()).unwrap().rows.is_empty());
    q.mode = QueryMode::History(a.memory);
    let history = f.read(q).unwrap();
    assert_eq!(history.rows.len(), 1);
    assert_eq!(history.rows[0].number, 1);
}

#[test]
fn publication_holds_an_actual_sqlite_write_lock_not_a_process_gate() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    let ctx = f.context();
    let mut reader = f.open();
    let mut writer = f.open();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    reader.before_commit = Some(Box::new(move || {
        entered_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }));
    let owned = ctx.clone();
    let read =
        thread::spawn(move || reader.query_metadata(Some(&owned), query(QueryMode::One(a.memory))));
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    writer
        .connection
        .busy_timeout(Duration::from_millis(50))
        .unwrap();
    assert_eq!(
        writer.write_temporal(Some(&ctx), revise(41, 13, a, Validity::Unknown)),
        Err(Error::Busy)
    );
    release_tx.send(()).unwrap();
    assert_eq!(read.join().unwrap().unwrap().rows[0].number, 1);
    writer
        .write_temporal(Some(&ctx), revise(41, 13, a, Validity::Unknown))
        .unwrap();
    assert_eq!(f.seq(), 3);
}
#[test]
fn a_modified_cursor_mac_is_rejected_without_audit_or_results() {
    let mut f = Fixture::new();
    f.write(command(40, 12, Validity::Unknown)).unwrap();
    f.write(command(41, 13, Validity::Unknown)).unwrap();
    let mut q = query(QueryMode::Current);
    q.limit = 1;
    let page = f.read(q.clone()).unwrap();
    let mut cursor = page.next_cursor.unwrap();
    cursor.0[0] ^= 1;
    q.cursor = Some(cursor);
    let before = f.count("metadata_read_audit");
    assert_eq!(f.read(q), Err(Error::InvalidCursor));
    assert_eq!(f.count("metadata_read_audit"), before);
}

#[test]
fn future_source_link_and_inconsistent_knowledge_root_are_integrity_failures() {
    let mut f = Fixture::new();
    let a = f.write(command(40, 12, Validity::Unknown)).unwrap();
    let token = f.read(query(QueryMode::One(a.memory))).unwrap().snapshot;
    f.source(scope(), id(2), 24, 25, 60, 61);
    // Structurally valid FK but invalid knowledge causality, injected via privileged fixture SQL.
    f.db.connection
        .execute(
            "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,'supports')",
            params![
                id(1).to_string(),
                id(3).to_string(),
                a.revision.to_string(),
                id(25).to_string()
            ],
        )
        .unwrap();
    let mut q = query(QueryMode::One(a.memory));
    q.as_of = Some(token);
    assert_eq!(f.read(q), Err(Error::Integrity));
    let mut g = Fixture::new();
    let a = g.write(command(40, 12, Validity::Unknown)).unwrap();
    g.write(revise(41, 13, a, Validity::Unknown)).unwrap();
    g.db.connection
        .execute(
            "UPDATE tenant SET knowledge_seq=2 WHERE id=?1",
            [id(1).to_string()],
        )
        .unwrap();
    assert_eq!(g.read(query(QueryMode::Current)), Err(Error::Integrity));
    // A Conflict-only commit must also prevent an inconsistent root from hiding its existence.
    let mut h = Fixture::new();
    let (a, b) = h.pair();
    h.conflict(
        ConflictTarget::Create {
            left: a.revision,
            right: b.revision,
        },
        42,
    )
    .unwrap();
    h.db.connection
        .execute(
            "UPDATE tenant SET knowledge_seq=knowledge_seq-1 WHERE id=?1",
            [id(1).to_string()],
        )
        .unwrap();
    assert_eq!(h.read(query(QueryMode::Current)), Err(Error::Integrity));
}
