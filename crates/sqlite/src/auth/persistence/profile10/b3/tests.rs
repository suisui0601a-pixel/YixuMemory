use super::super::read::ReadQuery;
use super::super::recovery::OwnerProcess;
use super::super::tests::{actor, installation, owned_root, provisions, request, sentinel};
use super::*;
use std::io::Write;
use std::time::Duration;

#[test]
fn f2_live_owner_observed_marker_missing_blocks_other_memory_and_write() {
    let root = owned_root();
    let mut app = app(root.path());
    let (a, _) = seed(&mut app);
    let event = deletion(&mut app, Target::Memory(a.memory));
    apply(&mut app, event);
    assert!(
        app.engine
            .delete_cleanup(event, |point, _| if point == "KeyObserved" {
                Err(Error::ResourceUnavailable)
            } else {
                Ok(())
            })
            .is_err()
    );
    assert_eq!(count(&app.engine.db, "deletion_receipt"), 0);
    let plan = plans(&app.engine)
        .unwrap()
        .into_iter()
        .find(|(p, _)| p.request.memory == a.memory)
        .unwrap()
        .0;
    let name = marker_name(&plan);
    let bytes = app.engine.vault.anchor.read(&name).unwrap();
    app.engine.vault.anchor.erase(&name, &bytes).unwrap();
    assert!(app.engine.reconcile(0).unwrap().blocked);
    let mut calls = 0;
    assert!(
        app.read_revision(query(102), |_| {
            calls += 1;
        })
        .is_err()
    );
    assert_eq!(calls, 0);
    let b = request(102);
    assert!(
        app.revise_memory(
            WriteRequest {
                request: fresh().unwrap(),
                revision: fresh().unwrap(),
                expected_current: Some(b.revision),
                ..b
            },
            &sentinel()
        )
        .is_err()
    );
    assert!(app.engine.delete_cleanup(event, |_, _| Ok(())).is_err());
    assert!(is_missing(&app.engine.vault.anchor.read(&name)));
    assert_eq!(count(&app.engine.db, "deletion_receipt"), 0);
    assert_eq!(stats(&app.engine), (2, 2, 2, 2));
}

#[test]
fn f2_complete_marker_receipt_remains_valid_after_clean_reopen() {
    let root = owned_root();
    let mut app = app(root.path());
    seed(&mut app);
    let event = deletion(&mut app, Target::Memory([101; 16]));
    apply(&mut app, event);
    app.engine.delete_cleanup(event, |_, _| Ok(())).unwrap();
    assert!(!app.engine.reconcile(0).unwrap().blocked);
    app.read_revision(query(102), |_| ()).unwrap();
    app.engine.clean_stop(0).unwrap();
    drop(app);
    let engine = Engine::open_existing(root.path(), installation()).unwrap();
    assert!(!engine.reconcile(0).unwrap().blocked);
    assert_eq!(count(&engine.db, "deletion_receipt"), 3);
    assert!(engine.recovery_only);
}

#[test]
fn f1_isolated_key_with_expired_a_grant_cannot_authorize_source_via_b() {
    let root = owned_root();
    let mut app = app(root.path());
    let (a, _) = seed(&mut app);
    erase_key(&app, a);
    app.engine.isolate_objects().unwrap();
    let tx = app
        .engine
        .db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    tx.execute(
        "INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",
        [actor().scope_bytes()],
    )
    .unwrap();
    let event = tx.last_insert_rowid();
    gates::make_effective(&tx, event).unwrap();
    tx.execute("UPDATE authority SET expiry=1 WHERE target=?1", [a.memory])
        .unwrap();
    tx.execute("UPDATE runtime SET epoch=epoch+1", []).unwrap();
    tx.execute("UPDATE authority SET epoch=(SELECT epoch FROM runtime)", [])
        .unwrap();
    tx.execute(
        "INSERT INTO security_receipt(event,epoch) SELECT ?1,epoch FROM runtime",
        [event],
    )
    .unwrap();
    tx.execute(
        "UPDATE security_event SET state='Applied' WHERE id=?1",
        [event],
    )
    .unwrap();
    tx.commit().unwrap();
    let before = stats(&app.engine);
    assert!(
        app.delete_content(DeleteCommand {
            request: fresh().unwrap(),
            target: Target::Source([96; 16])
        })
        .is_err()
    );
    assert_eq!(count(&app.engine.db, "deletion_intent"), 0);
    assert_eq!(stats(&app.engine), before);
    app.read_revision(query(102), |_| ()).unwrap();
}

