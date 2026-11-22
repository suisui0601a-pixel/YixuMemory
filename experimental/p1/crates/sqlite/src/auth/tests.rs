use super::*;
use std::{
    sync::{
        Arc, Barrier,
        atomic::{AtomicI64, Ordering},
        mpsc,
    },
    thread,
};
use tempfile::{TempDir, tempdir};
use yixu_application::metadata::MetadataApplication;
use yixu_ports::authorized_metadata::MetadataCapability;
#[derive(Clone)]
struct TestClock(Arc<AtomicI64>);
impl TestClock {
    fn new() -> Self {
        Self(Arc::new(AtomicI64::new(100)))
    }
    fn set(&self, n: i64) {
        self.0.store(n, Ordering::SeqCst);
    }
}
impl Clock for TestClock {
    fn utc_millis(&self) -> Result<i64> {
        let value = self.0.load(Ordering::SeqCst);
        if value < 0 {
            Err(Error::CapabilityUnavailable)
        } else {
            Ok(value)
        }
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
fn command(n: u8) -> MarkerWrite {
    MarkerWrite {
        scope: scope(),
        target: MarkerTarget::Create,
        expected_version: 0,
        idempotency: id(n + 1),
        capability: MetadataCapability::Append,
    }
}
fn update_command(n: u8, marker: Id, expected_version: u64) -> MarkerWrite {
    MarkerWrite {
        target: MarkerTarget::Existing(marker),
        expected_version,
        ..command(n)
    }
}
struct Fixture {
    _dir: TempDir,
    path: std::path::PathBuf,
    db: AuthDatabase<TestClock>,
    authority: BootstrapAuthority,
    clock: TestClock,
}
impl Fixture {
    fn new(grant: bool) -> Self {
        let dir = tempdir().unwrap();
        let path = dir.path().join("auth.sqlite");
        let clock = TestClock::new();
        let mut db = AuthDatabase::open(&path, clock.clone()).unwrap();
        let hash = Sha256::digest(path.to_string_lossy().as_bytes());
        let mut realm_bytes = [0; 16];
        realm_bytes.copy_from_slice(&hash[..16]);
        let authority = db.bootstrap(Id::from_bytes(realm_bytes)).unwrap();
        db.administer(
            &authority,
            AdminChange::Tenant {
                tenant: id(1),
                principal: id(2),
                subject: id(4),
                space: id(3),
            },
        )
        .unwrap();
        db.administer(
            &authority,
            AdminChange::Tenant {
                tenant: id(5),
                principal: id(6),
                subject: id(8),
                space: id(7),
            },
        )
        .unwrap();
        if grant {
            db.administer(
                &authority,
                AdminChange::Grant {
                    scope: scope(),
                    principal: id(2),
                    enabled: true,
                    expires_at_ms: 1000,
                },
            )
            .unwrap();
        }
        Self {
            _dir: dir,
            path,
            db,
            authority,
            clock,
        }
    }
    fn context(&self) -> AuthContext {
        self.db
            .identify(TrustedIdentity { subject: id(4) }, id(3))
            .unwrap()
    }
    fn open(&self) -> AuthDatabase<TestClock> {
        AuthDatabase::open(&self.path, self.clock.clone()).unwrap()
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
    fn epoch(&self) -> i64 {
        self.db
            .connection
            .query_row(
                "SELECT auth_epoch FROM tenant WHERE id=?1",
                [id(1).to_string()],
                |r| r.get(0),
            )
            .unwrap()
    }
    fn assert_empty(&self) {
        for t in [
            "metadata_marker",
            "metadata_retry",
            "event",
            "commit_record",
            "metadata_outbox",
            "metadata_audit",
        ] {
            assert_eq!(self.count(t), 0, "{t}");
        }
        assert_eq!(self.seq(), 0);
    }
    fn revoke(&mut self) {
        self.db
            .administer(
                &self.authority,
                AdminChange::Grant {
                    scope: scope(),
                    principal: id(2),
                    enabled: false,
                    expires_at_ms: 1000,
                },
            )
            .unwrap();
    }
}
#[test]
fn authorized_application_writes_metadata_atomically_and_reopens() {
    let f = Fixture::new(true);
    let ctx = f.context();
    let mut app = MetadataApplication::assemble(f.open());
    let result = app.execute(Some(&ctx), command(20)).unwrap();
    assert_eq!(
        (result.version, result.knowledge_seq, result.replayed),
        (1, 1, false)
    );
    assert_eq!(f.count("event"), 1);
    assert_eq!(f.count("metadata_retry"), 1);
    assert_eq!(f.count("metadata_outbox"), 1);
    assert_eq!(f.count("metadata_audit"), 1);
    assert_eq!(f.seq(), 1);
    drop(app);
    let mut app = MetadataApplication::assemble(f.open());
    let replay = app.execute(Some(&ctx), command(20)).unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.knowledge_seq, 1);
    assert_eq!(f.seq(), 1);
}
#[test]
fn unauthenticated_is_denied_without_business_changes() {
    let mut f = Fixture::new(true);
    assert_eq!(
        f.db.apply(None, command(20)),
        Err(Error::AuthorizationDenied)
    );
    f.assert_empty();
}
#[test]
fn owner_without_direct_grant_is_denied() {
    let mut f = Fixture::new(false);
    let ctx = f.context();
    assert_eq!(
        f.db.apply(Some(&ctx), command(20)),
        Err(Error::AuthorizationDenied)
    );
    f.assert_empty();
}
#[test]
fn unmapped_subject_or_self_described_admin_cannot_obtain_context() {
    let f = Fixture::new(true);
    for subject in [id(1), id(2), id(99)] {
        assert!(matches!(
            f.db.identify(TrustedIdentity { subject }, id(3)),
            Err(Error::AuthorizationDenied)
        ));
    }
    assert!(matches!(
        f.db.identify(TrustedIdentity { subject: id(4) }, id(7)),
        Err(Error::AuthorizationDenied)
    ));
    f.assert_empty();
}
#[test]
fn wrong_tenant_and_space_are_denied() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    let mut cmd = command(20);
    cmd.scope = Scope {
        tenant: id(5),
        space: id(7),
    };
    assert_eq!(f.db.apply(Some(&ctx), cmd), Err(Error::AuthorizationDenied));
    cmd.scope = Scope {
        tenant: id(1),
        space: id(7),
    };
    assert_eq!(f.db.apply(Some(&ctx), cmd), Err(Error::AuthorizationDenied));
    f.assert_empty();
}
#[test]
fn valid_foreign_space_idor_is_denied() {
    let mut f = Fixture::new(true);
    let other = Scope {
        tenant: id(1),
        space: id(9),
    };
    f.db.administer(
        &f.authority,
        AdminChange::CreateSpace {
            scope: other,
            owner: id(2),
        },
    )
    .unwrap();
    let ctx = f.context();
    let mut cmd = command(20);
    cmd.scope = other;
    assert_eq!(f.db.apply(Some(&ctx), cmd), Err(Error::AuthorizationDenied));
    f.assert_empty();
    // Even an authenticated context with a valid Grant for Space B cannot mutate A's marker.
    let created = f.db.apply(Some(&f.context()), command(20)).unwrap();
    f.db.administer(
        &f.authority,
        AdminChange::Grant {
            scope: other,
            principal: id(2),
            enabled: true,
            expires_at_ms: 1000,
        },
    )
    .unwrap();
    let other_ctx =
        f.db.identify(TrustedIdentity { subject: id(4) }, other.space)
            .unwrap();
    let mut valid_other_command = command(30);
    valid_other_command.scope = other;
    valid_other_command.target = MarkerTarget::Existing(created.marker);
    assert_eq!(
        f.db.apply(Some(&other_ctx), valid_other_command),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(f.seq(), 1);
    assert_eq!(f.count("event"), 1);
}
#[test]
fn expired_grant_and_old_retry_are_denied() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    f.db.apply(Some(&ctx), command(20)).unwrap();
    f.clock.set(1000);
    assert_eq!(
        f.db.apply(Some(&ctx), command(20)),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(
        f.db.apply(Some(&f.context()), command(30)),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(f.seq(), 1);
}
#[test]
fn revoked_grant_rejects_cloned_context_replay_and_fresh_identity() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    let cached = ctx.clone();
    f.db.apply(Some(&ctx), command(20)).unwrap();
    let epoch = f.epoch();
    f.revoke();
    assert_eq!(f.epoch(), epoch + 1);
    assert_eq!(
        f.db.apply(Some(&cached), command(20)),
        Err(Error::AuthorizationDenied)
    );
    let fresh = f.context();
    assert_eq!(
        f.db.apply(Some(&fresh), command(30)),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(f.seq(), 1);
}
#[test]
fn principal_disable_and_reenable_never_revive_old_context() {
    let mut f = Fixture::new(true);
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
    assert!(matches!(
        f.db.identify(TrustedIdentity { subject: id(4) }, id(3)),
        Err(Error::AuthorizationDenied)
    ));
    assert_eq!(
        f.db.apply(Some(&ctx), command(20)),
        Err(Error::AuthorizationDenied)
    );
    f.db.administer(
        &f.authority,
        AdminChange::Principal {
            tenant: id(1),
            principal: id(2),
            enabled: true,
        },
    )
    .unwrap();
    assert_eq!(
        f.db.apply(Some(&ctx), command(20)),
        Err(Error::AuthorizationDenied)
    );
    f.db.apply(Some(&f.context()), command(20)).unwrap();
}
#[test]
fn space_disable_and_tenant_disable_deny_old_and_new_identity() {
    for tenant in [false, true] {
        let mut f = Fixture::new(true);
        let ctx = f.context();
        let change = if tenant {
            AdminChange::TenantState {
                tenant: id(1),
                enabled: false,
            }
        } else {
            AdminChange::Space {
                scope: scope(),
                enabled: false,
            }
        };
        f.db.administer(&f.authority, change).unwrap();
        assert_eq!(
            f.db.apply(Some(&ctx), command(20)),
            Err(Error::AuthorizationDenied)
        );
        assert!(matches!(
            f.db.identify(TrustedIdentity { subject: id(4) }, id(3)),
            Err(Error::AuthorizationDenied)
        ));
        f.assert_empty();
    }
}
#[test]
fn disabled_identity_binding_is_rechecked() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    f.db.administer(
        &f.authority,
        AdminChange::Binding {
            subject: id(4),
            enabled: false,
        },
    )
    .unwrap();
    assert_eq!(
        f.db.apply(Some(&ctx), command(20)),
        Err(Error::AuthorizationDenied)
    );
    assert!(matches!(
        f.db.identify(TrustedIdentity { subject: id(4) }, id(3)),
        Err(Error::AuthorizationDenied)
    ));
    f.assert_empty();
}
#[test]
fn group_inheritance_is_not_an_implicit_allow() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    let mut cmd = command(20);
    cmd.capability = MetadataCapability::GroupInherited;
    assert_eq!(f.db.apply(Some(&ctx), cmd), Err(Error::AuthorizationDenied));
    assert!(
        f.db.connection
            .execute(
                "INSERT INTO direct_grant VALUES(?1,?2,?3,'admin',1,1000)",
                params![id(1).to_string(), id(3).to_string(), id(2).to_string()]
            )
            .is_err()
    );
    f.assert_empty();
}
#[test]
fn cross_tenant_grant_fk_and_admin_rollback_preserve_epoch() {
    let mut f = Fixture::new(false);
    let epoch = f.epoch();
    assert_eq!(
        f.db.administer(
            &f.authority,
            AdminChange::Grant {
                scope: scope(),
                principal: id(6),
                enabled: true,
                expires_at_ms: 1000
            }
        ),
        Err(Error::Constraint)
    );
    assert_eq!(
        f.db.administer(
            &f.authority,
            AdminChange::Grant {
                scope: Scope {
                    tenant: id(1),
                    space: id(7)
                },
                principal: id(2),
                enabled: true,
                expires_at_ms: 1000
            }
        ),
        Err(Error::Constraint)
    );
    assert_eq!(f.epoch(), epoch);
    assert_eq!(f.count("direct_grant"), 0);
    f.assert_empty();
}
#[test]
fn bootstrap_is_single_use_and_authority_is_realm_bound() {
    let mut f = Fixture::new(false);
    assert!(matches!(
        f.db.bootstrap(id(90)),
        Err(Error::AuthorizationDenied)
    ));
    let other = Fixture::new(false);
    let forged_other_realm = BootstrapAuthority { realm: id(44) };
    assert_eq!(
        f.db.administer(
            &forged_other_realm,
            AdminChange::Principal {
                tenant: id(1),
                principal: id(2),
                enabled: false
            }
        ),
        Err(Error::AuthorizationDenied)
    );
    // A real authority from a distinct realm cannot administer this DB either.
    assert_eq!(
        f.db.administer(
            &other.authority,
            AdminChange::Principal {
                tenant: id(1),
                principal: id(2),
                enabled: false
            }
        ),
        Err(Error::AuthorizationDenied)
    );
    f.assert_empty();
}
#[test]
fn context_is_bound_to_database_realm() {
    let mut first = Fixture::new(true);
    let mut second = Fixture::new(true);
    second
        .db
        .connection
        .execute("UPDATE auth_instance SET realm_id=?1", [id(77).to_string()])
        .unwrap();
    let ctx = first.context();
    assert_eq!(
        second.db.apply(Some(&ctx), command(20)),
        Err(Error::AuthorizationDenied)
    );
    first.db.apply(Some(&ctx), command(20)).unwrap();
    second.assert_empty();
}
#[test]
fn cas_conflict_and_idempotency_mismatch_do_not_consume_sequence() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    let created = f.db.apply(Some(&ctx), command(20)).unwrap();
    assert_eq!(
        f.db.apply(Some(&ctx), update_command(30, created.marker, 0)),
        Err(Error::CasConflict)
    );
    let mut altered = command(20);
    altered.expected_version = 1;
    altered.target = MarkerTarget::Existing(created.marker);
    assert_eq!(
        f.db.apply(Some(&ctx), altered),
        Err(Error::IdempotencyConflict)
    );
    assert_eq!(f.seq(), 1);
    assert_eq!(f.count("event"), 1);
}
#[test]
fn real_constraint_failure_rolls_back_sequence_event_and_retry() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    let created = f.db.apply(Some(&ctx), command(20)).unwrap();
    let cmd = update_command(30, created.marker, 1);
    // Real late SQLite constraint fault; event IDs can no longer be client selected.
    f.db.connection.execute_batch("CREATE TEMP TRIGGER fail_retry BEFORE INSERT ON metadata_retry BEGIN SELECT RAISE(ABORT,'injected synthetic constraint'); END;").unwrap();
    assert_eq!(f.db.apply(Some(&ctx), cmd), Err(Error::Constraint));
    assert_eq!(f.seq(), 1);
    for table in [
        "event",
        "metadata_retry",
        "metadata_outbox",
        "metadata_audit",
    ] {
        assert_eq!(f.count(table), 1, "{table}");
    }
    f.db.connection
        .execute_batch("DROP TRIGGER fail_retry")
        .unwrap();
    let receipt = f.db.apply(Some(&ctx), cmd).unwrap();
    assert_eq!((receipt.version, receipt.knowledge_seq), (2, 2));
    assert_eq!(receipt.marker, created.marker);
}
#[test]
fn expiration_during_write_rolls_back_all_business_rows() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    let clock = f.clock.clone();
    f.db.before_commit = Some(Box::new(move || clock.set(1000)));
    assert_eq!(
        f.db.apply(Some(&ctx), command(20)),
        Err(Error::AuthorizationDenied)
    );
    f.assert_empty();
}
#[test]
fn backward_or_unavailable_clock_fails_closed() {
    for invalid in [50, -1] {
        let mut f = Fixture::new(true);
        let ctx = f.context();
        let clock = f.clock.clone();
        f.db.before_commit = Some(Box::new(move || clock.set(invalid)));
        assert_eq!(
            f.db.apply(Some(&ctx), command(20)),
            Err(Error::AuthorizationDenied)
        );
        f.assert_empty();
    }
}
#[test]
fn authorization_epoch_invalidation_includes_added_grants() {
    let mut f = Fixture::new(false);
    let ctx = f.context();
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
    assert_eq!(
        f.db.apply(Some(&ctx), command(20)),
        Err(Error::AuthorizationDenied)
    );
    f.db.apply(Some(&f.context()), command(20)).unwrap();
}
#[test]
fn two_real_connections_same_cas_only_one_commits() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    let created = f.db.apply(Some(&ctx), command(10)).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for n in [20, 30] {
        let mut db = f.open();
        let ctx = ctx.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            db.apply(Some(&ctx), update_command(n, created.marker, 1))
        }));
    }
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results.contains(&Err(Error::CasConflict)));
    assert_eq!(f.seq(), 2);
}
#[test]
fn two_real_connections_same_idempotency_replay_only_once() {
    let f = Fixture::new(true);
    let ctx = f.context();
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for _ in 0..2 {
        let mut db = f.open();
        let ctx = ctx.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            db.apply(Some(&ctx), command(20)).unwrap()
        }));
    }
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| !r.replayed).count(), 1);
    assert_eq!(f.seq(), 1);
    assert_eq!(f.count("event"), 1);
}
fn revoke_change(disable: bool) -> AdminChange {
    if disable {
        AdminChange::Principal {
            tenant: id(1),
            principal: id(2),
            enabled: false,
        }
    } else {
        AdminChange::Grant {
            scope: scope(),
            principal: id(2),
            enabled: false,
            expires_at_ms: 1000,
        }
    }
}
fn revocation_first(disable: bool) {
    let f = Fixture::new(true);
    let ctx = f.context();
    let mut admin = f.open();
    let authority = BootstrapAuthority {
        realm: f.authority.realm,
    };
    let mut writer = f.open();
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    admin.before_commit = Some(Box::new(move || {
        held_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    let revoker = thread::spawn(move || admin.administer(&authority, revoke_change(disable)));
    held_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let request = thread::spawn(move || {
        attempt_tx.send(()).unwrap();
        writer.apply(Some(&ctx), command(20))
    });
    attempt_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    release_tx.send(()).unwrap();
    revoker.join().unwrap().unwrap();
    assert_eq!(request.join().unwrap(), Err(Error::AuthorizationDenied));
    f.assert_empty();
}
#[test]
fn grant_revocation_holding_sqlite_lock_precedes_queued_request() {
    revocation_first(false);
}
#[test]
fn principal_disable_holding_sqlite_lock_precedes_queued_request() {
    revocation_first(true);
}
#[test]
fn authorized_writer_holding_lock_can_precede_revocation_but_no_later_write() {
    let f = Fixture::new(true);
    let ctx = f.context();
    let cached = ctx.clone();
    let mut writer = f.open();
    let mut admin = f.open();
    let authority = BootstrapAuthority {
        realm: f.authority.realm,
    };
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    writer.before_commit = Some(Box::new(move || {
        held_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    let request = thread::spawn(move || writer.apply(Some(&ctx), command(20)));
    held_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let revoker = thread::spawn(move || {
        attempt_tx.send(()).unwrap();
        admin.administer(&authority, revoke_change(false))
    });
    attempt_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    release_tx.send(()).unwrap();
    request.join().unwrap().unwrap();
    revoker.join().unwrap().unwrap();
    let mut later = f.open();
    assert_eq!(
        later.apply(Some(&cached), command(20)),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(f.seq(), 1);
}
#[test]
fn grant_addition_and_write_are_serialized_and_old_context_does_not_gain_authority() {
    let f = Fixture::new(false);
    let ctx = f.context();
    let mut admin = f.open();
    let mut writer = f.open();
    let authority = BootstrapAuthority {
        realm: f.authority.realm,
    };
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    admin.before_commit = Some(Box::new(move || {
        held_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    let granting = thread::spawn(move || {
        admin.administer(
            &authority,
            AdminChange::Grant {
                scope: scope(),
                principal: id(2),
                enabled: true,
                expires_at_ms: 1000,
            },
        )
    });
    held_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let request = thread::spawn(move || writer.apply(Some(&ctx), command(20)));
    release_tx.send(()).unwrap();
    granting.join().unwrap().unwrap();
    assert_eq!(request.join().unwrap(), Err(Error::AuthorizationDenied));
    f.assert_empty();
}
#[test]
fn auth_schema_drift_and_profile_confusion_are_rejected() {
    let f = Fixture::new(true);
    f.db.connection
        .execute_batch("DROP TABLE metadata_retry")
        .unwrap();
    assert!(matches!(
        AuthDatabase::open(&f.path, f.clock.clone()),
        Err(Error::IncompatibleSchema)
    ));
    let dir = tempdir().unwrap();
    let path = dir.path().join("foundation.sqlite");
    drop(yixu_sqlite_test_support::MetadataLab::open(&path, Duration::from_millis(100)).unwrap());
    assert!(matches!(
        AuthDatabase::open(&path, TestClock::new()),
        Err(Error::IncompatibleSchema)
    ));
    let valid = Fixture::new(true);
    assert!(matches!(
        yixu_sqlite_test_support::MetadataLab::open(&valid.path, Duration::from_millis(100)),
        Err(Error::IncompatibleSchema)
    ));
}
#[test]
fn cross_process_worker() {
    use std::io::{Read, Write};
    let Some(path) = std::env::var_os("YIXU_AUTH_TEST_DB") else {
        return;
    };
    let mut db = AuthDatabase::open(Path::new(&path), TestClock::new()).unwrap();
    let ctx = db
        .identify(TrustedIdentity { subject: id(4) }, id(3))
        .unwrap();
    println!("CONTEXT_READY");
    std::io::stdout().flush().unwrap();
    let mut signal = [0];
    std::io::stdin().read_exact(&mut signal).unwrap();
    assert_eq!(signal, [b'G']);
    println!("ATTEMPTING_WRITE");
    std::io::stdout().flush().unwrap();
    db.connection
        .busy_timeout(Duration::from_millis(50))
        .unwrap();
    {
        let attempted = db
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error);
        assert!(
            matches!(attempted, Err(Error::Busy)),
            "real cross-process writer lock must be observed"
        );
    }
    db.connection.busy_timeout(Duration::from_secs(5)).unwrap();
    println!("SQLITE_BUSY_CONFIRMED");
    std::io::stdout().flush().unwrap();
    assert_eq!(
        db.apply(Some(&ctx), command(20)),
        Err(Error::AuthorizationDenied)
    );
    println!("AUTH_DENIED_AFTER_REVOKE");
}
#[test]
fn committed_revocation_denies_stale_context_in_another_process() {
    use std::{
        io::{BufRead, BufReader, Write},
        process::{Command, Stdio},
    };
    let f = Fixture::new(true);
    // Child opens before the revocation lock, so it really carries the old snapshot.
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "auth::tests::cross_process_worker",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("YIXU_AUTH_TEST_DB", &f.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    let mut lines = String::new();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        lines.push_str(&line);
        if line.contains("CONTEXT_READY") {
            break;
        }
    }
    let mut admin = f.open();
    let authority = BootstrapAuthority {
        realm: f.authority.realm,
    };
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    admin.before_commit = Some(Box::new(move || {
        held_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    let revoker = thread::spawn(move || admin.administer(&authority, revoke_change(false)));
    held_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    child.stdin.take().unwrap().write_all(b"G").unwrap();
    loop {
        let mut line = String::new();
        assert!(reader.read_line(&mut line).unwrap() > 0);
        lines.push_str(&line);
        if line.contains("SQLITE_BUSY_CONFIRMED") {
            break;
        }
    }
    release_tx.send(()).unwrap();
    revoker.join().unwrap().unwrap();
    for line in reader.lines() {
        lines.push_str(&line.unwrap());
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(lines.contains("AUTH_DENIED_AFTER_REVOKE"));
    f.assert_empty();
    println!(
        "cross-process evidence: old identity issued; revoker held BEGIN IMMEDIATE; child observed real SQLITE_BUSY; revocation committed; child denied; business seq=0"
    );
}
#[test]
fn grant_reenable_does_not_restore_cached_authority() {
    let mut f = Fixture::new(true);
    let old = f.context();
    f.revoke();
    f.db.administer(
        &f.authority,
        AdminChange::Grant {
            scope: scope(),
            principal: id(2),
            enabled: true,
            expires_at_ms: 2000,
        },
    )
    .unwrap();
    assert_eq!(
        f.db.apply(Some(&old), command(20)),
        Err(Error::AuthorizationDenied)
    );
    f.db.apply(Some(&f.context()), command(20)).unwrap();
}
#[test]
fn unavailable_space_missing_state_is_denied() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    f.db.connection
        .execute(
            "DELETE FROM auth_space WHERE tenant_id=?1 AND space_id=?2",
            params![id(1).to_string(), id(3).to_string()],
        )
        .unwrap();
    assert_eq!(
        f.db.apply(Some(&ctx), command(20)),
        Err(Error::AuthorizationDenied)
    );
    assert!(matches!(
        f.db.identify(TrustedIdentity { subject: id(4) }, id(3)),
        Err(Error::AuthorizationDenied)
    ));
    f.assert_empty();
}
#[test]
fn invalid_cas_integer_is_rejected_before_any_business_change() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    let mut cmd = command(20);
    cmd.expected_version = u64::MAX;
    assert_eq!(f.db.apply(Some(&ctx), cmd), Err(Error::InvalidInput));
    f.assert_empty();
}

#[test]
fn partial_bootstrap_admin_failure_rolls_back_tenant_principal_and_binding() {
    let mut f = Fixture::new(false);
    assert_eq!(
        f.db.administer(
            &f.authority,
            AdminChange::Tenant {
                tenant: id(11),
                principal: id(12),
                subject: id(4),
                space: id(13)
            }
        ),
        Err(Error::Constraint)
    );
    assert_eq!(f.count("auth_audit"), 2);
    assert!(
        f.db.connection
            .execute("DELETE FROM auth_audit", [])
            .is_err()
    );
    for table in [
        "tenant",
        "principal",
        "identity_binding",
        "space",
        "auth_space",
    ] {
        assert_eq!(f.count(table), 2, "{table}");
    }
    f.assert_empty();
}

mod r1;
