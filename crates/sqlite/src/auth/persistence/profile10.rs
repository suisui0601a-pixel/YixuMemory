//! B1 private Windows in-process Application assembly. No public or IPC body interface.
use super::{Error, LocalVault, LockedBuffer, OwnerKind, PayloadBinding, Result, SecurityScope};
use super::{content_crypto, raw, sql_error, winfs};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::{fs::File, path::Path};

#[path = "../../../../application/src/internal_content.rs"]
mod application;
#[path = "../../../../ipc/src/tests/transaction_gates.rs"]
mod gates;
mod read;
mod recovery;
mod tests;
mod write;

type Id = [u8; 16];
const DDL: &str = include_str!("../../../migrations/0010_local_content.sql");
const APP_ID: i64 = 1_498_952_246;

fn fresh() -> Result<Id> {
    Ok(raw(super::fresh()?))
}
fn trusted_now() -> Result<i64> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::CapabilityUnavailable)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| Error::CapabilityUnavailable)
}
fn storage_identity(
    anchor: &winfs::Anchor,
    files: &winfs::Anchor,
    vault: &LocalVault,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for directory in [anchor, files, &vault.anchor] {
        for part in directory
            .directory_identity()
            .map_err(|_| Error::Integrity)?
        {
            bytes.extend(part.to_be_bytes());
        }
    }
    Ok(bytes)
}
fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn check_layout(anchor: &winfs::Anchor, fresh: bool) -> Result<()> {
    for name in anchor.names().map_err(|_| Error::Integrity)? {
        let accepted = matches!(
            name.as_str(),
            "broker-lock" | "payloads" | "keys" | "temp" | "control"
        ) || (!fresh
            && matches!(
                name.as_str(),
                "content.sqlite" | "content.sqlite-wal" | "content.sqlite-shm"
            ));
        if !accepted {
            return Err(Error::Integrity);
        }
    }
    Ok(())
}
fn filename(id: Id) -> String {
    yixu_domain::Id::from_bytes(id).to_string()
}
fn identity(anchor: &winfs::Anchor, name: &str) -> Result<Vec<u8>> {
    Ok(anchor
        .object_identity(name)
        .map_err(|e| {
            if e.raw_os_error() == Some(2) {
                Error::ResourceUnavailable
            } else {
                Error::Integrity
            }
        })?
        .iter()
        .flat_map(|part| part.to_be_bytes())
        .collect())
}
fn sql<T>(result: rusqlite::Result<T>) -> Result<T> {
    result.map_err(sql_error)
}

#[derive(Clone, Copy)]
struct Installation {
    id: Id,
    realm: Id,
}
#[derive(Clone, Copy)]
struct Identity {
    scope: SecurityScope,
    principal: Id,
}
impl Identity {
    fn scope_bytes(self) -> Vec<u8> {
        [self.scope.realm, self.scope.tenant, self.scope.space].concat()
    }
}

/// Only the trusted internal host provisions grants and source/evidence dependencies.
struct Provision {
    identity: Identity,
    memory: Id,
    source: Id,
    evidence: Id,
    expiry: i64,
}

#[derive(Clone, Copy)]
struct WriteRequest {
    request: Id,
    memory: Id,
    revision: Id,
    evidence: Id,
    evidence_generation: i64,
    generation: i64,
    expected_current: Option<Id>,
}

#[derive(Clone)]
struct Plan {
    operation: Id,
    identity: Identity,
    request: WriteRequest,
    payload: Id,
    key: Id,
    incarnation: Id,
    epoch: i64,
    dispatch: [u8; 32],
}
impl Plan {
    fn expected_dispatch(&self, db: &Connection, installation: Installation) -> Result<[u8; 32]> {
        let scope = self.identity.scope_bytes();
        let (source, source_generation, valid): (Id, i64, bool) = sql(db.query_row(
            "SELECT e.source,e.source_generation,e.valid=1 AND s.valid=1 FROM evidence e
             JOIN source s ON s.scope=e.scope AND s.id=e.source AND s.generation=e.source_generation
             WHERE e.scope=?1 AND e.id=?2 AND e.generation=?3",
            params![
                scope,
                self.request.evidence,
                self.request.evidence_generation
            ],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        ))?;
        let mut dependencies = vec![
            (self.request.memory, 1),
            (self.request.revision, 1),
            (self.payload, self.request.generation),
            (self.key, self.request.generation),
            (self.request.evidence, self.request.evidence_generation),
            (source, source_generation),
        ];
        dependencies.sort();
        let mut nodes = Vec::new();
        for (id, generation) in dependencies {
            let retired:bool=sql(db.query_row(
                "SELECT EXISTS(SELECT 1 FROM r5_tombstone WHERE scope=?1 AND object=?2 AND generation=?3)",
                params![scope,id,generation],|r|r.get(0),
            ))?;
            nodes.push((id, generation, retired || !valid));
        }
        let graph = gates::live_graph_binding(self.request.revision, nodes)
            .ok_or(Error::AuthorizationDenied)?;
        let mut bytes = b"YIXU/P10/R4/TRUSTED-INPROCESS-WRITE/v1\0".to_vec();
        for field in [
            installation.id,
            self.incarnation,
            self.identity.scope.realm,
            self.identity.scope.tenant,
            self.identity.scope.space,
            self.identity.principal,
            self.request.request,
            self.request.memory,
            self.request.revision,
            self.request.evidence,
            self.operation,
            self.payload,
            self.key,
        ] {
            bytes.extend(field);
        }
        for field in [
            self.epoch,
            self.request.generation,
            self.request.evidence_generation,
        ] {
            bytes.extend(field.to_be_bytes());
        }
        bytes.extend(graph);
        Ok(digest(&bytes))
    }
    fn binding(&self, installation: Installation) -> PayloadBinding {
        let mut context = b"YIXU/P10/UTF8/FORMAT-1/INSTALLATION/MEMORY\0".to_vec();
        context.extend(installation.id);
        context.extend(self.request.memory);
        PayloadBinding {
            scope: self.identity.scope,
            owner_kind: OwnerKind::Revision,
            owner: self.request.revision,
            payload: self.payload,
            key: self.key,
            seal_generation: self.request.generation as u64,
            vault_incarnation: 1,
            protection: digest(&context),
        }
    }
}
#[derive(PartialEq, Eq)]
struct Publication {
    cipher_hash: [u8; 32],
    wrapper_hash: [u8; 32],
    cipher_identity: Vec<u8>,
    wrapper_identity: Vec<u8>,
}
#[derive(Debug, Eq, PartialEq)]
struct WriteReceipt {
    operation: Id,
    memory: Id,
    revision: Id,
    knowledge_seq: i64,
}