#[test]
fn f1_frozen_source_set_equals_applied_set_at_all_stages() {
    let root = owned_root();
    let mut app = app(root.path());
    seed(&mut app);
    let event = deletion(&mut app, Target::Source([96; 16]));
    let expected: BTreeSet<Id> = frozen_closure(&app.engine.db, event)
        .unwrap()
        .iter()
        .map(|p| p.operation)
        .collect();
    assert_eq!(expected.len(), 2);
    apply(&mut app, event);
    assert_eq!(
        frozen_closure(&app.engine.db, event)
            .unwrap()
            .iter()
            .map(|p| p.operation)
            .collect::<BTreeSet<_>>(),
        expected
    );
    assert!(!app.engine.reconcile(0).unwrap().blocked);
}

#[test]
fn f1_incomplete_isolated_source_denies_without_partial_closing() {
    let root = owned_root();
    let mut app = app(root.path());
    seed(&mut app);
    let definition: String = app
        .engine
        .db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='immutable_revision_evidence_DELETE'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    app.engine
        .db
        .execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER immutable_revision_evidence_DELETE;")
        .unwrap();
    app.engine
        .db
        .execute(
            "DELETE FROM revision_evidence WHERE memory=?1",
            [[101u8; 16]],
        )
        .unwrap();
    app.engine.db.execute_batch(&definition).unwrap();
    app.engine
        .db
        .execute_batch("PRAGMA foreign_keys=ON")
        .unwrap();
    app.engine.isolate_objects().unwrap();
    let before = stats(&app.engine);
    let events = count(&app.engine.db, "security_event");
    assert!(
        app.delete_content(DeleteCommand {
            request: fresh().unwrap(),
            target: Target::Source([96; 16])
        })
        .is_err()
    );
    assert_eq!(count(&app.engine.db, "security_event"), events);
    for table in [
        "deletion_intent",
        "deletion_member",
        "deletion_work",
        "r5_tombstone",
    ] {
        assert_eq!(count(&app.engine.db, table), 0);
    }
    assert_eq!(stats(&app.engine), before);
}

#[test]
fn source_deletion_covers_every_historical_revision_and_rejects_reuse() {
    let root = owned_root();
    let mut app = app(root.path());
    let (first, _) = seed(&mut app);
    let next = WriteRequest {
        request: fresh().unwrap(),
        revision: fresh().unwrap(),
        expected_current: Some(first.revision),
        ..first
    };
    app.revise_memory(next, &sentinel()).unwrap();
    let before = stats(&app.engine);
    let event = deletion(&mut app, Target::Source([96; 16]));
    apply(&mut app, event);
    assert_eq!(count(&app.engine.db, "deletion_member"), 3);
    for revision in [first.revision, next.revision] {
        let mut delivered = false;
        assert!(
            app.read_revision(
                ReadQuery {
                    memory: first.memory,
                    revision: Some(revision)
                },
                |_| {
                    delivered = true;
                }
            )
            .is_err()
        );
        assert!(!delivered);
    }
    assert!(app.read_revision(query(102), |_| ()).is_err());
    let retry = WriteRequest {
        request: fresh().unwrap(),
        revision: fresh().unwrap(),
        expected_current: Some(next.revision),
        ..next
    };
    assert!(app.revise_memory(retry, &sentinel()).is_err());
    assert_eq!(stats(&app.engine), before);
    app.engine.delete_cleanup(event, |_, _| Ok(())).unwrap();
    assert_eq!(
        app.engine.deletion_progress(event).unwrap(),
        ("Applied".into(), 3, 3, 3, 0)
    );
    assert_eq!(count(&app.engine.db, "deletion_receipt"), 9);
    let pointer: Id = app
        .engine
        .db
        .query_row(
            "SELECT current_revision FROM memory WHERE id=?1",
            [first.memory],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pointer, next.revision);
    assert_eq!(stats(&app.engine), before);
}

#[test]
fn unexplained_missing_backup_cannot_receive_expiry_receipt() {
    let root = owned_root();
    let mut app = app(root.path());
    let (req, _) = seed(&mut app);
    let id = app
        .engine
        .register_local_backup(actor(), req.request)
        .unwrap();
    let name = format!("backup-{}", filename(id));
    let bytes = app.engine.files.read(&name).unwrap();
    let event = deletion(&mut app, Target::Memory(req.memory));
    apply(&mut app, event);
    app.engine.files.erase(&name, &bytes).unwrap();
    assert!(app.engine.delete_cleanup(event, |_, _| Ok(())).is_err());
    assert_eq!(count(&app.engine.db, "backup_receipt"), 0);
    assert_eq!(app.engine.deletion_progress(event).unwrap().4, 1);
    assert!(app.engine.reconcile(0).unwrap().blocked);
    assert!(app.read_revision(query(102), |_| ()).is_err());
}

