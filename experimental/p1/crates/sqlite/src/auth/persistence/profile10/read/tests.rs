use super::super::recovery::OwnerProcess;
use super::super::tests::{
    actor, application, installation, owned_root, provisions, request, scan, sentinel,
};
use super::*;
use std::sync::{Arc, Barrier};

#[test]
fn corrupted_history_is_rejected_without_disrupting_another_memory() {
    for fault in ["Tag", "AAD", "Wrapper"] {
        let root = owned_root();
        let mut app = application(root.path());
        let body = sentinel();
        let first = request(101);
        app.create_memory(first, &body).unwrap();
        let other = request(102);
        app.create_memory(other, &body).unwrap();
        let plan = Engine::plan_row(&app.engine.db, actor(), first.request)
            .unwrap()
            .unwrap()
            .0;
        let (anchor, name) = if fault == "Wrapper" {
            (&app.engine.vault.anchor, filename(plan.key))
        } else {
            (&app.engine.files, filename(plan.payload))
        };
        let original = anchor.read(&name).unwrap();
        let mut broken = original.clone();
        let offset = if fault == "AAD" { 12 } else { broken.len() - 1 };
        broken[offset] ^= 1;
        anchor.erase(&name, &original).unwrap();
        anchor
            .publish(&filename(fresh().unwrap()), &name, &broken, 0, |_| {})
            .unwrap();
        let mut delivered = false;
        assert!(
            app.read_revision(
                ReadQuery {
                    memory: first.memory,
                    revision: Some(first.revision)
                },
                |_| {
                    delivered = true;
                }
            )
            .is_err()
        );
        assert!(!delivered);
        check(&mut app, current(other.memory), &body);
        println!("B2_INTEGRITY fault={fault} delivered=0 other_memory=readable");
    }
}

