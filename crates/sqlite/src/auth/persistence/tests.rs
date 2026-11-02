use super::*;
#[derive(Clone)]
struct TestClock;
impl TestClock {
    fn new() -> Self {
        Self
    }
}
impl Clock for TestClock {
    fn utc_millis(&self) -> Result<i64> {
        Ok(100)
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
use tempfile::{TempDir, tempdir};
struct Fixture {
    _dir: TempDir,
    path: PathBuf,
    db: AuthDatabase<TestClock>,
    store: Store,
    authority: BootstrapAuthority,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempdir().unwrap();
        assert!(dir.path().to_string_lossy().starts_with("C:"));
        let path = dir.path().join("content.sqlite");
        let mut db = AuthDatabase::open_profile(&path, TestClock::new(), 6).unwrap();
        let authority = db.bootstrap(id(90)).unwrap();
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
        for capability in ["memory.append", "memory.revise", "memory.read"] {
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
        let root = dir.path().join("cipher");
        std::fs::create_dir(&root).unwrap();
        Self {
            _dir: dir,
            path,
            db,
            store: Store { root },
            authority,
        }
    }
    fn ctx(&self) -> AuthContext {
        self.db
            .identify(TrustedIdentity { subject: id(4) }, id(3))
            .unwrap()
    }
    fn cmd(&self, k: u8) -> Command {
        Command {
            key: id(k),
            target: None,
            variant: 0,
        }
    }
    fn reopen(&mut self) {
        self.db = AuthDatabase::open_profile(&self.path, TestClock::new(), 6).unwrap();
    }
    fn count(&self, t: &str) -> i64 {
        self.db
            .connection
            .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get(0))
            .unwrap()
    }
}
#[test]
fn create_restart_read_revise_cas() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    let m = f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
    f.reopen();
    assert_eq!(
        f.db.release_digest(&ctx, c.key, &f.store).unwrap(),
        hash(CANARY)
    );
    let mut r = f.cmd(61);
    r.target = Some((m, 1));
    f.db.persist(&ctx, &r, &f.store, Stop::None).unwrap();
    let mut stale = f.cmd(62);
    stale.target = Some((m, 1));
    assert_eq!(
        f.db.persist(&ctx, &stale, &f.store, Stop::None),
        Err(Error::CasConflict)
    );
    assert_eq!(f.count("revision"), 2);
    assert_eq!(f.count("copy_manifest"), 16);
    assert_eq!(f.count("outbox_job"), 2);
}
#[test]
fn restart_at_each_boundary_and_reply_loss() {
    for stop in [
        Stop::Stage,
        Stop::Published,
        Stop::Seal,
        Stop::T1,
        Stop::Reply,
    ] {
        let mut f = Fixture::new();
        let ctx = f.ctx();
        let c = f.cmd(60);
        assert!(f.db.persist(&ctx, &c, &f.store, stop).is_err());
        if stop != Stop::Reply {
            assert_eq!(f.count("memory"), 0);
            assert_eq!(f.count("outbox_job"), 0);
            assert_eq!(f.count("commit_record"), 0);
            assert_eq!(f.count("copy_manifest"), 0);
            assert!(f.db.release_digest(&ctx, c.key, &f.store).is_err());
        }
        f.reopen();
        let m = f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
        assert_eq!(f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap(), m);
        assert_eq!(f.count("memory"), 1);
        assert_eq!(f.count("revision"), 1);
        assert_eq!(f.count("outbox_job"), 1);
        assert_eq!(f.count("commit_record"), 1);
    }
}
#[test]
fn different_request_same_key_rejected() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let mut c = f.cmd(60);
    f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
    c.variant = 1;
    assert_eq!(
        f.db.persist(&ctx, &c, &f.store, Stop::None),
        Err(Error::IdempotencyConflict)
    );
}
#[test]
fn corrupt_or_missing_ciphertext_never_released() {
    for missing in [true, false] {
        let mut f = Fixture::new();
        let ctx = f.ctx();
        let c = f.cmd(60);
        f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
        let p = f.db.plan(&ctx, c.key).unwrap().copies[0].0;
        if missing {
            std::fs::remove_file(f.store.path(p)).unwrap();
        } else {
            std::fs::write(f.store.path(p), b"corrupt").unwrap();
        }
        f.reopen();
        assert!(f.db.release_digest(&ctx, c.key, &f.store).is_err());
    }
}
#[test]
fn aad_cross_scope_and_generation_rejected() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
    let (p, k, o, kind, _) = f.db.plan(&ctx, c.key).unwrap().copies[0].clone();
    let b = binding(&ctx, p, k, o, kind);
    let bytes = f.store.read(p).unwrap();
    for change in 0..5 {
        let mut bad = b;
        match change {
            0 => bad.scope.space = [44; 16],
            1 => bad.scope.tenant = [45; 16],
            2 => bad.seal_generation += 1,
            3 => bad.owner = [46; 16],
            _ => bad.protection = [47; 32],
        }
        assert!(open(&bad, &bytes).is_err());
    }
}
#[test]
fn revoked_context_cannot_replay_or_release() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
    f.db.administer(
        &f.authority,
        AdminChange::MemoryGrant {
            scope: scope(),
            principal: id(2),
            capability: "memory.append",
            enabled: false,
            expires_at_ms: 1000,
        },
    )
    .unwrap();
    assert_eq!(
        f.db.persist(&ctx, &c, &f.store, Stop::None),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(
        f.db.release_digest(&ctx, c.key, &f.store),
        Err(Error::AuthorizationDenied)
    );
}
#[test]
fn old_profiles_never_automigrated() {
    let f = Fixture::new();
    assert!(AuthDatabase::open_profile(&f.path, TestClock::new(), 5).is_err());
}
#[test]
fn no_plaintext_in_db_wal_or_files() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
    for p in [
        f.path.clone(),
        PathBuf::from(format!("{}-wal", f.path.display())),
    ] {
        if let Ok(b) = std::fs::read(p) {
            assert!(!b.windows(CANARY.len()).any(|w| w == CANARY));
        }
    }
    for e in std::fs::read_dir(&f.store.root).unwrap() {
        let b = std::fs::read(e.unwrap().path()).unwrap();
        assert!(!b.windows(CANARY.len()).any(|w| w == CANARY));
    }
}
#[test]
fn no_replace_and_partial_file_fail_closed() {
    let f = Fixture::new();
    let p = id(66);
    f.store.publish(p, b"old").unwrap();
    assert!(f.store.publish(p, b"replacement").is_err());
    assert_eq!(f.store.read(p).unwrap(), b"old");
}

