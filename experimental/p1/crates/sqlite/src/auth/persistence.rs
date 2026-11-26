//! PRIVATE Windows synthetic persistent-content experiment. Not a Runtime adapter.
use super::*;
use std::path::PathBuf;
use yixu_domain::security_m3::{OwnerKind, PayloadBinding, SecurityScope};
#[path = "persistence/locked.rs"]
mod secure;
use secure::LockedBuffer;
const CANARY: &[u8] = b"M3B SYNTHETIC ONLY: blue marble remembers 42";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    None,
    Stage,
    Published,
    Seal,
    T1,
    Reply,
}
#[derive(Clone, Copy)]
enum IoFault {
    None,
    Enospc,
    ShortWrite,
    Sync,
}
mod broker;
mod content_crypto;
mod dpapi;
mod local_keys;
pub(crate) mod marker_io;
mod output;
mod profile10;
#[path = "persistence/winfs.rs"]
mod winfs;
use dpapi::LocalVault;
struct Store {
    root: PathBuf,
    anchor: winfs::Anchor,
}
fn envelope(b: &[u8]) -> Result<()> {
    if b.len() < 215 || &b[..8] != b"YIXUB001" {
        return Err(Error::Constraint);
    }
    let n = u32::from_be_bytes(b[8..12].try_into().unwrap()) as usize;
    if n == 0 || n > 4096 || b.len() != 215 + n {
        return Err(Error::Constraint);
    }
    Ok(())
}
impl Store {
    fn new(root: PathBuf) -> Result<Self> {
        let anchor = winfs::Anchor::open(&root).map_err(|_| Error::CapabilityUnavailable)?;
        Ok(Self { root, anchor })
    }
    fn path(&self, p: Id) -> PathBuf {
        self.root.join(p.to_string())
    }
    fn publish(&self, p: Id, b: &[u8]) -> Result<()> {
        self.publish_fault(p, b, IoFault::None)
    }
    fn publish_fault(&self, p: Id, b: &[u8], fault: IoFault) -> Result<()> {
        envelope(b)?;
        if matches!(fault, IoFault::Enospc) {
            return Err(Error::Database);
        }
        self.anchor
            .publish(
                &format!("pending-{}", fresh()?),
                &p.to_string(),
                b,
                match fault {
                    IoFault::ShortWrite => 1,
                    IoFault::Sync => 2,
                    _ => 0,
                },
                |point| {
                    if std::env::var("YIXU_W1_POINT").ok() == Some(point.to_string()) {
                        let ready = std::env::var_os("YIXU_W1_READY").unwrap();
                        std::fs::write(ready, b"ready").unwrap();
                        loop {
                            std::thread::park_timeout(Duration::from_secs(1));
                        }
                    }
                },
            )
            .map_err(|_| Error::ResourceUnavailable)
    }
    fn read(&self, p: Id) -> Result<Vec<u8>> {
        self.anchor.read(&p.to_string()).map_err(|e| {
            if e.raw_os_error() == Some(2) {
                Error::ResourceUnavailable
            } else {
                Error::Constraint
            }
        })
    }
}
#[derive(Clone)]
struct Command {
    key: Id,
    target: Option<(Id, i64)>,
    variant: u8,
}
struct Plan {
    event: Id,
    evidence: Id,
    memory: Id,
    revision: Id,
    copies: Vec<(Id, Id, Id, i64, String)>,
}
fn fresh() -> Result<Id> {
    let mut b = [0; 16];
    getrandom::fill(&mut b).map_err(|_| Error::ResourceUnavailable)?;
    Ok(Id::from_bytes(b))
}
fn pause_if(stop: Stop) {
    if std::env::var("YIXU_M3B_KILL").ok().as_deref() == Some(format!("{stop:?}").as_str()) {
        let ready = std::env::var_os("YIXU_M3B_READY").unwrap();
        std::fs::write(ready, b"ready").unwrap();
        loop {
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }
}
fn raw(id: Id) -> [u8; 16] {
    let s = id.to_string().replace("-", "");
    let mut out = [0; 16];
    for (i, v) in out.iter_mut().enumerate() {
        *v = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap();
    }
    out
}
fn hash(b: &[u8]) -> Vec<u8> {
    Sha256::digest(b).to_vec()
}
fn request(ctx: &AuthContext, c: &Command) -> Vec<u8> {
    hash(
        format!(
            "synthetic-m3b/v1|{}|{}|{:?}|{}",
            ctx.scope.tenant, ctx.scope.space, c.target, c.variant
        )
        .as_bytes(),
    )
}
fn binding(ctx: &AuthContext, p: Id, key: Id, owner: Id, kind: i64) -> PayloadBinding {
    PayloadBinding {
        scope: SecurityScope {
            realm: raw(ctx.realm),
            tenant: raw(ctx.scope.tenant),
            space: raw(ctx.scope.space),
        },
        owner_kind: match kind {
            1 => OwnerKind::Event,
            2 => OwnerKind::Evidence,
            _ => OwnerKind::Revision,
        },
        owner: raw(owner),
        payload: raw(p),
        key: raw(key),
        seal_generation: 1,
        vault_incarnation: 1,
        protection: [91; 32],
    }
}
// TEST KEY BACKEND: publicly derivable synthetic keys; no secrecy or deletion claims.
// It exists only in the unit-test binary. No KeyVault implementation is exported.
fn lab_key(key: Id) -> Result<LockedBuffer> {
    let mut k = LockedBuffer::new(32, false).map_err(|_| Error::CapabilityUnavailable)?;
    k.bytes_mut().copy_from_slice(&Sha256::digest(raw(key)));
    Ok(k)
}
fn synthetic(variant: u8) -> Result<&'static [u8]> {
    match variant {
        0 => Ok(CANARY),
        1 => Ok(b"M3B SYNTHETIC ONLY: blue marble remembers 43"),
        _ => Err(Error::InvalidInput),
    }
}
#[cfg(test)]
fn seal(b: &PayloadBinding) -> Result<Vec<u8>> {
    seal_variant(b, 0)
}
fn seal_variant(b: &PayloadBinding, variant: u8) -> Result<Vec<u8>> {
    let key = lab_key(Id::from_bytes(b.key))?;
    seal_using_key(b, variant, &key)
}
fn seal_using_key(b: &PayloadBinding, variant: u8, key: &LockedBuffer) -> Result<Vec<u8>> {
    content_crypto::seal(b, synthetic(variant)?, key)
}
fn open(b: &PayloadBinding, bytes: &[u8]) -> Result<Vec<u8>> {
    let k = lab_key(Id::from_bytes(b.key))?;
    open_using_key(b, bytes, &k)
}
fn open_using_key(b: &PayloadBinding, bytes: &[u8], k: &LockedBuffer) -> Result<Vec<u8>> {
    Ok(hash(decrypt_locked(b, bytes, k)?.bytes()))
}
fn decrypt_locked(b: &PayloadBinding, bytes: &[u8], k: &LockedBuffer) -> Result<LockedBuffer> {
    if content_crypto::encoded_length(bytes)? != CANARY.len() {
        return Err(Error::Constraint);
    }
    content_crypto::open(b, bytes, k)
}
impl<C: Clock> AuthDatabase<C> {
    fn content_check(tx: &Transaction<'_>, ctx: &AuthContext, now: i64, cap: &str) -> Result<()> {
        authorize_scope(tx, ctx, ctx.scope, cap, now)?;
        let blocked:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM deletion_request WHERE tenant_id=?1 AND space_id=?2) OR EXISTS(SELECT 1 FROM revision_invalidation WHERE tenant_id=?1 AND space_id=?2)",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string()],|r|r.get(0)).map_err(sql_error)?;
        if blocked {
            return Err(Error::CapabilityUnavailable);
        }
        Ok(())
    }
    fn plan(&self, ctx: &AuthContext, key: Id) -> Result<Plan> {
        let t = ctx.scope.tenant.to_string();
        let p = ctx.principal.to_string();
        let (e,v,m,r):(String,String,String,String)=self.connection.query_row("SELECT event_id,evidence_id,memory_id,revision_id FROM content_intent WHERE tenant_id=?1 AND space_id=?2 AND principal_id=?3 AND id=?4",params![t,ctx.scope.space.to_string(),p,key.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(sql_error)?;
        let mut q=self.connection.prepare("SELECT payload_id,key_id,owner_id,owner_kind,purpose FROM sealed_copy WHERE tenant_id=?1 AND principal_id=?2 AND intent_id=?3 ORDER BY owner_kind,purpose").map_err(sql_error)?;
        let raw = q
            .query_map(params![t, p, key.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .map_err(sql_error)?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(sql_error)?;
        let copies = raw
            .into_iter()
            .map(|(a, b, c, d, e)| Ok((a.parse()?, b.parse()?, c.parse()?, d, e)))
            .collect::<Result<Vec<_>>>()?;
        Ok(Plan {
            event: e.parse()?,
            evidence: v.parse()?,
            memory: m.parse()?,
            revision: r.parse()?,
            copies,
        })
    }
    fn stage(&mut self, ctx: &AuthContext, c: &Command) -> Result<String> {
        synthetic(c.variant)?;
        if ![6, 7, 8].contains(&self.profile) {
            return Err(Error::CapabilityUnavailable);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        Self::content_check(
            &tx,
            ctx,
            time(&self.clock)?,
            if c.target.is_some() {
                "memory.revise"
            } else {
                "memory.append"
            },
        )?;
        let old:Option<(Vec<u8>,String)>=tx.query_row("SELECT digest,state FROM content_intent WHERE tenant_id=?1 AND principal_id=?2 AND id=?3",params![ctx.scope.tenant.to_string(),ctx.principal.to_string(),c.key.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql_error)?;
        if let Some((d, s)) = old {
            if d != request(ctx, c) {
                return Err(Error::IdempotencyConflict);
            }
            tx.commit().map_err(sql_error)?;
            return Ok(s);
        }
        let (ev, e, m, r) = (
            fresh()?,
            fresh()?,
            c.target.map(|v| v.0).unwrap_or(fresh()?),
            fresh()?,
        );
        tx.execute(
            "INSERT INTO content_intent VALUES(?1,?2,?3,?4,?5,'staged',?6,?7,?8,?9,?10)",
            params![
                ctx.scope.tenant.to_string(),
                ctx.scope.space.to_string(),
                ctx.principal.to_string(),
                c.key.to_string(),
                request(ctx, c),
                ev.to_string(),
                e.to_string(),
                m.to_string(),
                r.to_string(),
                c.target.map(|v| v.1).unwrap_or(0)
            ],
        )
        .map_err(sql_error)?;
        for (owner, kind, purpose) in [
            (ev, 1, "content"),
            (e, 2, "content"),
            (e, 2, "locator"),
            (r, 3, "content"),
        ] {
            tx.execute(
                "INSERT INTO sealed_copy VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,1,0,NULL)",
                params![
                    ctx.scope.tenant.to_string(),
                    ctx.scope.space.to_string(),
                    ctx.principal.to_string(),
                    c.key.to_string(),
                    fresh()?.to_string(),
                    owner.to_string(),
                    kind,
                    purpose,
                    fresh()?.to_string()
                ],
            )
            .map_err(sql_error)?;
        }
        tx.commit().map_err(sql_error)?;
        Ok("staged".into())
    }
    fn persist(&mut self, ctx: &AuthContext, c: &Command, store: &Store, stop: Stop) -> Result<Id> {
        self.persist_backend(ctx, c, store, stop, None)
    }
    fn persist_backend(
        &mut self,
        ctx: &AuthContext,
        c: &Command,
        store: &Store,
        stop: Stop,
        vault: Option<&LocalVault>,
    ) -> Result<Id> {
        if !matches!((self.profile, vault.is_some()), (6, false) | (7 | 8, true)) {
            return Err(Error::CapabilityUnavailable);
        }
        let state = self.stage(ctx, c)?;
        let plan = self.plan(ctx, c.key)?;
        if plan.copies.len() != 4 {
            return Err(Error::Constraint);
        }
        if state == "committed" {
            return Ok(plan.memory);
        }
        if stop == Stop::Stage {
            pause_if(stop);
            return Err(Error::Database);
        }
        // All RNG/AEAD/file I/O occur OUTSIDE SQLite write transactions.
        if state == "staged" {
            let mut receipts = Vec::new();
            for &(p, k, o, kind, _) in &plan.copies {
                let b = binding(ctx, p, k, o, kind);
                let bytes = match store.read(p) {
                    Ok(existing) => {
                        let attempted:bool=self.connection.query_row("SELECT attempted=1 FROM sealed_copy WHERE tenant_id=?1 AND payload_id=?2",params![ctx.scope.tenant.to_string(),p.to_string()],|r|r.get(0)).map_err(sql_error)?;
                        if !attempted {
                            return Err(Error::CapabilityUnavailable);
                        }
                        self.open_backend(
                            ctx,
                            &b,
                            &existing,
                            vault,
                            if c.target.is_some() {
                                "memory.revise"
                            } else {
                                "memory.append"
                            },
                        )?;
                        existing
                    }
                    Err(Error::ResourceUnavailable) => {
                        // Persist the one-use claim BEFORE any key/nonce use. Unknown outcome is never resealed.
                        let tx = self
                            .connection
                            .transaction_with_behavior(TransactionBehavior::Immediate)
                            .map_err(sql_error)?;
                        Self::content_check(
                            &tx,
                            ctx,
                            time(&self.clock)?,
                            if c.target.is_some() {
                                "memory.revise"
                            } else {
                                "memory.append"
                            },
                        )?;
                        let changed=tx.execute("UPDATE sealed_copy SET attempted=1 WHERE tenant_id=?1 AND payload_id=?2 AND attempted=0",params![ctx.scope.tenant.to_string(),p.to_string()]).map_err(sql_error)?;
                        if changed != 1 {
                            return Err(Error::CapabilityUnavailable);
                        }
                        tx.commit().map_err(sql_error)?;
                        let bytes = if let Some(v) = vault {
                            let key = self.local_key(
                                ctx,
                                &b,
                                v,
                                if c.target.is_some() {
                                    "memory.revise"
                                } else {
                                    "memory.append"
                                },
                                true,
                            )?;
                            seal_using_key(&b, c.variant, &key)?
                        } else {
                            seal_variant(&b, c.variant)?
                        };
                        store.publish(p, &bytes)?;
                        bytes
                    }
                    Err(e) => return Err(e),
                };
                receipts.push((p, hash(&bytes)));
            }
            if stop == Stop::Published {
                pause_if(stop);
                return Err(Error::Database);
            }
            let tx = self
                .connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sql_error)?;
            Self::content_check(
                &tx,
                ctx,
                time(&self.clock)?,
                if c.target.is_some() {
                    "memory.revise"
                } else {
                    "memory.append"
                },
            )?;
            let current:String=tx.query_row("SELECT state FROM content_intent WHERE tenant_id=?1 AND principal_id=?2 AND id=?3",params![ctx.scope.tenant.to_string(),ctx.principal.to_string(),c.key.to_string()],|r|r.get(0)).map_err(sql_error)?;
            if !["staged", "sealed", "committed"].contains(&current.as_str()) {
                return Err(Error::CapabilityUnavailable);
            }
            let count:i64=tx.query_row("SELECT count(*) FROM sealed_copy WHERE tenant_id=?1 AND principal_id=?2 AND intent_id=?3 AND attempted=1",params![ctx.scope.tenant.to_string(),ctx.principal.to_string(),c.key.to_string()],|r|r.get(0)).map_err(sql_error)?;
            if receipts.len() != 4 || count != 4 {
                return Err(Error::Constraint);
            }
            for (p, h) in receipts {
                tx.execute("UPDATE sealed_copy SET envelope_hash=?3 WHERE tenant_id=?1 AND payload_id=?2 AND attempted=1 AND envelope_hash IS NULL",params![ctx.scope.tenant.to_string(),p.to_string(),&h]).map_err(sql_error)?;
                let actual:Vec<u8>=tx.query_row("SELECT envelope_hash FROM sealed_copy WHERE tenant_id=?1 AND payload_id=?2 AND attempted=1",params![ctx.scope.tenant.to_string(),p.to_string()],|r|r.get(0)).map_err(sql_error)?;
                if actual != h {
                    return Err(Error::Constraint);
                }
            }
            tx.execute("UPDATE content_intent SET state='sealed' WHERE tenant_id=?1 AND principal_id=?2 AND id=?3 AND state='staged'",params![ctx.scope.tenant.to_string(),ctx.principal.to_string(),c.key.to_string()]).map_err(sql_error)?;
            if vault.is_some() {
                Self::check_local_keys(&tx, ctx, c.key)?;
            }
            tx.commit().map_err(sql_error)?;
        } else if state != "sealed" {
            return Err(Error::CapabilityUnavailable);
        }
        if stop == Stop::Seal {
            pause_if(stop);
            return Err(Error::Database);
        }
        // Validate current immutable files before T1, not while holding the writer lock.
        self.validate_files_backend(
            ctx,
            c.key,
            store,
            vault,
            if c.target.is_some() {
                "memory.revise"
            } else {
                "memory.append"
            },
        )?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let now = time(&self.clock)?;
        Self::content_check(
            &tx,
            ctx,
            now,
            if c.target.is_some() {
                "memory.revise"
            } else {
                "memory.append"
            },
        )?;
        let status: String = tx
            .query_row(
                "SELECT state FROM content_intent WHERE tenant_id=?1 AND principal_id=?2 AND id=?3",
                params![
                    ctx.scope.tenant.to_string(),
                    ctx.principal.to_string(),
                    c.key.to_string()
                ],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        if status == "committed" {
            tx.commit().map_err(sql_error)?;
            return Ok(plan.memory);
        }
        if status != "sealed" {
            return Err(Error::CapabilityUnavailable);
        }
        let (version, number, parent) = if let Some((m, expected)) = c.target {
            let row:Option<(i64,i64,String,String)>=tx.query_row("SELECT m.version,r.number,r.id,m.state FROM memory m JOIN revision r ON r.tenant_id=m.tenant_id AND r.space_id=m.space_id AND r.memory_id=m.id AND r.id=m.current_revision_id WHERE m.tenant_id=?1 AND m.space_id=?2 AND m.id=?3",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),m.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(sql_error)?;
            let (v, n, p, s) = row.ok_or(Error::AuthorizationDenied)?;
            if s != "active" {
                return Err(Error::CapabilityUnavailable);
            }
            if v != expected {
                return Err(Error::CasConflict);
            }
            (v + 1, n + 1, Some(p))
        } else {
            (1, 1, None)
        };
        tx.execute(
            "UPDATE tenant SET knowledge_seq=knowledge_seq+1 WHERE id=?1",
            [ctx.scope.tenant.to_string()],
        )
        .map_err(sql_error)?;
        let seq: i64 = tx
            .query_row(
                "SELECT knowledge_seq FROM tenant WHERE id=?1",
                [ctx.scope.tenant.to_string()],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        tx.execute(
            "INSERT INTO commit_record VALUES(?1,?2,?3)",
            params![ctx.scope.tenant.to_string(), seq, now],
        )
        .map_err(sql_error)?;
        for &(p, k, _, _, ref purpose) in &plan.copies {
            tx.execute("INSERT INTO payload VALUES(?1,?2,?3,?4,'synthetic','sensitive',1,?3,?5,'synthetic-xchacha-v1',?3,1,'available')",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),p.to_string(),purpose,k.to_string()]).map_err(sql_error)?;
        }
        for &(p, _, _, _, _) in &plan.copies {
            for store_kind in ["payload", "key_backend"] {
                tx.execute(
                    "INSERT INTO copy_manifest VALUES(?1,?2,?3,?4,1,?5,'registered')",
                    params![
                        ctx.scope.tenant.to_string(),
                        ctx.scope.space.to_string(),
                        fresh()?.to_string(),
                        p.to_string(),
                        store_kind
                    ],
                )
                .map_err(sql_error)?;
            }
        }
        let ep = plan.copies.iter().find(|v| v.3 == 1).unwrap().0;
        let bp = plan
            .copies
            .iter()
            .find(|v| v.3 == 2 && v.4 == "content")
            .unwrap()
            .0;
        let lp = plan.copies.iter().find(|v| v.4 == "locator").unwrap().0;
        let rp = plan.copies.iter().find(|v| v.3 == 3).unwrap().0;
        let (t, s) = (ctx.scope.tenant.to_string(), ctx.scope.space.to_string());
        tx.execute(
            "INSERT INTO event VALUES(?1,?2,?3,'user_statement',?4,?5,?6,?6,?7,'1.0')",
            params![
                t,
                s,
                plan.event.to_string(),
                ep.to_string(),
                ctx.principal.to_string(),
                now,
                seq
            ],
        )
        .map_err(sql_error)?;
        tx.execute(
            "INSERT INTO evidence VALUES(?1,?2,?3,?4,?5,?6,'user_statement',?7)",
            params![
                t,
                s,
                plan.evidence.to_string(),
                plan.event.to_string(),
                bp.to_string(),
                lp.to_string(),
                now
            ],
        )
        .map_err(sql_error)?;
        if c.target.is_none() {
            tx.execute(
                "INSERT INTO memory VALUES(?1,?2,?3,?4,1,'active',?5)",
                params![
                    t,
                    s,
                    plan.memory.to_string(),
                    plan.revision.to_string(),
                    now
                ],
            )
            .map_err(sql_error)?;
        }
        tx.execute("INSERT INTO revision VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'user_assertion',?10,NULL,?10,?11)",params![t,s,plan.revision.to_string(),plan.memory.to_string(),number,parent,if number==1{None}else{Some(number-1)},plan.evidence.to_string(),rp.to_string(),now,seq]).map_err(sql_error)?;
        tx.execute(
            "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,'supports')",
            params![t, s, plan.revision.to_string(), plan.evidence.to_string()],
        )
        .map_err(sql_error)?;
        m2::insert_time(
            &tx,
            ctx.scope,
            plan.revision,
            version,
            yixu_domain::temporal::Validity::Unknown,
        )?;
        if c.target.is_some() {
            let n=tx.execute("UPDATE memory SET current_revision_id=?4,version=?5 WHERE tenant_id=?1 AND space_id=?2 AND id=?3 AND version=?6 AND state='active'",params![t,s,plan.memory.to_string(),plan.revision.to_string(),version,version-1]).map_err(sql_error)?;
            if n != 1 {
                return Err(Error::CasConflict);
            }
        }
        tx.execute(
            "INSERT INTO memory_audit VALUES(?1,?2,?3,?4,?5)",
            params![
                t,
                s,
                plan.revision.to_string(),
                ctx.principal.to_string(),
                if c.target.is_some() {
                    "memory.revise"
                } else {
                    "memory.append"
                }
            ],
        )
        .map_err(sql_error)?;
        tx.execute("INSERT INTO outbox_job VALUES(?1,?2,?3,'metadata_changed',?4,?4,NULL,?5,'pending',NULL,0,0,0)",params![t,s,fresh()?.to_string(),plan.memory.to_string(),version]).map_err(sql_error)?;
        tx.execute("UPDATE content_intent SET state='committed' WHERE tenant_id=?1 AND principal_id=?2 AND id=?3",params![t,ctx.principal.to_string(),c.key.to_string()]).map_err(sql_error)?;
        if stop == Stop::T1 {
            pause_if(stop);
            return Err(Error::Database);
        }
        // Epoch and expiry rechecked at the transaction's final authorization point.
        Self::content_check(
            &tx,
            ctx,
            time(&self.clock)?,
            if c.target.is_some() {
                "memory.revise"
            } else {
                "memory.append"
            },
        )?;
        if vault.is_some() {
            Self::check_local_keys(&tx, ctx, c.key)?;
        }
        tx.commit().map_err(sql_error)?;
        if stop == Stop::Reply {
            pause_if(stop);
            return Err(Error::Database);
        }
        Ok(plan.memory)
    }
    fn validate_files_backend(
        &mut self,
        ctx: &AuthContext,
        key: Id,
        store: &Store,
        vault: Option<&LocalVault>,
        cap: &str,
    ) -> Result<Vec<u8>> {
        let mut digest = None;
        let p = self.plan(ctx, key)?;
        for (id, k, o, kind, _) in p.copies {
            let bytes = store.read(id)?;
            let h: Vec<u8> = self
                .connection
                .query_row(
                    "SELECT envelope_hash FROM sealed_copy WHERE tenant_id=?1 AND payload_id=?2",
                    params![ctx.scope.tenant.to_string(), id.to_string()],
                    |r| r.get(0),
                )
                .map_err(sql_error)?;
            if hash(&bytes) != h {
                return Err(Error::Constraint);
            }
            digest =
                Some(self.open_backend(ctx, &binding(ctx, id, k, o, kind), &bytes, vault, cap)?);
        }
        digest.ok_or(Error::Constraint)
    }
    fn inventory_orphans(&self, store: &Store) -> Result<Vec<Id>> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(&store.root).map_err(|_| Error::Database)? {
            let e = e.map_err(|_| Error::Database)?;
            let Some(name) = e.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(p) = name.parse::<Id>() else { continue };
            let referenced: bool = self
                .connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM payload WHERE encrypted_ref=?1)",
                    [p.to_string()],
                    |r| r.get(0),
                )
                .map_err(sql_error)?;
            if !referenced {
                out.push(p)
            }
        }
        Ok(out)
    }
    fn cleanup_uncommitted(&mut self, ctx: &AuthContext, key: Id, store: &Store) -> Result<()> {
        if self.profile != 6 {
            return Err(Error::CapabilityUnavailable);
        }
        // Fence the entire staging intent before touching any file. Never clear a fence.
        let plan = self.plan(ctx, key)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        Self::content_check(&tx, ctx, time(&self.clock)?, "memory.append")?;
        let n=tx.execute("UPDATE content_intent SET state='blocked' WHERE tenant_id=?1 AND principal_id=?2 AND id=?3 AND state IN('staged','sealed','blocked')",params![ctx.scope.tenant.to_string(),ctx.principal.to_string(),key.to_string()]).map_err(sql_error)?;
        if n != 1 {
            return Err(Error::CapabilityUnavailable);
        }
        tx.commit().map_err(sql_error)?;
        for (p, k, o, kind, _) in plan.copies {
            match store.read(p) {
                Ok(bytes) => {
                    open(&binding(ctx, p, k, o, kind), &bytes)?;
                    store
                        .anchor
                        .erase(&p.to_string(), &bytes)
                        .map_err(|_| Error::Database)?;
                }
                Err(Error::ResourceUnavailable) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn release_digest(&mut self, ctx: &AuthContext, key: Id, store: &Store) -> Result<Vec<u8>> {
        self.release_backend(ctx, key, store, None)
    }
    fn release_backend(
        &mut self,
        ctx: &AuthContext,
        key: Id,
        store: &Store,
        vault: Option<&LocalVault>,
    ) -> Result<Vec<u8>> {
        if !matches!((self.profile, vault.is_some()), (6, false) | (7 | 8, true)) {
            return Err(Error::CapabilityUnavailable);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        Self::content_check(&tx, ctx, time(&self.clock)?, "memory.read")?;
        let state:String=tx.query_row("SELECT state FROM content_intent WHERE tenant_id=?1 AND space_id=?2 AND principal_id=?3 AND id=?4",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),ctx.principal.to_string(),key.to_string()],|r|r.get(0)).map_err(sql_error)?;
        if state != "committed" {
            return Err(Error::CapabilityUnavailable);
        }
        tx.commit().map_err(sql_error)?;
        let released_digest = self.validate_files_backend(ctx, key, store, vault, "memory.read")?;
        // No external output yet. Final writer lock serializes with revocation/fencing.
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        Self::content_check(&tx, ctx, time(&self.clock)?, "memory.read")?;
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM content_intent i JOIN memory m ON m.tenant_id=i.tenant_id AND m.space_id=i.space_id AND m.id=i.memory_id WHERE i.tenant_id=?1 AND i.space_id=?2 AND i.principal_id=?3 AND i.id=?4 AND i.state='committed' AND m.state='active' AND m.current_revision_id=i.revision_id)",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),ctx.principal.to_string(),key.to_string()],|r|r.get(0)).map_err(sql_error)?;
        if !valid {
            return Err(Error::CapabilityUnavailable);
        }
        if vault.is_some() {
            Self::check_local_keys(&tx, ctx, key)?;
        }
        tx.commit().map_err(sql_error)?;
        Ok(released_digest)
    }
}
#[cfg(test)]
mod tests;
