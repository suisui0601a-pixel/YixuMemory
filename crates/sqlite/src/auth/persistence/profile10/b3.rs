//! Profile 11 private Windows experiment, sharing the Profile 10 Application.
use super::recovery::Reconciliation;
use super::*;
use rusqlite::types::ValueRef;
use std::collections::BTreeSet;

const EXTENSION: &str = include_str!("../../../../migrations/0011_object_recovery.sql");
const TABLES: &[&str] = &[
    "object_quarantine",
    "deletion_intent",
    "deletion_member",
    "copy_manifest_b3",
    "backup_registration",
    "backup_receipt",
    "deletion_work",
    "deletion_observation",
    "deletion_receipt",
];

pub(super) fn foreign_keys_safe(db: &Connection, version: i64) -> Result<bool> {
    let mut stmt = sql(db.prepare("SELECT \"table\",fkid FROM pragma_foreign_key_check"))?;
    let failures = sql(sql(stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    }))?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    if version == 10 {
        return Ok(failures.is_empty());
    }
    for (table, key) in failures {
        let parent: String = sql(db.query_row(
            "SELECT \"table\" FROM pragma_foreign_key_list(?1) WHERE id=?2 LIMIT 1",
            params![table, key],
            |row| row.get(0),
        ))?;
        if !matches!(
            (table.as_str(), parent.as_str()),
            ("revision", "revision_evidence") | ("revision_evidence", "evidence")
        ) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn version(db: &Connection) -> Result<i64> {
    let value = sql(db.query_row("PRAGMA user_version", [], |r| r.get(0)))?;
    if value == 10 || value == 11 {
        Ok(value)
    } else {
        Err(Error::IncompatibleSchema)
    }
}
pub(super) fn schema_hash(version: i64) -> [u8; 32] {
    let mut bytes = DDL.as_bytes().to_vec();
    if version == 11 {
        bytes.extend(EXTENSION.as_bytes());
    }
    digest(&bytes)
}
pub(super) fn install(db: &Connection) -> Result<()> {
    sql(db.execute_batch(EXTENSION))?;
    for table in TABLES {
        for action in ["UPDATE", "DELETE"] {
            if *table == "deletion_work" && action == "UPDATE" {
                continue;
            }
            sql(db.execute_batch(&format!(
                "CREATE TRIGGER immutable_b3_{table}_{action} BEFORE {action} ON {table}
                 BEGIN SELECT RAISE(ABORT,'immutable B3 ledger'); END;"
            )))?;
        }
    }
    Ok(())
}
pub(super) fn hash_ledger(db: &Connection, hash: &mut Sha256) -> Result<()> {
    for table in TABLES {
        hash.update(table.as_bytes());
        let mut stmt = sql(db.prepare(&format!("SELECT * FROM {table} ORDER BY rowid")))?;
        let cols = stmt.column_count();
        let mut rows = sql(stmt.query([]))?;
        while let Some(row) = sql(rows.next())? {
            hash.update(b"B3row");
            for column in 0..cols {
                match sql(row.get_ref(column))? {
                    ValueRef::Null => hash.update([0]),
                    ValueRef::Integer(v) => {
                        hash.update([1]);
                        hash.update(v.to_be_bytes());
                    }
                    ValueRef::Text(v) | ValueRef::Blob(v) => {
                        hash.update([2]);
                        hash.update((v.len() as u64).to_be_bytes());
                        hash.update(v);
                    }
                    ValueRef::Real(_) => return Err(Error::Integrity),
                }
            }
        }
    }
    Ok(())
}
pub(super) fn check_quarantine(
    db: &Connection,
    identity: Identity,
    request: WriteRequest,
) -> Result<()> {
    if version(db)? == 11 && !shared_valid(db)? {
        return Err(Error::CapabilityUnavailable);
    }
    if version(db)? == 11
        && sql(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM write_intent i JOIN object_quarantine q USING(operation)
         WHERE i.scope=?1 AND i.memory=?2 AND i.revision=?3)",
            params![identity.scope_bytes(), request.memory, request.revision],
            |r| r.get::<_, bool>(0),
        ))?
    {
        return Err(Error::Integrity);
    }
    Ok(())
}
fn shared_valid(db: &Connection) -> Result<bool> {
    sql(db.query_row(
        "SELECT NOT (
          (SELECT epoch FROM runtime)<>7+(SELECT count(*) FROM security_receipt)
          OR (SELECT knowledge_seq FROM runtime)<>(SELECT count(*) FROM receipt)
          OR EXISTS(SELECT 1 FROM authority WHERE epoch<>(SELECT epoch FROM runtime))
          OR EXISTS(SELECT 1 FROM write_intent i WHERE (i.state='Committed')<>
             EXISTS(SELECT 1 FROM receipt r WHERE r.operation=i.operation))
          OR EXISTS(SELECT 1 FROM receipt r WHERE NOT EXISTS(
             SELECT 1 FROM outbox o WHERE o.operation=r.operation AND o.knowledge_seq=r.knowledge_seq))
          OR EXISTS(SELECT 1 FROM receipt r JOIN security_event e ON e.id=r.event
             WHERE e.kind<>'Write' OR e.state<>'Applied' OR e.scope<>r.scope)
          OR EXISTS(SELECT 1 FROM deletion_intent d JOIN security_event e ON e.id=d.event
             WHERE d.scope<>e.scope OR e.kind<>'Revocation'))",
        [],|r|r.get(0)))
}

