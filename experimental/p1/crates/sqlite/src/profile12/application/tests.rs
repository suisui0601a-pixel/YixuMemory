//! S2 ordinary metadata fixtures. No keys, body, native I/O, or file cleanup.
use super::*;
use crate::profile12::{
    schema_hash,
    tests::{Fixture, seed_with_binding},
};
use std::sync::atomic::AtomicI64;
fn id(n: u8) -> Id {
    [n; 16]
}
fn actor() -> Actor {
    Actor {
        scope: [1; 48],
        principal: id(9),
    }
}
fn remove_fixture_row(db: &Connection, table: &str, statement: &str) {
    let trigger = if table == "revision_evidence" {
        "immutable_revision_evidence_DELETE".to_string()
    } else {
        format!("immutable_p12_{table}_DELETE")
    };
    let original: String = db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name=?1",
            [&trigger],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    db.execute_batch(&format!("DROP TRIGGER {trigger}; {statement}"))
        .unwrap();
    db.execute_batch(&original).unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON").unwrap();
}
struct TestClock(AtomicI64);
impl Clock for TestClock {
    fn now(&self) -> Result<i64> {
        Ok(self.0.load(Ordering::SeqCst))
    }
}
struct Case {
    app: Application,
    fixture: Fixture,
    clock: Arc<TestClock>,
}
fn member(op: u8, source: u8, epoch: i64) -> Member {
    Member {
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
        epoch,
        dispatch: [0; 32],
        source: id(source),
        source_gen: 1,
        parent: None,
    }
}
impl Case {
    fn new() -> Self {
        let fixture = Fixture::fresh();
        fixture
            .db
            .execute(
                "INSERT INTO runtime VALUES(1,?1,?2,?3,?4,?5,?6,7,0,0,0)",
                params![id(2), id(1), id(3), id(4), vec![5u8; 36], schema_hash()],
            )
            .unwrap();
        for (n, op) in [10u8, 20, 30].into_iter().enumerate() {
            let m = member(op, if op == 30 { 70 } else { 60 }, 7 + n as i64);
            seed_with_binding(
                &fixture.db,
                op,
                if op == 30 { 70 } else { 60 },
                m.original_dispatch(id(2)).unwrap(),
            );
        }
        let clock = Arc::new(TestClock(AtomicI64::new(100)));
        let app = Application::attach(
            Connection::open(&fixture.path).unwrap(),
            id(2),
            id(1),
            id(3),
            RealmOrder::new(),
            clock.clone(),
        )
        .unwrap();
        Self {
            app,
            fixture,
            clock,
        }
    }
    fn event(&mut self) -> i64 {
        self.app
            .close(actor(), id(90), Target::Source(id(60)))
            .unwrap()
    }
    fn fence(&mut self) -> i64 {
        let e = self.event();
        self.app.fence(actor(), e).unwrap();
        e
    }
    fn snapshot(&self) -> Vec<(String, Vec<String>)> {
        let mut result = Vec::new();
        for table in [
            "runtime",
            "authority",
            "write_intent",
            "transition",
            "source",
            "evidence",
            "memory",
            "revision",
            "revision_evidence",
            "payload",
            "local_key",
            "copy_manifest_b3",
            "backup_registration",
            "backup_receipt",
            "outbox",
            "receipt",
            "security_event",
            "security_receipt",
            "deletion_intent",
            "deletion_closure",
            "deletion_closure_seal",
            "deletion_member",
            "deletion_work",
            "deletion_observation",
            "deletion_receipt",
            "r5_tombstone",
            "stop_receipt",
            "resume_receipt",
        ] {
            let mut q = self
                .fixture
                .db
                .prepare(&format!(
                    "SELECT quote(rowid),* FROM {table} ORDER BY rowid"
                ))
                .unwrap();
            let cols = q.column_count();
            let rows = q
                .query_map([], |r| {
                    Ok((0..cols)
                        .map(|i| format!("{:?}", r.get_ref(i).unwrap()))
                        .collect::<Vec<_>>()
                        .join("|"))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            result.push((table.into(), rows));
        }
        result
    }
    fn revision(&self) {
        let db = &self.fixture.db;
        let mut m = member(40, 70, 10);
        m.memory = id(11);
        let binding = m.original_dispatch(id(2)).unwrap();
        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        db.execute(
            "INSERT INTO evidence VALUES(?1,?2,1,?3,1,1)",
            params![actor().scope, m.evidence, m.source],
        )
        .unwrap();
        db.execute("INSERT INTO write_intent VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,1,?10,?11,10,?12,'Pending')",
            params![m.operation,m.scope,m.principal,m.request,m.memory,m.revision,m.payload,m.key,m.evidence,id(12),m.incarnation,binding]).unwrap();
        for state in ["Staged", "Sealing"] {
            db.execute(
                "UPDATE write_intent SET state=?1 WHERE operation=?2",
                params![state, m.operation],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO publication VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                m.operation,
                [6u8; 32],
                [7u8; 32],
                vec![8u8; 12],
                vec![9u8; 12],
                b"SYNTHETIC-METADATA-ONLY".as_slice()
            ],
        )
        .unwrap();
        db.execute(
            "UPDATE write_intent SET state='Published' WHERE operation=?1",
            [m.operation],
        )
        .unwrap();
        db.execute(
            "INSERT INTO security_event(scope,kind) VALUES(?1,'Write')",
            [m.scope],
        )
        .unwrap();
        let event = db.last_insert_rowid();
        db.execute(
            "UPDATE security_event SET state='EffectiveFence' WHERE id=?1",
            [event],
        )
        .unwrap();
        db.execute(
            "INSERT INTO revision VALUES(?1,?2,?3,2,?4)",
            params![m.scope, m.memory, m.revision, m.payload],
        )
        .unwrap();
        db.execute(
            "INSERT INTO local_key VALUES(?1,?2,?3,1,1,0)",
            params![m.scope, m.payload, m.key],
        )
        .unwrap();
        db.execute(
            "INSERT INTO payload VALUES(?1,?2,?3,?4,?5,1,?6)",
            params![m.scope, m.memory, m.revision, m.payload, m.key, m.operation],
        )
        .unwrap();
        db.execute(
            "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,1)",
            params![m.scope, m.memory, m.revision, m.evidence],
        )
        .unwrap();
        db.execute(
            "UPDATE memory SET current_revision=?1 WHERE scope=?2 AND id=?3",
            params![m.revision, m.scope, m.memory],
        )
        .unwrap();
        db.execute_batch("UPDATE runtime SET epoch=epoch+1,knowledge_seq=knowledge_seq+1; UPDATE authority SET epoch=(SELECT epoch FROM runtime)").unwrap();
        db.execute(
            "INSERT INTO security_receipt(event,epoch) SELECT ?1,epoch FROM runtime",
            [event],
        )
        .unwrap();
        db.execute(
            "UPDATE security_event SET state='Applied' WHERE id=?1",
            [event],
        )
        .unwrap();
        db.execute(
            "INSERT INTO receipt SELECT ?1,?2,?3,?4,knowledge_seq,?5 FROM runtime",
            params![m.operation, m.scope, m.memory, m.revision, event],
        )
        .unwrap();
        db.execute("INSERT INTO outbox SELECT operation,'MemoryCreated',knowledge_seq FROM receipt WHERE operation=?1",[m.operation]).unwrap();
        db.execute(
            "UPDATE write_intent SET state='Committed' WHERE operation=?1",
            [m.operation],
        )
        .unwrap();
        db.execute_batch("COMMIT").unwrap();
    }
}

#[test]
fn complete_source_closure_authorizes_and_applies_exactly_two_memories() {
    let mut c = Case::new();
    let before_seq: i64 = c
        .app
        .db
        .query_row("SELECT knowledge_seq FROM runtime", [], |r| r.get(0))
        .unwrap();
    let e = c.fence();
    c.app.apply(actor(), e).unwrap();
    assert_eq!(
        Application::operations(&c.app.db, "deletion_closure", e).unwrap(),
        BTreeSet::from([id(10), id(20)])
    );
    c.app.equal_sets(&c.app.db, e).unwrap();
    c.app.reconcile().unwrap();
    assert_eq!(
        c.app
            .db
            .query_row("SELECT count(*) FROM deletion_work", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        c.app
            .db
            .query_row("SELECT knowledge_seq FROM runtime", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        before_seq
    );
    assert_eq!(
        c.app
            .db
            .query_row("SELECT count(*) FROM outbox", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        3
    );
    c.app.admit_metadata(actor(), id(31)).unwrap();
    assert!(c.app.admit_metadata(actor(), id(11)).is_err());
}
#[test]
fn historical_revisions_from_distinct_sources_are_not_filtered_by_current() {
    let mut c = Case::new();
    c.revision();
    let a = c
        .app
        .authoritative_closure(&c.app.db, &actor().scope, Target::Source(id(60)))
        .unwrap();
    assert_eq!(
        a.iter().map(|m| m.operation).collect::<BTreeSet<_>>(),
        BTreeSet::from([id(10), id(20)])
    );
    let all = c
        .app
        .authoritative_closure(&c.app.db, &actor().scope, Target::Memory(id(11)))
        .unwrap();
    assert_eq!(all.len(), 2);
    let e = c
        .app
        .close(actor(), id(90), Target::Memory(id(11)))
        .unwrap();
    c.app.fence(actor(), e).unwrap();
    c.app.apply(actor(), e).unwrap();
    assert_eq!(
        Application::operations(&c.app.db, "deletion_member", e).unwrap(),
        BTreeSet::from([id(10), id(40)])
    );
    assert_eq!(
        c.app
            .db
            .query_row(
                "SELECT count(*) FROM evidence WHERE source=?1",
                [id(60)],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
}
#[test]
fn quarantine_is_included_and_missing_grant_rejects_without_any_closing() {
    let mut c = Case::new();
    c.fixture
        .db
        .execute(
            "INSERT INTO object_quarantine VALUES(?1,'KeyUnavailable',?2)",
            params![id(10), id(3)],
        )
        .unwrap();
    c.app
        .security_change(SecurityChange::Grant {
            actor: actor(),
            memory: id(11),
            enabled: false,
        })
        .unwrap();
    let before = c.snapshot();
    assert_eq!(
        c.app.close(actor(), id(90), Target::Source(id(60))),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(c.snapshot(), before);
}
#[test]
fn quarantined_key_unavailable_objects_still_authorize_and_remain_in_closure() {
    let mut c = Case::new();
    c.fixture
        .db
        .execute(
            "INSERT INTO object_quarantine VALUES(?1,'KeyUnavailable',?2)",
            params![id(10), id(3)],
        )
        .unwrap();
    let e = c.fence();
    c.app.apply(actor(), e).unwrap();
    assert!(
        Application::operations(&c.app.db, "deletion_member", e)
            .unwrap()
            .contains(&id(10))
    );
}
#[test]
fn missing_dependency_rejects_whole_operation_without_side_effects() {
    let mut c = Case::new();
    remove_fixture_row(
        &c.fixture.db,
        "revision_evidence",
        "DELETE FROM revision_evidence WHERE memory=x'0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b';",
    );
    attest(&c.app.db).unwrap(); // Exact guards restored; local FK diagnosis is allowed by S1.
    let before = c.snapshot();
    assert!(
        c.app
            .close(actor(), id(90), Target::Source(id(60)))
            .is_err()
    );
    assert_eq!(c.snapshot(), before);
}
#[test]
fn structurally_valid_but_rebound_source_is_rejected_by_committed_binding() {
    let mut c = Case::new();
    c.fixture
        .db
        .execute(
            "UPDATE evidence SET source=?1 WHERE id=?2",
            params![id(70), id(15)],
        )
        .unwrap();
    let before = c.snapshot();
    assert_eq!(
        c.app.close(actor(), id(90), Target::Source(id(60))),
        Err(Error::Integrity)
    );
    assert_eq!(c.snapshot(), before);
}
#[test]
fn expired_grant_at_closing_has_no_partial_records() {
    let mut c = Case::new();
    c.clock.0.store(2_000_000_000_001, Ordering::SeqCst);
    let before = c.snapshot();
    assert_eq!(
        c.app.close(actor(), id(90), Target::Source(id(60))),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(c.snapshot(), before);
}
#[test]
fn expired_after_closing_and_before_fence_remains_closing() {
    let mut c = Case::new();
    let e = c.event();
    let before = c.snapshot();
    c.clock.0.store(2_000_000_000_001, Ordering::SeqCst);
    assert_eq!(c.app.fence(actor(), e), Err(Error::AuthorizationDenied));
    assert_eq!(c.snapshot(), before);
}
#[test]
fn final_policy_sample_observes_expiry_and_rolls_back_work_epoch_and_receipt() {
    let mut c = Case::new();
    let e = c.fence();
    let before = c.snapshot();
    let clock = c.clock.clone();
    assert_eq!(
        c.app.apply_checked(actor(), e, move |_| {
            clock.0.store(2_000_000_000_001, Ordering::SeqCst);
            Ok(())
        }),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(c.snapshot(), before);
    assert!(c.app.db.is_autocommit());
}
#[test]
fn applied_injected_failure_rolls_back_all_rows_and_keeps_prior_fence() {
    let mut c = Case::new();
    let e = c.fence();
    let before = c.snapshot();
    assert_eq!(
        c.app
            .apply_checked(actor(), e, |_| Err(Error::InvalidInput)),
        Err(Error::InvalidInput)
    );
    assert_eq!(c.snapshot(), before);
    c.app.apply(actor(), e).unwrap();
}
#[test]
fn seal_and_request_replay_are_bound_and_currently_authorized() {
    let mut c = Case::new();
    let e = c.event();
    assert_eq!(
        c.app
            .close(actor(), id(90), Target::Source(id(60)))
            .unwrap(),
        e
    );
    assert_eq!(
        c.app.close(actor(), id(90), Target::Memory(id(11))),
        Err(Error::IdempotencyConflict)
    );
    c.app.fence(actor(), e).unwrap();
    c.app.apply(actor(), e).unwrap();
    let before = c.snapshot();
    c.app.apply(actor(), e).unwrap();
    assert_eq!(before, c.snapshot());
    c.app
        .security_change(SecurityChange::Grant {
            actor: actor(),
            memory: id(11),
            enabled: false,
        })
        .unwrap();
    let before = c.snapshot();
    assert_eq!(
        c.app.close(actor(), id(90), Target::Source(id(60))),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(before, c.snapshot());
}
#[test]
fn closing_blocks_later_security_changes_and_new_business_facts() {
    let mut c = Case::new();
    let _e = c.event();
    let before = c.snapshot();
    assert_eq!(
        c.app.security_change(SecurityChange::Epoch {
            scope: actor().scope
        }),
        Err(Error::CapabilityUnavailable)
    );
    assert_eq!(
        c.app.admit_metadata(actor(), id(31)),
        Err(Error::CapabilityUnavailable)
    );
    assert!(c.fixture.db.execute("INSERT INTO write_intent SELECT x'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',scope,principal,x'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',memory,x'cccccccccccccccccccccccccccccccc',x'dddddddddddddddddddddddddddddddd',x'eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee',evidence,evidence_generation,generation,revision,incarnation,epoch,dispatch_binding,'Pending' FROM write_intent LIMIT 1",[]).is_err());
    assert_eq!(c.snapshot(), before);
}
#[test]
fn epoch_changes_are_currently_revalidated_and_inconsistent_epoch_is_rejected() {
    let mut c = Case::new();
    c.app
        .security_change(SecurityChange::Epoch {
            scope: actor().scope,
        })
        .unwrap();
    let _ = c.event();
    c.fixture
        .db
        .execute("UPDATE runtime SET epoch=epoch+1", [])
        .unwrap();
    let before = c.snapshot();
    assert!(c.app.fence(actor(), 4).is_err());
    assert_eq!(c.snapshot(), before);
}
#[test]
fn overlapping_deletions_share_only_valid_canonical_work_without_rebinding() {
    let mut c = Case::new();
    let a = c
        .app
        .close(actor(), id(90), Target::Memory(id(11)))
        .unwrap();
    let b = c
        .app
        .close(actor(), id(91), Target::Source(id(60)))
        .unwrap();
    assert!(c.app.fence(actor(), b).is_err());
    c.app.fence(actor(), a).unwrap();
    c.app.apply(actor(), a).unwrap();
    c.app.fence(actor(), b).unwrap();
    c.app.apply(actor(), b).unwrap();
    c.app.reconcile().unwrap();
    let canonical: i64 = c
        .app
        .db
        .query_row(
            "SELECT event FROM deletion_work WHERE operation=?1",
            [id(10)],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(canonical, a);
    assert_eq!(
        Application::operations(&c.app.db, "deletion_member", b).unwrap(),
        BTreeSet::from([id(10), id(20)])
    );
}
#[test]
fn read_only_reconcile_never_advances_committed_closing_or_repairs_a_missing_seal() {
    let mut c = Case::new();
    let e = c.event();
    let before = c.snapshot();
    c.app.reconcile().unwrap();
    assert_eq!(c.snapshot(), before);
    remove_fixture_row(
        &c.fixture.db,
        "deletion_closure_seal",
        "DELETE FROM deletion_closure_seal;",
    );
    let before = c.snapshot();
    assert!(c.app.reconcile().is_err());
    assert_eq!(c.snapshot(), before);
    assert!(c.app.fence(actor(), e).is_err());
}
#[test]
fn wrong_principal_scope_and_unknown_target_are_denied_without_records() {
    for a in [
        Actor {
            principal: id(8),
            ..actor()
        },
        Actor {
            scope: [2; 48],
            ..actor()
        },
    ] {
        let mut c = Case::new();
        let before = c.snapshot();
        assert!(c.app.close(a, id(90), Target::Source(id(60))).is_err());
        assert_eq!(c.snapshot(), before);
    }
    let mut c = Case::new();
    let before = c.snapshot();
    assert_eq!(
        c.app.close(actor(), id(90), Target::Source(id(99))),
        Err(Error::ResourceUnavailable)
    );
    assert_eq!(c.snapshot(), before);
}
#[test]
fn security_qualification_changes_share_order_and_cannot_reenable_retired_objects() {
    for change in [
        SecurityChange::Principal {
            actor: actor(),
            enabled: false,
        },
        SecurityChange::Space {
            scope: actor().scope,
            enabled: false,
        },
        SecurityChange::Key {
            scope: actor().scope,
            memory: id(11),
            enabled: false,
        },
        SecurityChange::Copy {
            scope: actor().scope,
            memory: id(11),
            enabled: false,
        },
    ] {
        let mut c = Case::new();
        c.app.security_change(change).unwrap();
        assert_eq!(
            c.app.close(actor(), id(90), Target::Memory(id(11))),
            Err(Error::AuthorizationDenied)
        );
    }
    let mut c = Case::new();
    c.app
        .security_change(SecurityChange::Source {
            scope: actor().scope,
            source: id(60),
            enabled: false,
        })
        .unwrap();
    // Availability is not structural membership; deleting invalid sources remains possible.
    let e = c.fence();
    c.app.apply(actor(), e).unwrap();
    c.app.reconcile().unwrap();
}
#[test]
fn default_clock_is_server_time_not_a_request_parameter() {
    assert!(SystemClock.now().unwrap() > 0);
}
#[test]
fn s2_connection_rejects_temp_store_drift_without_changing_s1_package() {
    let mut c = Case::new();
    c.app.db.execute_batch("PRAGMA temp_store=FILE").unwrap();
    let before = c.snapshot();
    assert_eq!(
        c.app.close(actor(), id(90), Target::Memory(id(11))),
        Err(Error::Integrity)
    );
    assert_eq!(before, c.snapshot());
    assert_eq!(
        schema_hash(),
        [
            0x59, 0xa3, 0x23, 0xd8, 0x68, 0xdf, 0x02, 0xc3, 0xe8, 0x6f, 0x15, 0x95, 0x74, 0x5f,
            0x08, 0x93, 0x84, 0xb8, 0x67, 0x27, 0x76, 0x12, 0x5d, 0x02, 0x57, 0x9c, 0x3b, 0x45,
            0xdd, 0x30, 0x1d, 0xd8
        ]
    );
}
#[test]
fn second_connection_reconciles_metadata_but_does_not_receive_owner_recovery_authority() {
    let mut c = Case::new();
    let e = c.fence();
    c.app.apply(actor(), e).unwrap();
    let second = Application::attach(
        Connection::open(&c.fixture.path).unwrap(),
        id(2),
        id(1),
        id(3),
        c.app.order.clone(),
        c.clock.clone(),
    )
    .unwrap();
    second.reconcile().unwrap();
    assert!(
        Application::attach(
            Connection::open(&c.fixture.path).unwrap(),
            id(2),
            id(1),
            id(4),
            c.app.order.clone(),
            c.clock.clone()
        )
        .is_err()
    );
}

#[test]
fn application_final_difference_check_rejects_missing_and_equal_count_wrong_members() {
    for wrong_count_equal in [false, true] {
        let mut c = Case::new();
        let e = c.fence();
        let before = c.snapshot();
        let result = c.app.apply_checked(actor(), e, |db| {
            let trigger = if wrong_count_equal {
                "immutable_p12_deletion_member_UPDATE"
            } else {
                "immutable_p12_deletion_member_DELETE"
            };
            let definition: String = sql(db.query_row(
                "SELECT sql FROM sqlite_schema WHERE name=?1",
                [trigger],
                |r| r.get(0),
            ))?;
            sql(db.execute_batch(&format!("DROP TRIGGER {trigger}")))?;
            if wrong_count_equal {
                sql(db.execute_batch("PRAGMA defer_foreign_keys=ON"))?;
                sql(db.execute(
                    "UPDATE deletion_member SET operation=?1 WHERE event=?2 AND operation=?3",
                    params![id(30), e, id(20)],
                ))?;
            } else {
                sql(db.execute(
                    "DELETE FROM deletion_member WHERE event=?1 AND operation=?2",
                    params![e, id(20)],
                ))?;
            }
            sql(db.execute_batch(&definition))?;
            Ok(())
        });
        assert_eq!(result, Err(Error::Integrity));
        assert_eq!(c.snapshot(), before);
        assert!(c.app.db.is_autocommit());
    }
}

#[test]
fn failed_deferred_commit_explicitly_rolls_back_and_latches_shared_admission() {
    let mut c = Case::new();
    let e = c.fence();
    let before = c.snapshot();
    let result = c.app.apply_checked(actor(), e, |db| {
        sql(db.execute_batch("PRAGMA defer_foreign_keys=ON"))?;
        sql(db.execute(
            "INSERT INTO resume_receipt VALUES(?1,?2,10,?3)",
            params![id(98), id(99), [1u8; 32]],
        ))?;
        Ok(())
    });
    assert_eq!(result, Err(Error::CapabilityUnavailable));
    assert!(c.app.db.is_autocommit());
    assert_eq!(c.snapshot(), before);
    assert!(c.app.order.blocked.load(Ordering::SeqCst));
    assert!(c.app.apply(actor(), e).is_err());
}

#[test]
fn unknown_write_is_not_replayed_or_admitted_by_reconcile() {
    let mut c = Case::new();
    c.fixture.db.execute("INSERT INTO write_intent SELECT ?1,scope,principal,?2,memory,?3,?4,?5,evidence,evidence_generation,generation,revision,incarnation,epoch,dispatch_binding,'Pending' FROM write_intent LIMIT 1",params![id(80),id(81),id(82),id(83),id(84)]).unwrap();
    c.fixture
        .db
        .execute(
            "UPDATE write_intent SET state='Unknown' WHERE operation=?1",
            [id(80)],
        )
        .unwrap();
    let before = c.snapshot();
    assert_eq!(c.app.reconcile(), Err(Error::CapabilityUnavailable));
    assert_eq!(c.snapshot(), before);
    assert!(
        c.app
            .close(actor(), id(90), Target::Source(id(60)))
            .is_err()
    );
    assert_eq!(c.snapshot(), before);
}

#[test]
fn two_connections_order_delete_apply_before_waiting_revocation() {
    use std::sync::mpsc;
    let mut c = Case::new();
    let e = c.fence();
    let mut peer = Application::attach(
        Connection::open(&c.fixture.path).unwrap(),
        id(2),
        id(1),
        id(3),
        c.app.order.clone(),
        c.clock.clone(),
    )
    .unwrap();
    let (entered, wait_entered) = mpsc::channel();
    let (release, wait_release) = mpsc::channel();
    let (attempted, wait_attempted) = mpsc::channel();
    let mut app = c.app;
    let output = std::thread::spawn(move || {
        let result = app.apply_checked(actor(), e, |_| {
            entered.send(()).unwrap();
            wait_release.recv().unwrap();
            Ok(())
        });
        (app, result)
    });
    wait_entered.recv().unwrap();
    let revocation = std::thread::spawn(move || {
        attempted.send(()).unwrap();
        let r = peer.security_change(SecurityChange::Grant {
            actor: actor(),
            memory: id(11),
            enabled: false,
        });
        (peer, r)
    });
    wait_attempted.recv().unwrap();
    release.send(()).unwrap();
    let (app, result) = output.join().unwrap();
    result.unwrap();
    c.app = app;
    let (_peer, result) = revocation.join().unwrap();
    let revoke = result.unwrap();
    assert!(revoke > e);
    let state: String = c
        .app
        .db
        .query_row("SELECT state FROM security_event WHERE id=?1", [e], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(state, "Applied");
    assert_eq!(
        c.app.close(actor(), id(91), Target::Memory(id(11))),
        Err(Error::AuthorizationDenied)
    );
}

#[test]
fn second_connection_revocation_before_closing_denies_all_members_atomically() {
    let mut c = Case::new();
    let mut peer = Application::attach(
        Connection::open(&c.fixture.path).unwrap(),
        id(2),
        id(1),
        id(3),
        c.app.order.clone(),
        c.clock.clone(),
    )
    .unwrap();
    std::thread::spawn(move || {
        peer.security_change(SecurityChange::Grant {
            actor: actor(),
            memory: id(21),
            enabled: false,
        })
    })
    .join()
    .unwrap()
    .unwrap();
    let before = c.snapshot();
    assert_eq!(
        c.app.close(actor(), id(90), Target::Source(id(60))),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(c.snapshot(), before);
}

#[test]
fn before_applied_graph_changes_are_rejected_not_sealed_over() {
    let mut c = Case::new();
    let e = c.event();
    c.fixture
        .db
        .execute(
            "UPDATE evidence SET source=?1 WHERE id=?2",
            params![id(70), id(15)],
        )
        .unwrap();
    let before = c.snapshot();
    assert_eq!(c.app.fence(actor(), e), Err(Error::Integrity));
    assert_eq!(c.snapshot(), before);
}

#[test]
fn applied_member_damage_blocks_reconcile_and_an_unrelated_metadata_request() {
    let mut c = Case::new();
    let e = c.fence();
    c.app.apply(actor(), e).unwrap();
    remove_fixture_row(
        &c.fixture.db,
        "deletion_member",
        "DELETE FROM deletion_member WHERE operation=x'0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a';",
    );
    let before = c.snapshot();
    assert_eq!(c.app.reconcile(), Err(Error::Integrity));
    assert_eq!(c.snapshot(), before);
    assert!(c.app.admit_metadata(actor(), id(31)).is_err());
    assert_eq!(c.snapshot(), before);
}

#[test]
fn closure_parent_replace_paths_are_permanent_same_sqlite_version_regressions() {
    let mut c = Case::new();
    let _e = c.event();
    let before = c.snapshot();
    for sql in [
        "INSERT OR REPLACE INTO deletion_closure SELECT * FROM deletion_closure",
        "UPDATE OR REPLACE deletion_closure SET operation=operation",
        "INSERT OR REPLACE INTO deletion_intent SELECT * FROM deletion_intent",
        "UPDATE OR REPLACE deletion_intent SET event=event",
        "INSERT OR REPLACE INTO deletion_closure_seal SELECT * FROM deletion_closure_seal",
    ] {
        assert!(c.fixture.db.execute_batch(sql).is_err());
        assert_eq!(c.snapshot(), before);
    }
    c.app.reconcile().unwrap();
}

#[test]
fn incorrect_shared_work_binding_is_not_silently_ignored() {
    let mut c = Case::new();
    let a = c
        .app
        .close(actor(), id(90), Target::Memory(id(11)))
        .unwrap();
    c.app.fence(actor(), a).unwrap();
    c.app.apply(actor(), a).unwrap();
    // Controlled metadata damage; no marker file is created or changed.
    let guard: String = c
        .fixture
        .db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='p12_work_identity'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    c.fixture
        .db
        .execute_batch("DROP TRIGGER p12_work_identity")
        .unwrap();
    c.fixture
        .db
        .execute("UPDATE deletion_work SET marker=x'00'", [])
        .unwrap();
    c.fixture.db.execute_batch(&guard).unwrap();
    let before = c.snapshot();
    assert_eq!(
        c.app.close(actor(), id(91), Target::Source(id(60))),
        Err(Error::Integrity)
    );
    assert_eq!(c.snapshot(), before);
}

#[test]
fn epoch_changed_inside_apply_is_not_accepted_as_the_original_authorization() {
    let mut c = Case::new();
    let e = c.fence();
    let before = c.snapshot();
    assert_eq!(
        c.app.apply_checked(actor(), e, |db| {
            sql(db.execute("UPDATE runtime SET epoch=epoch+1", []))?;
            sql(db.execute("UPDATE authority SET epoch=(SELECT epoch FROM runtime)", []))?;
            Ok(())
        }),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(c.snapshot(), before);
}

#[test]
fn object_ineligibility_does_not_become_a_shared_integrity_fault() {
    let mut c = Case::new();
    c.fixture
        .db
        .execute(
            "INSERT INTO object_quarantine VALUES(?1,'KeyUnavailable',?2)",
            params![id(10), id(3)],
        )
        .unwrap();
    assert_eq!(
        c.app.admit_metadata(actor(), id(11)),
        Err(Error::AuthorizationDenied)
    );
    c.app.admit_metadata(actor(), id(31)).unwrap();
    remove_fixture_row(
        &c.fixture.db,
        "revision_evidence",
        "DELETE FROM revision_evidence WHERE memory=x'0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b';",
    );
    // Unrelated Memory qualification is local; Source deletion must still reject
    // the unprovable Source graph in its entirety.
    c.app.admit_metadata(actor(), id(31)).unwrap();
    assert_eq!(
        c.app.close(actor(), id(90), Target::Source(id(60))),
        Err(Error::Integrity)
    );
}

#[test]
fn runtime_package_binding_is_not_replaced_by_the_format_rows_claim() {
    let mut c = Case::new();
    let guard: String = c
        .fixture
        .db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='runtime_identity'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    c.fixture
        .db
        .execute_batch("DROP TRIGGER runtime_identity")
        .unwrap();
    c.fixture
        .db
        .execute("UPDATE runtime SET schema_hash=?1", [[0u8; 32]])
        .unwrap();
    c.fixture.db.execute_batch(&guard).unwrap();
    attest(&c.app.db).unwrap(); // The trusted package/format is intact; Owner binding is not.
    let before = c.snapshot();
    assert_eq!(
        c.app.close(actor(), id(90), Target::Source(id(60))),
        Err(Error::Integrity)
    );
    assert_eq!(c.snapshot(), before);
}
