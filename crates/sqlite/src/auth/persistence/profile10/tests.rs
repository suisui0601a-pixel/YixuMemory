use super::recovery::OwnerProcess;
use super::write::Point;
use super::*;
use std::io::Write;
use std::time::{Duration, Instant};

pub(super) fn installation() -> Installation {
    Installation {
        id: [91; 16],
        realm: [92; 16],
    }
}
pub(super) fn actor() -> Identity {
    Identity {
        scope: SecurityScope {
            realm: installation().realm,
            tenant: [93; 16],
            space: [94; 16],
        },
        principal: [95; 16],
    }
}
pub(super) fn provisions() -> Vec<Provision> {
    [101, 102]
        .into_iter()
        .map(|id| Provision {
            identity: actor(),
            memory: [id; 16],
            source: [96; 16],
            evidence: [97; 16],
            expiry: i64::MAX,
        })
        .collect()
}
pub(super) fn request(memory: u8) -> WriteRequest {
    WriteRequest {
        request: fresh().unwrap(),
        memory: [memory; 16],
        revision: fresh().unwrap(),
        evidence: [97; 16],
        evidence_generation: 1,
        generation: 1,
        expected_current: None,
    }
}
pub(super) struct OwnedRoot(std::path::PathBuf);
impl OwnedRoot {
    pub(super) fn path(&self) -> &Path {
        &self.0
    }
}
pub(super) fn owned_root() -> OwnedRoot {
    let base = std::env::var_os("YIXU_R6_TMP")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let root = tempfile::Builder::new()
        .prefix("yixu-p10-")
        .tempdir_in(base)
        .unwrap();
    for name in ["payloads", "keys", "temp", "control"] {
        std::fs::create_dir(root.path().join(name)).unwrap();
    }
    // Retain crash evidence and ciphertext for the B1 audit. No unknown orphan cleanup.
    OwnedRoot(root.keep())
}
pub(super) fn application(root: &Path) -> Application {
    Application {
        engine: Engine::create(root, installation(), &provisions()).unwrap(),
        identity: actor(),
    }
}
fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn row_plan(app: &Application, req: WriteRequest) -> Plan {
    Engine::plan_row(&app.engine.db, app.identity, req.request)
        .unwrap()
        .unwrap()
        .0
}
pub(super) fn sentinel() -> zeroize::Zeroizing<String> {
    zeroize::Zeroizing::new(format!(
        "唯一正文哨兵·{}·日本語とmémoire",
        filename(fresh().unwrap())
    ))
}

#[test]
fn real_utf8_application_write_random_keys_nonces_and_atomic_graph() {
    let root = owned_root();
    let mut app = application(root.path());
    let body = sentinel();
    let one = request(101);
    let two = request(102);
    let a = app.create_memory(one, &body).unwrap();
    let b = app
        .create_memory(two, "另一条实际正文，与第一条长度不同。")
        .unwrap();
    assert_eq!(a.knowledge_seq, 1);
    assert_eq!(b.knowledge_seq, 2);
    for table in [
        "write_intent",
        "publication",
        "memory",
        "revision",
        "payload",
        "local_key",
        "revision_evidence",
        "receipt",
        "outbox",
        "security_receipt",
    ] {
        assert_eq!(count(&app.engine.db, table), 2, "table {table}");
    }
    assert_eq!(count(&app.engine.db, "transition"), 10);
    let p = row_plan(&app, one);
    let q = row_plan(&app, two);
    let (ka, _) = app.engine.vault.load(&p.binding(installation())).unwrap();
    let (kb, _) = app.engine.vault.load(&q.binding(installation())).unwrap();
    assert!(ka.bytes() != kb.bytes());
    let ea = app.engine.files.read(&filename(p.payload)).unwrap();
    let eb = app.engine.files.read(&filename(q.payload)).unwrap();
    let offset = 12 + p.binding(installation()).aad(body.len() as u32).len();
    assert!(ea[offset..offset + 24] != eb[offset..offset + 24]);
    let (plain, _) = app.engine.verify_files(&p).unwrap();
    assert!(plain.bytes() == body.as_bytes());
    assert!(content_crypto::open(&p.binding(installation()), &ea, &kb).is_err());
    assert!(!app.engine.reconcile(10).unwrap().blocked);
}