fn authorize_delete(
    db: &Connection,
    identity: Identity,
    kind: &str,
    target: Id,
    epoch: i64,
) -> Result<()> {
    let memories: Vec<Id> = if kind == "Memory" {
        if !sql(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM memory WHERE scope=?1 AND id=?2)",
            params![identity.scope_bytes(), target],
            |r| r.get::<_, bool>(0),
        ))? {
            return Err(Error::ResourceUnavailable);
        }
        vec![target]
    } else {
        let mut stmt = sql(db.prepare(
            "SELECT DISTINCT re.memory FROM revision_evidence re JOIN evidence e
             ON e.scope=re.scope AND e.id=re.evidence AND e.generation=re.generation
             WHERE e.scope=?1 AND e.source=?2 ORDER BY re.memory",
        ))?;
        sql(sql(stmt.query_map(params![identity.scope_bytes(), target], |r| r.get(0)))?.collect())?
    };
    if memories.is_empty() {
        return Err(Error::ResourceUnavailable);
    }
    for memory in memories {
        if sql(gates::authorized_snapshot(
            db,
            &identity.scope_bytes(),
            &identity.principal,
            &memory,
            epoch,
            trusted_now()?,
        ))?
        .is_none()
        {
            return Err(Error::AuthorizationDenied);
        }
    }
    Ok(())
}