#[test]
fn process_child() {
    let Some(path) = std::env::var_os("YIXU_M3B_DB") else {
        return;
    };
    let mut db = AuthDatabase::open_profile(Path::new(&path), TestClock::new(), 6).unwrap();
    let ctx = db
        .identify(TrustedIdentity { subject: id(4) }, id(3))
        .unwrap();
    let store = Store {
        root: PathBuf::from(std::env::var_os("YIXU_M3B_STORE").unwrap()),
    };
    let point = std::env::var("YIXU_M3B_KILL").unwrap();
    let stop = match point.as_str() {
        "Stage" => Stop::Stage,
        "Published" => Stop::Published,
        "Seal" => Stop::Seal,
        "T1" => Stop::T1,
        _ => Stop::Reply,
    };
    let _ = db.persist(
        &ctx,
        &Command {
            key: id(60),
            target: None,
            variant: 0,
        },
        &store,
        stop,
    );
    panic!("did not reach kill point");
}
#[test]
fn parent_kills_process_at_five_persistence_boundaries() {
    for stop in [
        Stop::Stage,
        Stop::Published,
        Stop::Seal,
        Stop::T1,
        Stop::Reply,
    ] {
        let mut f = Fixture::new();
        let ready = f._dir.path().join("ready");
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "auth::persistence::tests::process_child",
                "--nocapture",
            ])
            .env("YIXU_M3B_DB", &f.path)
            .env("YIXU_M3B_STORE", &f.store.root)
            .env("YIXU_M3B_KILL", format!("{stop:?}"))
            .env("YIXU_M3B_READY", &ready)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        while !ready.exists()
            && std::time::Instant::now() < deadline
            && child.try_wait().unwrap().is_none()
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        let reached = ready.exists();
        let _ = child.kill();
        child.wait().unwrap();
        assert!(reached, "child failed at {stop:?}");
        f.reopen();
        let ctx = f.ctx();
        let c = f.cmd(60);
        f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
        assert_eq!(f.count("revision"), 1);
        assert_eq!(f.count("outbox_job"), 1);
        assert_eq!(
            f.db.release_digest(&ctx, c.key, &f.store).unwrap(),
            hash(CANARY)
        );
    }
}
#[test]
fn claimed_but_missing_seal_is_unknown_never_reencrypted() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    f.db.stage(&ctx, &c).unwrap();
    f.db.connection
        .execute("UPDATE sealed_copy SET attempted=1", [])
        .unwrap();
    f.reopen();
    assert_eq!(
        f.db.persist(&ctx, &c, &f.store, Stop::None),
        Err(Error::CapabilityUnavailable)
    );
    assert_eq!(f.count("memory"), 0);
    assert_eq!(std::fs::read_dir(&f.store.root).unwrap().count(), 0);
}
#[test]
fn two_connections_cas_and_idempotent_replay() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    let m = f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
    let path = f.path.clone();
    let root = f.store.root.clone();
    let other_ctx = ctx.clone();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let b = barrier.clone();
    let handle = std::thread::spawn(move || {
        let mut db = AuthDatabase::open_profile(&path, TestClock::new(), 6).unwrap();
        b.wait();
        db.persist(
            &other_ctx,
            &Command {
                key: id(62),
                target: Some((m, 1)),
                variant: 0,
            },
            &Store { root },
            Stop::None,
        )
    });
    barrier.wait();
    let a = f.db.persist(
        &ctx,
        &Command {
            key: id(61),
            target: Some((m, 1)),
            variant: 0,
        },
        &f.store,
        Stop::None,
    );
    let b = handle.join().unwrap();
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(a == Err(Error::CasConflict) || b == Err(Error::CasConflict));
    assert_eq!(f.count("revision"), 2);
}
#[test]
fn profile_six_drift_and_unsealed_admission_rejected() {
    let f = Fixture::new();
    assert!(f.db.connection.execute("INSERT INTO payload VALUES(?1,?2,?3,'content','synthetic','sensitive',1,?3,?3,'synthetic-xchacha-v1',?3,1,'available')",params![id(1).to_string(),id(3).to_string(),id(44).to_string()]).is_err());
    f.db.connection
        .execute_batch("DROP TRIGGER payload_no_admission")
        .unwrap();
    assert!(AuthDatabase::open_profile(&f.path, TestClock::new(), 6).is_err());
}