#[test]
fn duplicate_requests_require_same_identity_metadata_and_actual_body() {
    let root = owned_root();
    let mut app = application(root.path());
    let req = request(101);
    let body = sentinel();
    let first = app.create_memory(req, &body).unwrap();
    assert_eq!(app.create_memory(req, &body).unwrap(), first);
    assert!(matches!(
        app.create_memory(req, "不同正文"),
        Err(Error::IdempotencyConflict)
    ));
    let mut changed = req;
    changed.revision = fresh().unwrap();
    assert!(matches!(
        app.create_memory(changed, &body),
        Err(Error::IdempotencyConflict)
    ));
    changed = req;
    changed.request = fresh().unwrap();
    assert!(matches!(
        app.create_memory(changed, &body),
        Err(Error::CasConflict)
    ));
    assert_eq!(count(&app.engine.db, "receipt"), 1);
    let identity = Identity {
        principal: [77; 16],
        ..actor()
    };
    assert!(
        app.engine
            .write(identity, req, &body, 10, |_, _| Ok(()))
            .is_err()
    );
    assert_eq!(count(&app.engine.db, "write_intent"), 1);
}

#[test]
fn scope_generation_cas_and_invalid_evidence_fail_before_publication() {
    let root = owned_root();
    let mut app = application(root.path());
    let base = request(101);
    for req in [
        WriteRequest {
            generation: 0,
            ..base
        },
        WriteRequest {
            generation: 2,
            ..base
        },
        WriteRequest {
            evidence: [78; 16],
            ..base
        },
        WriteRequest {
            evidence_generation: 2,
            ..base
        },
        WriteRequest {
            expected_current: Some([79; 16]),
            ..base
        },
    ] {
        assert!(app.create_memory(req, "正文").is_err());
    }
    let identity = Identity {
        scope: SecurityScope {
            space: [76; 16],
            ..actor().scope
        },
        ..actor()
    };
    assert!(
        app.engine
            .write(identity, base, "正文", 10, |_, _| Ok(()))
            .is_err()
    );
    assert_eq!(count(&app.engine.db, "write_intent"), 0);
    assert_eq!(
        std::fs::read_dir(root.path().join("payloads"))
            .unwrap()
            .count(),
        0
    );
    app.engine
        .db
        .execute("UPDATE evidence SET valid=0", [])
        .unwrap();
    assert!(app.create_memory(base, "正文").is_err());
}

#[test]
fn revocation_and_closing_use_the_shared_r4_r5_transaction_boundary() {
    let root = owned_root();
    let mut app = application(root.path());
    let event = app.engine.close_grant(app.identity).unwrap();
    app.engine.apply_revocation(event, app.identity).unwrap();
    assert!(app.create_memory(request(101), "不得写入").is_err());
    assert_eq!(count(&app.engine.db, "memory"), 0);
    drop(app);
    let root = owned_root();
    let mut app = application(root.path());
    let req = request(101);
    let outcome = app.engine.write(
        app.identity,
        req,
        "关闭竞争中的正文",
        10,
        |point, engine| {
            if point == Point::Sealed {
                let mut concurrent = Connection::open(root.path().join("content.sqlite")).unwrap();
                configure(&concurrent).unwrap();
                let tx = concurrent
                    .transaction_with_behavior(TransactionBehavior::Immediate)
                    .unwrap();
                tx.execute(
                    "INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",
                    [actor().scope_bytes()],
                )
                .unwrap();
                tx.commit().unwrap();
                // Proof that W1/DPAPI ran without retaining a SQLite writer lock.
                assert!(gates::closing(&engine.db).unwrap());
            }
            Ok(())
        },
    );
    assert!(outcome.is_err());
    assert_eq!(count(&app.engine.db, "receipt"), 0);
    assert_eq!(count(&app.engine.db, "memory"), 0);
    assert!(app.engine.reconcile(10).unwrap().blocked);
}

