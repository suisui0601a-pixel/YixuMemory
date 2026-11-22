use super::super::tests::{Fixture, TestClock};
use super::*;
fn id(n: u8) -> Id {
    Id::from_bytes([n; 16])
}
fn vault(f: &Fixture) -> LocalVault {
    let p = f._dir.path().join("keys");
    std::fs::create_dir(&p).unwrap();
    LocalVault::new(p).unwrap()
}
#[test]
fn real_random_wrapping_restart_and_content_loop() {
    let mut f = Fixture::profile(7);
    let v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    let m =
        f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
            .unwrap();
    let p = f.db.plan(&ctx, c.key).unwrap();
    let mut hashes = Vec::new();
    for (id, k, o, kind, _) in &p.copies {
        let b = binding(&ctx, *id, *k, *o, *kind);
        let (dek, _) = v.load(&b).unwrap();
        assert!(dek.bytes() != Sha256::digest(raw(*k)).as_slice());
        hashes.push(hash(dek.bytes()));
    }
    hashes.sort();
    hashes.dedup();
    assert_eq!(hashes.len(), 4);
    f.db = AuthDatabase::open_profile(&f.path, TestClock::new(), 7).unwrap();
    assert_eq!(
        f.db.release_backend(&ctx, c.key, &f.store, Some(&v))
            .unwrap(),
        hash(CANARY)
    );
    let mut r = f.cmd(71);
    r.target = Some((m, 1));
    f.db.persist_backend(&ctx, &r, &f.store, Stop::None, Some(&v))
        .unwrap();
}
#[test]
fn wrong_scope_generation_entropy_and_wrapped_tamper_refused() {
    let mut f = Fixture::profile(7);
    let v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
        .unwrap();
    let (p, k, o, kind, _) = f.db.plan(&ctx, c.key).unwrap().copies[0].clone();
    let b = binding(&ctx, p, k, o, kind);
    let bytes = v.anchor.read(&k.to_string()).unwrap();
    for n in 0..6 {
        let mut wrong = b;
        match n {
            0 => wrong.scope.tenant = [34; 16],
            1 => wrong.scope.space = [35; 16],
            2 => wrong.seal_generation += 1,
            3 => wrong.key = [36; 16],
            4 => wrong.owner = [37; 16],
            _ => wrong.protection = [38; 32],
        };
        assert!(unprotect(&wrong, &bytes).is_err());
    }
    let mut corrupt = bytes;
    let n = corrupt.len() - 1;
    corrupt[n] ^= 1;
    assert!(unprotect(&b, &corrupt).is_err());
}
#[test]
fn revocation_persists_and_no_reenable_or_fake_fallback() {
    let mut f = Fixture::profile(7);
    let v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
        .unwrap();
    let (p, k, o, kind, _) = f.db.plan(&ctx, c.key).unwrap().copies[0].clone();
    let b = binding(&ctx, p, k, o, kind);
    f.db.revoke_local(&f.authority, &b, &v).unwrap();
    assert!(
        f.db.release_backend(&ctx, c.key, &f.store, Some(&v))
            .is_err()
    );
    f.db = AuthDatabase::open_profile(&f.path, TestClock::new(), 7).unwrap();
    assert!(
        f.db.release_backend(&ctx, c.key, &f.store, Some(&v))
            .is_err()
    );
    assert!(
        f.db.connection
            .execute("UPDATE local_key_registry SET revoked=0", [])
            .is_err()
    );
    assert!(f.db.persist(&ctx, &c, &f.store, Stop::None).is_err());
    assert!(f.db.release_digest(&ctx, c.key, &f.store).is_err());
}
#[test]
fn missing_wrapped_key_and_unknown_gate_fail_closed() {
    let mut f = Fixture::profile(7);
    let v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
        .unwrap();
    let (_, k, _, _, _) = f.db.plan(&ctx, c.key).unwrap().copies[0].clone();
    let bytes = v.anchor.read(&k.to_string()).unwrap();
    v.anchor.erase(&k.to_string(), &bytes).unwrap();
    assert!(
        f.db.release_backend(&ctx, c.key, &f.store, Some(&v))
            .is_err()
    );
    f.db.connection
        .execute("UPDATE local_key_gate SET state='unknown'", [])
        .unwrap();
    assert!(
        f.db.persist_backend(&ctx, &f.cmd(71), &f.store, Stop::None, Some(&v))
            .is_err()
    );
}

