//! Real SQLite infrastructure for an isolated metadata laboratory, NOT a production repository.
//! The standard VFS is used only on synthetic temporary files. No content adapter exists.
use rusqlite::{Connection, ErrorCode, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
use yixu_domain::{Error, EvidenceEnvelope, Id, Result, RevisionEnvelope, Scope};
use yixu_ports::{MemoryRepository, MetadataTransaction, UnitOfWork};

pub const SCHEMA_VERSION: i64 = 1;
pub const APPLICATION_ID: i64 = 0x59495855;
pub const MIGRATION: &str = include_str!("../migrations/0001_foundation.sql");
pub(crate) const MIGRATION_RECORD: &str = "CREATE TABLE migration_record(version INTEGER PRIMARY KEY, checksum TEXT NOT NULL, phase TEXT NOT NULL CHECK(phase='applied')) STRICT;";
pub fn migration_checksum() -> String {
    Sha256::digest(MIGRATION.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn natural(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::Database)
}

fn db_error(error: rusqlite::Error) -> Error {
    match error {
        rusqlite::Error::SqliteFailure(e, _) => match e.code {
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => Error::Busy,
            ErrorCode::ConstraintViolation => Error::Constraint,
            _ => Error::Database,
        },
        rusqlite::Error::QueryReturnedNoRows => Error::NotFound,
        _ => Error::Database,
    }
}
fn integer(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| Error::InvalidInput)
}

pub struct MetadataLab {
    connection: Connection,
}
impl MetadataLab {
    /// No production-safe filesystem/VFS, authentication or encryption claim is made.
    pub fn open(path: &Path, busy_timeout: Duration) -> Result<Self> {
        let mut connection = Connection::open(path).map_err(db_error)?;
        connection.busy_timeout(busy_timeout).map_err(db_error)?;
        let app_id: i64 = connection
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(db_error)?;
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(db_error)?;
        let table_count: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if (app_id != 0 && app_id != APPLICATION_ID)
            || !(0..=SCHEMA_VERSION).contains(&version)
            || (app_id == 0 && (table_count != 0 || version != 0))
        {
            return Err(Error::IncompatibleSchema);
        }
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA recursive_triggers=ON; PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY;").map_err(db_error)?;
        let foreign_keys: i64 = connection
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .map_err(db_error)?;
        if foreign_keys != 1 {
            return Err(Error::Database);
        }
        let journal: String = connection
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .map_err(db_error)?;
        if journal != "wal" {
            let selected: String = connection
                .query_row("PRAGMA journal_mode=WAL", [], |r| r.get(0))
                .map_err(db_error)?;
            if selected != "wal" {
                return Err(Error::Database);
            }
        }
        Self::migrate(&mut connection)?;
        Ok(Self { connection })
    }
    fn migrate(connection: &mut Connection) -> Result<()> {
        Self::migrate_sql(connection, MIGRATION)
    }
    fn migrate_sql(connection: &mut Connection, migration_sql: &str) -> Result<()> {
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let version: i64 = tx
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(db_error)?;
        if version == 0 {
            let objects: i64 = tx
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                    [],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            if objects != 0 {
                return Err(Error::IncompatibleSchema);
            }
            tx.execute_batch(MIGRATION_RECORD).map_err(db_error)?;
            tx.execute_batch(migration_sql).map_err(db_error)?;
            tx.execute(
                "INSERT INTO migration_record VALUES(?1,?2,'applied')",
                params![SCHEMA_VERSION, migration_checksum()],
            )
            .map_err(db_error)?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(db_error)?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(db_error)?;
        } else if version != SCHEMA_VERSION {
            return Err(Error::IncompatibleSchema);
        }
        let history: Vec<(i64, String, String)> = {
            let mut statement = tx
                .prepare("SELECT version,checksum,phase FROM migration_record ORDER BY version")
                .map_err(db_error)?;
            statement
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(db_error)?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(db_error)?
        };
        if history.len() != 1 || history[0].0 != SCHEMA_VERSION || history[0].2 != "applied" {
            return Err(Error::IncompatibleSchema);
        }
        if history[0].1 != migration_checksum() {
            return Err(Error::MigrationChecksum);
        }
        crate::schema::validate(&tx)?;
        let integrity: String = tx
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(db_error)?;
        if integrity != "ok" {
            return Err(Error::Constraint);
        }
        let violations: i64 = tx
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .map_err(db_error)?;
        if violations != 0 {
            return Err(Error::Constraint);
        }
        tx.commit().map_err(db_error)
    }
    pub fn create_tenant(&mut self, tenant: Id) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        tx.execute(
            "INSERT INTO tenant VALUES(?1,'active',1,0)",
            params![tenant.to_string()],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)
    }
    pub fn knowledge_seq(&self, tenant: Id) -> Result<u64> {
        self.connection
            .query_row(
                "SELECT knowledge_seq FROM tenant WHERE id=?1",
                params![tenant.to_string()],
                |r| r.get::<_, i64>(0),
            )
            .map_err(db_error)
            .and_then(natural)
    }
    pub fn foreign_keys_enabled(&self) -> Result<bool> {
        self.connection
            .pragma_query_value(None, "foreign_keys", |r| r.get::<_, i64>(0))
            .map(|v| v == 1)
            .map_err(db_error)
    }
    pub fn integrity_check(&self) -> Result<()> {
        let integrity: String = self
            .connection
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(db_error)?;
        let violations: i64 = self
            .connection
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .map_err(db_error)?;
        if integrity == "ok" && violations == 0 {
            Ok(())
        } else {
            Err(Error::Constraint)
        }
    }
    pub fn sqlite_version(&self) -> &'static str {
        rusqlite::version()
    }
}