#[test]
fn dpapi_boundary_file_write_and_no_replace_failures_retain_unknown_intents() {
    for mode in 0..3 {
        let root = owned_root();
        let mut app = application(root.path());
        let req = request(101);
        let result = app.engine.write(
            app.identity,
            req,
            "失败时只能留下密文",
            10,
            |point, engine| {
                if mode == 0 && point == Point::BeforeKey {
                    return Err(Error::CapabilityUnavailable);
                }
                if point == Point::BeforeCipher && mode > 0 {
                    let plan = Engine::plan_row(&engine.db, actor(), req.request)?
                        .unwrap()
                        .0;
                    let name = filename(plan.payload);
                    if mode == 1 {
                        return engine
                            .files
                            .publish("injected-partial", &name, b"ciphertext-only", 1, |_| {})
                            .map_err(|_| Error::ResourceUnavailable);
                    }
                    engine
                        .files
                        .publish("injected-existing", &name, b"foreign-ciphertext", 0, |_| {})
                        .map_err(|_| Error::ResourceUnavailable)?;
                }
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(count(&app.engine.db, "receipt"), 0);
        assert_eq!(count(&app.engine.db, "memory"), 0);
        assert!(app.engine.reconcile(10).unwrap().blocked);
        assert!(app.create_memory(req, "禁止换密钥重试").is_err());
    }
}

#[test]
fn real_t1_transaction_rollback_keeps_every_business_table_empty() {
    let root = owned_root();
    let mut app = application(root.path());
    let req = request(101);
    let result = app.engine.write(
        app.identity,
        req,
        "T1 回滚的正文",
        10,
        |point, engine| {
            if point == Point::BeforeCommit {
                engine.before_commit = Some(Box::new(|| Err(Error::Database)));
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    for table in [
        "memory",
        "revision",
        "payload",
        "local_key",
        "revision_evidence",
        "receipt",
        "outbox",
        "security_event",
        "security_receipt",
    ] {
        assert_eq!(count(&app.engine.db, table), 0, "table {table}");
    }
    assert_eq!(count(&app.engine.db, "publication"), 1);
    assert!(app.engine.reconcile(10).unwrap().blocked);
}

#[test]
fn restart_detects_missing_cipher_tamper_wrapped_key_and_replacement_identity() {
    for mode in 0..5 {
        let root = owned_root();
        let mut app = application(root.path());
        let req = request(101);
        app.create_memory(req, "持久化后损坏检查").unwrap();
        let plan = row_plan(&app, req);
        app.engine.clean_stop(10).unwrap();
        drop(app);
        let cipher = root.path().join("payloads").join(filename(plan.payload));
        let key = root.path().join("keys").join(filename(plan.key));
        // Adversarial fixture edits are isolated to this test's owned ciphertext.
        match mode {
            0 => std::fs::remove_file(&cipher).unwrap(),
            1 => {
                let mut b = std::fs::read(&cipher).unwrap();
                let last = b.len() - 1;
                b[last] ^= 1;
                std::fs::write(&cipher, b).unwrap();
            }
            2 => {
                let mut b = std::fs::read(&key).unwrap();
                b[10] ^= 1;
                std::fs::write(&key, b).unwrap();
            }
            3 => {
                let b = std::fs::read(&cipher).unwrap();
                std::fs::rename(&cipher, cipher.with_extension("retained")).unwrap();
                std::fs::write(&cipher, b).unwrap();
            }
            _ => std::fs::remove_file(&key).unwrap(),
        }
        let opened = Engine::open_existing(root.path(), installation()).unwrap();
        let report = opened.reconcile(10).unwrap();
        assert!(report.blocked);
        assert_eq!(report.committed, 0);
        if mode == 0 {
            assert!(
                report
                    .states
                    .iter()
                    .any(|(_, s)| s == "CiphertextMissingOrUnavailable")
            );
        }
        if mode == 4 {
            assert!(report.states.iter().any(|(_, s)| s == "KeyUnavailable"));
        }
    }
}

#[test]
fn schema_profile_installation_realm_and_immutable_graph_reject() {
    let root = owned_root();
    let mut app = application(root.path());
    let req = request(101);
    app.create_memory(req, "不可变版本").unwrap();
    assert!(
        app.engine
            .db
            .execute("UPDATE revision SET number=2", [])
            .is_err()
    );
    assert!(app.engine.db.execute("DELETE FROM receipt", []).is_err());
    drop(app);
    assert!(Engine::create(root.path(), installation(), &provisions()).is_err());
    assert!(
        Engine::open_existing(
            root.path(),
            Installation {
                id: [75; 16],
                ..installation()
            }
        )
        .is_err()
    );
    assert!(
        Engine::open_existing(
            root.path(),
            Installation {
                realm: [75; 16],
                ..installation()
            }
        )
        .is_err()
    );
    let db = Connection::open(root.path().join("content.sqlite")).unwrap();
    db.execute_batch("PRAGMA user_version=9").unwrap();
    drop(db);
    assert!(Engine::open_existing(root.path(), installation()).is_err());
    let db = Connection::open(root.path().join("content.sqlite")).unwrap();
    db.execute_batch("PRAGMA user_version=10; CREATE TABLE unexpected(x TEXT)")
        .unwrap();
    drop(db);
    assert!(Engine::open_existing(root.path(), installation()).is_err());
}

pub(super) fn scan(root: &Path, needle: &[u8], live_lock: Option<&File>) -> (usize, u64) {
    let locked = live_lock.map(|lock| {
        assert_eq!(lock.metadata().unwrap().len(), 0);
        root.join("broker-lock")
    });
    fn recurse(root: &Path, needle: &[u8], n: &mut usize, bytes: &mut u64, locked: Option<&Path>) {
        for entry in std::fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            assert!(
                !entry
                    .file_name()
                    .to_string_lossy()
                    .as_bytes()
                    .windows(needle.len())
                    .any(|w| w == needle)
            );
            if entry.file_type().unwrap().is_dir() {
                recurse(&path, needle, n, bytes, locked);
            } else {
                if locked == Some(path.as_path()) {
                    // Live exclusive W1 lock: zero bytes verified through its owned HANDLE.
                    *n += 1;
                    continue;
                }
                let contents = std::fs::read(&path).unwrap();
                assert!(
                    !contents.windows(needle.len()).any(|w| w == needle),
                    "plaintext canary detected"
                );
                let text = std::str::from_utf8(needle).unwrap();
                let utf16: Vec<u8> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
                assert!(
                    !contents.windows(utf16.len()).any(|w| w == utf16),
                    "UTF16 canary detected"
                );
                *n += 1;
                *bytes += contents.len() as u64;
            }
        }
    }
    let mut n = 0;
    let mut bytes = 0;
    recurse(root, needle, &mut n, &mut bytes, locked.as_deref());
    (n, bytes)
}

#[test]
fn plaintext_sentinel_absent_from_database_wal_shm_temp_names_and_artifacts() {
    let root = owned_root();
    let mut app = application(root.path());
    let body = sentinel();
    app.create_memory(request(101), &body).unwrap();
    let (files, bytes) = scan(root.path(), body.as_bytes(), Some(&app.engine._lock));
    assert!(files >= 5 && bytes > 0);
    app.engine.clean_stop(10).unwrap();
    drop(app);
    let (files2, bytes2) = scan(root.path(), body.as_bytes(), None);
    assert!(files2 >= 4 && bytes2 > 0);
    println!(
        "P10_LEAK_SCAN files_live={files} bytes_live={bytes} files_closed={files2} bytes_closed={bytes2} matches=0"
    );
}

// This helper never emits plaintext; only harness status and opaque incarnation reach stdout/disk.
#[test]
fn owned_process_entry() {
    let Some(root) = std::env::var_os("YIXU_P10_CHILD") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let mode = std::env::var("YIXU_P10_POINT").unwrap();
    let seed = std::env::var("YIXU_P10_SEED").unwrap();
    let body = zeroize::Zeroizing::new(format!("子进程正文哨兵·{seed}·mémoire"));
    let nonce = raw(std::env::var("YIXU_P10_NONCE")
        .unwrap()
        .parse::<yixu_domain::Id>()
        .unwrap());
    if mode.starts_with("B3") {
        super::b3::tests::owner_round(&root, &mode, nonce);
        return;
    }
    if mode.starts_with("B2") {
        super::read::tests::owner_round(&root, &mode, &seed, nonce);
        return;
    }
    let mut app = Application {
        engine: Engine::create_supervised(&root, installation(), &provisions(), nonce).unwrap(),
        identity: actor(),
    };
    println!(
        "\nP10_OWNER_START {} {} {} {}",
        filename(installation().id),
        filename(installation().realm),
        filename(app.engine.incarnation),
        filename(nonce)
    );
    std::io::stdout().flush().unwrap();
    let req = request(101);
    let checkpoint = |name: &str| {
        std::fs::write(root.join("control").join("checkpoint"), name.as_bytes()).unwrap();
        loop {
            std::thread::park_timeout(Duration::from_millis(100));
        }
    };
    app.engine
        .write(app.identity, req, &body, 10, |point, engine| {
            if mode == "T1During" && point == Point::BeforeCommit {
                let ready = root.join("control").join("checkpoint");
                engine.before_commit = Some(Box::new(move || {
                    std::fs::write(ready, b"T1During").unwrap();
                    loop {
                        std::thread::park_timeout(Duration::from_millis(100));
                    }
                }));
            }
            if format!("{point:?}") == mode {
                checkpoint(&mode);
            }
            Ok(())
        })
        .unwrap();
    if mode == "CleanRevoked" {
        let event = app.engine.close_grant(app.identity).unwrap();
        app.engine.apply_revocation(event, app.identity).unwrap();
    }
    if mode == "Clean" || mode == "CleanRevoked" {
        app.engine.clean_stop(10).unwrap();
    }
    drop(app);
    std::io::stdout().flush().unwrap();
}

#[test]
fn namespace_must_be_fresh_and_unknown_objects_block_recovery() {
    for name in [
        "content.sqlite-wal",
        "content.sqlite-shm",
        "old-database",
        "unexpected",
    ] {
        let root = owned_root();
        std::fs::write(root.path().join(name), b"stale nonplaintext fixture").unwrap();
        assert!(Engine::create(root.path(), installation(), &provisions()).is_err());
        assert!(!root.path().join("content.sqlite").exists());
    }
    let root = owned_root();
    std::fs::write(
        root.path()
            .join("payloads")
            .join(filename(fresh().unwrap())),
        b"old ciphertext",
    )
    .unwrap();
    assert!(Engine::create(root.path(), installation(), &provisions()).is_err());
    assert!(!root.path().join("content.sqlite").exists());
    let root = owned_root();
    let mut app = application(root.path());
    app.create_memory(request(101), "合法正文").unwrap();
    app.engine.clean_stop(10).unwrap();
    drop(app);
    std::fs::write(
        root.path().join("keys").join(filename(fresh().unwrap())),
        b"unknown wrapped object",
    )
    .unwrap();
    let reopened = Engine::open_existing(root.path(), installation()).unwrap();
    let report = reopened.reconcile(10).unwrap();
    assert!(report.blocked);
    assert!(
        report
            .states
            .iter()
            .any(|(_, state)| state == "UnregisteredStorageObjects")
    );
}

#[test]
fn expired_grants_use_the_os_clock_and_recheck_before_t1() {
    let root = owned_root();
    let mut grants = provisions();
    for grant in &mut grants {
        grant.expiry = 1;
    }
    let mut app = Application {
        engine: Engine::create(root.path(), installation(), &grants).unwrap(),
        identity: actor(),
    };
    assert!(app.create_memory(request(101), "过期请求").is_err());
    let root = owned_root();
    let mut grants = provisions();
    let expiry = trusted_now().unwrap() + 2;
    for grant in &mut grants {
        grant.expiry = expiry;
    }
    let mut engine = Engine::create(root.path(), installation(), &grants).unwrap();
    let result = engine.write(
        actor(),
        request(101),
        "写入中授权到期",
        10,
        |point, _| {
            if point == Point::Sealed {
                std::thread::sleep(Duration::from_secs(3));
            }
            Ok(())
        },
    );
    assert!(result.is_err());
    assert_eq!(count(&engine.db, "receipt"), 0);
}

#[test]
fn clean_stop_and_exit_proofs_cannot_be_reassociated_or_replace_ledger_checks() {
    let a = owned_root();
    let b = owned_root();
    let owner_a = child(a.path(), "Clean", &filename(fresh().unwrap()));
    let owner_b = child(b.path(), "Clean", &filename(fresh().unwrap()));
    let proof_a = owner_a.wait_clean().unwrap();
    let proof_b = owner_b.wait_clean().unwrap();
    assert!(Engine::resume(b.path(), installation(), proof_a, 10).is_err());
    assert!(Engine::resume(a.path(), installation(), proof_b, 10).is_err());
    let root = owned_root();
    let owner = child(root.path(), "Clean", &filename(fresh().unwrap()));
    let proof = owner.wait_clean().unwrap();
    let db = Connection::open(root.path().join("content.sqlite")).unwrap();
    db.execute("UPDATE source SET valid=0", []).unwrap();
    drop(db);
    assert!(Engine::resume(root.path(), installation(), proof, 10).is_err());
}

#[test]
fn clean_restart_preserves_revocation_for_settled_content() {
    let root = owned_root();
    let owner = child(root.path(), "CleanRevoked", &filename(fresh().unwrap()));
    let proof = owner.wait_clean().unwrap();
    let engine = Engine::resume(root.path(), installation(), proof, 10).unwrap();
    let report = engine.reconcile(10).unwrap();
    assert!(!report.blocked);
    assert_eq!(report.committed, 1);
    assert!(
        report
            .states
            .iter()
            .any(|(_, state)| state == "SecurityStateDenied")
    );
    let mut app = Application {
        engine,
        identity: actor(),
    };
    assert!(
        app.create_memory(request(102), "已撤权主体不得写入")
            .is_err()
    );
    app.engine.clean_stop(10).unwrap();
}

#[test]
fn request_ids_are_scoped_and_reusing_a_payload_key_is_rejected() {
    let root = owned_root();
    let other = Identity {
        principal: [80; 16],
        scope: SecurityScope {
            space: [81; 16],
            ..actor().scope
        },
    };
    let mut grants = provisions();
    grants.push(Provision {
        identity: other,
        memory: [101; 16],
        source: [96; 16],
        evidence: [97; 16],
        expiry: i64::MAX,
    });
    let mut app = Application {
        engine: Engine::create(root.path(), installation(), &grants).unwrap(),
        identity: actor(),
    };
    let req = request(101);
    app.create_memory(req, "域一正文").unwrap();
    let first = row_plan(&app, req);
    app.identity = other;
    app.create_memory(req, "域二正文").unwrap();
    assert_eq!(count(&app.engine.db, "receipt"), 2);
    let second = row_plan(&app, req);
    assert!(first.key != second.key);
    assert!(app.engine.db.execute(
        "INSERT INTO write_intent SELECT ?1,scope,principal,?2,memory,?3,?4,key_id,
         evidence,evidence_generation,generation,expected_current,incarnation,epoch,dispatch_binding,'Pending'
         FROM write_intent WHERE operation=?5",
        params![fresh().unwrap(),fresh().unwrap(),fresh().unwrap(),fresh().unwrap(),second.operation],
    ).is_err());
    assert_eq!(count(&app.engine.db, "write_intent"), 2);
}

fn child(root: &Path, mode: &str, seed: &str) -> OwnerProcess {
    OwnerProcess::launch(root, installation(), mode, seed).unwrap()
}

#[test]
fn owned_child_clean_restart_reopens_database_files_dpapi_aead_and_r5() {
    let root = owned_root();
    let seed = filename(fresh().unwrap());
    let child = child(root.path(), "Clean", &seed);
    let inc = child.incarnation();
    let proof = child.wait_clean().unwrap();
    let reopened = Engine::resume(root.path(), installation(), proof, 10).unwrap();
    assert!(reopened.incarnation != inc);
    let report = reopened.reconcile(10).unwrap();
    assert!(!report.blocked);
    assert_eq!(report.committed, 1);
    let mut app = Application {
        engine: reopened,
        identity: actor(),
    };
    app.create_memory(request(102), "新实例的另一条正文")
        .unwrap();
    app.engine.clean_stop(10).unwrap();
    drop(app);
    let needle = format!("子进程正文哨兵·{seed}·mémoire");
    let (files, bytes) = scan(root.path(), needle.as_bytes(), None);
    println!("P10_CHILD_LEAK_SCAN files={files} bytes={bytes} matches=0");
}

#[test]
fn owned_child_kill_at_each_stage_preserves_evidence_and_blocks_replay() {
    for point in [
        "Pending",
        "Staged",
        "Wrapped",
        "Published",
        "Sealed",
        "T1During",
        "Committed",
    ] {
        let root = owned_root();
        let seed = filename(fresh().unwrap());
        let mut child = child(root.path(), point, &seed);
        let deadline = Instant::now() + Duration::from_secs(30);
        while !root.path().join("control").join("checkpoint").exists()
            && Instant::now() < deadline
            && !child.has_exited().unwrap()
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        let reached = root.path().join("control").join("checkpoint").exists();
        child.kill().unwrap();
        assert!(reached, "checkpoint {point}");
        let reopened = Engine::open_existing(root.path(), installation()).unwrap();
        let report = reopened.reconcile(10).unwrap();
        if point == "Committed" {
            assert!(report.blocked);
            assert_eq!(report.committed, 1);
            let clean: bool = reopened
                .db
                .query_row("SELECT clean FROM runtime", [], |r| r.get(0))
                .unwrap();
            assert!(!clean);
        } else {
            assert!(report.blocked);
            assert_eq!(count(&reopened.db, "receipt"), 0);
            assert_eq!(count(&reopened.db, "memory"), 0);
        }
        assert_eq!(count(&reopened.db, "write_intent"), 1);
        assert_eq!(report.states.len(), 1);
        drop(reopened);
        let needle = format!("子进程正文哨兵·{seed}·mémoire");
        let (files, bytes) = scan(root.path(), needle.as_bytes(), None);
        println!("P10_CRASH point={point} files={files} bytes={bytes} plaintext_matches=0");
    }
}
