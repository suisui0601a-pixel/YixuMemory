//! Real SQLite schema fixtures only. No Policy, DEK, filesystem store or delete Application.
use super::*;
use rusqlite::ErrorCode;
use std::path::PathBuf;

fn id(n: u8) -> [u8; 16] {
    [n; 16]
}
fn scope() -> Vec<u8> {
    vec![1; 48]
}
fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|v| format!("{v:02x}")).collect()
}

pub(super) struct Fixture {
    pub(super) db: Connection,
    _dir: Option<tempfile::TempDir>,
    pub(super) path: PathBuf,
}
impl Fixture {
    pub(super) fn fresh() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("yixu-p12-schema-")
            .tempdir()
            .unwrap();
        let path = dir.path().join("schema.sqlite");
        let mut connection = Connection::open(&path).unwrap();
        install_fresh(&mut connection).unwrap();
        println!("P12_SCHEMA_DB={}", path.display());
        let dir = if std::env::var_os("YIXU_VERIFICATION_OUT").is_some() {
            let _ = dir.keep();
            None
        } else {
            Some(dir)
        };
        Self {
            db: connection,
            _dir: dir,
            path,
        }
    }
    fn seeded() -> Self {
        let f = Self::fresh();
        f.db.execute(
            "INSERT INTO runtime VALUES(1,?1,?2,?3,?4,?5,?6,7,0,0,0)",
            params![
                id(2),
                id(1),
                id(3),
                id(4),
                vec![5u8; 36],
                schema_hash().as_slice()
            ],
        )
        .unwrap();
        for op in [10u8, 20, 30] {
            seed(&f.db, op, if op == 30 { 70 } else { 60 });
        }
        attest(&f.db).unwrap();
        f
    }
    fn closing(&self, members: &[u8], seal: bool) -> i64 {
        self.db.execute_batch("BEGIN IMMEDIATE").unwrap();
        self.db
            .execute(
                "INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",
                [scope()],
            )
            .unwrap();
        let event = self.db.last_insert_rowid();
        self.db
            .execute(
                "INSERT INTO deletion_intent VALUES(?1,?2,?3,?4,'Source',?5)",
                params![event, scope(), id(9), id(100 + event as u8), id(60)],
            )
            .unwrap();
        for &member in members {
            self.db
                .execute(
                    "INSERT INTO deletion_closure VALUES(?1,?2)",
                    params![event, id(member)],
                )
                .unwrap();
        }
        if seal {
            self.db
                .execute("INSERT INTO deletion_closure_seal VALUES(?1)", [event])
                .unwrap();
        }
        self.db.execute_batch("COMMIT").unwrap();
        event
    }
    fn fenced(&self) -> i64 {
        let event = self.closing(&[10, 20], true);
        self.db
            .execute(
                "UPDATE security_event SET state='EffectiveFence' WHERE id=?1",
                [event],
            )
            .unwrap();
        event
    }
}

