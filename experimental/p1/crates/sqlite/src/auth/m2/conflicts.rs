//! Explicit decisions only: never infer truth, overwrite a Revision, or remove either source.
use super::*;
fn revision(tx: &Transaction<'_>, scope: Scope, id: Id) -> Result<(i64, Vec<Id>)> {
    let seq: Option<i64> = tx
        .query_row(
            "SELECT knowledge_seq FROM revision WHERE tenant_id=?1 AND space_id=?2 AND id=?3",
            params![
                scope.tenant.to_string(),
                scope.space.to_string(),
                id.to_string()
            ],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    let seq = seq.ok_or(Error::AuthorizationDenied)?;
    let mut s=tx.prepare("SELECT evidence_id FROM revision_evidence WHERE tenant_id=?1 AND space_id=?2 AND revision_id=?3 ORDER BY evidence_id").map_err(sql_error)?;
    let evidence = s
        .query_map(
            params![
                scope.tenant.to_string(),
                scope.space.to_string(),
                id.to_string()
            ],
            |r| r.get::<_, String>(0),
        )
        .map_err(sql_error)?
        .map(|r| parse_id(r.map_err(sql_error)?))
        .collect::<Result<Vec<_>>>()?;
    if evidence.is_empty() {
        return Err(Error::Integrity);
    }
    Ok((seq, evidence))
}
fn history(
    tx: &Transaction<'_>,
    scope: Scope,
    id: Id,
    seq: u64,
) -> Result<(Id, Id, Vec<ConflictDecision>)> {
    let endpoints:Option<(String,String)>=tx.query_row("SELECT left_revision_id,right_revision_id FROM conflict WHERE tenant_id=?1 AND space_id=?2 AND id=?3 AND knowledge_seq<=?4",params![scope.tenant.to_string(),scope.space.to_string(),id.to_string(),i64::try_from(seq).map_err(|_|Error::InvalidToken)?],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql_error)?;
    let (left, right) = endpoints.ok_or(Error::AuthorizationDenied)?;
    let left = parse_id(left)?;
    let right = parse_id(right)?;
    revision(tx, scope, left)?;
    revision(tx, scope, right)?;
    let mut s=tx.prepare("SELECT id,number,parent_number,status,resolution_revision_id,basis_revision_id,event_id,knowledge_seq FROM conflict_decision WHERE tenant_id=?1 AND space_id=?2 AND conflict_id=?3 ORDER BY number").map_err(sql_error)?;
    let records = s
        .query_map(
            params![
                scope.tenant.to_string(),
                scope.space.to_string(),
                id.to_string()
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            },
        )
        .map_err(sql_error)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(sql_error)?;
    if records.is_empty() {
        return Err(Error::Integrity);
    }
    let mut decisions = Vec::new();
    let mut last_seq = 0;
    for (i, (id, number, parent, status, resolution, basis, event, knowledge_seq)) in
        records.into_iter().enumerate()
    {
        let n = i64::try_from(i + 1).map_err(|_| Error::Integrity)?;
        if number != n
            || parent != if n == 1 { None } else { Some(n - 1) }
            || knowledge_seq <= last_seq
        {
            return Err(Error::Integrity);
        }
        last_seq = knowledge_seq;
        let status = match status.as_str() {
            "open" => ConflictStatus::Open,
            "resolved" => ConflictStatus::Resolved,
            _ => return Err(Error::Integrity),
        };
        let resolution = resolution.map(parse_id).transpose()?;
        let basis = basis.map(parse_id).transpose()?;
        if (status == ConflictStatus::Resolved) != resolution.is_some()
            || (n == 1 && status != ConflictStatus::Open)
            || n > 1 && status == ConflictStatus::Open && basis.is_none()
        {
            return Err(Error::Integrity);
        }
        if let Some(r) = resolution {
            let (_, sources) = revision(tx, scope, r)?;
            for side in [left, right] {
                if revision(tx, scope, side)?
                    .1
                    .iter()
                    .any(|e| !sources.contains(e))
                {
                    return Err(Error::Integrity);
                }
            }
        }
        if let Some(r) = basis {
            revision(tx, scope, r)?;
        }
        let event = parse_id(event)?;
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM event WHERE tenant_id=?1 AND space_id=?2 AND id=?3 AND kind='system_action' AND payload_id IS NULL AND knowledge_seq=?4)",params![scope.tenant.to_string(),scope.space.to_string(),event.to_string(),knowledge_seq],|r|r.get(0)).map_err(sql_error)?;
        if !valid {
            return Err(Error::Integrity);
        }
        if natural(knowledge_seq)? <= seq {
            decisions.push(ConflictDecision {
                id: parse_id(id)?,
                number: natural(number)?,
                status,
                resolution_revision: resolution,
                basis_revision: basis,
                event,
                knowledge_seq: natural(knowledge_seq)?,
            });
        }
    }
    if decisions.is_empty() {
        return Err(Error::Integrity);
    }
    Ok((left, right, decisions))
}
pub(super) fn read<C: Clock>(
    db: &mut AuthDatabase<C>,
    context: Option<&AuthContext>,
    q: ConflictQuery,
) -> Result<ConflictHistory> {
    if db.profile != 5 {
        return Err(Error::CapabilityUnavailable);
    }
    let ctx = context.ok_or(Error::AuthorizationDenied)?;
    let authority = db
        .query_tokens
        .clone()
        .ok_or(Error::CapabilityUnavailable)?;
    let tx = db
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let start = time(&db.clock)?;
    authorize_scope(&tx, ctx, q.scope, "memory.history", start)?;
    fence(&tx, q.scope)?;
    integrity(&tx, q.scope)?;
    let seq = snapshot(&authority, ctx, q.as_of.as_ref(), live_seq(&tx, ctx)?)?;
    let (left, right, decisions) = history(&tx, q.scope, q.conflict, seq)?;
    let out = ConflictHistory {
        conflict: q.conflict,
        left,
        right,
        decisions,
        snapshot: authority.knowledge(ctx, seq)?,
    };
    audit_read(
        &tx,
        ctx,
        seq,
        &format!("conflict-history/{}", q.conflict),
        start,
        db.ids.as_mut(),
    )?;
    if let Some(hook) = db.before_commit.take() {
        hook();
    }
    let now = time(&db.clock)?;
    if now < start {
        return Err(Error::AuthorizationDenied);
    }
    authorize_scope(&tx, ctx, q.scope, "memory.history", now)?;
    fence(&tx, q.scope)?;
    tx.commit().map_err(sql_error)?;
    Ok(out)
}
fn allocate(
    tx: &Transaction<'_>,
    scope: Scope,
    table: &str,
    ids: &mut dyn ResourceIds,
) -> Result<Id> {
    allocate_free(
        tx,
        scope.tenant,
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE tenant_id=?1 AND id=?2)"),
        ids,
    )
}
pub(super) fn write<C: Clock>(
    db: &mut AuthDatabase<C>,
    context: Option<&AuthContext>,
    c: ConflictWrite,
) -> Result<ConflictReceipt> {
    if db.profile != 5 {
        return Err(Error::CapabilityUnavailable);
    }
    let ctx = context.ok_or(Error::AuthorizationDenied)?;
    let tx = db
        .connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let start = time(&db.clock)?;
    authorize_scope(&tx, ctx, c.scope, "memory.revise", start)?;
    fence(&tx, c.scope)?;
    integrity(&tx, c.scope)?;
    let target = match c.target {
        ConflictTarget::Create { left, right } => {
            let (mut l, mut r) = (left.to_string(), right.to_string());
            if l > r {
                std::mem::swap(&mut l, &mut r)
            }
            format!("create:{l}:{r}")
        }
        ConflictTarget::Resolve {
            conflict,
            expected_version,
            resolution_revision,
        } => format!("resolve:{conflict}:{expected_version}:{resolution_revision}"),
        ConflictTarget::Reopen {
            conflict,
            expected_version,
            new_revision,
        } => format!("reopen:{conflict}:{expected_version}:{new_revision}"),
    };
    let digest = Sha256::digest(format!(
        "conflict/v1|{}|{}|{target}",
        c.scope.tenant, c.scope.space
    ))
    .to_vec();
    type Replay = (Vec<u8>, String, String, i64, i64);
    let replay:Option<Replay>=tx.query_row("SELECT request_digest,conflict_id,decision_id,version,knowledge_seq FROM conflict_retry WHERE tenant_id=?1 AND principal_id=?2 AND key_id=?3",params![c.scope.tenant.to_string(),ctx.principal.to_string(),c.idempotency.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(sql_error)?;
    let out = if let Some((old, conflict, decision, version, seq)) = replay {
        if old != digest {
            return Err(Error::IdempotencyConflict);
        }
        let conflict = parse_id(conflict)?;
        history(&tx, c.scope, conflict, live_seq(&tx, ctx)?)?;
        ConflictReceipt {
            conflict,
            decision: parse_id(decision)?,
            version: natural(version)?,
            knowledge_seq: natural(seq)?,
            replayed: true,
        }
    } else {
        let (conflict, number, status, resolution, basis, created) = match c.target {
            ConflictTarget::Create { left, right } => {
                if left == right {
                    return Err(Error::InvalidInput);
                }
                revision(&tx, c.scope, left)?;
                revision(&tx, c.scope, right)?;
                let (mut l, mut r) = (left.to_string(), right.to_string());
                if l > r {
                    std::mem::swap(&mut l, &mut r)
                }
                let duplicate:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM conflict WHERE tenant_id=?1 AND space_id=?2 AND left_revision_id=?3 AND right_revision_id=?4)",params![c.scope.tenant.to_string(),c.scope.space.to_string(),l,r],|r|r.get(0)).map_err(sql_error)?;
                if duplicate {
                    return Err(Error::StateConflict);
                }
                (
                    allocate(&tx, c.scope, "conflict", db.ids.as_mut())?,
                    1,
                    "open",
                    None,
                    None,
                    Some((l, r)),
                )
            }
            ConflictTarget::Resolve {
                conflict,
                expected_version,
                resolution_revision,
            }
            | ConflictTarget::Reopen {
                conflict,
                expected_version,
                new_revision: resolution_revision,
            } => {
                let (left, right, decisions) =
                    history(&tx, c.scope, conflict, live_seq(&tx, ctx)?)?;
                let last = decisions.last().ok_or(Error::Integrity)?;
                if last.number != expected_version {
                    return Err(Error::CasConflict);
                }
                let (revision_seq, evidence) = revision(&tx, c.scope, resolution_revision)?;
                let mut both = revision(&tx, c.scope, left)?.1;
                both.extend(revision(&tx, c.scope, right)?.1);
                let resolving = matches!(c.target, ConflictTarget::Resolve { .. });
                if resolving {
                    if last.status != ConflictStatus::Open {
                        return Err(Error::StateConflict);
                    }
                    if both.iter().any(|e| !evidence.contains(e)) {
                        return Err(Error::InvalidInput);
                    }
                } else {
                    if last.status != ConflictStatus::Resolved {
                        return Err(Error::StateConflict);
                    }
                    let resolved_revision = last.resolution_revision.ok_or(Error::Integrity)?;
                    let (_, resolved_evidence) = revision(&tx, c.scope, resolved_revision)?;
                    if natural(revision_seq)? <= last.knowledge_seq
                        || evidence.iter().all(|e| resolved_evidence.contains(e))
                    {
                        return Err(Error::InvalidInput);
                    }
                }
                (
                    conflict,
                    i64::try_from(last.number)
                        .map_err(|_| Error::Integrity)?
                        .checked_add(1)
                        .ok_or(Error::Integrity)?,
                    if resolving { "resolved" } else { "open" },
                    if resolving {
                        Some(resolution_revision.to_string())
                    } else {
                        None
                    },
                    if resolving {
                        None
                    } else {
                        Some(resolution_revision.to_string())
                    },
                    None,
                )
            }
        };
        let decision = allocate(&tx, c.scope, "conflict_decision", db.ids.as_mut())?;
        let event =
            super::super::allocate(&tx, c.scope.tenant, ResourceKind::Event, db.ids.as_mut())?;
        if tx.execute("UPDATE tenant SET knowledge_seq=knowledge_seq+1 WHERE id=?1 AND knowledge_seq<9223372036854775807",[c.scope.tenant.to_string()]).map_err(sql_error)?!=1{return Err(Error::Constraint)}
        let seq = i64::try_from(live_seq(&tx, ctx)?).map_err(|_| Error::Integrity)?;
        tx.execute(
            "INSERT INTO commit_record VALUES(?1,?2,?3)",
            params![c.scope.tenant.to_string(), seq, start],
        )
        .map_err(sql_error)?;
        tx.execute(
            "INSERT INTO event VALUES(?1,?2,?3,'system_action',NULL,?4,?5,?5,?6,'1.0')",
            params![
                c.scope.tenant.to_string(),
                c.scope.space.to_string(),
                event.to_string(),
                ctx.principal.to_string(),
                start,
                seq
            ],
        )
        .map_err(sql_error)?;
        if let Some((left, right)) = created {
            tx.execute(
                "INSERT INTO conflict VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    c.scope.tenant.to_string(),
                    c.scope.space.to_string(),
                    conflict.to_string(),
                    left,
                    right,
                    seq
                ],
            )
            .map_err(sql_error)?;
        }
        tx.execute(
            "INSERT INTO conflict_decision VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                c.scope.tenant.to_string(),
                c.scope.space.to_string(),
                decision.to_string(),
                conflict.to_string(),
                number,
                if number == 1 { None } else { Some(number - 1) },
                status,
                resolution,
                basis,
                event.to_string(),
                seq
            ],
        )
        .map_err(sql_error)?;
        tx.execute(
            "INSERT INTO conflict_retry VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                c.scope.tenant.to_string(),
                c.scope.space.to_string(),
                ctx.principal.to_string(),
                c.idempotency.to_string(),
                digest,
                conflict.to_string(),
                decision.to_string(),
                number,
                seq
            ],
        )
        .map_err(sql_error)?;
        tx.execute(
            "INSERT INTO conflict_audit VALUES(?1,?2,?3,?4)",
            params![
                c.scope.tenant.to_string(),
                c.scope.space.to_string(),
                decision.to_string(),
                ctx.principal.to_string()
            ],
        )
        .map_err(sql_error)?;
        tx.execute(
            "INSERT INTO conflict_outbox VALUES(?1,?2,?3,'pending')",
            params![
                c.scope.tenant.to_string(),
                c.scope.space.to_string(),
                decision.to_string()
            ],
        )
        .map_err(sql_error)?;
        if tx.execute("UPDATE space SET graph_version=graph_version+1 WHERE tenant_id=?1 AND id=?2 AND graph_version<9223372036854775807",params![c.scope.tenant.to_string(),c.scope.space.to_string()]).map_err(sql_error)?!=1{return Err(Error::Constraint)}
        ConflictReceipt {
            conflict,
            decision,
            version: natural(number)?,
            knowledge_seq: natural(seq)?,
            replayed: false,
        }
    };
    if let Some(hook) = db.before_commit.take() {
        hook();
    }
    let now = time(&db.clock)?;
    if now < start {
        return Err(Error::AuthorizationDenied);
    }
    authorize_scope(&tx, ctx, c.scope, "memory.revise", now)?;
    fence(&tx, c.scope)?;
    tx.commit().map_err(sql_error)?;
    Ok(out)
}