#[test]
fn concurrent_revision_requests_are_serialized_and_only_one_cas_commits() {
    let root = owned_root();
    let path = root.path().to_owned();
    let (sender, receiver) = std::sync::mpsc::channel();
    let host = std::thread::spawn(move || {
        let mut app = application(&path);
        let body = sentinel();
        let initial = request(101);
        app.create_memory(initial, &body).unwrap();
        let mut success = 0;
        for _ in 0..2 {
            let request_id = receiver.recv().unwrap();
            let next = WriteRequest {
                request: request_id,
                ..revised(initial)
            };
            match app.revise_memory(next, &body) {
                Ok(_) => success += 1,
                Err(Error::CasConflict) => {}
                Err(_) => panic!("unexpected revision failure"),
            }
        }
        assert_eq!(success, 1);
        let totals: (i64, i64, i64) = app
            .engine
            .db
            .query_row(
                "SELECT (SELECT count(*) FROM revision),(SELECT count(*) FROM receipt),
             (SELECT knowledge_seq FROM runtime)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(totals, (2, 2, 2));
    });
    std::thread::scope(|scope| {
        for _ in 0..2 {
            let send = sender.clone();
            scope.spawn(move || {
                send.send(fresh().unwrap()).unwrap();
            });
        }
    });
    host.join().unwrap();
}

#[test]
fn final_borrow_orders_handoff_before_a_second_connection_can_commit_closing() {
    let root = owned_root();
    let mut app = application(root.path());
    let first = request(101);
    let body = sentinel();
    app.create_memory(first, &body).unwrap();
    let path = root.path().join("content.sqlite");
    app.read_revision(current(first.memory), |text| {
        assert!(text == body.as_str());
        std::thread::spawn(move || {
            let db = Connection::open(path).unwrap();
            db.busy_timeout(std::time::Duration::from_millis(20))
                .unwrap();
            assert!(
                db.execute(
                    "INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",
                    [actor().scope_bytes()]
                )
                .is_err()
            );
        })
        .join()
        .unwrap();
    })
    .unwrap();
    let event = app.engine.close_grant(actor()).unwrap();
    app.engine.apply_revocation(event, actor()).unwrap();
    assert!(app.read_revision(current(first.memory), |_| ()).is_err());
}

fn current(memory: Id) -> ReadQuery {
    ReadQuery {
        memory,
        revision: None,
    }
}
fn revised(first: WriteRequest) -> WriteRequest {
    WriteRequest {
        request: fresh().unwrap(),
        revision: fresh().unwrap(),
        expected_current: Some(first.revision),
        ..first
    }
}
fn check(app: &mut Application, query: ReadQuery, expected: &str) {
    let matched = app
        .read_revision(query, |actual| actual == expected)
        .unwrap();
    assert!(matched, "content mismatch");
}

#[test]
fn real_read_revision_cas_idempotency_and_no_plaintext_artifacts() {
    let root = owned_root();
    let mut app = application(root.path());
    let first = request(101);
    let one = sentinel();
    let two = sentinel();
    app.create_memory(first, &one).unwrap();
    check(&mut app, current(first.memory), &one);
    let second = revised(first);
    app.revise_memory(second, &two).unwrap();
    check(&mut app, current(first.memory), &two);
    check(
        &mut app,
        ReadQuery {
            memory: first.memory,
            revision: Some(first.revision),
        },
        &one,
    );
    assert_eq!(app.revise_memory(second, &two).unwrap().knowledge_seq, 2);
    assert!(matches!(
        app.revise_memory(second, &one),
        Err(Error::IdempotencyConflict)
    ));
    assert!(matches!(
        app.revise_memory(revised(first), &two),
        Err(Error::CasConflict)
    ));
    let n: i64 = app
        .engine
        .db
        .query_row("SELECT count(*) FROM revision", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
    let parent: Id = app
        .engine
        .db
        .query_row(
            "SELECT i.expected_current FROM revision v JOIN payload p ON p.revision=v.id
         JOIN write_intent i ON i.operation=p.operation WHERE v.number=2",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(parent, first.revision);
    assert!(
        app.engine
            .db
            .execute("UPDATE revision SET number=3", [])
            .is_err()
    );
    for body in [&one, &two] {
        let (files, bytes) = scan(root.path(), body.as_bytes(), Some(&app.engine._lock));
        println!("B2_LEAK_SCAN files={files} bytes={bytes} matches=0");
    }
    app.engine.clean_stop(0).unwrap();
    drop(app);
    for body in [&one, &two] {
        scan(root.path(), body.as_bytes(), None);
    }
}

#[test]
fn live_sqlite_security_races_deny_delivery_at_every_crypto_boundary() {
    for point in [
        ReadPoint::CipherRead,
        ReadPoint::BeforeUnwrap,
        ReadPoint::Decrypted,
    ] {
        for mutation in ["Closing", "Source", "Generation", "Epoch", "Revoke"] {
            let root = owned_root();
            let mut app = application(root.path());
            let first = request(101);
            let body = sentinel();
            app.create_memory(first, &body).unwrap();
            let barrier = Arc::new(Barrier::new(2));
            let other = barrier.clone();
            let path = root.path().join("content.sqlite");
            let worker = std::thread::spawn(move || {
                let db = Connection::open(path).unwrap();
                other.wait();
                let tx = rusqlite::Transaction::new_unchecked(&db, TransactionBehavior::Immediate)
                    .unwrap();
                tx.execute(
                    "INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",
                    [actor().scope_bytes()],
                )
                .unwrap();
                if mutation != "Closing" {
                    let id = tx.last_insert_rowid();
                    tx.execute(
                        "UPDATE security_event SET state='EffectiveFence' WHERE id=?1",
                        [id],
                    )
                    .unwrap();
                    match mutation {
                        "Source" => {
                            tx.execute("UPDATE source SET valid=0", []).unwrap();
                        }
                        "Generation" => {
                            tx.execute("UPDATE authority SET key_gen=2", []).unwrap();
                        }
                        "Revoke" => {
                            tx.execute("UPDATE authority SET grant_ok=0", []).unwrap();
                        }
                        "Epoch" => {}
                        _ => unreachable!(),
                    }
                    tx.execute("UPDATE runtime SET epoch=epoch+1", []).unwrap();
                    tx.execute("UPDATE authority SET epoch=(SELECT epoch FROM runtime)", [])
                        .unwrap();
                    tx.execute(
                        "INSERT INTO security_receipt(event,epoch) SELECT ?1,epoch FROM runtime",
                        [id],
                    )
                    .unwrap();
                    tx.execute(
                        "UPDATE security_event SET state='Applied' WHERE id=?1",
                        [id],
                    )
                    .unwrap();
                }
                tx.commit().unwrap();
                other.wait();
            });
            let mut reached = false;
            let mut delivered = false;
            let result = app.engine.read_borrow(
                actor(),
                current(first.memory),
                |at| {
                    if !reached && std::mem::discriminant(&at) == std::mem::discriminant(&point) {
                        reached = true;
                        barrier.wait();
                        barrier.wait();
                    }
                    Ok(())
                },
                |_| {
                    delivered = true;
                },
            );
            worker.join().unwrap();
            assert!(reached && result.is_err() && !delivered);
            println!("B2_RACE point={point:?} mutation={mutation} delivered=0");
        }
    }
}

#[test]
fn scope_principal_expiry_fence_and_object_corruption_never_fall_back() {
    for fault in [
        "Memory",
        "Revision",
        "Tenant",
        "Space",
        "Principal",
        "Expired",
        "Fence",
        "Current",
        "Payload",
        "Key",
        "Evidence",
    ] {
        let root = owned_root();
        let mut app = application(root.path());
        let first = request(101);
        let body = sentinel();
        app.create_memory(first, &body).unwrap();
        let mut query = current(first.memory);
        match fault {
            "Memory" => query.memory = [88; 16],
            "Revision" => query.revision = Some([88; 16]),
            "Tenant" => app.identity.scope.tenant = [88; 16],
            "Space" => app.identity.scope.space = [88; 16],
            "Principal" => app.identity.principal = [88; 16],
            "Fence" => {
                app.engine.close_grant(actor()).unwrap();
            }
            "Expired" => {
                let e = app.engine.close_grant(actor()).unwrap();
                app.engine
                    .db
                    .execute(
                        "UPDATE security_event SET state='EffectiveFence' WHERE id=?1",
                        [e],
                    )
                    .unwrap();
                app.engine
                    .db
                    .execute("UPDATE authority SET expiry=1", [])
                    .unwrap();
            }
            "Current" => {
                app.engine.db.execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER memory_current; UPDATE memory SET current_revision=zeroblob(16)").unwrap();
            }
            "Evidence" => {
                app.engine.db.execute_batch("PRAGMA foreign_keys=OFF; DROP TRIGGER immutable_revision_evidence_UPDATE; UPDATE revision_evidence SET generation=99").unwrap();
            }
            "Payload" | "Key" => {
                let plan = Engine::plan_row(&app.engine.db, actor(), first.request)
                    .unwrap()
                    .unwrap()
                    .0;
                let (anchor, name) = if fault == "Key" {
                    (&app.engine.vault.anchor, filename(plan.key))
                } else {
                    (&app.engine.files, filename(plan.payload))
                };
                let ciphertext = anchor.read(&name).unwrap();
                anchor.erase(&name, &ciphertext).unwrap();
            }
            _ => unreachable!(),
        }
        let mut delivered = false;
        assert!(
            app.read_revision(query, |_| {
                delivered = true;
            })
            .is_err()
        );
        assert!(!delivered);
        println!("B2_DENIED fault={fault}");
    }
}

pub(in super::super) fn owner_round(root: &Path, mode: &str, seed: &str, nonce: Id) {
    let one = zeroize::Zeroizing::new(format!("B2原始正文·{seed}·mémoire"));
    let two = zeroize::Zeroizing::new(format!("B2修订正文·{seed}·日本語"));
    let engine = if mode == "B2Create" {
        Engine::create_supervised(root, installation(), &provisions(), nonce).unwrap()
    } else {
        Engine::resume_supervised(root, installation(), nonce).unwrap()
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
    use std::io::Write;
    std::io::stdout().flush().unwrap();
    if mode == "B2Create" {
        app.create_memory(request(101), &one).unwrap();
    } else {
        let first: Id = app
            .engine
            .db
            .query_row("SELECT id FROM revision WHERE number=1", [], |r| r.get(0))
            .unwrap();
        check(
            &mut app,
            ReadQuery {
                memory: [101; 16],
                revision: Some(first),
            },
            &one,
        );
        if mode == "B2Revise" {
            let mut next = request(101);
            next.expected_current = Some(first);
            app.revise_memory(next, &two).unwrap();
        }
        check(&mut app, current([101; 16]), &two);
    }
    app.engine.clean_stop(0).unwrap();
    drop(app);
    for body in [&one, &two] {
        scan(root, body.as_bytes(), None);
    }
    println!("B2_OWNER_STOP mode={mode} leak_matches=0");
}

#[test]
fn three_owners_two_real_restarts_read_current_and_immutable_history() {
    let root = owned_root();
    let seed = filename(fresh().unwrap());
    let mut proof = None;
    for mode in ["B2Create", "B2Revise", "B2History"] {
        let owner =
            OwnerProcess::launch_next(root.path(), installation(), mode, &seed, proof).unwrap();
        proof = Some(owner.wait_clean().unwrap());
        println!("B2_LIFECYCLE mode={mode} stop_receipt=verified child_exit=verified");
    }
    let engine = Engine::open_existing(root.path(), installation()).unwrap();
    assert!(!engine.reconcile(0).unwrap().blocked);
    let n: (i64, i64, i64) = engine
        .db
        .query_row(
            "SELECT (SELECT count(*) FROM stop_receipt),(SELECT count(*) FROM resume_receipt),
         (SELECT count(*) FROM revision)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(n, (3, 2, 2));
}