#[test]
fn simulated_io_failure_does_not_create_business_facts() {
    for fault in [IoFault::Enospc, IoFault::ShortWrite, IoFault::Sync] {
        let mut f = Fixture::new();
        let ctx = f.ctx();
        let c = f.cmd(60);
        f.db.stage(&ctx, &c).unwrap();
        let (p, k, o, kind, _) = f.db.plan(&ctx, c.key).unwrap().copies[0].clone();
        let bytes = seal(&binding(&ctx, p, k, o, kind)).unwrap();
        assert!(f.store.publish_fault(p, &bytes, fault).is_err());
        assert_eq!(f.count("memory"), 0);
        assert!(f.db.release_digest(&ctx, c.key, &f.store).is_err());
    }
}
#[test]
fn orphan_inventory_and_fenced_known_cleanup() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    assert!(f.db.persist(&ctx, &c, &f.store, Stop::Published).is_err());
    assert_eq!(f.db.inventory_orphans(&f.store).unwrap().len(), 4);
    std::fs::write(f.store.root.join("unrecognized"), b"preserve").unwrap();
    f.db.cleanup_uncommitted(&ctx, c.key, &f.store).unwrap();
    assert!(f.db.inventory_orphans(&f.store).unwrap().is_empty());
    assert!(f.store.root.join("unrecognized").exists());
    assert!(f.db.persist(&ctx, &c, &f.store, Stop::None).is_err());
    assert_eq!(f.count("memory"), 0);
}
#[test]
fn committed_objects_never_orphan_cleaned() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
    assert!(f.db.inventory_orphans(&f.store).unwrap().is_empty());
    assert!(f.db.cleanup_uncommitted(&ctx, c.key, &f.store).is_err());
    assert_eq!(
        f.db.release_digest(&ctx, c.key, &f.store).unwrap(),
        hash(CANARY)
    );
}