struct Engine {
    db: Connection,
    installation: Installation,
    incarnation: Id,
    files: winfs::Anchor,
    vault: LocalVault,
    _lock: File,
    before_commit: Option<Box<dyn FnOnce() -> Result<()>>>,
    recovery_only: bool,
}

fn configure(db: &Connection) -> Result<()> {
    sql(db.busy_timeout(std::time::Duration::from_secs(2)))?;
    sql(db.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;
         PRAGMA recursive_triggers=ON; PRAGMA temp_store=MEMORY;",
    ))
}
fn attest(db: &Connection) -> Result<()> {
    type Definition = (String, String, String, Option<String>);
    fn definitions(db: &Connection) -> Result<Vec<Definition>> {
        let mut stmt =
            sql(db.prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name"))?;
        sql(sql(stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        }))?
        .collect())
    }
    let expected = sql(Connection::open_in_memory())?;
    install_schema(&expected)?;
    if definitions(db)? != definitions(&expected)?
        || sql(db.query_row("PRAGMA application_id", [], |r| r.get::<_, i64>(0)))? != APP_ID
        || sql(db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)))? != 10
        || sql(db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0)))? != "ok"
        || sql(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
            [],
            |r| r.get::<_, bool>(0),
        ))?
    {
        return Err(Error::IncompatibleSchema);
    }
    Ok(())
}
fn install_schema(db: &Connection) -> Result<()> {
    sql(db.execute_batch(DDL))
}

