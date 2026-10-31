//! PRIVATE cfg(test) blocked-envelope adapter. Never a production Payload admission path.
use super::*;
use yixu_ports::memory::{AuthorizedMemory, MemoryReceipt, MemoryTarget, MemoryWrite};

enum MemoryResourceKind {
    Memory,
    Revision,
    Job,
}
fn allocate_memory(
    tx: &Transaction<'_>,
    tenant: Id,
    kind: MemoryResourceKind,
    ids: &mut dyn ResourceIds,
) -> Result<Id> {
    let sql = match kind {
        MemoryResourceKind::Memory => {
            "SELECT EXISTS(SELECT 1 FROM memory WHERE tenant_id=?1 AND id=?2)"
        }
        MemoryResourceKind::Revision => {
            "SELECT EXISTS(SELECT 1 FROM revision WHERE tenant_id=?1 AND id=?2)"
        }
        MemoryResourceKind::Job => {
            "SELECT EXISTS(SELECT 1 FROM outbox_job WHERE tenant_id=?1 AND id=?2)"
        }
    };
    allocate_free(tx, tenant, sql, ids)
}

fn digest(command: &MemoryWrite) -> Result<Vec<u8>> {
    if command.evidence.is_empty() || command.evidence.len() > 64 {
        return Err(Error::InvalidInput);
    }
    let mut evidence: Vec<_> = command.evidence.iter().map(Id::to_string).collect();
    evidence.sort();
    if evidence.windows(2).any(|pair| pair[0] == pair[1])
        || !command.evidence.contains(&command.primary_evidence)
    {
        return Err(Error::InvalidInput);
    }
    let target = match command.target {
        MemoryTarget::Create => "create".to_owned(),
        MemoryTarget::Revise {
            memory,
            expected_version,
        } => format!("revise:{memory}:{expected_version}"),
    };
    Ok(Sha256::digest(
        format!(
            "memory-envelope/v1|{}|{}|{target}|{}|{}|{}",
            command.scope.tenant,
            command.scope.space,
            command.primary_evidence,
            command.content_payload,
            evidence.join(",")
        )
        .as_bytes(),
    )
    .to_vec())
}

fn validate_references(tx: &Transaction<'_>, command: &MemoryWrite) -> Result<()> {
    let tenant = command.scope.tenant.to_string();
    let space = command.scope.space.to_string();
    // No partial deletion semantics are implemented here. ANY pending/recorded deletion in the
    // Space blocks this laboratory operation; it cannot rehabilitate an invalidated Revision.
    let fenced: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM deletion_request WHERE tenant_id=?1 AND space_id=?2)",
            params![tenant, space],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    if fenced {
        return Err(Error::CapabilityUnavailable);
    }
    for evidence in &command.evidence {
        // Cross-Space, cross-Tenant and absent references have ONE public outcome.
        let valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM evidence e JOIN event v ON v.tenant_id=e.tenant_id AND v.space_id=e.space_id AND v.id=e.event_id JOIN payload b ON b.tenant_id=e.tenant_id AND b.space_id=e.space_id AND b.id=e.payload_id JOIN payload l ON l.tenant_id=e.tenant_id AND l.space_id=e.space_id AND l.id=e.locator_payload_id WHERE e.tenant_id=?1 AND e.space_id=?2 AND e.id=?3 AND b.state='blocked' AND b.purpose='content' AND l.state='blocked' AND l.purpose='locator' AND v.kind='system_action' AND v.payload_id IS NULL AND e.source_kind='user_statement')",params![tenant,space,evidence.to_string()],|r|r.get(0)).map_err(sql_error)?;
        if !valid {
            return Err(Error::AuthorizationDenied);
        }
    }
    let payload_valid: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM payload WHERE tenant_id=?1 AND space_id=?2 AND id=?3 AND state='blocked' AND purpose='content' AND encryption_profile='metadata-fixture-not-encrypted')",params![tenant,space,command.content_payload.to_string()],|r|r.get(0)).map_err(sql_error)?;
    if !payload_valid {
        return Err(Error::AuthorizationDenied);
    }
    let owned: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM payload_owner WHERE tenant_id=?1 AND space_id=?2 AND payload_id=?3)",params![tenant,space,command.content_payload.to_string()],|r|r.get(0)).map_err(sql_error)?;
    if owned {
        return Err(Error::ResourceUnavailable);
    }
    Ok(())
}