#[test]
fn os_output_is_wiped_on_success_and_invalid_plaintext_length() {
    let b = binding(&Fixture::profile(7).ctx(), id(42), id(43), id(44), 2);
    let aad = b.aad(32);
    LAST_WIPE.set(false);
    let out = call(&[3; 10], &aad, true).unwrap();
    let mut wrapped = b"YIXUK001".to_vec();
    wrapped.extend(out.bytes());
    drop(out);
    assert!(LAST_WIPE.get());
    LAST_WIPE.set(false);
    assert!(unprotect(&b, &wrapped).is_err());
    assert!(LAST_WIPE.get());
}
#[test]
fn unwrap_racing_key_revocation_has_no_output() {
    let mut f = Fixture::profile(7);
    let mut v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
        .unwrap();
    let (p, k, o, kind, _) = f.db.plan(&ctx, c.key).unwrap().copies[0].clone();
    let b = binding(&ctx, p, k, o, kind);
    let path = f.path.clone();
    let realm = f.authority.realm;
    let keyroot = f._dir.path().join("keys");
    let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
    let g = gate.clone();
    let writer = std::thread::spawn(move || {
        let mut db = AuthDatabase::open_profile(&path, TestClock::new(), 7).unwrap();
        g.wait();
        let vault = LocalVault::new(keyroot).unwrap();
        db.revoke_local(&BootstrapAuthority { realm }, &b, &vault)
            .unwrap();
        g.wait();
    });
    v.after_unprotect = Some(Box::new(move || {
        gate.wait();
        gate.wait();
    }));
    assert!(
        f.db.release_backend(&ctx, c.key, &f.store, Some(&v))
            .is_err()
    );
    writer.join().unwrap();
}
#[test]
fn unwrap_racing_grant_revocation_and_wrong_principal_rejected() {
    let mut f = Fixture::profile(7);
    let mut v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
        .unwrap();
    let mut wrong = ctx.clone();
    wrong.principal = id(98);
    assert!(
        f.db.release_backend(&wrong, c.key, &f.store, Some(&v))
            .is_err()
    );
    let path = f.path.clone();
    let realm = f.authority.realm;
    let scope = ctx.scope;
    v.after_unprotect = Some(Box::new(move || {
        let mut db = AuthDatabase::open_profile(&path, TestClock::new(), 7).unwrap();
        db.administer(
            &BootstrapAuthority { realm },
            AdminChange::MemoryGrant {
                scope,
                principal: id(2),
                capability: "memory.read",
                enabled: false,
                expires_at_ms: 1000,
            },
        )
        .unwrap();
    }));
    assert!(
        f.db.release_backend(&ctx, c.key, &f.store, Some(&v))
            .is_err()
    );
}
#[test]
fn true_profile_rejects_old_profile_and_rollback_does_not_publish_facts() {
    let mut f = Fixture::profile(7);
    let v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    assert!(
        f.db.persist_backend(&ctx, &c, &f.store, Stop::T1, Some(&v))
            .is_err()
    );
    assert_eq!(f.count("revision"), 0);
    assert_eq!(f.count("commit_record"), 0);
    assert_eq!(f.count("outbox_job"), 0);
    assert_eq!(f.count("local_key_registry"), 4);
    assert!(AuthDatabase::open_profile(&f.path, TestClock::new(), 6).is_err());
    f.db = AuthDatabase::open_profile(&f.path, TestClock::new(), 7).unwrap();
    f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
        .unwrap();
    assert_eq!(f.count("revision"), 1);
}
#[test]
fn dpapi_process_child() {
    let Some(path) = std::env::var_os("YIXU_W2_DB") else {
        return;
    };
    let mut db = AuthDatabase::open_profile(Path::new(&path), TestClock::new(), 7).unwrap();
    let ctx = db
        .identify(TrustedIdentity { subject: id(4) }, id(3))
        .unwrap();
    let store = Store::new(PathBuf::from(std::env::var_os("YIXU_W2_STORE").unwrap())).unwrap();
    let v = LocalVault::new(PathBuf::from(std::env::var_os("YIXU_W2_VAULT").unwrap())).unwrap();
    let output = db.release_backend(&ctx, id(70), &store, Some(&v)).unwrap();
    assert_eq!(output, hash(CANARY));
}
#[test]
fn distinct_process_after_restart_unwraps_only_current_authorized_keys() {
    let mut f = Fixture::profile(7);
    let v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
        .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "auth::persistence::dpapi::tests::dpapi_process_child",
        ])
        .env("YIXU_W2_DB", &f.path)
        .env("YIXU_W2_STORE", &f.store.root)
        .env("YIXU_W2_VAULT", f._dir.path().join("keys"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn wrong_dek_and_plaintext_key_persistence_are_rejected() {
    let mut f = Fixture::profile(7);
    let v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
        .unwrap();
    for (p, k, o, kind, _) in f.db.plan(&ctx, c.key).unwrap().copies {
        let b = binding(&ctx, p, k, o, kind);
        let (dek, _) = v.load(&b).unwrap();
        let bytes = f.store.read(p).unwrap();
        let wrong = lab_key(k).unwrap();
        assert!(open_using_key(&b, &bytes, &wrong).is_err());
        let mut paths = Vec::new();
        for e in std::fs::read_dir(f._dir.path()).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                for file in std::fs::read_dir(p).unwrap() {
                    paths.push(file.unwrap().path());
                }
            } else {
                paths.push(p);
            }
        }
        for path in paths {
            if let Ok(bytes) = std::fs::read(path) {
                assert!(
                    !bytes.windows(32).any(|w| w == dek.bytes()),
                    "plaintext DEK must not persist"
                );
            }
        }
    }
}
#[test]
fn vault_marker_survives_sqlite_only_backup_and_key_paths_are_hardened() {
    let mut f = Fixture::profile(7);
    let v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
        .unwrap();
    let (p, k, o, kind, _) = f.db.plan(&ctx, c.key).unwrap().copies[0].clone();
    let b = binding(&ctx, p, k, o, kind);
    let backup = f._dir.path().join("db-backup.sqlite");
    f.db.connection
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
        .unwrap();
    std::fs::copy(&f.path, &backup).unwrap();
    f.db.revoke_local(&f.authority, &b, &v).unwrap();
    let mut old = AuthDatabase::open_profile(&backup, TestClock::new(), 7).unwrap();
    assert!(
        old.release_backend(&ctx, c.key, &f.store, Some(&v))
            .is_err()
    );
    assert!(LocalVault::new(f._dir.path().join("missing")).is_err());
}