impl Engine {
    fn create(root: &Path, installation: Installation, grants: &[Provision]) -> Result<Self> {
        Self::create_supervised(root, installation, grants, fresh()?)
    }
    fn create_supervised(
        root: &Path,
        installation: Installation,
        grants: &[Provision],
        owner_nonce: Id,
    ) -> Result<Self> {
        if installation.id == [0; 16] || installation.realm == [0; 16] || grants.is_empty() {
            return Err(Error::InvalidInput);
        }
        let anchor = winfs::Anchor::open(root).map_err(|_| Error::CapabilityUnavailable)?;
        let lock = anchor.broker_lock().map_err(|_| Error::Busy)?;
        check_layout(&anchor, true)?;
        let files = winfs::Anchor::open(&root.join("payloads"))
            .map_err(|_| Error::CapabilityUnavailable)?;
        let vault = LocalVault::new(root.join("keys"))?;
        if !files.names().map_err(|_| Error::Integrity)?.is_empty()
            || !vault
                .anchor
                .names()
                .map_err(|_| Error::Integrity)?
                .is_empty()
        {
            return Err(Error::Integrity);
        }
        let store_identity = storage_identity(&anchor, &files, &vault)?;
        // SQLite's own VFS remains the metadata boundary. Cipher/key I/O only uses W1.
        let path = root.join("content.sqlite");
        File::create_new(&path).map_err(|_| Error::StateConflict)?;
        let mut db = sql(Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        ))?;
        configure(&db)?;
        sql(db.execute_batch("PRAGMA journal_mode=WAL"))?;
        let incarnation = fresh()?;
        let tx = sql(db.transaction_with_behavior(TransactionBehavior::Immediate))?;
        install_schema(&tx)?;
        sql(tx.execute(
            "INSERT INTO runtime VALUES(1,?1,?2,?3,?4,?5,?6,7,0,0,0)",
            params![
                installation.id,
                installation.realm,
                incarnation,
                owner_nonce,
                store_identity,
                digest(DDL.as_bytes())
            ],
        ))?;
        for grant in grants {
            if grant.identity.scope.realm != installation.realm
                || grant.identity.principal == [0; 16]
                || grant.memory == [0; 16]
                || grant.source == [0; 16]
                || grant.evidence == [0; 16]
            {
                return Err(Error::InvalidInput);
            }
            let scope = grant.identity.scope_bytes();
            sql(tx.execute(
                "INSERT INTO authority(scope,principal,target,epoch,expiry) VALUES(?1,?2,?3,7,?4)",
                params![scope, grant.identity.principal, grant.memory, grant.expiry],
            ))?;
            sql(tx.execute(
                "INSERT INTO source VALUES(?1,?2,1,1) ON CONFLICT DO NOTHING",
                params![scope, grant.source],
            ))?;
            sql(tx.execute(
                "INSERT INTO evidence VALUES(?1,?2,1,?3,1,1) ON CONFLICT DO NOTHING",
                params![scope, grant.evidence, grant.source],
            ))?;
        }
        sql(tx.commit())?;
        attest(&db)?;
        Ok(Self {
            db,
            installation,
            incarnation,
            files,
            vault,
            _lock: lock,
            before_commit: None,
            recovery_only: false,
        })
    }

    fn authorize(
        db: &Connection,
        identity: Identity,
        request: WriteRequest,
        installation: Installation,
        incarnation: Id,
        _fixture_time: i64,
    ) -> Result<i64> {
        let now = trusted_now()?;
        let (actual_install, realm, owner, epoch, blocked, clean): (Id, Id, Id, i64, bool, bool) =
            sql(db.query_row(
                "SELECT installation,realm,incarnation,epoch,blocked,clean FROM runtime WHERE id=1",
                [],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                    ))
                },
            ))?;
        if actual_install != installation.id
            || realm != installation.realm
            || identity.scope.realm != realm
            || owner != incarnation
            || blocked
            || clean
            || sql(gates::closing(db))?
        {
            return Err(Error::CapabilityUnavailable);
        }
        let snapshot = sql(gates::authorized_snapshot(
            db,
            &identity.scope_bytes(),
            &identity.principal,
            &request.memory,
            epoch,
            now,
        ))?
        .ok_or(Error::AuthorizationDenied)?;
        if request.generation != snapshot[4] || request.evidence_generation != snapshot[3] {
            return Err(Error::AuthorizationDenied);
        }
        let valid: bool = sql(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM evidence e JOIN source s
             ON s.scope=e.scope AND s.id=e.source AND s.generation=e.source_generation
             WHERE e.scope=?1 AND e.id=?2 AND e.generation=?3 AND e.valid=1 AND s.valid=1)",
            params![
                identity.scope_bytes(),
                request.evidence,
                request.evidence_generation
            ],
            |r| r.get(0),
        ))?;
        if !valid {
            return Err(Error::AuthorizationDenied);
        }
        Ok(epoch)
    }

    fn close_grant(&mut self, identity: Identity) -> Result<i64> {
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",
            [identity.scope_bytes()],
        ))?;
        let event = tx.last_insert_rowid();
        sql(tx.commit())?;
        Ok(event)
    }
    fn apply_revocation(&mut self, event: i64, identity: Identity) -> Result<()> {
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let pending: bool = sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM write_intent WHERE state<>'Committed')",
            [],
            |r| r.get(0),
        ))?;
        let matching: bool = sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM security_event WHERE id=?1 AND scope=?2 AND kind='Revocation')",
            params![event,identity.scope_bytes()], |r| r.get(0),
        ))?;
        if pending || !matching || sql(gates::make_effective(&tx, event))? != 1 {
            return Err(Error::CapabilityUnavailable);
        }
        sql(tx.execute(
            "UPDATE authority SET grant_ok=0 WHERE scope=?1 AND principal=?2",
            params![identity.scope_bytes(), identity.principal],
        ))?;
        sql(tx.execute("UPDATE runtime SET epoch=epoch+1", []))?;
        sql(tx.execute("UPDATE authority SET epoch=(SELECT epoch FROM runtime)", []))?;
        sql(tx.execute(
            "INSERT INTO security_receipt(event,epoch) SELECT ?1,epoch FROM runtime",
            [event],
        ))?;
        sql(tx.execute(
            "UPDATE security_event SET state='Applied' WHERE id=?1",
            [event],
        ))?;
        sql(tx.commit())
    }
}

/// Private Application owns the trusted identity; WriteRequest contains no authority issuer.
struct Application {
    engine: Engine,
    identity: Identity,
}
impl Application {
    fn create_memory(&mut self, request: WriteRequest, body: &str) -> Result<WriteReceipt> {
        if request.expected_current.is_some() {
            return Err(Error::InvalidInput);
        }
        application::InternalContentApplication::new(self).create_memory(request, body)
    }
}
impl application::ContentWrite for Application {
    type Request = WriteRequest;
    type Receipt = WriteReceipt;
    type Error = Error;
    fn persist_utf8(&mut self, request: WriteRequest, body: &str) -> Result<WriteReceipt> {
        self.engine
            .write(self.identity, request, body, trusted_now()?, |_, _| Ok(()))
    }
}