impl<C: Clock> AuthorizedMemory for AuthDatabase<C> {
    type Context = AuthContext;
    fn apply_memory(
        &mut self,
        context: Option<&AuthContext>,
        command: MemoryWrite,
    ) -> Result<MemoryReceipt> {
        if self.profile != 4 {
            return Err(Error::CapabilityUnavailable);
        }
        let ctx = context.ok_or(Error::AuthorizationDenied)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let start = time(&self.clock)?;
        let capability = command.target.capability();
        authorize_scope(&tx, ctx, command.scope, capability, start)?;
        let request_digest = digest(&command)?;
        type Replay = (Vec<u8>, String, String, i64, i64, i64);
        let replay: Option<Replay> = tx.query_row("SELECT request_digest,memory_id,revision_id,version,number,knowledge_seq FROM memory_retry WHERE tenant_id=?1 AND principal_id=?2 AND key_id=?3",params![ctx.scope.tenant.to_string(),ctx.principal.to_string(),command.idempotency.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional().map_err(sql_error)?;
        let receipt = if let Some((old, memory, revision, version, number, seq)) = replay {
            if old != request_digest {
                return Err(Error::IdempotencyConflict);
            }
            // A receipt is not readable content and does not revive its old revision. Fences
            // must still be checked even for replays, without demanding an unused Payload.
            let fenced: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM deletion_request WHERE tenant_id=?1 AND space_id=?2) OR EXISTS(SELECT 1 FROM memory WHERE tenant_id=?1 AND space_id=?2 AND id=?3 AND state<>'source_blocked') OR EXISTS(SELECT 1 FROM revision_invalidation WHERE tenant_id=?1 AND space_id=?2 AND revision_id=?4)",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),memory,revision],|r|r.get(0)).map_err(sql_error)?;
            if fenced {
                return Err(Error::CapabilityUnavailable);
            }
            MemoryReceipt {
                memory: memory.parse()?,
                revision: revision.parse()?,
                version: natural(version)?,
                revision_number: natural(number)?,
                knowledge_seq: natural(seq)?,
                replayed: true,
            }
        } else {
            let (memory, version, number, parent, parent_number) = match command.target {
                MemoryTarget::Create => (
                    allocate_memory(
                        &tx,
                        ctx.scope.tenant,
                        MemoryResourceKind::Memory,
                        self.ids.as_mut(),
                    )?,
                    1,
                    1,
                    None,
                    None,
                ),
                MemoryTarget::Revise {
                    memory,
                    expected_version,
                } => {
                    let expected =
                        i64::try_from(expected_version).map_err(|_| Error::InvalidInput)?;
                    type Current = (i64, String, i64, String);
                    let current: Option<Current> = tx.query_row("SELECT m.version,r.id,r.number,m.state FROM memory m JOIN revision r ON r.tenant_id=m.tenant_id AND r.space_id=m.space_id AND r.memory_id=m.id AND r.id=m.current_revision_id WHERE m.tenant_id=?1 AND m.space_id=?2 AND m.id=?3",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),memory.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(sql_error)?;
                    let (version, parent, number, state) =
                        current.ok_or(Error::AuthorizationDenied)?;
                    if state != "source_blocked" {
                        return Err(Error::CapabilityUnavailable);
                    }
                    let invalid:bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM revision_invalidation i JOIN revision r ON r.tenant_id=i.tenant_id AND r.space_id=i.space_id AND r.id=i.revision_id WHERE r.tenant_id=?1 AND r.space_id=?2 AND r.memory_id=?3)",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),memory.to_string()],|r|r.get(0)).map_err(sql_error)?;
                    if invalid {
                        return Err(Error::CapabilityUnavailable);
                    }
                    if version != expected {
                        return Err(Error::CasConflict);
                    }
                    (
                        memory,
                        version.checked_add(1).ok_or(Error::Constraint)?,
                        number.checked_add(1).ok_or(Error::Constraint)?,
                        Some(parent),
                        Some(number),
                    )
                }
            };
            let revision = allocate_memory(
                &tx,
                ctx.scope.tenant,
                MemoryResourceKind::Revision,
                self.ids.as_mut(),
            )?;
            let job = allocate_memory(
                &tx,
                ctx.scope.tenant,
                MemoryResourceKind::Job,
                self.ids.as_mut(),
            )?;
            let changed = tx.execute("UPDATE tenant SET knowledge_seq=knowledge_seq+1 WHERE id=?1 AND knowledge_seq<9223372036854775807",[ctx.scope.tenant.to_string()]).map_err(sql_error)?;
            if changed != 1 {
                return Err(Error::Constraint);
            }
            let seq: i64 = tx
                .query_row(
                    "SELECT knowledge_seq FROM tenant WHERE id=?1",
                    [ctx.scope.tenant.to_string()],
                    |r| r.get(0),
                )
                .map_err(sql_error)?;
            tx.execute(
                "INSERT INTO commit_record VALUES(?1,?2,?3)",
                params![ctx.scope.tenant.to_string(), seq, start],
            )
            .map_err(sql_error)?;
            // Only cfg(test) internal assembly can install this independent test-support fixture.
            // It proves source graph atomicity without introducing an ingestion DTO or content.
            if let Some(source) = self.source_fixture.take() {
                source.seed(&tx, ctx.scope, ctx.principal, seq)?;
            }
            validate_references(&tx, &command)?;
            if matches!(command.target, MemoryTarget::Create) {
                tx.execute(
                    "INSERT INTO memory VALUES(?1,?2,?3,?4,1,'source_blocked',?5)",
                    params![
                        ctx.scope.tenant.to_string(),
                        ctx.scope.space.to_string(),
                        memory.to_string(),
                        revision.to_string(),
                        start
                    ],
                )
                .map_err(sql_error)?;
            }
            tx.execute("INSERT INTO revision VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'user_assertion',?10,NULL,?10,?11)",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),revision.to_string(),memory.to_string(),number,parent,parent_number,command.primary_evidence.to_string(),command.content_payload.to_string(),start,seq]).map_err(sql_error)?;
            for evidence in &command.evidence {
                tx.execute(
                    "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,'supports')",
                    params![
                        ctx.scope.tenant.to_string(),
                        ctx.scope.space.to_string(),
                        revision.to_string(),
                        evidence.to_string()
                    ],
                )
                .map_err(sql_error)?;
            }
            if let MemoryTarget::Revise {
                expected_version, ..
            } = command.target
            {
                let changed=tx.execute("UPDATE memory SET version=?4,current_revision_id=?5 WHERE tenant_id=?1 AND space_id=?2 AND id=?3 AND version=?6 AND state='source_blocked'",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),memory.to_string(),version,revision.to_string(),i64::try_from(expected_version).map_err(|_|Error::InvalidInput)?]).map_err(sql_error)?;
                if changed != 1 {
                    return Err(Error::CasConflict);
                }
            }
            let latest: bool = tx.query_row("SELECT r.number=(SELECT max(x.number) FROM revision x WHERE x.tenant_id=m.tenant_id AND x.space_id=m.space_id AND x.memory_id=m.id) FROM memory m JOIN revision r ON r.tenant_id=m.tenant_id AND r.space_id=m.space_id AND r.memory_id=m.id AND r.id=m.current_revision_id WHERE m.tenant_id=?1 AND m.space_id=?2 AND m.id=?3",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),memory.to_string()],|r|r.get(0)).map_err(sql_error)?;
            if !latest {
                return Err(Error::Constraint);
            }
            tx.execute(
                "INSERT INTO memory_retry VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    ctx.scope.tenant.to_string(),
                    ctx.scope.space.to_string(),
                    ctx.principal.to_string(),
                    command.idempotency.to_string(),
                    request_digest,
                    memory.to_string(),
                    revision.to_string(),
                    version,
                    number,
                    seq
                ],
            )
            .map_err(sql_error)?;
            tx.execute(
                "INSERT INTO memory_audit VALUES(?1,?2,?3,?4,?5)",
                params![
                    ctx.scope.tenant.to_string(),
                    ctx.scope.space.to_string(),
                    revision.to_string(),
                    ctx.principal.to_string(),
                    capability
                ],
            )
            .map_err(sql_error)?;
            tx.execute("INSERT INTO outbox_job VALUES(?1,?2,?3,'metadata_changed',?4,?4,NULL,?5,'pending',NULL,0,0,0)",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),job.to_string(),memory.to_string(),version]).map_err(sql_error)?;
            let changed=tx.execute("UPDATE space SET graph_version=graph_version+1 WHERE tenant_id=?1 AND id=?2 AND graph_version<9223372036854775807",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string()]).map_err(sql_error)?;
            if changed != 1 {
                return Err(Error::Constraint);
            }
            MemoryReceipt {
                memory,
                revision,
                version: natural(version)?,
                revision_number: natural(number)?,
                knowledge_seq: natural(seq)?,
                replayed: false,
            }
        };
        if let Some(hook) = self.before_commit.take() {
            hook();
        }
        let final_time = time(&self.clock)?;
        if final_time < start {
            return Err(Error::AuthorizationDenied);
        }
        authorize_scope(&tx, ctx, command.scope, capability, final_time)?;
        tx.commit().map_err(sql_error)?;
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests;