pub(super) fn persist_isolations(
    db: &Connection,
    owner: Id,
    isolated: &[(Id, &str)],
) -> Result<()> {
    for &(operation, reason) in isolated {
        sql(db.execute(
            "INSERT INTO object_quarantine VALUES(?1,?2,?3)
            ON CONFLICT(operation) DO NOTHING",
            params![operation, reason, owner],
        ))?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Target {
    Memory(Id),
    Source(Id),
}
impl Target {
    fn parts(self) -> (&'static str, Id) {
        match self {
            Self::Memory(id) => ("Memory", id),
            Self::Source(id) => ("Source", id),
        }
    }
}
struct DeleteCommand {
    request: Id,
    target: Target,
}

fn plans(engine: &Engine) -> Result<Vec<(Plan, String)>> {
    plans_db(&engine.db)
}
fn plans_db(db: &Connection) -> Result<Vec<(Plan, String)>> {
    let mut stmt =
        sql(db.prepare("SELECT scope,principal,request FROM write_intent ORDER BY operation"))?;
    let rows = sql(sql(stmt.query_map([], |r| {
        Ok((
            r.get::<_, Vec<u8>>(0)?,
            r.get::<_, Id>(1)?,
            r.get::<_, Id>(2)?,
        ))
    }))?
    .collect::<rusqlite::Result<Vec<_>>>())?;
    let mut result = Vec::new();
    for (scope, principal, request) in rows {
        if scope.len() != 48 {
            return Err(Error::Integrity);
        }
        let identity = Identity {
            scope: SecurityScope {
                realm: scope[..16].try_into().map_err(|_| Error::Integrity)?,
                tenant: scope[16..32].try_into().map_err(|_| Error::Integrity)?,
                space: scope[32..].try_into().map_err(|_| Error::Integrity)?,
            },
            principal,
        };
        result.push(Engine::plan_row(db, identity, request)?.ok_or(Error::Integrity)?);
    }
    Ok(result)
}
fn marker(plan: &Plan, installation: Installation, event: i64) -> Vec<u8> {
    let mut bytes = b"YIXU/B3/REVOKED/v1\0".to_vec();
    bytes.extend(plan.binding(installation).aad(32));
    bytes.extend(plan.operation);
    bytes.extend(plan.incarnation);
    bytes.extend(event.to_be_bytes());
    bytes
}
type WorkRecord = (i64, Vec<u8>, Option<Vec<u8>>);
fn work(db: &Connection, operation: Id) -> Result<Option<WorkRecord>> {
    sql(db
        .query_row(
            "SELECT event,marker,marker_identity FROM deletion_work WHERE operation=?1",
            [operation],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional())
}
fn has_receipt(db: &Connection, operation: Id, action: &str) -> Result<bool> {
    sql(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM deletion_receipt WHERE operation=?1 AND action=?2)",
        params![operation, action],
        |r| r.get(0),
    ))
}
fn marker_name(plan: &Plan) -> String {
    format!("revoked-{}", filename(plan.key))
}
fn is_missing<T>(result: &std::io::Result<T>) -> bool {
    result.as_ref().is_err_and(|e| e.raw_os_error() == Some(2))
}

impl Engine {
    pub(super) fn b3_isolations(&self) -> Result<Vec<(Id, &'static str)>> {
        if version(&self.db)? != 11 {
            return Ok(Vec::new());
        }
        let mut result = Vec::new();
        for (plan, state) in plans(self)? {
            if state != "Committed" || work(&self.db, plan.operation)?.is_some() {
                continue;
            }
            let quarantined: bool = sql(self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM object_quarantine WHERE operation=?1)",
                [plan.operation],
                |r| r.get(0),
            ))?;
            if quarantined {
                continue;
            }
            let relation:bool=sql(self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM revision r JOIN payload p ON p.scope=r.scope
                AND p.memory=r.memory AND p.revision=r.id AND p.id=r.payload
                JOIN revision_evidence re ON re.scope=r.scope AND re.memory=r.memory AND re.revision=r.id
                WHERE p.operation=?1 AND re.evidence=?2 AND re.generation=?3)",
                params![plan.operation,plan.request.evidence,plan.request.evidence_generation],|r|r.get(0)))?;
            if !relation {
                result.push((plan.operation, "Dependency"));
                continue;
            }
            if let Err(error) = self.verify_files(&plan) {
                let reason = match error {
                    Error::ResourceUnavailable => "PayloadUnavailable",
                    Error::CapabilityUnavailable => "KeyUnavailable",
                    _ => "Integrity",
                };
                result.push((plan.operation, reason));
            }
        }
        Ok(result)
    }

    pub(super) fn reconcile_b3(&self) -> Result<Reconciliation> {
        let bad:bool=sql(self.db.query_row(
            "SELECT (SELECT blocked FROM runtime) OR EXISTS(SELECT 1 FROM security_event WHERE state<>'Applied')
              OR EXISTS(SELECT 1 FROM write_intent WHERE state<>'Committed')
              OR (SELECT knowledge_seq FROM runtime)<>(SELECT count(*) FROM receipt)
              OR (SELECT epoch FROM runtime)<>7+(SELECT count(*) FROM security_receipt)
              OR EXISTS(SELECT 1 FROM authority WHERE epoch<>(SELECT epoch FROM runtime))
              OR EXISTS(SELECT 1 FROM write_intent i WHERE NOT EXISTS(SELECT 1 FROM receipt r
                  JOIN outbox o USING(operation) WHERE r.operation=i.operation
                    AND o.knowledge_seq=r.knowledge_seq AND r.scope=i.scope AND r.revision=i.revision AND r.memory=i.memory))
              OR EXISTS(SELECT 1 FROM deletion_intent d JOIN security_event e ON e.id=d.event
                  WHERE e.scope<>d.scope OR e.kind<>'Revocation')
              OR EXISTS(SELECT 1 FROM receipt r JOIN security_event e ON e.id=r.event
                  WHERE e.kind<>'Write' OR e.state<>'Applied' OR e.scope<>r.scope)",
            [],|r|r.get(0)))?;
        let clean: bool = sql(self
            .db
            .query_row("SELECT clean FROM runtime", [], |r| r.get(0)))?;
        let mut report = Reconciliation {
            blocked: bad || (self.recovery_only && !clean),
            committed: 0,
            states: Vec::new(),
        };
        report.blocked |= !shared_valid(&self.db)?;
        let isolated = self.b3_isolations()?;
        for (plan, state) in plans(self)? {
            if let Some((event, bytes, observed_identity)) = work(&self.db, plan.operation)? {
                let name = marker_name(&plan);
                if bytes != marker(&plan, self.installation, event) {
                    report.blocked = true;
                }
                let actual = self.vault.anchor.read(&name);
                if let Ok(actual) = &actual {
                    if *actual != bytes
                        || observed_identity.as_ref().is_some_and(|expected| {
                            identity(&self.vault.anchor, &name).as_ref() != Ok(expected)
                        })
                    {
                        report.blocked = true;
                    }
                } else if !is_missing(&actual)
                    || has_receipt(&self.db, plan.operation, "KeyUnavailable")?
                {
                    report.blocked = true;
                }
                for (action, anchor, name) in [
                    ("CiphertextRemoved", &self.files, filename(plan.payload)),
                    ("WrapperRemoved", &self.vault.anchor, filename(plan.key)),
                ] {
                    let data = anchor.read(&name);
                    if has_receipt(&self.db, plan.operation, action)? && !is_missing(&data) {
                        report.blocked = true;
                    }
                    if data.is_err() && !is_missing(&data) {
                        report.blocked = true;
                    }
                }
                report
                    .states
                    .push((plan.operation, "LogicallyDeletedCleanupPending".into()));
                continue;
            }
            if state != "Committed" {
                report.blocked = true;
                continue;
            }
            report.committed += 1;
            let held: bool = sql(self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM object_quarantine WHERE operation=?1)",
                [plan.operation],
                |r| r.get(0),
            ))?;
            report.states.push((
                plan.operation,
                if held {
                    "Quarantined".into()
                } else if isolated.iter().any(|(op, _)| *op == plan.operation) {
                    "IsolationRequired".into()
                } else {
                    "CommittedReceipt".into()
                },
            ));
        }
        if !self.namespace_valid()? {
            report.blocked = true;
        }
        Ok(report)
    }

    fn isolate_objects(&mut self) -> Result<usize> {
        if version(&self.db)? != 11 || self.recovery_only || self.reconcile(0)?.blocked {
            return Err(Error::CapabilityUnavailable);
        }
        let isolated = self.b3_isolations()?;
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        persist_isolations(&tx, self.incarnation, &isolated)?;
        sql(tx.commit())?;
        Ok(isolated.len())
    }

    fn delete_close(&mut self, identity: Identity, command: DeleteCommand) -> Result<i64> {
        if version(&self.db)? != 11 || self.recovery_only {
            return Err(Error::CapabilityUnavailable);
        }
        let (kind, target) = command.target.parts();
        if command.request == [0; 16] || target == [0; 16] {
            return Err(Error::InvalidInput);
        }
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let (owner, epoch, blocked, clean): (Id, i64, bool, bool) = sql(tx.query_row(
            "SELECT incarnation,epoch,blocked,clean FROM runtime",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        ))?;
        if owner != self.incarnation
            || blocked
            || clean
            || identity.scope.realm != self.installation.realm
        {
            return Err(Error::CapabilityUnavailable);
        }
        authorize_delete(&tx, identity, kind, target, epoch)?;
        if !shared_valid(&tx)? {
            return Err(Error::Integrity);
        }
        let old:Option<(i64,String,Id)>=sql(tx.query_row(
            "SELECT event,kind,target FROM deletion_intent WHERE scope=?1 AND principal=?2 AND request=?3",
            params![identity.scope_bytes(),identity.principal,command.request],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional())?;
        if let Some((event, old_kind, old_target)) = old {
            return if old_kind == kind && old_target == target {
                Ok(event)
            } else {
                Err(Error::IdempotencyConflict)
            };
        }
        sql(tx.execute(
            "INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",
            [identity.scope_bytes()],
        ))?;
        let event = tx.last_insert_rowid();
        sql(tx.execute(
            "INSERT INTO deletion_intent VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                event,
                identity.scope_bytes(),
                identity.principal,
                command.request,
                kind,
                target
            ],
        ))?;
        sql(tx.commit())?;
        Ok(event)
    }

    fn delete_fence(&mut self, event: i64) -> Result<()> {
        if self.recovery_only || version(&self.db)? != 11 {
            return Err(Error::CapabilityUnavailable);
        }
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let exists: bool = sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM deletion_intent WHERE event=?1)",
            [event],
            |r| r.get(0),
        ))?;
        if !exists || sql(gates::make_effective(&tx, event))? != 1 {
            return Err(Error::CapabilityUnavailable);
        }
        sql(tx.commit())
    }

    fn delete_apply(&mut self, event: i64, checkpoint: impl FnOnce() -> Result<()>) -> Result<()> {
        if self.recovery_only || version(&self.db)? != 11 {
            return Err(Error::CapabilityUnavailable);
        }
        let installation = self.installation;
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let selected = plans_db(&tx)?;
        let (scope,kind,target,principal):(Vec<u8>,String,Id,Id)=sql(tx.query_row(
            "SELECT d.scope,d.kind,d.target,d.principal FROM deletion_intent d JOIN security_event e ON e.id=d.event
             WHERE d.event=?1 AND e.state='EffectiveFence'",[event],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))))?;
        if scope.len() != 48 || scope[..16] != self.installation.realm || !shared_valid(&tx)? {
            return Err(Error::Integrity);
        }
        let epoch: i64 = sql(tx.query_row("SELECT epoch FROM runtime", [], |r| r.get(0)))?;
        let identity = Identity {
            scope: SecurityScope {
                realm: scope[..16].try_into().map_err(|_| Error::Integrity)?,
                tenant: scope[16..32].try_into().map_err(|_| Error::Integrity)?,
                space: scope[32..].try_into().map_err(|_| Error::Integrity)?,
            },
            principal,
        };
        authorize_delete(&tx, identity, &kind, target, epoch)?;
        for (plan, state) in selected {
            if plan.identity.scope_bytes() != scope || state != "Committed" {
                continue;
            }
            let member = if kind == "Memory" {
                plan.request.memory == target
            } else {
                sql(tx.query_row("SELECT EXISTS(SELECT 1 FROM evidence WHERE scope=?1 AND id=?2 AND generation=?3 AND source=?4)",
                    params![scope,plan.request.evidence,plan.request.evidence_generation,target],|r|r.get(0)))?
            };
            if !member {
                continue;
            }
            sql(tx.execute(
                "INSERT INTO deletion_member VALUES(?1,?2)",
                params![event, plan.operation],
            ))?;
            for object in [plan.request.revision, plan.payload, plan.key] {
                sql(tx.execute(
                    "INSERT INTO r5_tombstone VALUES(?1,?2,?3,?4) ON CONFLICT DO NOTHING",
                    params![scope, object, 1, event],
                ))?;
            }
            sql(tx.execute(
                "INSERT INTO deletion_work(operation,event,generation,marker) VALUES(?1,?2,1,?3)
                ON CONFLICT(operation) DO NOTHING",
                params![plan.operation, event, marker(&plan, installation, event)],
            ))?;
        }
        if kind == "Memory" {
            sql(tx.execute(
                "INSERT INTO r5_tombstone VALUES(?1,?2,1,?3) ON CONFLICT DO NOTHING",
                params![scope, target, event],
            ))?;
        } else {
            sql(tx.execute(
                "INSERT INTO r5_tombstone SELECT scope,id,generation,?1 FROM source
                WHERE scope=?2 AND id=?3 ON CONFLICT DO NOTHING",
                params![event, scope, target],
            ))?;
            sql(tx.execute(
                "INSERT INTO r5_tombstone SELECT scope,id,generation,?1 FROM evidence
                WHERE scope=?2 AND source=?3 ON CONFLICT DO NOTHING",
                params![event, scope, target],
            ))?;
        }
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
        checkpoint()?;
        sql(tx.commit())
    }
}

impl Engine {
    fn delete_cleanup(
        &mut self,
        event: i64,
        mut checkpoint: impl FnMut(&str, &Self) -> Result<()>,
    ) -> Result<()> {
        if self.recovery_only
            || version(&self.db)? != 11
            || sql(gates::closing(&self.db))?
            || !shared_valid(&self.db)?
        {
            return Err(Error::CapabilityUnavailable);
        }
        let live: (Id, bool, bool) =
            sql(self
                .db
                .query_row("SELECT incarnation,blocked,clean FROM runtime", [], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                }))?;
        if live != (self.incarnation, false, false) {
            return Err(Error::CapabilityUnavailable);
        }
        for (plan, state) in plans(self)? {
            let belongs:bool=sql(self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM deletion_member m JOIN security_event e ON e.id=m.event
                 WHERE m.event=?1 AND m.operation=?2 AND e.state='Applied')",
                params![event,plan.operation],|r|r.get(0)))?;
            if !belongs || state != "Committed" {
                continue;
            }
            let (_, bytes, _) = work(&self.db, plan.operation)?.ok_or(Error::Integrity)?;
            let marker_name = marker_name(&plan);
            let actual = self.vault.anchor.read(&marker_name);
            match actual {
                Ok(actual) if actual == bytes => {}
                Err(ref e) if e.raw_os_error() == Some(2) => {
                    self.vault
                        .anchor
                        .publish(
                            &format!("pending-revoke-{}", filename(fresh()?)),
                            &marker_name,
                            &bytes,
                            0,
                            |_| {},
                        )
                        .map_err(|_| Error::ResourceUnavailable)?;
                }
                _ => return Err(Error::Integrity),
            }
            checkpoint("MarkerPublished", self)?;
            let actual_id = identity(&self.vault.anchor, &marker_name)?;
            let tx = sql(self
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate))?;
            let recorded: Option<Vec<u8>> = sql(tx.query_row(
                "SELECT marker_identity FROM deletion_work WHERE operation=?1",
                [plan.operation],
                |r| r.get(0),
            ))?;
            if let Some(recorded) = recorded {
                if recorded != actual_id {
                    return Err(Error::Integrity);
                }
            } else {
                sql(tx.execute(
                    "UPDATE deletion_work SET marker_identity=?1 WHERE operation=?2",
                    params![actual_id, plan.operation],
                ))?;
            }
            sql(tx.commit())?;
            self.cleanup_observe(plan.operation, "KeyUnavailable")?;
            checkpoint("KeyObserved", self)?;
            self.cleanup_receipt(plan.operation, "KeyUnavailable")?;
            checkpoint("KeyUnavailable", self)?;
            for (action, name, wrapper) in [
                ("CiphertextRemoved", filename(plan.payload), false),
                ("WrapperRemoved", filename(plan.key), true),
            ] {
                let anchor = if wrapper {
                    &self.vault.anchor
                } else {
                    &self.files
                };
                let bytes = anchor.read(&name);
                match bytes {
                    Ok(bytes) => {
                        let (expected_hash,expected_id):(Vec<u8>,Vec<u8>)=sql(self.db.query_row(
                            if wrapper {"SELECT wrapper_hash,wrapper_identity FROM publication WHERE operation=?1"}
                            else {"SELECT cipher_hash,cipher_identity FROM publication WHERE operation=?1"},
                            [plan.operation],|r|Ok((r.get(0)?,r.get(1)?))))?;
                        if digest(&bytes).as_slice() != expected_hash
                            || identity(anchor, &name)? != expected_id
                        {
                            return Err(Error::Integrity);
                        }
                        anchor
                            .erase_verified(&name, &bytes, &expected_id)
                            .map_err(|_| Error::Integrity)?;
                    }
                    Err(ref error) if error.raw_os_error() == Some(2) => {
                        if !has_receipt(&self.db, plan.operation, action)? {
                            return Err(Error::Integrity);
                        }
                    }
                    _ => return Err(Error::Integrity),
                }
                checkpoint(action, self)?;
                self.cleanup_observe(plan.operation, action)?;
                self.cleanup_receipt(plan.operation, action)?;
                checkpoint("ReceiptCommitted", self)?;
            }
            self.expire_local_backups(&plan, &mut checkpoint)?;
        }
        Ok(())
    }
    fn cleanup_observe(&mut self, operation: Id, action: &str) -> Result<()> {
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "INSERT INTO deletion_observation VALUES(?1,?2,1) ON CONFLICT DO NOTHING",
            params![operation, action],
        ))?;
        sql(tx.commit())
    }
    fn cleanup_receipt(&mut self, operation: Id, action: &str) -> Result<()> {
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        sql(tx.execute(
            "INSERT INTO deletion_receipt SELECT * FROM deletion_observation
            WHERE operation=?1 AND action=?2 ON CONFLICT DO NOTHING",
            params![operation, action],
        ))?;
        sql(tx.commit())
    }
    fn deletion_progress(&self, event: i64) -> Result<(String, i64, i64, i64, i64)> {
        sql(self.db.query_row(
            "SELECT e.state,count(DISTINCT m.operation),
             count(DISTINCT CASE WHEN r.action='KeyUnavailable' THEN r.operation END),
             count(DISTINCT CASE WHEN r.action='CiphertextRemoved' THEN r.operation END),
             (SELECT count(*) FROM backup_registration b JOIN deletion_member p USING(operation)
                WHERE p.event=?1 AND NOT EXISTS(SELECT 1 FROM backup_receipt r
                  WHERE r.operation=b.operation AND r.backup=b.backup AND r.generation=b.generation))
             FROM security_event e LEFT JOIN deletion_member m ON m.event=e.id
             LEFT JOIN deletion_receipt r ON r.operation=m.operation WHERE e.id=?1 GROUP BY e.id",
            [event],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))))
    }
}
impl Application {
    fn delete_content(&mut self, command: DeleteCommand) -> Result<i64> {
        self.engine.delete_close(self.identity, command)
    }
}