#[test]
fn valid_dpapi_replacement_and_registry_tamper_cannot_release() {
    let mut f = Fixture::profile(7);
    let v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
        .unwrap();
    let (p, k, o, kind, _) = f.db.plan(&ctx, c.key).unwrap().copies[0].clone();
    let b = binding(&ctx, p, k, o, kind);
    assert!(
        f.db.connection
            .execute(
                "UPDATE local_key_registry SET wrapped_hash=zeroblob(32)",
                []
            )
            .is_err()
    );
    let mut replacement = LockedBuffer::new(32, false).unwrap();
    getrandom::fill(replacement.bytes_mut()).unwrap();
    let wrapped = protect(&b, &replacement).unwrap();
    let old = v.anchor.read(&k.to_string()).unwrap();
    v.anchor.erase(&k.to_string(), &old).unwrap();
    v.anchor
        .publish(
            "pending-valid-replacement",
            &k.to_string(),
            &wrapped,
            0,
            |_| {},
        )
        .unwrap();
    assert!(
        f.db.release_backend(&ctx, c.key, &f.store, Some(&v))
            .is_err()
    );
}

#[test]
fn malformed_output_length_never_drives_cleanup_slice() {
    LAST_WIPE.set(false);
    let mut out = call(&[8; 32], b"synthetic-context", true).unwrap();
    out.0.len = u32::MAX;
    drop(out);
    assert!(LAST_WIPE.get());
    assert!(call(b"not DPAPI", b"context", false).is_err());
}

#[test]
fn marker_publish_failure_keeps_committed_revocation_after_restart() {
    let mut f = Fixture::profile(7);
    let v = vault(&f);
    let ctx = f.ctx();
    let c = f.cmd(70);
    f.db.persist_backend(&ctx, &c, &f.store, Stop::None, Some(&v))
        .unwrap();
    let (p, k, o, kind, _) = f.db.plan(&ctx, c.key).unwrap().copies[0].clone();
    let b = binding(&ctx, p, k, o, kind);
    std::fs::create_dir(f._dir.path().join("keys").join(format!("revoked-{k}"))).unwrap();
    assert!(f.db.revoke_local(&f.authority, &b, &v).is_err());
    f.db = AuthDatabase::open_profile(&f.path, TestClock::new(), 7).unwrap();
    assert!(
        f.db.release_backend(&ctx, c.key, &f.store, Some(&v))
            .is_err()
    );
    let revoked: bool =
        f.db.connection
            .query_row(
                "SELECT revoked=1 FROM local_key_registry WHERE key_id=?1",
                [k.to_string()],
                |r| r.get(0),
            )
            .unwrap();
    assert!(revoked);
}
