use std::time::Duration;
use yixu_domain::Id;
use yixu_sqlite_test_support::MetadataLab;
#[test]
fn f1_explicit_lab_retains_synthetic_bootstrap_only() {
    let d = tempfile::tempdir().unwrap();
    let mut lab =
        MetadataLab::open(&d.path().join("f1.sqlite"), Duration::from_millis(100)).unwrap();
    lab.create_tenant(Id::from_bytes([1; 16])).unwrap();
    assert_eq!(
        yixu_application::NoContentRuntime::default().write_metadata(None),
        Err(yixu_domain::Error::CapabilityUnavailable)
    );
}
#[test]
fn f2_missing_admission_trigger_is_rejected() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("f2.sqlite");
    drop(MetadataLab::open(&p, Duration::from_millis(100)).unwrap());
    let c = rusqlite::Connection::open(&p).unwrap();
    c.execute_batch("DROP TRIGGER payload_no_admission")
        .unwrap();
    drop(c);
    assert!(
        MetadataLab::open(&p, Duration::from_millis(100)).is_err(),
        "unchanged checksum hid dropped trigger"
    );
}

type SchemaRow = (String, String, String, Option<String>);
fn snapshot(c: &rusqlite::Connection) -> Vec<SchemaRow> {
    c.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}
fn drift(sql: &str) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("drift.sqlite");
    drop(MetadataLab::open(&p, Duration::from_millis(100)).unwrap());
    let c = rusqlite::Connection::open(&p).unwrap();
    let before: String = c
        .query_row("SELECT checksum FROM migration_record", [], |r| r.get(0))
        .unwrap();
    c.execute_batch(sql).unwrap();
    let after: String = c
        .query_row("SELECT checksum FROM migration_record", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, after);
    let changed_schema = snapshot(&c);
    drop(c);
    assert!(matches!(
        MetadataLab::open(&p, Duration::from_millis(100)),
        Err(yixu_domain::Error::IncompatibleSchema)
    ));
    let c = rusqlite::Connection::open(&p).unwrap();
    assert_eq!(
        snapshot(&c),
        changed_schema,
        "startup must not repair drift"
    );
    assert_eq!(
        before,
        c.query_row::<String, _, _>("SELECT checksum FROM migration_record", [], |r| r.get(0))
            .unwrap()
    );
    assert_eq!(
        c.query_row::<i64, _, _>("SELECT count(*) FROM tenant", [], |r| r.get(0))
            .unwrap(),
        0
    );
}
#[test]
fn weakened_trigger_same_name_is_rejected_without_repair() {
    drift(
        "DROP TRIGGER payload_no_admission; CREATE TRIGGER payload_no_admission BEFORE INSERT ON payload WHEN 0 BEGIN SELECT RAISE(ABORT,'content capability unavailable'); END;",
    );
}
#[test]
fn deleted_named_index_is_rejected() {
    drift("DROP INDEX revision_evidence_reverse;");
}
#[test]
fn missing_implicit_unique_index_is_rejected() {
    // SQLite rightly forbids DROP INDEX on a UNIQUE autoindex. Controlled offline
    // corruption simulates loss of the index while preserving migration history.
    drift(
        "PRAGMA writable_schema=ON; DELETE FROM sqlite_schema WHERE type='index' AND name=(SELECT name FROM pragma_index_list('payload') WHERE origin='u' AND name IN (SELECT name FROM sqlite_schema WHERE type='index') LIMIT 1); PRAGMA writable_schema=OFF;",
    );
}
#[test]
fn weakened_same_space_foreign_key_is_rejected() {
    drift(
        "PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql=replace(sql,'FOREIGN KEY(tenant_id,space_id,event_id) REFERENCES event(tenant_id,space_id,id)','FOREIGN KEY(tenant_id,event_id) REFERENCES event(tenant_id,id)') WHERE name='evidence'; PRAGMA writable_schema=OFF;",
    );
}
#[test]
fn weakened_unique_definition_is_rejected() {
    drift(
        "PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql=replace(sql,'UNIQUE(key_ref),','UNIQUE(key_ref,id),') WHERE name='payload'; PRAGMA writable_schema=OFF;",
    );
}
#[test]
fn additional_persistent_object_is_rejected() {
    drift("CREATE TABLE unexpected(n INTEGER) STRICT;");
}
#[test]
fn replaced_object_type_is_rejected() {
    drift("DROP INDEX outbox_claim; CREATE VIEW outbox_claim AS SELECT id FROM tenant;");
}
#[test]
fn weakened_parent_null_guard_is_rejected() {
    drift(
        "PRAGMA writable_schema=ON; UPDATE sqlite_schema SET sql=replace(sql,'AND parent_number IS NOT NULL ','') WHERE name='revision'; PRAGMA writable_schema=OFF;",
    );
}
#[test]
fn valid_database_reopens_with_foreign_keys_and_integrity() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("valid.sqlite");
    for _ in 0..3 {
        let lab = MetadataLab::open(&p, Duration::from_millis(100)).unwrap();
        assert!(lab.foreign_keys_enabled().unwrap());
        lab.integrity_check().unwrap();
    }
    println!(
        "SQLite {} on synthetic C NTFS temporary DB",
        rusqlite::version()
    );
}
#[test]
fn harmless_sql_formatting_and_analyze_are_accepted() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("format.sqlite");
    drop(MetadataLab::open(&p, Duration::from_millis(100)).unwrap());
    let c = rusqlite::Connection::open(&p).unwrap();
    c.execute_batch("DROP TRIGGER payload_no_admission; create trigger payload_no_admission /* harmless */ before insert on payload when NEW.state <> 'blocked' begin select RAISE(ABORT,'content capability unavailable'); end; ANALYZE;").unwrap();
    drop(c);
    MetadataLab::open(&p, Duration::from_millis(100))
        .unwrap()
        .integrity_check()
        .unwrap();
}
#[test]
fn literal_changes_are_not_normalized_away() {
    drift(
        "DROP TRIGGER payload_no_admission; CREATE TRIGGER payload_no_admission BEFORE INSERT ON payload WHEN NEW.state<>'BLOCKED' BEGIN SELECT RAISE(ABORT,'content capability unavailable'); END;",
    );
}