impl Engine {
    fn namespace_valid(&self) -> Result<bool> {
        let mut expected_payloads = BTreeSet::new();
        let mut expected_keys = BTreeSet::new();
        for (plan, _) in plans(self)? {
            expected_payloads.insert(filename(plan.payload));
            expected_keys.insert(filename(plan.key));
            for (action, anchor, name) in [
                ("CiphertextRemoved", &self.files, filename(plan.payload)),
                ("WrapperRemoved", &self.vault.anchor, filename(plan.key)),
            ] {
                if has_receipt(&self.db, plan.operation, action)?
                    && !is_missing(&anchor.read(&name))
                {
                    return Ok(false);
                }
            }
            if let Some((event, bytes, recorded)) = work(&self.db, plan.operation)? {
                let name = marker_name(&plan);
                expected_keys.insert(name.clone());
                if bytes != marker(&plan, self.installation, event) {
                    return Ok(false);
                }
                let found = self.vault.anchor.read(&name);
                match found {
                    Ok(found) => {
                        if found != bytes
                            || recorded.as_ref().is_some_and(|expected| {
                                identity(&self.vault.anchor, &name).as_ref() != Ok(expected)
                            })
                        {
                            return Ok(false);
                        }
                    }
                    Err(e) if e.raw_os_error() == Some(2) => {
                        if has_receipt(&self.db, plan.operation, "KeyUnavailable")? {
                            return Ok(false);
                        }
                    }
                    _ => return Ok(false),
                }
            }
        }
        for backup in backups(&self.db, None)? {
            let name = format!("backup-{}", filename(backup.id));
            expected_payloads.insert(name.clone());
            let data = self.files.read(&name);
            if backup.expired {
                if !is_missing(&data) {
                    return Ok(false);
                }
            } else if let Ok(data) = data {
                if digest(&data) != backup.hash || identity(&self.files, &name)? != backup.identity
                {
                    return Ok(false);
                }
            } else {
                return Ok(false);
            }
        }
        Ok(!self
            .files
            .names()
            .map_err(|_| Error::Integrity)?
            .iter()
            .any(|name| !expected_payloads.contains(name))
            && !self
                .vault
                .anchor
                .names()
                .map_err(|_| Error::Integrity)?
                .iter()
                .any(|name| !expected_keys.contains(name)))
    }