#[test]
fn reappearing_settled_ciphertext_or_wrapper_blocks_every_read() {
    for wrapper in [false, true] {
        let root = owned_root();
        let mut app = app(root.path());
        let (req, _) = seed(&mut app);
        let plan = Engine::plan_row(&app.engine.db, actor(), req.request)
            .unwrap()
            .unwrap()
            .0;
        let name = filename(if wrapper { plan.key } else { plan.payload });
        let bytes = if wrapper {
            &app.engine.vault.anchor
        } else {
            &app.engine.files
        }
        .read(&name)
        .unwrap();
        let event = deletion(&mut app, Target::Memory(req.memory));
        apply(&mut app, event);
        app.engine.delete_cleanup(event, |_, _| Ok(())).unwrap();
        let anchor = if wrapper {
            &app.engine.vault.anchor
        } else {
            &app.engine.files
        };
        anchor
            .publish(&filename(fresh().unwrap()), &name, &bytes, 0, |_| {})
            .unwrap();
        assert!(app.engine.reconcile(0).unwrap().blocked);
        let mut delivered = false;
        assert!(
            app.read_revision(query(102), |_| {
                delivered = true;
            })
            .is_err()
        );
        assert!(!delivered);
        assert_eq!(stats(&app.engine), (2, 2, 2, 2));
    }
}

#[test]
fn incomplete_revision_dependency_is_quarantined_without_changing_any_history_row() {
    let root = owned_root();
    let mut app = app(root.path());
    let (req, _) = seed(&mut app);
    let trigger: String = app
        .engine
        .db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='immutable_revision_evidence_DELETE'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    app.engine
        .db
        .execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER immutable_revision_evidence_DELETE")
        .unwrap();
    app.engine
        .db
        .execute(
            "DELETE FROM revision_evidence WHERE memory=?1",
            [req.memory],
        )
        .unwrap();
    app.engine.db.execute_batch(&trigger).unwrap();
    app.engine
        .db
        .execute_batch("PRAGMA foreign_keys=ON")
        .unwrap();
    assert_eq!(app.engine.isolate_objects().unwrap(), 1);
    assert!(app.read_revision(query(101), |_| ()).is_err());
    app.read_revision(query(102), |_| ()).unwrap();
    assert_eq!(stats(&app.engine), (2, 2, 2, 2));
    app.engine.clean_stop(0).unwrap();
    drop(app);
    let reopened = Engine::open_existing(root.path(), installation()).unwrap();
    assert!(!reopened.reconcile(0).unwrap().blocked);
    assert_eq!(count(&reopened.db, "revision_evidence"), 1);
    assert_eq!(count(&reopened.db, "object_quarantine"), 1);
}

#[test]
fn real_delete_markers_and_remaining_files_contain_no_utf8_or_utf16_plaintext() {
    let root = owned_root();
    let mut app = app(root.path());
    let body = sentinel();
    app.create_memory(request(101), &body).unwrap();
    app.create_memory(request(102), &body).unwrap();
    let event = deletion(&mut app, Target::Source([96; 16]));
    apply(&mut app, event);
    app.engine.delete_cleanup(event, |_, _| Ok(())).unwrap();
    let (files, bytes) =
        super::super::tests::scan(root.path(), body.as_bytes(), Some(&app.engine._lock));
    println!("B3_LEAK_SCAN files={files} bytes={bytes} utf8/utf16_matches=0");
    app.engine.clean_stop(0).unwrap();
    drop(app);
    super::super::tests::scan(root.path(), body.as_bytes(), None);
}

#[test]
fn two_concurrent_delete_requests_are_queued_once_and_processed_in_order() {
    let root = owned_root();
    let path = root.path().to_owned();
    let (sender, receiver) = std::sync::mpsc::channel();
    let host = std::thread::spawn(move || {
        let mut app = app(&path);
        seed(&mut app);
        let mut requests = Vec::new();
        for _ in 0..2 {
            let target = receiver.recv().unwrap();
            requests.push(deletion(&mut app, target));
        }
        assert!(app.engine.delete_fence(requests[1]).is_err());
        for event in &requests {
            apply(&mut app, *event);
        }
        app.engine
            .delete_cleanup(requests[1], |_, _| Ok(()))
            .unwrap();
        app.engine
            .delete_cleanup(requests[0], |_, _| Ok(()))
            .unwrap();
        assert_eq!(stats(&app.engine), (2, 2, 2, 2));
        assert_eq!(count(&app.engine.db, "deletion_intent"), 2);
        assert_eq!(count(&app.engine.db, "deletion_receipt"), 6);
        assert!(app.read_revision(query(101), |_| ()).is_err());
        assert!(app.read_revision(query(102), |_| ()).is_err());
    });
    std::thread::scope(|scope| {
        for target in [Target::Memory([101; 16]), Target::Source([96; 16])] {
            let sender = sender.clone();
            scope.spawn(move || sender.send(target).unwrap());
        }
    });
    host.join().unwrap();
}