#[test]
fn process_writer() {
    let Some(path) = std::env::var_os("YIXU_M3B_WRITER") else {
        return;
    };
    let mut db = AuthDatabase::open_profile(Path::new(&path), TestClock::new(), 6).unwrap();
    let ctx = db
        .identify(TrustedIdentity { subject: id(4) }, id(3))
        .unwrap();
    let store = Store {
        root: PathBuf::from(std::env::var_os("YIXU_M3B_STORE").unwrap()),
    };
    let result = db.persist(
        &ctx,
        &Command {
            key: id(60),
            target: None,
            variant: 0,
        },
        &store,
        Stop::None,
    );
    if result.is_err() {
        std::process::exit(71);
    }
}
#[test]
fn multiprocess_same_key_converges_without_second_fact() {
    let mut f = Fixture::new();
    let mut children = Vec::new();
    for _ in 0..2 {
        children.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "auth::persistence::tests::process_writer"])
                .env("YIXU_M3B_WRITER", &f.path)
                .env("YIXU_M3B_STORE", &f.store.root)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
    }
    for mut c in children {
        let code = c.wait().unwrap().code();
        assert!(code == Some(0) || code == Some(71));
    }
    f.reopen();
    let ctx = f.ctx();
    let c = f.cmd(60);
    f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
    assert_eq!(f.count("memory"), 1);
    assert_eq!(f.count("outbox_job"), 1);
}
#[test]
fn source_blocked_and_invalidation_never_release() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    let m = f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
    f.db.connection
        .execute(
            "UPDATE memory SET state='source_blocked' WHERE id=?1",
            [m.to_string()],
        )
        .unwrap();
    assert_eq!(
        f.db.release_digest(&ctx, c.key, &f.store),
        Err(Error::CapabilityUnavailable)
    );
}
#[test]
fn wrong_scope_and_principal_cannot_obtain_receipt() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
    let mut forged = ctx.clone();
    for principal in [true, false] {
        if principal {
            forged.principal = id(88)
        } else {
            forged = ctx.clone();
            forged.scope.space = id(89)
        }
        assert_eq!(
            f.db.persist(&forged, &c, &f.store, Stop::None),
            Err(Error::AuthorizationDenied)
        );
        assert_eq!(
            f.db.release_digest(&forged, c.key, &f.store),
            Err(Error::AuthorizationDenied)
        );
    }
}

#[test]
fn old_revision_requires_unimplemented_history_release_not_current_read() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    let m = f.db.persist(&ctx, &c, &f.store, Stop::None).unwrap();
    let mut r = f.cmd(61);
    r.target = Some((m, 1));
    f.db.persist(&ctx, &r, &f.store, Stop::None).unwrap();
    assert_eq!(
        f.db.release_digest(&ctx, c.key, &f.store),
        Err(Error::CapabilityUnavailable)
    );
    assert_eq!(
        f.db.release_digest(&ctx, r.key, &f.store).unwrap(),
        hash(CANARY)
    );
}

#[test]
fn copied_valid_envelope_without_durable_attempt_claim_is_rejected() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let c = f.cmd(60);
    f.db.stage(&ctx, &c).unwrap();
    for (p, k, o, kind, _) in f.db.plan(&ctx, c.key).unwrap().copies {
        let bytes = seal(&binding(&ctx, p, k, o, kind)).unwrap();
        f.store.publish(p, &bytes).unwrap();
    }
    assert!(f.db.persist(&ctx, &c, &f.store, Stop::None).is_err());
    assert_eq!(f.count("memory"), 0);
    assert_eq!(f.count("outbox_job"), 0);
    assert_eq!(f.count("commit_record"), 0);
}

#[test]
fn distinct_synthetic_bodies_have_distinct_released_digests() {
    let mut f = Fixture::new();
    let ctx = f.ctx();
    let a = f.cmd(60);
    let mut b = f.cmd(61);
    b.variant = 1;
    f.db.persist(&ctx, &a, &f.store, Stop::None).unwrap();
    f.db.persist(&ctx, &b, &f.store, Stop::None).unwrap();
    let ad = f.db.release_digest(&ctx, a.key, &f.store).unwrap();
    let bd = f.db.release_digest(&ctx, b.key, &f.store).unwrap();
    assert_ne!(ad, bd);
    assert_eq!(bd, hash(synthetic(1).unwrap()));
}