    pub(super) fn b3_guard_storage(&mut self) -> Result<()> {
        if version(&self.db)? != 11 {
            return Ok(());
        }
        let valid = shared_valid(&self.db).and_then(|valid| Ok(valid && self.namespace_valid()?));
        if !matches!(valid, Ok(true)) {
            let _ = self.db.execute("UPDATE runtime SET blocked=1", []);
            return Err(Error::CapabilityUnavailable);
        }
        Ok(())
    }

    pub(super) fn b3_guard_read(&mut self) -> Result<()> {
        self.b3_guard_storage()?;
        if version(&self.db)? == 11
            && sql(self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM write_intent WHERE state<>'Committed')",
                [],
                |r| r.get::<_, bool>(0),
            ))?
        {
            let _ = self.db.execute("UPDATE runtime SET blocked=1", []);
            return Err(Error::CapabilityUnavailable);
        }
        Ok(())
    }
}

struct Backup {
    operation: Id,
    id: Id,
    generation: i64,
    hash: [u8; 32],
    identity: Vec<u8>,
    expired: bool,
}
fn backups(db: &Connection, operation: Option<Id>) -> Result<Vec<Backup>> {
    let mut query=sql(db.prepare(
        "SELECT b.operation,b.backup,b.generation,b.artifact_hash,b.artifact_identity,
           EXISTS(SELECT 1 FROM backup_receipt r WHERE r.operation=b.operation AND r.backup=b.backup AND r.generation=b.generation)
         FROM backup_registration b WHERE ?1 IS NULL OR b.operation=?1 ORDER BY b.backup"))?;
    sql(sql(query.query_map([operation], |row| {
        Ok(Backup {
            operation: row.get(0)?,
            id: row.get(1)?,
            generation: row.get(2)?,
            hash: row.get(3)?,
            identity: row.get(4)?,
            expired: row.get(5)?,
        })
    }))?
    .collect())
}
impl Engine {
    fn register_local_backup(&mut self, identity: Identity, request: Id) -> Result<Id> {
        self.b3_guard_storage()?;
        if version(&self.db)? != 11 || self.recovery_only {
            return Err(Error::CapabilityUnavailable);
        }
        let (plan, state) =
            Self::plan_row(&self.db, identity, request)?.ok_or(Error::ResourceUnavailable)?;
        if state != "Committed" {
            return Err(Error::CapabilityUnavailable);
        }
        Self::authorize(
            &self.db,
            identity,
            plan.request,
            self.installation,
            self.incarnation,
            0,
        )?;
        let cipher = self
            .files
            .read(&filename(plan.payload))
            .map_err(|_| Error::Integrity)?;
        let wrapper = self
            .vault
            .anchor
            .read(&filename(plan.key))
            .map_err(|_| Error::Integrity)?;
        let id = fresh()?;
        let name = format!("backup-{}", filename(id));
        let mut bytes = b"YIXU/B3/LOCAL-ENCRYPTED-BACKUP/v1\0".to_vec();
        bytes.extend(plan.binding(self.installation).aad(32));
        bytes.extend((cipher.len() as u64).to_be_bytes());
        bytes.extend(cipher);
        bytes.extend(wrapper);
        self.files
            .publish(
                &format!("pending-{}", filename(fresh()?)),
                &name,
                &bytes,
                0,
                |_| {},
            )
            .map_err(|_| Error::ResourceUnavailable)?;
        let file_id = super::identity(&self.files, &name)?;
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        Self::authorize(
            &tx,
            identity,
            plan.request,
            self.installation,
            self.incarnation,
            0,
        )?;
        sql(tx.execute(
            "INSERT INTO backup_registration VALUES(?1,?2,1,?3,?4)",
            params![plan.operation, id, digest(&bytes), file_id],
        ))?;
        sql(tx.commit())?;
        Ok(id)
    }
    fn expire_local_backups(
        &mut self,
        plan: &Plan,
        checkpoint: &mut impl FnMut(&str, &Self) -> Result<()>,
    ) -> Result<()> {
        for backup in backups(&self.db, Some(plan.operation))? {
            if backup.expired {
                continue;
            }
            checkpoint("BeforeBackup", self)?;
            let name = format!("backup-{}", filename(backup.id));
            match self.files.read(&name) {
                Ok(bytes) => {
                    if digest(&bytes) != backup.hash {
                        return Err(Error::Integrity);
                    }
                    self.files
                        .erase_verified(&name, &bytes, &backup.identity)
                        .map_err(|_| Error::Integrity)?;
                }
                // Absence alone is not evidence that this registered object was erased.
                // A crash between erase and receipt remains blocked; it is never guessed complete.
                _ => return Err(Error::Integrity),
            }
            checkpoint("BackupRemoved", self)?;
            let tx = sql(self
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate))?;
            sql(tx.execute(
                "INSERT INTO backup_receipt VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",
                params![backup.operation, backup.id, backup.generation],
            ))?;
            sql(tx.commit())?;
            checkpoint("BackupExpired", self)?;
        }
        Ok(())
    }
}

pub(super) mod tests;
