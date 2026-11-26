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
    let (observe, observations) = mpsc::channel();
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
    // The first connection is now inside Applied with RealmOrder held. Install
    // the observer only now, so its events belong to the waiting second command.
    *peer.order.contention_observer.lock().unwrap() = Some(observe);
    let revocation = std::thread::spawn(move || {
        let r = peer.security_change(SecurityChange::Grant {
            actor: actor(),
            memory: id(11),
            enabled: false,
        });
        (peer, r)
    });
    assert_eq!(
        observations
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap(),
        LockObservation::Contended
    );
    assert!(matches!(
        observations.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    let held_state: String = c
        .fixture
        .db
        .query_row("SELECT state FROM security_event WHERE id=?1", [e], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(held_state, "EffectiveFence");
    println!(
        "R3 second=try_lock(WouldBlock) first_event={e} committed_state={held_state}; releasing_only_after_contention"
    );
    release.send(()).unwrap();
    assert_eq!(
        observations
            .recv_timeout(std::time::Duration::from_secs(30))
            .unwrap(),
        LockObservation::Acquired
    );
    let (app, result) = output.join().unwrap();
    result.unwrap();
    c.app = app;
    let (_peer, result) = revocation.join().unwrap();
    let revoke = result.unwrap();
    *c.app.order.contention_observer.lock().unwrap() = None;
    assert!(revoke > e);
    let state: String = c
        .app
        .db
        .query_row("SELECT state FROM security_event WHERE id=?1", [e], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(state, "Applied");
    let (revoked, epoch): (bool, i64) = c
        .app
        .db
        .query_row(
            "SELECT grant_ok=0,epoch FROM authority WHERE scope=?1 AND principal=?2 AND target=?3",
            params![actor().scope, actor().principal, id(11)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(revoked);
    println!(
        "R3 order=Contended->release_first->Acquired->revocation_completed delete_event={e} delete_state={state} revoke_event={revoke} grant_revoked={revoked} epoch={epoch}"
    );
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

#[derive(Clone, Copy, Debug)]
enum LedgerDamage {
    MissingOutbox,
    MisboundReceipt,
    SwappedOutboxSequence,
    NonCommittedWithReceipt,
}
fn damage_ledger(c: &Case, damage: LedgerDamage) {
    let (trigger, mutation) = match damage {
        LedgerDamage::MissingOutbox => (
            "immutable_outbox_DELETE",
            "DELETE FROM outbox WHERE operation=x'1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e';",
        ),
        LedgerDamage::MisboundReceipt => (
            "immutable_receipt_UPDATE",
            "UPDATE receipt SET memory=x'15151515151515151515151515151515',revision=x'16161616161616161616161616161616' WHERE operation=x'1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e';",
        ),
        LedgerDamage::SwappedOutboxSequence => (
            "immutable_outbox_DELETE",
            "DELETE FROM outbox WHERE operation IN(x'0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a',x'1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e'); INSERT INTO outbox VALUES(x'0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a','MemoryCreated',3); INSERT INTO outbox VALUES(x'1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e','MemoryCreated',1);",
        ),
        LedgerDamage::NonCommittedWithReceipt => (
            "intent_progress",
            "UPDATE write_intent SET state='Unknown' WHERE operation=x'1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e';",
        ),
    };
    let definition: String = c
        .fixture
        .db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name=?1",
            [trigger],
            |r| r.get(0),
        )
        .unwrap();
    c.fixture
        .db
        .execute_batch(&format!(
            "BEGIN IMMEDIATE; DROP TRIGGER {trigger}; {mutation}"
        ))
        .unwrap();
    c.fixture.db.execute_batch(&definition).unwrap();
    c.fixture.db.execute_batch("COMMIT").unwrap();
    // The exact schema and actual foreign keys still pass: this is a data ledger
    // invariant, not a weakened-schema or dangling-FK test.
    attest(&c.app.db).unwrap();
}
fn ledger_rejection(damage: LedgerDamage) {
    let mut all_correct = true;
    for entry in ["reconcile", "admit_metadata", "close"] {
        let mut c = Case::new();
        damage_ledger(&c, damage);
        let before = c.snapshot();
        let result = match entry {
            "reconcile" => c.app.reconcile(),
            "admit_metadata" => c.app.admit_metadata(actor(), id(11)),
            _ => c
                .app
                .close(actor(), id(90), Target::Source(id(60)))
                .map(|_| ()),
        };
        let unchanged = c.snapshot() == before;
        let blocked = c.app.order.blocked.load(Ordering::SeqCst);
        println!(
            "R1 damage={damage:?} entry={entry} result={result:?} blocked={blocked} autocommit={} snapshot_unchanged={unchanged}",
            c.app.db.is_autocommit()
        );
        all_correct &=
            result == Err(Error::Integrity) && unchanged && blocked && c.app.db.is_autocommit();
    }
    assert!(
        all_correct,
        "all entry points must reject the same shared ledger corruption"
    );
}
#[test]
fn remediation_r1_unrelated_outbox_missing_blocks_every_entry() {
    ledger_rejection(LedgerDamage::MissingOutbox);
}
#[test]
fn remediation_r1_fk_valid_receipt_rebinding_blocks_every_entry() {
    ledger_rejection(LedgerDamage::MisboundReceipt);
}
#[test]
fn remediation_r1_fk_valid_outbox_sequence_swap_blocks_every_entry() {
    ledger_rejection(LedgerDamage::SwappedOutboxSequence);
}
#[test]
fn remediation_r1_non_committed_intent_with_receipt_is_integrity_not_pending() {
    ledger_rejection(LedgerDamage::NonCommittedWithReceipt);
}

#[test]
fn remediation_r2_unknown_external_event_never_latches_or_leaves_a_transaction() {
    let mut all_correct = true;
    for method in ["fence", "apply"] {
        let mut c = Case::new();
        let before = c.snapshot();
        let result = if method == "fence" {
            c.app.fence(actor(), 9_999_999)
        } else {
            c.app.apply(actor(), 9_999_999)
        };
        let unchanged = c.snapshot() == before;
        let blocked = c.app.order.blocked.load(Ordering::SeqCst);
        let live = c.app.db.is_autocommit();
        let next = c.app.close(actor(), id(90), Target::Source(id(60)));
        let progressed = match next {
            Ok(e) => c
                .app
                .fence(actor(), e)
                .and_then(|_| c.app.apply(actor(), e)),
            Err(e) => Err(e),
        };
        println!(
            "R2 method={method} result={result:?} blocked={blocked} autocommit={live} snapshot_unchanged={unchanged} subsequent_legal={progressed:?}"
        );
        all_correct &= result == Err(Error::ResourceUnavailable)
            && !blocked
            && live
            && unchanged
            && progressed.is_ok();
    }
    assert!(all_correct, "unknown input is not shared corruption");
}

fn mutate_fixture(db: &Connection, trigger: &str, statement: &str) {
    let definition: String = db
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name=?1",
            [trigger],
            |r| r.get(0),
        )
        .unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE")
        .unwrap();
    db.execute_batch(&format!("DROP TRIGGER {trigger}; {statement}"))
        .unwrap();
    db.execute_batch(&definition).unwrap();
    db.execute_batch("COMMIT; PRAGMA foreign_keys=ON").unwrap();
}
#[test]
fn remediation_r2_persisted_intent_parent_damage_remains_integrity_and_blocking() {
    for fault in ["scope", "kind", "missing_parent"] {
        let mut c = Case::new();
        let e = c.event();
        match fault {
            "scope" => mutate_fixture(
                &c.fixture.db,
                "immutable_p12_deletion_intent_UPDATE",
                &format!("UPDATE deletion_intent SET scope=zeroblob(48) WHERE event={e}"),
            ),
            "kind" => mutate_fixture(
                &c.fixture.db,
                "security_progress",
                &format!("UPDATE security_event SET kind='Write' WHERE id={e}"),
            ),
            _ => mutate_fixture(
                &c.fixture.db,
                "keep_security_event",
                &format!("DELETE FROM security_event WHERE id={e}"),
            ),
        }
        let before = c.snapshot();
        // Direct internal lookup and the externally initiated transaction must
        // both classify a persisted broken relation as Integrity, not absence.
        assert!(matches!(
            c.app.intent_required(&c.app.db, e),
            Err(Error::Integrity)
        ));
        let result = c.app.fence(actor(), e);
        println!(
            "R2 persisted_fault={fault} result={result:?} blocked={} autocommit={} snapshot_unchanged={}",
            c.app.order.blocked.load(Ordering::SeqCst),
            c.app.db.is_autocommit(),
            c.snapshot() == before
        );
        assert_eq!(result, Err(Error::Integrity));
        assert!(c.app.order.blocked.load(Ordering::SeqCst));
        assert!(c.app.db.is_autocommit());
        assert_eq!(c.snapshot(), before);
    }
}
#[test]
fn remediation_r2_internal_missing_intent_or_work_is_not_external_absence() {
    let mut c = Case::new();
    let e = c.fence();
    c.app.apply(actor(), e).unwrap();
    let selected = c.app.frozen(&c.app.db, e).unwrap();
    assert!(
        c.app
            .intent_optional(&c.app.db, 9_999_999)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        c.app.intent_required(&c.app.db, 9_999_999),
        Err(Error::Integrity)
    ));
    assert!(matches!(
        c.app.frozen(&c.app.db, 9_999_999),
        Err(Error::Integrity)
    ));
    // Work points to a real Write event with no deletion intent, and its marker
    // metadata matches that reference. No marker file is published or observed.
    let marker = selected[0].marker(id(2), 1);
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
        .execute_batch("PRAGMA foreign_keys=OFF; BEGIN IMMEDIATE; DROP TRIGGER p12_work_identity")
        .unwrap();
    c.fixture
        .db
        .execute(
            "UPDATE deletion_work SET event=1,marker=?1 WHERE operation=?2",
            params![marker, selected[0].operation],
        )
        .unwrap();
    c.fixture.db.execute_batch(&guard).unwrap();
    c.fixture
        .db
        .execute_batch("COMMIT; PRAGMA foreign_keys=ON")
        .unwrap();
    assert_eq!(
        c.app.validate_work(&c.app.db, &selected[0], e),
        Err(Error::Integrity)
    );
    let before = c.snapshot();
    assert_eq!(c.app.reconcile(), Err(Error::Integrity));
    assert!(c.app.order.blocked.load(Ordering::SeqCst));
    assert_eq!(c.snapshot(), before);
}

#[derive(Clone, Copy, Debug)]
enum FollowupFault {
    OrphanWrite,
    SharedWrite,
    ExtraWork,
}
fn followup_fault(c: &mut Case, fault: FollowupFault) {
    match fault {
        FollowupFault::OrphanWrite => {
            let db = &c.fixture.db;
            db.execute_batch("BEGIN IMMEDIATE").unwrap();
            db.execute(
                "INSERT INTO security_event(scope,kind) VALUES(?1,'Write')",
                [actor().scope],
            )
            .unwrap();
            let e = db.last_insert_rowid();
            db.execute(
                "UPDATE security_event SET state='EffectiveFence' WHERE id=?1",
                [e],
            )
            .unwrap();
            db.execute_batch("UPDATE runtime SET epoch=epoch+1; UPDATE authority SET epoch=(SELECT epoch FROM runtime)").unwrap();
            db.execute(
                "INSERT INTO security_receipt(event,epoch) SELECT ?1,epoch FROM runtime",
                [e],
            )
            .unwrap();
            db.execute("UPDATE security_event SET state='Applied' WHERE id=?1", [e])
                .unwrap();
            db.execute_batch("COMMIT").unwrap();
        }
        FollowupFault::SharedWrite => mutate_fixture(
            &c.fixture.db,
            "immutable_receipt_UPDATE",
            "UPDATE receipt SET event=1 WHERE operation=x'1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e1e';",
        ),
        FollowupFault::ExtraWork => {
            let e = c
                .app
                .close(actor(), id(90), Target::Memory(id(11)))
                .unwrap();
            c.app.fence(actor(), e).unwrap();
            c.app.apply(actor(), e).unwrap();
            let m = c
                .app
                .authoritative_closure(&c.app.db, &actor().scope, Target::Memory(id(31)))
                .unwrap()
                .remove(0);
            let db = &c.fixture.db;
            let guard: String = db
                .query_row(
                    "SELECT sql FROM sqlite_schema WHERE name='p12_work_fenced'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            db.execute_batch("BEGIN IMMEDIATE; DROP TRIGGER p12_work_fenced")
                .unwrap();
            db.execute(
                "INSERT INTO deletion_work(operation,event,generation,marker) VALUES(?1,?2,1,?3)",
                params![m.operation, e, m.marker(id(2), e)],
            )
            .unwrap();
            db.execute_batch(&guard).unwrap();
            db.execute_batch("COMMIT").unwrap();
        }
    }
    attest(&c.app.db).unwrap();
}
fn followup_reject(fault: FollowupFault) {
    let mut all = true;
    for entry in ["owner", "reconcile", "admit", "close"] {
        let mut c = Case::new();
        followup_fault(&mut c, fault);
        let before = c.snapshot();
        let result = match entry {
            "owner" => c.app.transaction(|app, db| app.owner(db).map(|_| ())),
            "reconcile" => c.app.reconcile(),
            "admit" => c.app.admit_metadata(actor(), id(21)),
            _ => c
                .app
                .close(actor(), id(91), Target::Source(id(70)))
                .map(|_| ()),
        };
        let blocked = c.app.order.blocked.load(Ordering::SeqCst);
        let unchanged = before == c.snapshot();
        println!(
            "R1b fault={fault:?} entry={entry} result={result:?} blocked={blocked} unchanged={unchanged} autocommit={}",
            c.app.db.is_autocommit()
        );
        all &= result == Err(Error::Integrity) && blocked && unchanged && c.app.db.is_autocommit();
    }
    assert!(
        all,
        "shared inconsistency cannot hide outside requested objects"
    );
}
#[test]
fn followup_a1_write_without_business_receipt_is_rejected() {
    followup_reject(FollowupFault::OrphanWrite);
}
#[test]
fn followup_a2_two_receipts_sharing_write_event_are_rejected() {
    followup_reject(FollowupFault::SharedWrite);
}
#[test]
fn followup_b1_extra_work_outside_canonical_members_is_rejected() {
    followup_reject(FollowupFault::ExtraWork);
}

#[test]
fn followup_b2_canonical_historical_work_is_reused_without_rebinding() {
    let mut c = Case::new();
    let a = c
        .app
        .close(actor(), id(90), Target::Memory(id(11)))
        .unwrap();
    c.app.fence(actor(), a).unwrap();
    c.app.apply(actor(), a).unwrap();
    let before: (i64, Vec<u8>) = c
        .app
        .db
        .query_row(
            "SELECT event,marker FROM deletion_work WHERE operation=?1",
            [id(10)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let b = c
        .app
        .close(actor(), id(91), Target::Source(id(60)))
        .unwrap();
    c.app.fence(actor(), b).unwrap();
    c.app.apply(actor(), b).unwrap();
    c.app.reconcile().unwrap();
    let after: (i64, Vec<u8>) = c
        .app
        .db
        .query_row(
            "SELECT event,marker FROM deletion_work WHERE operation=?1",
            [id(10)],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(after.0, a);
    assert!(
        Application::operations(&c.app.db, "deletion_member", b)
            .unwrap()
            .contains(&id(10))
    );
    c.app.admit_metadata(actor(), id(31)).unwrap();
    let rev = c
        .app
        .security_change(SecurityChange::Grant {
            actor: actor(),
            memory: id(31),
            enabled: false,
        })
        .unwrap();
    assert_eq!(
        c.app
            .db
            .query_row("SELECT count(*) FROM receipt WHERE event=?1", [rev], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
    c.app.reconcile().unwrap();
    println!(
        "R1b B2 canonical={a} reused_by={b} unchanged_marker=true revocation_without_business_receipt={rev}"
    );
}
#[test]
fn followup_b3_invalid_work_scope_generation_canonical_and_marker_are_blocked() {
    for fault in ["scope", "generation", "canonical", "marker"] {
        let mut c = Case::new();
        let e = c.fence();
        c.app.apply(actor(), e).unwrap();
        match fault {
            "scope" => {
                let mut other = actor().scope;
                other[47] = 2;
                let hex = other.iter().map(|x| format!("{x:02x}")).collect::<String>();
                mutate_fixture(
                    &c.fixture.db,
                    "immutable_p12_deletion_intent_UPDATE",
                    &format!("UPDATE deletion_intent SET scope=x'{hex}' WHERE event={e}"),
                );
                mutate_fixture(
                    &c.fixture.db,
                    "security_progress",
                    &format!("UPDATE security_event SET scope=x'{hex}' WHERE id={e}"),
                );
                // Parent/intent scopes match and all FKs are valid. Only the
                // Work's operation belongs to a different Space.
                attest(&c.app.db).unwrap();
            }
            "generation" => {
                c.fixture
                    .db
                    .execute_batch("PRAGMA ignore_check_constraints=ON")
                    .unwrap();
                mutate_fixture(
                    &c.fixture.db,
                    "p12_work_identity",
                    "UPDATE deletion_work SET generation=2;",
                );
                c.fixture
                    .db
                    .execute_batch("PRAGMA ignore_check_constraints=OFF")
                    .unwrap();
            }
            "canonical" => mutate_fixture(
                &c.fixture.db,
                "p12_work_identity",
                "UPDATE deletion_work SET event=1;",
            ),
            _ => mutate_fixture(
                &c.fixture.db,
                "p12_work_identity",
                "UPDATE deletion_work SET marker=x'00';",
            ),
        }
        let before = c.snapshot();
        let result = c.app.reconcile();
        println!(
            "R1b B3 fault={fault} result={result:?} blocked={} unchanged={}",
            c.app.order.blocked.load(Ordering::SeqCst),
            before == c.snapshot()
        );
        assert_eq!(result, Err(Error::Integrity));
        assert!(c.app.order.blocked.load(Ordering::SeqCst));
        assert_eq!(before, c.snapshot());
        assert!(c.app.admit_metadata(actor(), id(31)).is_err());
        assert_eq!(before, c.snapshot());
    }
}

fn isolation_a2_fixture(c: &Case) {
    let db = &c.fixture.db;
    let original: i64 = db
        .query_row(
            "SELECT event FROM receipt WHERE operation=?1",
            [id(30)],
            |r| r.get(0),
        )
        .unwrap();
    assert_ne!(original, 1);
    let guards = ["security_progress", "immutable_receipt_UPDATE"].map(|name| {
        db.query_row("SELECT sql FROM sqlite_schema WHERE name=?1", [name], |r| {
            r.get::<_, String>(0)
        })
        .unwrap()
    });
    db.execute_batch(
        "BEGIN IMMEDIATE; DROP TRIGGER security_progress; DROP TRIGGER immutable_receipt_UPDATE",
    )
    .unwrap();
    db.execute(
        "UPDATE security_event SET kind='Revocation' WHERE id=?1",
        [original],
    )
    .unwrap();
    db.execute("UPDATE receipt SET event=1 WHERE operation=?1", [id(30)])
        .unwrap();
    for guard in guards {
        db.execute_batch(&guard).unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    attest(db).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        0
    );
    assert_eq!(db.query_row("SELECT count(*) FROM security_event e WHERE e.kind='Write' AND
        (e.state<>'Applied' OR NOT EXISTS(SELECT 1 FROM receipt r WHERE r.event=e.id AND r.scope=e.scope)
         OR NOT EXISTS(SELECT 1 FROM security_receipt s WHERE s.event=e.id AND s.phase='Applied'))",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    assert_eq!(db.query_row("SELECT count(*) FROM receipt r WHERE NOT EXISTS(SELECT 1 FROM security_event e
        JOIN security_receipt s ON s.event=e.id WHERE e.id=r.event AND e.scope=r.scope AND e.kind='Write'
        AND e.state='Applied' AND s.phase='Applied')",[],|r|r.get::<_,i64>(0)).unwrap(),0);
    // Test-only projection of the exact implementation SQL. No source mutation:
    // demonstrate that removing only the duplicate predicate admits this fixture.
    let source = include_str!("../application.rs");
    let start = source.find("\"SELECT epoch=7+").unwrap() + 1;
    let suffix = "FROM runtime WHERE id=1";
    let end = start + source[start..].find(suffix).unwrap() + suffix.len();
    let query = &source[start..end];
    let duplicate = "         AND NOT EXISTS(SELECT 1 FROM receipt a JOIN receipt b ON a.event=b.event\n             WHERE a.operation<>b.operation)\n";
    assert_eq!(query.matches(duplicate).count(), 1);
    let without = query.replacen(duplicate, "", 1);
    assert!(!db.query_row(query, [], |r| r.get::<_, bool>(0)).unwrap());
    assert!(db.query_row(&without, [], |r| r.get::<_, bool>(0)).unwrap());
    println!(
        "ISOLATION A2 original_event={original} converted=Revocation orphan_writes=0 receipt_parents_valid=true exact_ledger=false without_duplicate_predicate=true"
    );
}

#[test]
fn isolation_a2_duplicate_receipt_event_without_orphan_write() {
    for entry in ["owner", "reconcile", "admit", "close"] {
        let mut c = Case::new();
        isolation_a2_fixture(&c);
        let before = c.snapshot();
        let result = match entry {
            "owner" => c.app.transaction(|app, db| app.owner(db).map(|_| ())),
            "reconcile" => c.app.reconcile(),
            "admit" => c.app.admit_metadata(actor(), id(21)),
            _ => c
                .app
                .close(actor(), id(91), Target::Source(id(70)))
                .map(|_| ()),
        };
        println!(
            "ISOLATION A2 entry={entry} result={result:?} blocked={} unchanged={}",
            c.app.order.blocked.load(Ordering::SeqCst),
            before == c.snapshot()
        );
        assert_eq!(result, Err(Error::Integrity));
        assert!(c.app.order.blocked.load(Ordering::SeqCst));
        assert!(c.app.db.is_autocommit());
        assert_eq!(before, c.snapshot());
    }
}

#[test]
fn isolation_b3_later_applied_event_is_not_the_original_canonical() {
    for entry in ["reconcile", "admit", "close"] {
        let mut c = Case::new();
        let a = c
            .app
            .close(actor(), id(90), Target::Memory(id(11)))
            .unwrap();
        c.app.fence(actor(), a).unwrap();
        c.app.apply(actor(), a).unwrap();
        let b = c
            .app
            .close(actor(), id(91), Target::Source(id(60)))
            .unwrap();
        c.app.fence(actor(), b).unwrap();
        c.app.apply(actor(), b).unwrap();
        let member = c
            .app
            .frozen(&c.app.db, a)
            .unwrap()
            .into_iter()
            .find(|m| m.operation == id(10))
            .unwrap();
        c.app.frozen(&c.app.db, b).unwrap();
        c.app.reconcile().unwrap();
        assert!(a < b);
        assert_eq!(
            c.app
                .db
                .query_row(
                    "SELECT event FROM deletion_work WHERE operation=?1",
                    [id(10)],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            a
        );
        let marker = member.marker(id(2), b);
        let db = &c.fixture.db;
        let guard: String = db
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name='p12_work_identity'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        db.execute_batch("BEGIN IMMEDIATE; DROP TRIGGER p12_work_identity")
            .unwrap();
        db.execute(
            "UPDATE deletion_work SET event=?1,marker=?2 WHERE operation=?3",
            params![b, marker, id(10)],
        )
        .unwrap();
        db.execute_batch(&guard).unwrap();
        db.execute_batch("COMMIT").unwrap();
        attest(db).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
        validate_global_ledger(db).unwrap();
        assert!(persisted_intent_links_valid(db).unwrap());
        for e in [a, b] {
            assert_eq!(c.app.intent_required(db, e).unwrap().2, "Applied");
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM deletion_closure_seal WHERE event=?1",
                    [e],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                1
            );
            c.app.equal_sets(db, e).unwrap();
            assert!(
                Application::operations(db, "deletion_member", e)
                    .unwrap()
                    .contains(&id(10))
            );
        }
        // B accepts its own well-formed member/work binding; only earlier A
        // rejects borrowing a later canonical event. No missing-parent shortcut.
        c.app.validate_work(db, &member, b).unwrap();
        c.app.frozen(db, b).unwrap();
        assert_eq!(c.app.validate_work(db, &member, a), Err(Error::Integrity));
        assert!(matches!(c.app.frozen(db, a), Err(Error::Integrity)));
        let before = c.snapshot();
        let result = match entry {
            "reconcile" => c.app.reconcile(),
            "admit" => c.app.admit_metadata(actor(), id(31)),
            _ => c
                .app
                .close(actor(), id(92), Target::Source(id(70)))
                .map(|_| ()),
        };
        println!(
            "ISOLATION B3 earlier={a} later={b} valid_later_binding=true earlier_rejects=true entry={entry} result={result:?} unchanged={}",
            before == c.snapshot()
        );
        assert_eq!(result, Err(Error::Integrity));
        assert!(c.app.order.blocked.load(Ordering::SeqCst));
        assert!(c.app.db.is_autocommit());
        assert_eq!(before, c.snapshot());
    }
}