// A completely synthetic, content-free committed metadata graph, constructed through
// all real schema phases/constraints. These hashes/identities are NOT crypto evidence.
fn seed(c: &Connection, op: u8, source: u8) {
    seed_with_binding(c, op, source, [6; 32]);
}
pub(super) fn seed_with_binding(c: &Connection, op: u8, source: u8, binding: [u8; 32]) {
    c.execute_batch("BEGIN IMMEDIATE").unwrap();
    c.execute("INSERT INTO authority(scope,principal,target,epoch,expiry) SELECT ?1,?2,?3,epoch,2000000000000 FROM runtime",
        params![scope(),id(9),id(op+1)]).unwrap();
    c.execute(
        "INSERT INTO source VALUES(?1,?2,1,1) ON CONFLICT DO NOTHING",
        params![scope(), id(source)],
    )
    .unwrap();
    c.execute(
        "INSERT INTO evidence VALUES(?1,?2,1,?3,1,1)",
        params![scope(), id(op + 5), id(source)],
    )
    .unwrap();
    c.execute("INSERT INTO write_intent VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,1,NULL,?10,(SELECT epoch FROM runtime),?11,'Pending')",
        params![id(op),scope(),id(9),id(op+6),id(op+1),id(op+2),id(op+3),id(op+4),id(op+5),id(3),binding]).unwrap();
    c.execute(
        "UPDATE write_intent SET state='Staged' WHERE operation=?1",
        [id(op)],
    )
    .unwrap();
    c.execute(
        "UPDATE write_intent SET state='Sealing' WHERE operation=?1",
        [id(op)],
    )
    .unwrap();
    c.execute(
        "INSERT INTO publication VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            id(op),
            id32(6),
            id32(7),
            vec![8u8; 12],
            vec![9u8; 12],
            b"SCHEMA-ONLY-NO-CONTENT".as_slice()
        ],
    )
    .unwrap();
    c.execute(
        "UPDATE write_intent SET state='Published' WHERE operation=?1",
        [id(op)],
    )
    .unwrap();
    c.execute(
        "INSERT INTO security_event(scope,kind) VALUES(?1,'Write')",
        [scope()],
    )
    .unwrap();
    let event = c.last_insert_rowid();
    c.execute(
        "UPDATE security_event SET state='EffectiveFence' WHERE id=?1",
        [event],
    )
    .unwrap();
    c.execute(
        "INSERT INTO memory VALUES(?1,?2,?3,'active')",
        params![scope(), id(op + 1), id(op + 2)],
    )
    .unwrap();
    c.execute(
        "INSERT INTO revision VALUES(?1,?2,?3,1,?4)",
        params![scope(), id(op + 1), id(op + 2), id(op + 3)],
    )
    .unwrap();
    c.execute(
        "INSERT INTO local_key VALUES(?1,?2,?3,1,1,0)",
        params![scope(), id(op + 3), id(op + 4)],
    )
    .unwrap();
    c.execute(
        "INSERT INTO payload VALUES(?1,?2,?3,?4,?5,1,?6)",
        params![
            scope(),
            id(op + 1),
            id(op + 2),
            id(op + 3),
            id(op + 4),
            id(op)
        ],
    )
    .unwrap();
    c.execute(
        "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,1)",
        params![scope(), id(op + 1), id(op + 2), id(op + 5)],
    )
    .unwrap();
    c.execute(
        "UPDATE runtime SET epoch=epoch+1,knowledge_seq=knowledge_seq+1",
        [],
    )
    .unwrap();
    c.execute("UPDATE authority SET epoch=(SELECT epoch FROM runtime)", [])
        .unwrap();
    c.execute(
        "INSERT INTO security_receipt(event,epoch) SELECT ?1,epoch FROM runtime",
        [event],
    )
    .unwrap();
    c.execute(
        "UPDATE security_event SET state='Applied' WHERE id=?1",
        [event],
    )
    .unwrap();
    c.execute(
        "INSERT INTO receipt SELECT ?1,?2,?3,?4,knowledge_seq,?5 FROM runtime",
        params![id(op), scope(), id(op + 1), id(op + 2), event],
    )
    .unwrap();
    c.execute("INSERT INTO outbox SELECT operation,'MemoryCreated',knowledge_seq FROM receipt WHERE operation=?1",[id(op)]).unwrap();
    c.execute(
        "UPDATE write_intent SET state='Committed' WHERE operation=?1",
        [id(op)],
    )
    .unwrap();
    c.execute_batch("COMMIT").unwrap();
}
fn id32(n: u8) -> Vec<u8> {
    vec![n; 32]
}
fn constraint(result: rusqlite::Result<usize>) {
    let error = result.unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(ErrorCode::ConstraintViolation),
        "{error}"
    );
}
fn count(c: &Connection, table: &str) -> i64 {
    c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}
fn state(c: &Connection, event: i64) -> String {
    c.query_row(
        "SELECT state FROM security_event WHERE id=?1",
        [event],
        |r| r.get(0),
    )
    .unwrap()
}
fn receipt(c: &Connection, event: i64) {
    c.execute("UPDATE runtime SET epoch=epoch+1", []).unwrap();
    c.execute("UPDATE authority SET epoch=(SELECT epoch FROM runtime)", [])
        .unwrap();
    c.execute(
        "INSERT INTO security_receipt(event,epoch) SELECT ?1,epoch FROM runtime",
        [event],
    )
    .unwrap();
}
fn members(c: &Connection, event: i64, ops: &[u8]) {
    for &op in ops {
        c.execute(
            "INSERT INTO deletion_member VALUES(?1,?2)",
            params![event, id(op)],
        )
        .unwrap();
    }
}

