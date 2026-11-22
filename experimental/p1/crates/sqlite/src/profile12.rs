//! S1-only fresh schema package; private test assembly, not an Application or runtime.
use rusqlite::{Connection, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use yixu_domain::{Error, Result};

const BASE: &str = include_str!("../migrations/0010_local_content.sql");
const EXTENSION: &str = include_str!("../migrations/0012_deletion_schema.sql");
const PROFILE: i64 = 12;
const APPLICATION_ID: i64 = 1_498_952_246;
const CONTENT_FORMAT: i64 = 1;
const FORMAT_TAG: &str = "YIXU/P12/SCHEMA/v1";
const DEFINITION_ENCODING: &[u8] = b"exact-sqlite-schema-tuples/v1";
const TABLES: &[&str] = &[
    "object_quarantine",
    "deletion_intent",
    "deletion_closure",
    "deletion_closure_seal",
    "deletion_member",
    "copy_manifest_b3",
    "backup_registration",
    "backup_receipt",
    "deletion_work",
    "deletion_observation",
    "deletion_receipt",
    "profile12_format",
];

fn db<T>(result: rusqlite::Result<T>) -> Result<T> {
    result.map_err(|_| Error::IncompatibleSchema)
}

fn generated_guards() -> String {
    let mut sql = String::new();
    for table in TABLES {
        for action in ["UPDATE", "DELETE"] {
            if *table == "deletion_work" && action == "UPDATE" {
                continue;
            }
            sql.push_str(&format!(
                "CREATE TRIGGER immutable_p12_{table}_{action} BEFORE {action} ON {table}\n\
                 BEGIN SELECT RAISE(ABORT,'immutable P12 ledger'); END;\n"
            ));
        }
    }
    sql
}

fn hash_package(base: &[u8], extension: &[u8], generated: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"YIXU/P12/SCHEMA-PACKAGE/v1\0");
    for part in [
        base,
        extension,
        generated,
        &PROFILE.to_be_bytes(),
        &APPLICATION_ID.to_be_bytes(),
        &CONTENT_FORMAT.to_be_bytes(),
        FORMAT_TAG.as_bytes(),
        DEFINITION_ENCODING,
    ] {
        hash.update((part.len() as u64).to_be_bytes());
        hash.update(part);
    }
    hash.finalize().into()
}

fn schema_hash() -> [u8; 32] {
    hash_package(
        BASE.as_bytes(),
        EXTENSION.as_bytes(),
        generated_guards().as_bytes(),
    )
}

fn configure(connection: &Connection) -> Result<()> {
    if !connection.is_autocommit() {
        return Err(Error::IncompatibleSchema);
    }
    db(connection.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA recursive_triggers=ON; PRAGMA temp_store=MEMORY;",
    ))?;
    configured(connection)
}

fn configured(connection: &Connection) -> Result<()> {
    for name in ["foreign_keys", "recursive_triggers"] {
        if db(connection.pragma_query_value(None, name, |r| r.get::<_, i64>(0)))? != 1 {
            return Err(Error::IncompatibleSchema);
        }
    }
    if db(connection.query_row(
        "SELECT count(*) FROM pragma_database_list WHERE name NOT IN ('main','temp')",
        [],
        |r| r.get::<_, i64>(0),
    ))? != 0
        || db(
            connection.query_row("SELECT count(*) FROM sqlite_temp_schema", [], |r| {
                r.get::<_, i64>(0)
            }),
        )? != 0
    {
        return Err(Error::IncompatibleSchema);
    }
    Ok(())
}

fn write_schema(connection: &Connection) -> Result<()> {
    db(connection.execute_batch(BASE))?;
    db(connection.execute_batch(EXTENSION))?;
    db(connection.execute_batch(&generated_guards()))?;
    db(connection.execute(
        "INSERT INTO profile12_format VALUES(1,?1,?2,?3,?4)",
        params![
            PROFILE,
            CONTENT_FORMAT,
            FORMAT_TAG,
            schema_hash().as_slice()
        ],
    ))?;
    Ok(())
}

fn install_fresh(connection: &mut Connection) -> Result<()> {
    install_fresh_checked(connection, || Ok(()))
}

// Test-only failure checkpoint cannot replace or skip the real attestation.
fn install_fresh_checked(
    connection: &mut Connection,
    checkpoint: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if !connection.is_autocommit()
        || db(connection.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)))? != 0
        || db(connection.query_row("PRAGMA application_id", [], |r| r.get::<_, i64>(0)))? != 0
        || db(
            connection.query_row("SELECT count(*) FROM main.sqlite_schema", [], |r| {
                r.get::<_, i64>(0)
            }),
        )? != 0
    {
        return Err(Error::IncompatibleSchema);
    }
    configure(connection)?;
    let tx = db(connection.transaction_with_behavior(TransactionBehavior::Immediate))?;
    write_schema(&tx)?;
    attest(&tx)?;
    checkpoint()?;
    db(tx.commit())
}

type Definition = (String, String, String, Option<String>);
fn definitions(connection: &Connection) -> Result<Vec<Definition>> {
    let mut query = db(connection
        .prepare("SELECT type,name,tbl_name,sql FROM main.sqlite_schema ORDER BY type,name"))?;
    db(db(query.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))))?.collect())
}

fn foreign_keys_safe(connection: &Connection) -> Result<bool> {
    let mut query = db(connection.prepare("PRAGMA main.foreign_key_check"))?;
    let rows = db(query.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(2)?))))?;
    for row in rows {
        let (table, parent) = db(row)?;
        if !matches!(
            (table.as_str(), parent.as_str()),
            ("revision", "revision_evidence") | ("revision_evidence", "evidence")
        ) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn attest(connection: &Connection) -> Result<()> {
    configured(connection)?;
    if db(connection.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)))? != PROFILE
        || db(connection.query_row("PRAGMA application_id", [], |r| r.get::<_, i64>(0)))?
            != APPLICATION_ID
    {
        return Err(Error::IncompatibleSchema);
    }
    let expected = db(Connection::open_in_memory())?;
    configure(&expected)?;
    write_schema(&expected)?;
    if definitions(connection)? != definitions(&expected)?
        || db(connection.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)))?
            != "ok"
        || !foreign_keys_safe(connection)?
    {
        return Err(Error::IncompatibleSchema);
    }
    let (profile, content, tag, hash): (i64, i64, String, Vec<u8>) = db(connection.query_row(
        "SELECT profile,content_format,format_tag,schema_hash FROM main.profile12_format WHERE id=1",
        [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ))?;
    if profile != PROFILE || content != CONTENT_FORMAT || tag != FORMAT_TAG || hash != schema_hash()
    {
        return Err(Error::IncompatibleSchema);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
