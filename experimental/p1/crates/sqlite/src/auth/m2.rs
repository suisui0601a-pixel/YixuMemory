//! Profile 5, private cfg(test) no-content metadata. All reads take the SQLite writer lock.
use super::*;
use yixu_domain::{
    MemoryState,
    temporal::{Bound, Precision, TemporalMatch, Validity},
};
use yixu_ports::{memory::MemoryReceipt, memory_m2::*};
mod conflicts;
pub(super) mod tokens;
pub(super) const DDL: &str = include_str!("../../migrations/0005_memory_m2.sql");
const MAX_ROWS: usize = 10_000;

pub(super) fn insert_time(
    tx: &Transaction<'_>,
    scope: Scope,
    revision: Id,
    version: i64,
    v: Validity,
) -> Result<()> {
    v.validate()?;
    let (known, from, to) = match v {
        Validity::Unknown => (false, None, None),
        Validity::Known { from, to } => (true, Some(from), to),
    };
    tx.execute(
        "INSERT INTO revision_time VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            scope.tenant.to_string(),
            scope.space.to_string(),
            revision.to_string(),
            version,
            known,
            from.map(|b| b.earliest_ms),
            from.map(|b| b.latest_ms),
            from.map(|b| b.precision.code()),
            to.map(|b| b.earliest_ms),
            to.map(|b| b.latest_ms),
            to.map(|b| b.precision.code())
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}
type TimeRow = (
    bool,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
);
fn parse_time(row: TimeRow) -> Result<Validity> {
    let (known, a, b, p, c, d, q) = row;
    let v = if !known {
        if [a, b, p, c, d, q].iter().any(Option::is_some) {
            return Err(Error::Integrity);
        }
        Validity::Unknown
    } else {
        let from = Bound {
            earliest_ms: a.ok_or(Error::Integrity)?,
            latest_ms: b.ok_or(Error::Integrity)?,
            precision: Precision::from_code(p.ok_or(Error::Integrity)?)?,
        };
        let to = match (c, d, q) {
            (None, None, None) => None,
            (Some(c), Some(d), Some(q)) => Some(Bound {
                earliest_ms: c,
                latest_ms: d,
                precision: Precision::from_code(q)?,
            }),
            _ => return Err(Error::Integrity),
        };
        Validity::Known { from, to }
    };
    v.validate().map_err(|_| Error::Integrity)?;
    Ok(v)
}
fn parse_id(s: String) -> Result<Id> {
    s.parse().map_err(|_| Error::Integrity)
}
fn fence(tx: &Transaction<'_>, scope: Scope) -> Result<()> {
    let closed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM deletion_request WHERE tenant_id=?1 AND space_id=?2) OR EXISTS(SELECT 1 FROM revision_invalidation WHERE tenant_id=?1 AND space_id=?2) OR EXISTS(SELECT 1 FROM memory WHERE tenant_id=?1 AND space_id=?2 AND state<>'source_blocked')",params![scope.tenant.to_string(),scope.space.to_string()],|r|r.get(0)).map_err(sql_error)?;
    if closed {
        return Err(Error::CapabilityUnavailable);
    }
    Ok(())
}
fn integrity(tx: &Transaction<'_>, scope: Scope) -> Result<()> {
    let tenant = scope.tenant.to_string();
    let space = scope.space.to_string();
    let fk: i64 = tx
        .pragma_query_value(None, "foreign_keys", |r| r.get(0))
        .map_err(sql_error)?;
    if fk != 1 {
        return Err(Error::Integrity);
    }
    for (table, budget) in [
        ("memory", 1000),
        ("revision", MAX_ROWS),
        ("evidence", MAX_ROWS),
        ("conflict_decision", MAX_ROWS),
    ] {
        let count:i64=tx.query_row(&format!("SELECT count(*) FROM (SELECT 1 FROM {table} WHERE tenant_id=?1 AND space_id=?2 LIMIT ?3)"),params![tenant,space,i64::try_from(budget+1).map_err(|_|Error::ResourceUnavailable)?],|r|r.get(0)).map_err(sql_error)?;
        if count > i64::try_from(budget).map_err(|_| Error::ResourceUnavailable)? {
            return Err(Error::ResourceUnavailable);
        }
    }
    let root_ok:bool=tx.query_row("SELECT t.knowledge_seq=coalesce((SELECT max(c.knowledge_seq) FROM commit_record c WHERE c.tenant_id=t.id),0) FROM tenant t WHERE t.id=?1",[tenant.clone()],|r|r.get(0)).map_err(sql_error)?;
    if !root_ok {
        return Err(Error::Integrity);
    }
    let future:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM revision r JOIN tenant t ON t.id=r.tenant_id LEFT JOIN commit_record c ON c.tenant_id=r.tenant_id AND c.knowledge_seq=r.knowledge_seq LEFT JOIN revision p ON p.tenant_id=r.tenant_id AND p.space_id=r.space_id AND p.id=r.parent_revision_id WHERE r.tenant_id=?1 AND r.space_id=?2 AND (c.knowledge_seq IS NULL OR r.knowledge_seq>t.knowledge_seq OR p.knowledge_seq>r.knowledge_seq)) OR EXISTS(SELECT 1 FROM revision_evidence re JOIN revision r ON r.tenant_id=re.tenant_id AND r.space_id=re.space_id AND r.id=re.revision_id JOIN evidence e ON e.tenant_id=re.tenant_id AND e.space_id=re.space_id AND e.id=re.evidence_id JOIN event v ON v.tenant_id=e.tenant_id AND v.space_id=e.space_id AND v.id=e.event_id WHERE re.tenant_id=?1 AND re.space_id=?2 AND v.knowledge_seq>r.knowledge_seq)",params![tenant,space],|r|r.get(0)).map_err(sql_error)?;
    if future {
        return Err(Error::Integrity);
    }
    // Check actual CURRENT before as_of/filters, on the same snapshot as the returned envelopes.
    let broken:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM memory m LEFT JOIN revision r ON r.tenant_id=m.tenant_id AND r.space_id=m.space_id AND r.memory_id=m.id AND r.id=m.current_revision_id WHERE m.tenant_id=?1 AND m.space_id=?2 AND (r.id IS NULL OR r.number<>(SELECT max(x.number) FROM revision x WHERE x.tenant_id=m.tenant_id AND x.space_id=m.space_id AND x.memory_id=m.id) OR r.number<>(SELECT count(*) FROM revision x WHERE x.tenant_id=m.tenant_id AND x.space_id=m.space_id AND x.memory_id=m.id)))",params![tenant,space],|r|r.get(0)).map_err(sql_error)?;
    if broken {
        return Err(Error::Integrity);
    }
    let broken:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM revision r LEFT JOIN memory m ON m.tenant_id=r.tenant_id AND m.space_id=r.space_id AND m.id=r.memory_id LEFT JOIN revision p ON p.tenant_id=r.tenant_id AND p.space_id=r.space_id AND p.memory_id=r.memory_id AND p.id=r.parent_revision_id AND p.number=r.number-1 LEFT JOIN revision_evidence e ON e.tenant_id=r.tenant_id AND e.space_id=r.space_id AND e.revision_id=r.id AND e.evidence_id=r.primary_evidence_id LEFT JOIN revision_time tm ON tm.tenant_id=r.tenant_id AND tm.space_id=r.space_id AND tm.revision_id=r.id LEFT JOIN payload b ON b.tenant_id=r.tenant_id AND b.space_id=r.space_id AND b.id=r.content_payload_id WHERE r.tenant_id=?1 AND r.space_id=?2 AND (m.id IS NULL OR e.evidence_id IS NULL OR tm.revision_id IS NULL OR b.id IS NULL OR b.purpose<>'content' OR b.state<>'blocked' OR (r.number=1 AND (r.parent_revision_id IS NOT NULL OR r.parent_number IS NOT NULL)) OR (r.number>1 AND (p.id IS NULL OR r.parent_number IS NULL OR r.parent_number<>r.number-1))))",params![tenant,space],|r|r.get(0)).map_err(sql_error)?;
    if broken {
        return Err(Error::Integrity);
    }
    let broken:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM revision_evidence re LEFT JOIN revision r ON r.tenant_id=re.tenant_id AND r.space_id=re.space_id AND r.id=re.revision_id LEFT JOIN evidence e ON e.tenant_id=re.tenant_id AND e.space_id=re.space_id AND e.id=re.evidence_id WHERE re.tenant_id=?1 AND re.space_id=?2 AND (r.id IS NULL OR e.id IS NULL)) OR EXISTS(SELECT 1 FROM evidence e LEFT JOIN event v ON v.tenant_id=e.tenant_id AND v.space_id=e.space_id AND v.id=e.event_id LEFT JOIN payload b ON b.tenant_id=e.tenant_id AND b.space_id=e.space_id AND b.id=e.payload_id LEFT JOIN payload l ON l.tenant_id=e.tenant_id AND l.space_id=e.space_id AND l.id=e.locator_payload_id WHERE e.tenant_id=?1 AND e.space_id=?2 AND (v.id IS NULL OR b.id IS NULL OR l.id IS NULL OR b.purpose<>'content' OR l.purpose<>'locator' OR b.state<>'blocked' OR l.state<>'blocked'))",params![tenant,space],|r|r.get(0)).map_err(sql_error)?;
    if broken {
        return Err(Error::Integrity);
    }
    Ok(())
}
fn live_seq(tx: &Transaction<'_>, ctx: &AuthContext) -> Result<u64> {
    let seq: i64 = tx
        .query_row(
            "SELECT knowledge_seq FROM tenant WHERE id=?1",
            [ctx.scope.tenant.to_string()],
            |r| r.get(0),
        )
        .map_err(sql_error)?;
    natural(seq)
}
fn snapshot(
    authority: &tokens::TokenAuthority,
    ctx: &AuthContext,
    provided: Option<&KnowledgeToken>,
    live: u64,
) -> Result<u64> {
    let seq = provided
        .map(|t| authority.open_knowledge(ctx, t))
        .transpose()?
        .unwrap_or(live);
    if seq > live {
        return Err(Error::InvalidToken);
    }
    Ok(seq)
}
fn fingerprint(ctx: &AuthContext, q: &MetadataQuery) -> String {
    let mode = match q.mode {
        QueryMode::One(id) => format!("one:{id}"),
        QueryMode::Current => "current".to_owned(),
        QueryMode::History(id) => format!("history:{id}"),
    };
    let bytes = Sha256::digest(
        format!(
            "metadata-query/v1|{}|{}|{}|{}|{}|{}|{mode}|{:?}|{}|{:?}|{:?}|{}",
            ctx.realm,
            ctx.scope.tenant,
            ctx.scope.space,
            ctx.principal,
            ctx.tenant_epoch,
            ctx.space_epoch,
            q.valid_at,
            q.include_uncertain,
            q.evidence,
            q.assertion_kind,
            q.limit
        )
        .as_bytes(),
    );
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn read_rows(tx: &Transaction<'_>, q: &MetadataQuery, seq: u64) -> Result<Vec<RevisionMetadata>> {
    let (history, memory) = match q.mode {
        QueryMode::Current => (false, None),
        QueryMode::One(id) => (false, Some(id.to_string())),
        QueryMode::History(id) => (true, Some(id.to_string())),
    };
    let records = {
        let mut statement=tx.prepare("SELECT r.memory_id,r.id,r.number,r.knowledge_seq,r.recorded_at_ms,r.assertion_kind,r.primary_evidence_id,tm.memory_version_at_revision,tm.known,tm.from_min,tm.from_max,tm.from_precision,tm.to_min,tm.to_max,tm.to_precision FROM revision r JOIN revision_time tm ON tm.tenant_id=r.tenant_id AND tm.space_id=r.space_id AND tm.revision_id=r.id WHERE r.tenant_id=?1 AND r.space_id=?2 AND r.knowledge_seq<=?3 AND (?4 IS NULL OR r.memory_id=?4) AND (?5 OR r.number=(SELECT max(x.number) FROM revision x WHERE x.tenant_id=r.tenant_id AND x.space_id=r.space_id AND x.memory_id=r.memory_id AND x.knowledge_seq<=?3)) AND (?6 IS NULL OR r.assertion_kind=?6) AND (?7 IS NULL OR EXISTS(SELECT 1 FROM revision_evidence e WHERE e.tenant_id=r.tenant_id AND e.space_id=r.space_id AND e.revision_id=r.id AND e.evidence_id=?7))").map_err(sql_error)?;
        statement
            .query_map(
                params![
                    q.scope.tenant.to_string(),
                    q.scope.space.to_string(),
                    i64::try_from(seq).map_err(|_| Error::InvalidToken)?,
                    memory,
                    history,
                    q.assertion_kind.map(AssertionKind::text),
                    q.evidence.map(|v| v.to_string())
                ],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, i64>(7)?,
                        (
                            r.get::<_, bool>(8)?,
                            r.get::<_, Option<i64>>(9)?,
                            r.get::<_, Option<i64>>(10)?,
                            r.get::<_, Option<i64>>(11)?,
                            r.get::<_, Option<i64>>(12)?,
                            r.get::<_, Option<i64>>(13)?,
                            r.get::<_, Option<i64>>(14)?,
                        ),
                    ))
                },
            )
            .map_err(sql_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_error)?
    };
    let mut rows = Vec::new();
    for (memory, revision, number, knowledge_seq, recorded_at_ms, kind, primary, version, time) in
        records
    {
        let validity = parse_time(time)?;
        let matched = q.valid_at.map(|v| validity.at(v));
        if matches!(matched, Some(TemporalMatch::Outside))
            || !q.include_uncertain
                && matches!(
                    matched,
                    Some(TemporalMatch::Unknown | TemporalMatch::Possible)
                )
        {
            continue;
        }
        let mut statement=tx.prepare("SELECT e.id,e.event_id,v.occurred_at_ms,v.recorded_at_ms,v.knowledge_seq FROM revision_evidence re JOIN evidence e ON e.tenant_id=re.tenant_id AND e.space_id=re.space_id AND e.id=re.evidence_id JOIN event v ON v.tenant_id=e.tenant_id AND v.space_id=e.space_id AND v.id=e.event_id WHERE re.tenant_id=?1 AND re.space_id=?2 AND re.revision_id=?3 ORDER BY e.id").map_err(sql_error)?;
        let values = statement
            .query_map(
                params![
                    q.scope.tenant.to_string(),
                    q.scope.space.to_string(),
                    revision
                ],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                },
            )
            .map_err(sql_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        let evidence = values
            .into_iter()
            .map(|(id, event, occurred, recorded, seq)| {
                Ok(EvidenceMetadata {
                    id: parse_id(id)?,
                    event: parse_id(event)?,
                    occurred_at_ms: occurred,
                    recorded_at_ms: recorded,
                    knowledge_seq: natural(seq)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let kind = match kind.as_str() {
            "extracted_fact" => AssertionKind::ExtractedFact,
            "model_inference" => AssertionKind::ModelInference,
            "fictional_setting" => AssertionKind::FictionalSetting,
            "user_assertion" => AssertionKind::UserAssertion,
            _ => return Err(Error::Integrity),
        };
        let conflicts = {
            let mut s=tx.prepare("SELECT c.id FROM conflict c JOIN revision l ON l.tenant_id=c.tenant_id AND l.space_id=c.space_id AND l.id=c.left_revision_id JOIN revision r ON r.tenant_id=c.tenant_id AND r.space_id=c.space_id AND r.id=c.right_revision_id WHERE c.tenant_id=?1 AND c.space_id=?2 AND c.knowledge_seq<=?3 AND (l.memory_id=?4 OR r.memory_id=?4) ORDER BY c.id").map_err(sql_error)?;
            s.query_map(
                params![
                    q.scope.tenant.to_string(),
                    q.scope.space.to_string(),
                    i64::try_from(seq).map_err(|_| Error::InvalidToken)?,
                    memory
                ],
                |r| r.get::<_, String>(0),
            )
            .map_err(sql_error)?
            .map(|r| parse_id(r.map_err(sql_error)?))
            .collect::<Result<Vec<_>>>()?
        };
        rows.push(RevisionMetadata {
            memory: parse_id(memory)?,
            revision: parse_id(revision)?,
            number: natural(number)?,
            memory_version_at_revision: natural(version)?,
            knowledge_seq: natural(knowledge_seq)?,
            recorded_at_ms,
            validity,
            temporal_match: matched,
            assertion_kind: kind,
            primary_evidence: parse_id(primary)?,
            evidence,
            state: MemoryState::SourceBlocked,
            content_available: false,
            conflicts,
        });
    }
    rows.sort_by_key(|r| {
        (
            r.knowledge_seq,
            r.memory.to_string(),
            r.number,
            r.revision.to_string(),
        )
    });
    Ok(rows)
}
fn audit_read(
    tx: &Transaction<'_>,
    ctx: &AuthContext,
    seq: u64,
    fingerprint: &str,
    now: i64,
    ids: &mut dyn ResourceIds,
) -> Result<()> {
    let id = allocate_free(
        tx,
        ctx.scope.tenant,
        "SELECT EXISTS(SELECT 1 FROM metadata_read_audit WHERE tenant_id=?1 AND id=?2)",
        ids,
    )?;
    tx.execute(
        "INSERT INTO metadata_read_audit VALUES(?1,?2,?3,?4,?5,?6,?7)",
        params![
            ctx.scope.tenant.to_string(),
            ctx.scope.space.to_string(),
            id.to_string(),
            ctx.principal.to_string(),
            i64::try_from(seq).map_err(|_| Error::InvalidToken)?,
            Sha256::digest(fingerprint.as_bytes()).to_vec(),
            now
        ],
    )
    .map_err(sql_error)?;
    Ok(())
}
impl<C: Clock> MemoryM2 for AuthDatabase<C> {
    type Context = AuthContext;
    fn write_temporal(
        &mut self,
        ctx: Option<&AuthContext>,
        command: TemporalWrite,
    ) -> Result<MemoryReceipt> {
        if self.profile != 5 {
            return Err(Error::CapabilityUnavailable);
        }
        self.apply_envelope(ctx, command.write, Some(command.validity))
    }
    fn query_metadata(
        &mut self,
        context: Option<&AuthContext>,
        q: MetadataQuery,
    ) -> Result<MetadataPage> {
        if self.profile != 5 {
            return Err(Error::CapabilityUnavailable);
        }
        let authority = self
            .query_tokens
            .clone()
            .ok_or(Error::CapabilityUnavailable)?;
        let ctx = context.ok_or(Error::AuthorizationDenied)?;
        let capability = match q.mode {
            QueryMode::Current => "memory.search",
            QueryMode::One(_) => "memory.read",
            QueryMode::History(_) => "memory.history",
        };
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let start = time(&self.clock)?;
        authorize_scope(&tx, ctx, q.scope, capability, start)?;
        if q.retrieval != Retrieval::Metadata || q.state != MemoryState::SourceBlocked {
            return Err(Error::UnsupportedQuery);
        }
        if q.limit == 0 || q.limit > 100 {
            return Err(Error::InvalidInput);
        }
        fence(&tx, ctx.scope)?;
        integrity(&tx, ctx.scope)?;
        let live = live_seq(&tx, ctx)?;
        let hash = fingerprint(ctx, &q);
        let (seq, offset) = if let Some(c) = &q.cursor {
            let (seq, offset) = authority.open_cursor(&hash, c)?;
            if seq > live {
                return Err(Error::InvalidCursor);
            }
            if let Some(token) = &q.as_of
                && authority.open_knowledge(ctx, token)? != seq
            {
                return Err(Error::InvalidCursor);
            }
            (seq, offset)
        } else {
            (snapshot(&authority, ctx, q.as_of.as_ref(), live)?, 0)
        };
        let all = read_rows(&tx, &q, seq)?;
        if offset > all.len() {
            return Err(Error::InvalidCursor);
        }
        let end = offset.saturating_add(usize::from(q.limit)).min(all.len());
        let page = MetadataPage {
            rows: all[offset..end].to_vec(),
            snapshot: authority.knowledge(ctx, seq)?,
            next_cursor: if end < all.len() {
                Some(authority.cursor(&hash, seq, end)?)
            } else {
                None
            },
        };
        audit_read(&tx, ctx, seq, &hash, start, self.ids.as_mut())?;
        if let Some(hook) = self.before_commit.take() {
            hook();
        }
        let end_time = time(&self.clock)?;
        if end_time < start {
            return Err(Error::AuthorizationDenied);
        }
        authorize_scope(&tx, ctx, q.scope, capability, end_time)?;
        fence(&tx, ctx.scope)?;
        tx.commit().map_err(sql_error)?;
        Ok(page)
    }
    fn write_conflict(
        &mut self,
        ctx: Option<&AuthContext>,
        command: ConflictWrite,
    ) -> Result<ConflictReceipt> {
        conflicts::write(self, ctx, command)
    }
    fn conflict_history(
        &mut self,
        ctx: Option<&AuthContext>,
        query: ConflictQuery,
    ) -> Result<ConflictHistory> {
        conflicts::read(self, ctx, query)
    }
}
#[cfg(test)]
mod tests;

pub(super) fn check_for_write(tx: &Transaction<'_>, scope: Scope) -> Result<()> {
    fence(tx, scope)?;
    integrity(tx, scope)
}