impl MemoryRepository for MetadataLab {
    fn memory_version(&self, scope: Scope, memory: Id) -> Result<u64> {
        self.connection
            .query_row(
                "SELECT version FROM memory WHERE tenant_id=?1 AND space_id=?2 AND id=?3",
                params![
                    scope.tenant.to_string(),
                    scope.space.to_string(),
                    memory.to_string()
                ],
                |r| r.get::<_, i64>(0),
            )
            .map_err(db_error)
            .and_then(natural)
    }
}
impl UnitOfWork for MetadataLab {
    type Transaction<'a> = MetadataTx<'a>;
    fn begin(&mut self, tenant: Id) -> Result<MetadataTx<'_>> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let count = tx.execute("UPDATE tenant SET knowledge_seq=knowledge_seq+1 WHERE id=?1 AND status='active' AND knowledge_seq<9223372036854775807",params![tenant.to_string()]).map_err(db_error)?;
        if count != 1 {
            return Err(Error::NotFound);
        }
        let seq: i64 = tx
            .query_row(
                "SELECT knowledge_seq FROM tenant WHERE id=?1",
                params![tenant.to_string()],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        let seq = natural(seq)?;
        tx.execute(
            "INSERT INTO commit_record VALUES(?1,?2,0)",
            params![tenant.to_string(), integer(seq)?],
        )
        .map_err(db_error)?;
        Ok(MetadataTx {
            tx,
            tenant,
            seq,
            failed: false,
        })
    }
}