#[test]
fn damage_after_clean_stop_is_isolated_in_atomic_owner_admission() {
    let root = owned_root();
    let owner = OwnerProcess::launch(root.path(), installation(), "B3Clean", "").unwrap();
    let proof = owner.wait_clean().unwrap();
    let engine = Engine::open_existing(root.path(), installation()).unwrap();
    let (plan, _) = plans(&engine)
        .unwrap()
        .into_iter()
        .find(|(p, _)| p.request.memory == [101; 16])
        .unwrap();
    let bytes = engine.files.read(&filename(plan.payload)).unwrap();
    engine.files.erase(&filename(plan.payload), &bytes).unwrap();
    assert_eq!(count(&engine.db, "object_quarantine"), 0);
    drop(engine);
    let resumed =
        OwnerProcess::launch_next(root.path(), installation(), "B3Resume", "", Some(proof))
            .unwrap();
    let _proof = resumed.wait_clean().unwrap();
    let engine = Engine::open_existing(root.path(), installation()).unwrap();
    assert_eq!(count(&engine.db, "object_quarantine"), 1);
    assert!(!engine.reconcile(0).unwrap().blocked);
    assert_eq!(stats(&engine), (2, 2, 2, 2));
}

#[test]
fn crash_after_resume_has_no_second_automatic_owner_admission() {
    let root = owned_root();
    let owner = OwnerProcess::launch(root.path(), installation(), "B3Isolate", "").unwrap();
    let proof = owner.wait_clean().unwrap();
    let next = OwnerProcess::launch_next(
        root.path(),
        installation(),
        "B3ResumeCrash",
        "",
        Some(proof),
    )
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    while !root.path().join("control/checkpoint").exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    next.kill().unwrap();
    let engine = Engine::open_existing(root.path(), installation()).unwrap();
    assert!(engine.reconcile(0).unwrap().blocked);
    assert_eq!(count(&engine.db, "object_quarantine"), 1);
    assert_eq!(count(&engine.db, "resume_receipt"), 1);
    assert_eq!(stats(&engine), (2, 2, 2, 2));
}

#[test]
fn identical_bytes_replacement_is_not_an_authorized_cleanup_target() {
    let root = owned_root();
    let mut app = app(root.path());
    let (req, _) = seed(&mut app);
    let plan = Engine::plan_row(&app.engine.db, actor(), req.request)
        .unwrap()
        .unwrap()
        .0;
    let name = filename(plan.payload);
    let bytes = app.engine.files.read(&name).unwrap();
    let event = deletion(&mut app, Target::Memory(req.memory));
    apply(&mut app, event);
    app.engine.files.erase(&name, &bytes).unwrap();
    app.engine
        .files
        .publish(&filename(fresh().unwrap()), &name, &bytes, 0, |_| {})
        .unwrap();
    assert!(app.engine.delete_cleanup(event, |_, _| Ok(())).is_err());
    assert_eq!(app.engine.files.read(&name).unwrap(), bytes);
    assert!(app.read_revision(query(101), |_| ()).is_err());
}

#[test]
fn outbox_view_distinguishes_creation_from_revision_without_rewriting_old_action() {
    let root = owned_root();
    let mut app = app(root.path());
    let (a, _) = seed(&mut app);
    let next = WriteRequest {
        request: fresh().unwrap(),
        revision: fresh().unwrap(),
        expected_current: Some(a.revision),
        ..a
    };
    app.revise_memory(next, &sentinel()).unwrap();
    let mut stmt = app
        .engine
        .db
        .prepare(
            "SELECT version,action,number FROM content_outbox_v2 WHERE memory=?1 ORDER BY number",
        )
        .unwrap();
    let events: Vec<(i64, String, i64)> = stmt
        .query_map([a.memory], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap();
    assert_eq!(
        events,
        vec![
            (2, "MemoryCreated".into(), 1),
            (2, "RevisionCreated".into(), 2)
        ]
    );
    assert_eq!(count(&app.engine.db, "outbox"), 3);
}

#[test]
fn deleting_during_decryption_prevents_delivery_and_foreign_scope_cannot_enqueue() {
    let root = owned_root();
    let mut app = app(root.path());
    seed(&mut app);
    let path = root.path().join("content.sqlite");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let other = barrier.clone();
    let request = fresh().unwrap();
    let worker = std::thread::spawn(move || {
        let db = Connection::open(path).unwrap();
        other.wait();
        let tx = rusqlite::Transaction::new_unchecked(&db, TransactionBehavior::Immediate).unwrap();
        tx.execute(
            "INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",
            [actor().scope_bytes()],
        )
        .unwrap();
        let event = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO deletion_intent VALUES(?1,?2,?3,?4,'Memory',?5)",
            params![
                event,
                actor().scope_bytes(),
                actor().principal,
                request,
                [101u8; 16]
            ],
        )
        .unwrap();
        tx.commit().unwrap();
        other.wait();
    });
    let mut at_checkpoint = false;
    let mut delivered = false;
    assert!(
        app.engine
            .read_borrow(
                actor(),
                query(101),
                |point| {
                    if matches!(point, super::super::read::ReadPoint::Decrypted) {
                        at_checkpoint = true;
                        barrier.wait();
                        barrier.wait();
                    }
                    Ok(())
                },
                |_| {
                    delivered = true;
                }
            )
            .is_err()
    );
    worker.join().unwrap();
    assert!(at_checkpoint && !delivered);
    let previous = count(&app.engine.db, "deletion_intent");
    app.identity.scope.space = [3; 16];
    assert!(
        app.delete_content(DeleteCommand {
            request: fresh().unwrap(),
            target: Target::Source([96; 16])
        })
        .is_err()
    );
    assert_eq!(count(&app.engine.db, "deletion_intent"), previous);
}