#[test]
fn fresh_install_has_distinct_complete_package_and_format() {
    let f = Fixture::fresh();
    attest(&f.db).unwrap();
    assert_eq!(count(&f.db, "runtime"), 0); // S1 supplies no Owner/Application.
    assert_eq!(count(&f.db, "profile12_format"), 1);
    for table in TABLES {
        for action in ["UPDATE", "DELETE"] {
            if *table == "deletion_work" && action == "UPDATE" {
                continue;
            }
            assert!(
                definitions(&f.db)
                    .unwrap()
                    .iter()
                    .any(|d| d.1 == format!("immutable_p12_{table}_{action}"))
            );
        }
    }
    println!(
        "P12_SQLITE={} PACKAGE_HASH={} FORMAT={FORMAT_TAG}",
        rusqlite::version(),
        hex(schema_hash())
    );
}

#[test]
fn fresh_only_refuses_reinstall_or_nonempty_zero_version_database() {
    let mut f = Fixture::fresh();
    let before = definitions(&f.db).unwrap();
    assert_eq!(install_fresh(&mut f.db), Err(Error::IncompatibleSchema));
    assert_eq!(definitions(&f.db).unwrap(), before);
    let mut c = Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE TABLE unrelated(id INTEGER)")
        .unwrap();
    assert_eq!(install_fresh(&mut c), Err(Error::IncompatibleSchema));
    assert_eq!(count(&c, "unrelated"), 0);
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    let mut temp_connection = Connection::open_in_memory().unwrap();
    temp_connection
        .execute_batch("CREATE TEMP TABLE keep_me(value INTEGER); INSERT INTO keep_me VALUES(42)")
        .unwrap();
    assert_eq!(
        install_fresh(&mut temp_connection),
        Err(Error::IncompatibleSchema)
    );
    assert_eq!(
        temp_connection
            .query_row("SELECT value FROM temp.keep_me", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        42
    );
}

#[test]
fn failed_fresh_install_rolls_back_all_objects_format_and_header() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("failed-fresh.sqlite");
    let mut c = Connection::open(&path).unwrap();
    assert_eq!(
        install_fresh_checked(&mut c, || Err(Error::Database)),
        Err(Error::Database)
    );
    assert!(c.is_autocommit());
    assert!(definitions(&c).unwrap().is_empty());
    assert_eq!(
        c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        c.pragma_query_value(None, "application_id", |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(c);
    let mut reopened = Connection::open(&path).unwrap();
    install_fresh(&mut reopened).unwrap();
    attest(&reopened).unwrap();
}

#[test]
fn member_composite_fk_is_immediate_and_refuses_extra_operation() {
    let f = Fixture::seeded();
    let e = f.fenced();
    f.db.execute_batch("BEGIN IMMEDIATE").unwrap();
    let error =
        f.db.execute(
            "INSERT INTO deletion_member VALUES(?1,?2)",
            params![e, id(30)],
        )
        .unwrap_err();
    assert_eq!(error.sqlite_error().unwrap().extended_code, 787); // FOREIGN KEY
    assert_eq!(count(&f.db, "deletion_member"), 0);
    f.db.execute_batch("ROLLBACK").unwrap();
    assert_eq!(state(&f.db, e), "EffectiveFence");
}

#[test]
fn composite_primary_keys_and_not_null_are_enforced() {
    let f = Fixture::seeded();
    let e = f.closing(&[10], false);
    constraint(f.db.execute(
        "INSERT INTO deletion_closure VALUES(?1,?2)",
        params![e, id(10)],
    ));
    constraint(f.db.execute("INSERT INTO deletion_closure VALUES(?1,NULL)", [e]));
    constraint(f.db.execute("INSERT INTO deletion_closure VALUES(NULL,?1)", [id(10)]));
    f.db.execute("INSERT INTO deletion_closure_seal VALUES(?1)", [e])
        .unwrap();
    constraint(f.db.execute("INSERT INTO deletion_closure_seal VALUES(NULL)", []));
    f.db.execute(
        "UPDATE security_event SET state='EffectiveFence' WHERE id=?1",
        [e],
    )
    .unwrap();
    f.db.execute_batch("BEGIN IMMEDIATE").unwrap();
    members(&f.db, e, &[10]);
    constraint(f.db.execute(
        "INSERT INTO deletion_member VALUES(?1,?2)",
        params![e, id(10)],
    ));
    constraint(f.db.execute("INSERT INTO deletion_member VALUES(?1,NULL)", [e]));
    constraint(f.db.execute("INSERT INTO deletion_member VALUES(NULL,?1)", [id(10)]));
    f.db.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn seal_requires_nonempty_matching_closing_and_is_immutable() {
    let f = Fixture::seeded();
    let e = f.closing(&[], false);
    constraint(f.db.execute("INSERT INTO deletion_closure_seal VALUES(?1)", [e]));
    f.db.execute(
        "INSERT INTO deletion_closure VALUES(?1,?2)",
        params![e, id(10)],
    )
    .unwrap();
    f.db.execute("INSERT INTO deletion_closure_seal VALUES(?1)", [e])
        .unwrap();
    constraint(f.db.execute("UPDATE deletion_closure_seal SET event=event", []));
    constraint(f.db.execute("DELETE FROM deletion_closure_seal", []));
    constraint(f.db.execute(
        "INSERT INTO deletion_closure VALUES(?1,?2)",
        params![e, id(20)],
    ));
    constraint(f.db.execute("UPDATE deletion_closure SET operation=operation", []));
    constraint(f.db.execute("DELETE FROM deletion_closure", []));
}

#[test]
fn effective_fence_requires_a_sealed_closure() {
    let f = Fixture::seeded();
    let e = f.closing(&[10], false);
    constraint(f.db.execute(
        "UPDATE security_event SET state='EffectiveFence' WHERE id=?1",
        [e],
    ));
    assert_eq!(state(&f.db, e), "Closing");
}

#[test]
fn scope_kind_phase_and_operation_guards_refuse_mismatches() {
    let f = Fixture::seeded();
    let e = f.closing(&[10], false);
    constraint(f.db.execute(
        "INSERT INTO deletion_member VALUES(?1,?2)",
        params![e, id(10)],
    ));
    constraint(f.db.execute(
        "INSERT INTO deletion_closure VALUES(?1,?2)",
        params![e, id(99)],
    ));
    f.db.execute_batch("BEGIN IMMEDIATE").unwrap();
    f.db.execute(
        "INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",
        [vec![2u8; 48]],
    )
    .unwrap();
    let other = f.db.last_insert_rowid();
    constraint(f.db.execute(
        "INSERT INTO deletion_intent VALUES(?1,?2,?3,?4,'Source',?5)",
        params![other, scope(), id(9), id(99), id(60)],
    ));
    f.db.execute(
        "INSERT INTO deletion_intent VALUES(?1,?2,?3,?4,'Source',?5)",
        params![other, vec![2u8; 48], id(9), id(99), id(60)],
    )
    .unwrap();
    constraint(f.db.execute(
        "INSERT INTO deletion_closure VALUES(?1,?2)",
        params![other, id(10)],
    ));
    f.db.execute_batch("ROLLBACK").unwrap();
    f.db.execute(
        "INSERT INTO security_event(scope,kind) VALUES(?1,'Write')",
        [scope()],
    )
    .unwrap();
    let write_event = f.db.last_insert_rowid();
    constraint(f.db.execute(
        "INSERT INTO deletion_intent VALUES(?1,?2,?3,?4,'Source',?5)",
        params![write_event, scope(), id(9), id(98), id(60)],
    ));
}

#[test]
fn deferred_member_receipt_fk_fails_commit_and_requires_explicit_rollback() {
    let f = Fixture::seeded();
    let e = f.fenced();
    f.db.execute_batch("BEGIN IMMEDIATE").unwrap();
    members(&f.db, e, &[10, 20]);
    assert_eq!(count(&f.db, "deletion_member"), 2); // INSERT allowed before receipt.
    assert!(f.db.execute_batch("COMMIT").is_err());
    assert!(!f.db.is_autocommit()); // failed COMMIT is NOT an automatic rollback.
    f.db.execute_batch("ROLLBACK").unwrap();
    assert!(f.db.is_autocommit());
    assert_eq!(count(&f.db, "deletion_member"), 0);
    assert_eq!(state(&f.db, e), "EffectiveFence");
    attest(&f.db).unwrap();
}

#[test]
fn exact_set_applied_commit_satisfies_all_deferred_constraints() {
    let f = Fixture::seeded();
    let e = f.fenced();
    f.db.execute_batch("BEGIN IMMEDIATE").unwrap();
    members(&f.db, e, &[10, 20]);
    receipt(&f.db, e);
    f.db.execute("UPDATE security_event SET state='Applied' WHERE id=?1", [e])
        .unwrap();
    f.db.execute_batch("COMMIT").unwrap();
    assert_eq!(state(&f.db, e), "Applied");
    assert_eq!(count(&f.db, "deletion_member"), 2);
    constraint(f.db.execute("UPDATE deletion_member SET operation=operation", []));
    constraint(f.db.execute("DELETE FROM deletion_member", []));
    assert_eq!(count(&f.db, "receipt"), 3);
    assert_eq!(count(&f.db, "outbox"), 3);
    attest(&f.db).unwrap();
}

#[test]
fn applied_guard_rejects_missing_member_and_entire_transaction_rolls_back() {
    let f = Fixture::seeded();
    let e = f.fenced();
    let epoch: i64 =
        f.db.query_row("SELECT epoch FROM runtime", [], |r| r.get(0))
            .unwrap();
    f.db.execute_batch("BEGIN IMMEDIATE").unwrap();
    members(&f.db, e, &[10]);
    f.db.execute(
        "INSERT INTO deletion_work VALUES(?1,?2,1,?3,NULL)",
        params![id(10), e, b"synthetic-marker-metadata-only".as_slice()],
    )
    .unwrap();
    f.db.execute(
        "INSERT INTO r5_tombstone VALUES(?1,?2,1,?3)",
        params![scope(), id(12), e],
    )
    .unwrap();
    receipt(&f.db, e);
    constraint(f.db.execute("UPDATE security_event SET state='Applied' WHERE id=?1", [e]));
    f.db.execute_batch("ROLLBACK").unwrap();
    for t in ["deletion_member", "deletion_work", "r5_tombstone"] {
        assert_eq!(count(&f.db, t), 0);
    }
    assert_eq!(count(&f.db, "security_receipt"), 3);
    assert_eq!(
        f.db.query_row("SELECT epoch FROM runtime", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        epoch
    );
    assert_eq!(
        f.db.query_row("SELECT knowledge_seq FROM runtime", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        3
    );
    assert_eq!(count(&f.db, "revision"), 3);
    assert_eq!(count(&f.db, "receipt"), 3);
    assert_eq!(count(&f.db, "outbox"), 3);
    assert_eq!(state(&f.db, e), "EffectiveFence");
    attest(&f.db).unwrap();
}

#[test]
fn applied_guard_independently_checks_both_differences_not_just_counts() {
    for ops in [&[10u8, 30][..], &[10u8, 20, 30][..]] {
        let f = Fixture::seeded();
        let e = f.fenced();
        // Isolate the trigger layer on a NEW test DB only. Formal attestation
        // rejects FK-OFF; no Application can use this test connection.
        f.db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        assert_eq!(attest(&f.db), Err(Error::IncompatibleSchema));
        f.db.execute_batch("BEGIN IMMEDIATE").unwrap();
        members(&f.db, e, ops);
        receipt(&f.db, e);
        constraint(f.db.execute("UPDATE security_event SET state='Applied' WHERE id=?1", [e]));
        f.db.execute_batch("ROLLBACK; PRAGMA foreign_keys=ON")
            .unwrap();
        assert_eq!(count(&f.db, "deletion_member"), 0);
        attest(&f.db).unwrap();
        println!("P12_TRIGGER_LAYER rejected members={ops:?}");
    }
}

#[test]
fn exact_schema_attestation_rejects_missing_replaced_and_extra_objects() {
    for change in [
        "DROP TRIGGER immutable_p12_deletion_closure_DELETE",
        "DROP TRIGGER immutable_p12_deletion_closure_DELETE; CREATE TRIGGER immutable_p12_deletion_closure_DELETE BEFORE DELETE ON deletion_closure BEGIN SELECT 1; END;",
        "CREATE TABLE unexpected(id INTEGER)",
        "CREATE INDEX unexpected_index ON deletion_member(operation)",
    ] {
        let f = Fixture::fresh();
        f.db.execute_batch(change).unwrap();
        assert_eq!(attest(&f.db), Err(Error::IncompatibleSchema));
        println!("P12_ATTEST rejected {change}");
    }
}

#[test]
fn schema_attestation_includes_table_fk_definition_and_implicit_indexes() {
    let f = Fixture::fresh();
    let definition: String =
        f.db.query_row(
            "SELECT sql FROM sqlite_schema WHERE name='deletion_member'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        definitions(&f.db)
            .unwrap()
            .iter()
            .any(|d| d.1 == "sqlite_autoindex_deletion_member_1")
    );
    f.db.execute_batch("DROP TABLE deletion_member").unwrap();
    f.db.execute_batch(&definition.replace(
        "REFERENCES deletion_closure(event,operation)",
        "REFERENCES deletion_closure(operation,event)",
    ))
    .unwrap();
    for action in ["UPDATE", "DELETE"] {
        f.db.execute_batch(&format!("CREATE TRIGGER immutable_p12_deletion_member_{action} BEFORE {action} ON deletion_member BEGIN SELECT RAISE(ABORT,'immutable P12 ledger'); END;")).unwrap();
    }
    assert_eq!(attest(&f.db), Err(Error::IncompatibleSchema));
}

#[test]
fn attestation_does_not_trust_a_database_stored_hash() {
    let f = Fixture::fresh();
    let guard: String =
        f.db.query_row(
            "SELECT sql FROM sqlite_schema WHERE name='immutable_p12_profile12_format_UPDATE'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    f.db.execute_batch("DROP TRIGGER immutable_p12_profile12_format_UPDATE")
        .unwrap();
    f.db.execute("UPDATE profile12_format SET schema_hash=?1", [id32(0)])
        .unwrap();
    f.db.execute_batch(&guard).unwrap();
    assert_eq!(attest(&f.db), Err(Error::IncompatibleSchema));
}

#[test]
fn package_hash_covers_sql_generated_constraints_and_format() {
    let generated = generated_guards();
    let expected = schema_hash();
    for (base, extension, guards) in [
        (format!("{BASE}\n"), EXTENSION.to_owned(), generated.clone()),
        (BASE.to_owned(), format!("{EXTENSION}\n"), generated.clone()),
        (
            BASE.to_owned(),
            EXTENSION.to_owned(),
            format!("{generated}\n"),
        ),
    ] {
        assert_ne!(
            hash_package(base.as_bytes(), extension.as_bytes(), guards.as_bytes()),
            expected
        );
    }
    assert_ne!(expected, Sha256::digest(BASE.as_bytes()).as_slice());
}

#[test]
fn connection_flags_temp_shadowing_and_attached_databases_are_rejected() {
    for change in [
        "PRAGMA foreign_keys=OFF",
        "PRAGMA recursive_triggers=OFF",
        "CREATE TEMP TABLE shadow(id INTEGER)",
        "ATTACH ':memory:' AS extra",
    ] {
        let f = Fixture::fresh();
        f.db.execute_batch(change).unwrap();
        assert_eq!(attest(&f.db), Err(Error::IncompatibleSchema));
    }
}

#[test]
fn p12_reopens_consistently_without_admitting_an_owner_or_content() {
    let f = Fixture::seeded();
    let path = f.path.clone();
    drop(f.db);
    let c = Connection::open(&path).unwrap();
    configure(&c).unwrap();
    attest(&c).unwrap();
    assert_eq!(count(&c, "memory"), 3);
    assert_eq!(count(&c, "revision"), 3);
    assert_eq!(count(&c, "profile12_format"), 1);
}

#[test]
fn legacy_profiles_and_unknown_versions_are_not_p12_or_migrated() {
    for version in [9, 10, 11, 13] {
        let mut c = Connection::open_in_memory().unwrap();
        configure(&c).unwrap();
        c.execute_batch(BASE).unwrap();
        if version == 11 {
            c.execute_batch(include_str!("../../migrations/0011_object_recovery.sql"))
                .unwrap();
        }
        c.pragma_update(None, "user_version", version).unwrap();
        let before = definitions(&c).unwrap();
        assert_eq!(attest(&c), Err(Error::IncompatibleSchema));
        assert_eq!(install_fresh(&mut c), Err(Error::IncompatibleSchema));
        assert_eq!(definitions(&c).unwrap(), before);
        assert_eq!(
            c.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            version
        );
    }
}

#[test]
fn frozen_legacy_sql_hashes_and_p11_generator_are_byte_exact() {
    for (name, bytes, expected) in [
        (
            "P9",
            include_bytes!("../../migrations/0009_output.sql").as_slice(),
            "a90d8fe5acaff260fa36771e1c39541480ec2cc884134ea72da7c9cad078dba8",
        ),
        (
            "P10",
            BASE.as_bytes(),
            "88678456e70aec2ec369200e6bc64caebe7c7c6d9d536ffebcd361736788a135",
        ),
        (
            "P11",
            include_bytes!("../../migrations/0011_object_recovery.sql").as_slice(),
            "13aed47a9438d90ec116c90e8abe1eddc8ec9213a40aa6a2bc99629636e1df97",
        ),
        (
            "P11 generator",
            include_bytes!("../auth/persistence/profile10/b3.rs").as_slice(),
            "823720f1c85442f5c0efe69157aa1bd790e2d66b583528baa653a096e8bc3dbd",
        ),
    ] {
        assert_eq!(hex(Sha256::digest(bytes)), expected, "{name}");
    }
}