pub struct MetadataTx<'a> {
    tx: Transaction<'a>,
    tenant: Id,
    seq: u64,
    failed: bool,
}
impl MetadataTx<'_> {
    fn scope(&mut self, scope: Scope) -> Result<()> {
        if scope.tenant == self.tenant {
            Ok(())
        } else {
            self.failed = true;
            Err(Error::Constraint)
        }
    }
    fn execute(&mut self, sql: &str, parameters: impl rusqlite::Params) -> Result<usize> {
        if self.failed {
            return Err(Error::Constraint);
        }
        match self.tx.execute(sql, parameters).map_err(db_error) {
            Ok(value) => Ok(value),
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }
    fn integer(&mut self, value: u64) -> Result<i64> {
        match integer(value) {
            Ok(n) => Ok(n),
            Err(e) => {
                self.failed = true;
                Err(e)
            }
        }
    }
    pub fn add_principal(&mut self, id: Id) -> Result<()> {
        self.execute(
            "INSERT INTO principal VALUES(?1,?2,'user','active')",
            params![self.tenant.to_string(), id.to_string()],
        )?;
        Ok(())
    }
    pub fn add_personal_space(&mut self, space: Id, owner: Id) -> Result<()> {
        self.execute(
            "INSERT INTO space VALUES(?1,?2,'personal',?3,NULL,1,0,'sensitive')",
            params![
                self.tenant.to_string(),
                space.to_string(),
                owner.to_string()
            ],
        )?;
        Ok(())
    }
    pub fn append_system_event(&mut self, space: Id, event: Id, actor: Id) -> Result<()> {
        let sequence = self.integer(self.seq)?;
        self.execute(
            "INSERT INTO event VALUES(?1,?2,?3,'system_action',NULL,?4,0,0,?5,'1.0')",
            params![
                self.tenant.to_string(),
                space.to_string(),
                event.to_string(),
                actor.to_string(),
                sequence
            ],
        )?;
        Ok(())
    }
    pub fn add_evidence_envelope(&mut self, value: EvidenceEnvelope) -> Result<()> {
        self.scope(value.scope)?;
        self.execute(
            "INSERT INTO evidence VALUES(?1,?2,?3,?4,?5,?6,'user_statement',0)",
            params![
                self.tenant.to_string(),
                value.scope.space.to_string(),
                value.id.to_string(),
                value.event.to_string(),
                value.payload.to_string(),
                value.locator.to_string()
            ],
        )?;
        Ok(())
    }
    /// Blocked envelope only: not an available memory and not an admission of content.
    pub fn add_blocked_memory_envelope(
        &mut self,
        space: Id,
        memory: Id,
        current: Id,
    ) -> Result<()> {
        self.execute(
            "INSERT INTO memory VALUES(?1,?2,?3,?4,1,'source_blocked',0)",
            params![
                self.tenant.to_string(),
                space.to_string(),
                memory.to_string(),
                current.to_string()
            ],
        )?;
        Ok(())
    }
    pub fn add_revision_envelope(&mut self, value: RevisionEnvelope) -> Result<()> {
        let sequence = self.integer(self.seq)?;
        self.scope(value.scope)?;
        let number = self.integer(value.number)?;
        let parent_number = if number > 1 { Some(number - 1) } else { None };
        self.execute("INSERT INTO revision VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'user_assertion',?10,?11,0,?12)",params![self.tenant.to_string(),value.scope.space.to_string(),value.id.to_string(),value.memory.to_string(),number,value.parent.map(|v|v.to_string()),parent_number,value.primary_evidence.to_string(),value.content_payload.to_string(),value.valid_from_ms,value.valid_to_ms,sequence])?;
        Ok(())
    }
    pub fn link_evidence(&mut self, space: Id, revision: Id, evidence: Id) -> Result<()> {
        self.execute(
            "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,'supports')",
            params![
                self.tenant.to_string(),
                space.to_string(),
                revision.to_string(),
                evidence.to_string()
            ],
        )?;
        Ok(())
    }
    pub fn cas_current(&mut self, space: Id, memory: Id, expected: u64, current: Id) -> Result<()> {
        let expected = self.integer(expected)?;
        let changed=self.execute("UPDATE memory SET version=version+1,current_revision_id=?4 WHERE tenant_id=?1 AND space_id=?2 AND id=?3 AND version=?5 AND state='source_blocked'",params![self.tenant.to_string(),space.to_string(),memory.to_string(),current.to_string(),expected])?;
        if changed == 1 {
            Ok(())
        } else {
            self.failed = true;
            Err(Error::CasConflict)
        }
    }
    pub fn enqueue_metadata(
        &mut self,
        space: Id,
        id: Id,
        memory: Id,
        generation: u64,
    ) -> Result<()> {
        let generation = self.integer(generation)?;
        self.execute("INSERT INTO outbox_job VALUES(?1,?2,?3,'metadata_changed',?4,?4,NULL,?5,'pending',NULL,0,0,0)",params![self.tenant.to_string(),space.to_string(),id.to_string(),memory.to_string(),generation])?;
        Ok(())
    }
}
impl MetadataTransaction for MetadataTx<'_> {
    fn knowledge_seq(&self) -> u64 {
        self.seq
    }
    fn commit(self) -> Result<()> {
        if self.failed {
            return Err(Error::Constraint);
        }
        // This verifies the canonical current pointer at the actual UoW commit boundary.
        let stale: i64=self.tx.query_row("SELECT count(*) FROM memory m JOIN revision r ON r.tenant_id=m.tenant_id AND r.space_id=m.space_id AND r.memory_id=m.id AND r.id=m.current_revision_id WHERE m.tenant_id=?1 AND r.number<>(SELECT max(x.number) FROM revision x WHERE x.tenant_id=m.tenant_id AND x.memory_id=m.id)",params![self.tenant.to_string()],|r|r.get(0)).map_err(db_error)?;
        if stale != 0 {
            return Err(Error::Constraint);
        }
        // rusqlite's consuming commit drops and rolls back on deferred-FK failure.
        self.tx.commit().map_err(db_error)
    }
}

#[cfg(test)]
mod migration_faults {
    use super::*;
    #[test]
    fn actual_migrator_rolls_back_schema_and_record_on_ddl_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("failed-migration.sqlite");
        let mut c = Connection::open(&path).unwrap();
        let invalid = "CREATE TABLE partial(n INTEGER); INVALID SQL;";
        assert_eq!(
            MetadataLab::migrate_sql(&mut c, invalid),
            Err(Error::Database)
        );
        let count: i64 = c
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        drop(c);
        MetadataLab::open(&path, Duration::from_millis(50))
            .unwrap()
            .integrity_check()
            .unwrap();
    }
    #[test]
    fn actual_sqlite_full_rolls_back_sequence_and_recovers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("full.sqlite");
        let mut lab = MetadataLab::open(&path, Duration::from_millis(50)).unwrap();
        let tenant = Id::from_bytes([1; 16]);
        lab.create_tenant(tenant).unwrap();
        let pages: i64 = lab
            .connection
            .pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
        lab.connection
            .pragma_update(None, "max_page_count", pages)
            .unwrap();
        let mut tx = lab.begin(tenant).unwrap();
        let mut hit_full = false;
        for n in 1_u64..10000 {
            let mut b = [0; 16];
            b[8..].copy_from_slice(&n.to_be_bytes());
            if tx.add_principal(Id::from_bytes(b)) == Err(Error::Database) {
                hit_full = true;
                break;
            }
        }
        assert!(hit_full);
        assert_eq!(tx.commit(), Err(Error::Constraint));
        assert_eq!(lab.knowledge_seq(tenant).unwrap(), 0);
        let count: i64 = lab
            .connection
            .query_row("SELECT count(*) FROM principal", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        lab.connection
            .pragma_update(None, "max_page_count", 100000)
            .unwrap();
        let mut tx = lab.begin(tenant).unwrap();
        tx.add_principal(Id::from_bytes([2; 16])).unwrap();
        tx.commit().unwrap();
        lab.integrity_check().unwrap();
    }
}