#[test]
fn old_backup_reopen_does_not_grant_new_owner_or_resurrect_deleted_content() {
    let root = owned_root();
    let mut app = app(root.path());
    seed(&mut app);
    let backup = root.path().join("control/before-delete.sqlite");
    app.engine
        .db
        .execute("VACUUM INTO ?1", [backup.to_str().unwrap()])
        .unwrap();
    let event = deletion(&mut app, Target::Source([96; 16]));
    apply(&mut app, event);
    app.engine.delete_cleanup(event, |_, _| Ok(())).unwrap();
    app.engine.clean_stop(0).unwrap();
    drop(app);
    // Restore only this test's known database. Original backup and cleanup files remain as evidence.
    std::fs::copy(&backup, root.path().join("content.sqlite")).unwrap();
    let mut old = Application {
        engine: Engine::open_existing(root.path(), installation()).unwrap(),
        identity: actor(),
    };
    assert!(old.engine.reconcile(0).unwrap().blocked);
    assert!(old.read_revision(query(101), |_| ()).is_err());
    assert!(old.read_revision(query(102), |_| ()).is_err());
    assert!(old.engine.recovery_only);
}

#[test]
fn pending_write_and_schema_drift_cannot_be_reclassified_as_local_isolation() {
    let root = owned_root();
    let mut app = app(root.path());
    let req = request(101);
    assert!(
        app.engine
            .write(actor(), req, &sentinel(), 0, |point, _| {
                if point == super::super::write::Point::Staged {
                    Err(Error::ResourceUnavailable)
                } else {
                    Ok(())
                }
            })
            .is_err()
    );
    assert!(app.engine.reconcile(0).unwrap().blocked);
    assert!(app.engine.isolate_objects().is_err());
    assert_eq!(count(&app.engine.db, "object_quarantine"), 0);
    app.engine
        .db
        .execute_batch("DROP TRIGGER b3_quarantine_committed")
        .unwrap();
    assert!(matches!(
        attest(&app.engine.db),
        Err(Error::IncompatibleSchema)
    ));
}

#[test]
fn applied_transaction_failure_rolls_back_all_tombstones_members_and_work() {
    let root = owned_root();
    let mut app = app(root.path());
    seed(&mut app);
    let event = deletion(&mut app, Target::Memory([101; 16]));
    app.engine.delete_fence(event).unwrap();
    assert!(
        app.engine
            .delete_apply(event, || Err(Error::ResourceUnavailable))
            .is_err()
    );
    for table in ["deletion_member", "deletion_work", "r5_tombstone"] {
        assert_eq!(count(&app.engine.db, table), 0);
    }
    assert_eq!(stats(&app.engine), (2, 2, 2, 2));
    assert!(gates::closing(&app.engine.db).unwrap());
    assert!(app.read_revision(query(102), |_| ()).is_err());
}

fn app(root: &Path) -> Application {
    Application {
        engine: Engine::create_version(root, installation(), &provisions(), fresh().unwrap(), 11)
            .unwrap(),
        identity: actor(),
    }
}
fn query(memory: u8) -> ReadQuery {
    ReadQuery {
        memory: [memory; 16],
        revision: None,
    }
}
fn seed(app: &mut Application) -> (WriteRequest, WriteRequest) {
    let a = request(101);
    let b = request(102);
    let body = sentinel();
    app.create_memory(a, &body).unwrap();
    app.create_memory(b, &body).unwrap();
    (a, b)
}
fn count(db: &Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn deletion(app: &mut Application, target: Target) -> i64 {
    app.delete_content(DeleteCommand {
        request: fresh().unwrap(),
        target,
    })
    .unwrap()
}
fn apply(app: &mut Application, event: i64) {
    app.engine.delete_fence(event).unwrap();
    app.engine.delete_apply(event, || Ok(())).unwrap();
}
fn stats(engine: &Engine) -> (i64, i64, i64, i64) {
    (
        engine
            .db
            .query_row("SELECT knowledge_seq FROM runtime", [], |r| r.get(0))
            .unwrap(),
        count(&engine.db, "revision"),
        count(&engine.db, "receipt"),
        count(&engine.db, "outbox"),
    )
}
fn erase_key(app: &Application, req: WriteRequest) {
    let plan = Engine::plan_row(&app.engine.db, actor(), req.request)
        .unwrap()
        .unwrap()
        .0;
    let name = filename(plan.key);
    let bytes = app.engine.vault.anchor.read(&name).unwrap();
    app.engine.vault.anchor.erase(&name, &bytes).unwrap();
}

#[test]
fn object_quarantine_preserves_manifest_and_other_memory() {
    let root = owned_root();
    let mut app = app(root.path());
    let (a, _) = seed(&mut app);
    let before = stats(&app.engine);
    erase_key(&app, a);
    assert_eq!(app.engine.isolate_objects().unwrap(), 1);
    assert_eq!(count(&app.engine.db, "object_quarantine"), 1);
    assert!(app.read_revision(query(101), |_| ()).is_err());
    app.read_revision(query(102), |_| ()).unwrap();
    assert_eq!(stats(&app.engine), before);
    app.engine.clean_stop(0).unwrap();
    drop(app);
    let reopened = Engine::open_existing(root.path(), installation()).unwrap();
    assert!(!reopened.reconcile(0).unwrap().blocked);
    assert_eq!(count(&reopened.db, "object_quarantine"), 1);
    assert_eq!(stats(&reopened), before);
    // Evidence remains present; reopen alone never confers owner admission.
    assert!(reopened.recovery_only);
}

#[test]
fn expired_grant_alone_denies_without_any_unsettled_event() {
    let root = owned_root();
    let mut app = app(root.path());
    seed(&mut app);
    let db = &mut app.engine.db;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    tx.execute(
        "INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",
        [actor().scope_bytes()],
    )
    .unwrap();
    let event = tx.last_insert_rowid();
    tx.execute(
        "UPDATE security_event SET state='EffectiveFence' WHERE id=?1",
        [event],
    )
    .unwrap();
    tx.execute("UPDATE authority SET expiry=1", []).unwrap();
    tx.execute("UPDATE runtime SET epoch=epoch+1", []).unwrap();
    tx.execute("UPDATE authority SET epoch=(SELECT epoch FROM runtime)", [])
        .unwrap();
    tx.execute(
        "INSERT INTO security_receipt(event,epoch) SELECT ?1,epoch FROM runtime",
        [event],
    )
    .unwrap();
    tx.execute(
        "UPDATE security_event SET state='Applied' WHERE id=?1",
        [event],
    )
    .unwrap();
    tx.commit().unwrap();
    assert!(!gates::closing(&app.engine.db).unwrap());
    let mut delivered = false;
    assert!(matches!(
        app.read_revision(query(101), |_| {
            delivered = true;
        }),
        Err(Error::AuthorizationDenied)
    ));
    assert!(!delivered);
}

#[test]
fn memory_deletion_closes_history_and_clears_registered_copy_and_wrapper() {
    let root = owned_root();
    let mut app = app(root.path());
    let (first, _) = seed(&mut app);
    let next = WriteRequest {
        request: fresh().unwrap(),
        revision: fresh().unwrap(),
        expected_current: Some(first.revision),
        ..first
    };
    app.revise_memory(next, &sentinel()).unwrap();
    let before = stats(&app.engine);
    let event = deletion(&mut app, Target::Memory(first.memory));
    assert!(app.read_revision(query(102), |_| ()).is_err());
    apply(&mut app, event);
    assert!(app.read_revision(query(101), |_| ()).is_err());
    assert!(
        app.read_revision(
            ReadQuery {
                memory: first.memory,
                revision: Some(first.revision)
            },
            |_| ()
        )
        .is_err()
    );
    app.read_revision(query(102), |_| ()).unwrap();
    app.engine.delete_cleanup(event, |_, _| Ok(())).unwrap();
    assert_eq!(
        app.engine.deletion_progress(event).unwrap(),
        ("Applied".into(), 2, 2, 2, 0)
    );
    assert_eq!(stats(&app.engine), before);
    assert_eq!(count(&app.engine.db, "copy_manifest_b3"), 3);
    assert_eq!(count(&app.engine.db, "deletion_receipt"), 6);
    assert_eq!(count(&app.engine.db, "evidence"), 1);
    assert!(!app.engine.reconcile(0).unwrap().blocked);
    app.engine.delete_cleanup(event, |_, _| Ok(())).unwrap();
    assert_eq!(count(&app.engine.db, "deletion_receipt"), 6);
    app.engine.clean_stop(0).unwrap();
    drop(app);
    let reopened = Engine::open_existing(root.path(), installation()).unwrap();
    assert!(!reopened.reconcile(0).unwrap().blocked);
    assert_eq!(stats(&reopened), before);
}

#[test]
fn overlapping_source_and_memory_deletes_share_progress_without_unfencing() {
    let root = owned_root();
    let mut app = app(root.path());
    seed(&mut app);
    let key = fresh().unwrap();
    let first = app
        .delete_content(DeleteCommand {
            request: key,
            target: Target::Memory([101; 16]),
        })
        .unwrap();
    let second = deletion(&mut app, Target::Source([96; 16]));
    assert_eq!(
        app.delete_content(DeleteCommand {
            request: key,
            target: Target::Memory([101; 16])
        })
        .unwrap(),
        first
    );
    assert!(matches!(
        app.delete_content(DeleteCommand {
            request: key,
            target: Target::Source([96; 16])
        }),
        Err(Error::IdempotencyConflict)
    ));
    assert!(app.engine.delete_fence(second).is_err());
    apply(&mut app, first);
    assert!(app.engine.delete_cleanup(first, |_, _| Ok(())).is_err());
    assert!(app.read_revision(query(102), |_| ()).is_err());
    apply(&mut app, second);
    app.engine.delete_cleanup(second, |_, _| Ok(())).unwrap();
    assert_eq!(app.engine.deletion_progress(first).unwrap().2, 1);
    assert_eq!(app.engine.deletion_progress(second).unwrap().2, 2);
    for mem in [101, 102] {
        assert!(app.read_revision(query(mem), |_| ()).is_err());
    }
    assert_eq!(count(&app.engine.db, "deletion_work"), 2);
    assert_eq!(count(&app.engine.db, "deletion_receipt"), 6);
    assert_eq!(stats(&app.engine), (2, 2, 2, 2));
}

#[test]
fn marker_tampering_and_global_epoch_damage_remain_global_blockers() {
    let root = owned_root();
    let mut app = app(root.path());
    seed(&mut app);
    let event = deletion(&mut app, Target::Memory([101; 16]));
    apply(&mut app, event);
    app.engine.delete_cleanup(event, |_, _| Ok(())).unwrap();
    let (plan, _) = plans(&app.engine)
        .unwrap()
        .into_iter()
        .find(|(p, _)| p.request.memory == [101; 16])
        .unwrap();
    let name = marker_name(&plan);
    let bytes = app.engine.vault.anchor.read(&name).unwrap();
    app.engine.vault.anchor.erase(&name, &bytes).unwrap();
    assert!(app.engine.reconcile(0).unwrap().blocked);
    assert!(app.read_revision(query(102), |_| ()).is_err());
    assert!(app.engine.clean_stop(0).is_err());
    let root = owned_root();
    let mut app = self::app(root.path());
    seed(&mut app);
    app.engine
        .db
        .execute("UPDATE runtime SET epoch=epoch+1", [])
        .unwrap();
    assert!(app.engine.reconcile(0).unwrap().blocked);
    assert!(app.read_revision(query(102), |_| ()).is_err());
}

#[test]
fn cleanup_failure_keeps_logical_fence_and_backup_expiry_unproven() {
    let root = owned_root();
    let mut app = app(root.path());
    let (req, _) = seed(&mut app);
    app.engine
        .register_local_backup(actor(), req.request)
        .unwrap();
    let event = deletion(&mut app, Target::Memory(req.memory));
    apply(&mut app, event);
    assert!(
        app.engine
            .delete_cleanup(event, |point, _| {
                if point == "MarkerPublished" {
                    Err(Error::ResourceUnavailable)
                } else {
                    Ok(())
                }
            })
            .is_err()
    );
    assert!(app.read_revision(query(101), |_| ()).is_err());
    assert!(
        app.engine
            .delete_cleanup(event, |point, _| {
                if point == "BeforeBackup" {
                    Err(Error::ResourceUnavailable)
                } else {
                    Ok(())
                }
            })
            .is_err()
    );
    assert_eq!(
        app.engine.deletion_progress(event).unwrap(),
        ("Applied".into(), 1, 1, 1, 1)
    );
    assert_eq!(count(&app.engine.db, "backup_registration"), 1);
    assert_eq!(count(&app.engine.db, "backup_receipt"), 0);
    app.engine.delete_cleanup(event, |_, _| Ok(())).unwrap();
    assert_eq!(count(&app.engine.db, "backup_receipt"), 1);
    assert_eq!(app.engine.deletion_progress(event).unwrap().4, 0);
}

#[test]
fn deletion_requires_every_affected_grant_before_any_partial_work() {
    let root = owned_root();
    let mut app = app(root.path());
    seed(&mut app);
    app.identity.principal = [4; 16];
    assert!(
        app.delete_content(DeleteCommand {
            request: fresh().unwrap(),
            target: Target::Source([96; 16])
        })
        .is_err()
    );
    assert_eq!(count(&app.engine.db, "deletion_intent"), 0);
    assert_eq!(count(&app.engine.db, "deletion_member"), 0);
    assert_eq!(count(&app.engine.db, "r5_tombstone"), 0);
}

pub(in super::super) fn owner_round(root: &Path, mode: &str, nonce: Id) {
    let engine = if mode.starts_with("B3Resume") {
        Engine::resume_supervised(root, installation(), nonce).unwrap()
    } else {
        Engine::create_version(root, installation(), &provisions(), nonce, 11).unwrap()
    };
    let mut app = Application {
        engine,
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
    let pause = || {
        std::fs::write(root.join("control/checkpoint"), mode).unwrap();
        loop {
            std::thread::park_timeout(Duration::from_millis(50));
        }
    };
    if mode == "B3Resume" || mode == "B3ResumeCrash" {
        assert!(app.read_revision(query(101), |_| ()).is_err());
        app.read_revision(query(102), |_| ()).unwrap();
        assert_eq!(count(&app.engine.db, "object_quarantine"), 1);
        if mode == "B3ResumeCrash" {
            pause();
        }
        app.engine.clean_stop(0).unwrap();
        return;
    }
    let (req, _) = seed(&mut app);
    if mode == "B3Isolate" || mode == "B3Clean" {
        if mode == "B3Isolate" {
            erase_key(&app, req);
        }
        app.engine.clean_stop(0).unwrap();
        return;
    }
    if mode == "B3BeforeDelete" {
        app.engine
            .register_local_backup(actor(), req.request)
            .unwrap();
        pause();
    }
    app.engine
        .register_local_backup(actor(), req.request)
        .unwrap();
    let event = deletion(&mut app, Target::Memory(req.memory));
    if mode == "B3Closing" {
        pause();
    }
    app.engine.delete_fence(event).unwrap();
    if mode == "B3Fence" {
        pause();
    }
    app.engine
        .delete_apply(event, || {
            if mode == "B3CommitBefore" {
                pause();
            }
            Ok(())
        })
        .unwrap();
    if mode == "B3Applied" {
        pause();
    }
    app.engine
        .delete_cleanup(event, |point, _| {
            if mode == format!("B3{point}") {
                pause();
            }
            Ok(())
        })
        .unwrap();
    app.engine.clean_stop(0).unwrap();
}

#[test]
fn real_process_restart_quarantines_one_object_and_preserves_other_reads() {
    let root = owned_root();
    let first = OwnerProcess::launch(root.path(), installation(), "B3Isolate", "").unwrap();
    let proof = first.wait_clean().unwrap();
    let second =
        OwnerProcess::launch_next(root.path(), installation(), "B3Resume", "", Some(proof))
            .unwrap();
    let _proof = second.wait_clean().unwrap();
    let engine = Engine::open_existing(root.path(), installation()).unwrap();
    assert!(!engine.reconcile(0).unwrap().blocked);
    assert_eq!(stats(&engine), (2, 2, 2, 2));
    assert_eq!(count(&engine.db, "object_quarantine"), 1);
}

#[test]
fn real_crash_matrix_keeps_shared_admission_blocked_and_all_original_records() {
    for stage in [
        "BeforeDelete",
        "Closing",
        "Fence",
        "CommitBefore",
        "Applied",
        "MarkerPublished",
        "KeyObserved",
        "KeyUnavailable",
        "CiphertextRemoved",
        "WrapperRemoved",
        "ReceiptCommitted",
        "BackupRemoved",
        "BackupExpired",
    ] {
        let root = owned_root();
        let mode = format!("B3{stage}");
        let owner = OwnerProcess::launch(root.path(), installation(), &mode, "").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while !root.path().join("control/checkpoint").exists() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        owner.kill().unwrap();
        let engine = Engine::open_existing(root.path(), installation()).unwrap();
        let report = engine.reconcile(0).unwrap();
        assert!(report.blocked);
        assert_eq!(stats(&engine), (2, 2, 2, 2));
        let current: Id = engine
            .db
            .query_row(
                "SELECT current_revision FROM memory WHERE id=?1",
                [[101; 16]],
                |r| r.get(0),
            )
            .unwrap();
        let historical: Id = engine
            .db
            .query_row(
                "SELECT id FROM revision WHERE memory=?1",
                [[101; 16]],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(current, historical);
        assert_eq!(count(&engine.db, "copy_manifest_b3"), 2);
        assert_eq!(count(&engine.db, "local_key"), 2);
        assert_eq!(count(&engine.db, "payload"), 2);
        let generation: i64 = engine
            .db
            .query_row("SELECT min(generation) FROM local_key", [], |r| r.get(0))
            .unwrap();
        assert_eq!(generation, 1);
        println!(
            "B3_CRASH stage={stage} seq/revision/receipt/outbox=2/2/2/2 key/payload/copy=2/2/2 generation=1 admission=blocked"
        );
    }
}
