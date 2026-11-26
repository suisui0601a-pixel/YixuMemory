//! Private metadata Application with opt-in S3 synthetic Marker health assembly.
//! No content, KeyVault, output, migration or Ready recovery API.
//! A trusted synthetic host supplies actor/owner and one shared ordering capability.
use super::{attest, configure, gates};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use yixu_domain::security_m3::{OwnerKind, PayloadBinding, SecurityScope};
use yixu_domain::{Error, Result};

type Id = [u8; 16];
type Scope = [u8; 48];
fn sql<T>(r: rusqlite::Result<T>) -> Result<T> {
    r.map_err(|_| Error::Database)
}
fn require(value: bool) -> Result<()> {
    if value { Ok(()) } else { Err(Error::Integrity) }
}
// One global ledger invariant, independent of the requested Memory/Source.
// PKs provide cardinality; these bidirectional relations provide identity.
fn validate_global_ledger(db: &Connection) -> Result<()> {
    require(sql(db.query_row(
        "SELECT epoch=7+(SELECT count(*) FROM security_receipt)
         AND knowledge_seq=(SELECT count(*) FROM receipt)
         AND NOT EXISTS(SELECT 1 FROM authority WHERE epoch<>runtime.epoch)
         AND NOT EXISTS(SELECT 1 FROM write_intent i WHERE (i.state='Committed')<>
             EXISTS(SELECT 1 FROM receipt r WHERE r.operation=i.operation))
         AND NOT EXISTS(SELECT 1 FROM receipt r WHERE NOT EXISTS(
             SELECT 1 FROM write_intent i WHERE i.operation=r.operation AND i.state='Committed'
             AND i.scope=r.scope AND i.memory=r.memory AND i.revision=r.revision))
         AND NOT EXISTS(SELECT 1 FROM receipt r WHERE NOT EXISTS(
             SELECT 1 FROM security_event e JOIN security_receipt s ON s.event=e.id
             WHERE e.id=r.event AND e.scope=r.scope AND e.kind='Write'
             AND e.state='Applied' AND s.phase='Applied'))
         AND NOT EXISTS(SELECT 1 FROM security_event e WHERE e.kind='Write' AND (
             e.state<>'Applied' OR NOT EXISTS(SELECT 1 FROM receipt r WHERE r.event=e.id AND r.scope=e.scope)
             OR NOT EXISTS(SELECT 1 FROM security_receipt s WHERE s.event=e.id AND s.phase='Applied')))
         AND NOT EXISTS(SELECT 1 FROM receipt a JOIN receipt b ON a.event=b.event
             WHERE a.operation<>b.operation)
         AND NOT EXISTS(SELECT 1 FROM receipt r WHERE NOT EXISTS(
             SELECT 1 FROM outbox o WHERE o.operation=r.operation
             AND o.knowledge_seq=r.knowledge_seq AND o.action='MemoryCreated'))
         AND NOT EXISTS(SELECT 1 FROM outbox o WHERE o.action<>'MemoryCreated' OR NOT EXISTS(
             SELECT 1 FROM receipt r JOIN write_intent i ON i.operation=r.operation
             WHERE r.operation=o.operation AND r.knowledge_seq=o.knowledge_seq
             AND i.state='Committed' AND i.scope=r.scope AND i.memory=r.memory AND i.revision=r.revision))
         FROM runtime WHERE id=1", [], |r| r.get(0),
    ))?)
}
fn persisted_intent_links_valid(db: &Connection) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT NOT EXISTS(SELECT 1 FROM deletion_intent d WHERE NOT EXISTS(
             SELECT 1 FROM security_event e WHERE e.id=d.event AND e.scope=d.scope AND e.kind='Revocation'))
         AND NOT EXISTS(SELECT 1 FROM deletion_work w WHERE NOT EXISTS(
             SELECT 1 FROM deletion_intent d JOIN security_event e ON e.id=d.event
             JOIN write_intent i ON i.operation=w.operation
             JOIN deletion_member m ON m.event=d.event AND m.operation=w.operation
             JOIN deletion_closure c ON c.event=m.event AND c.operation=m.operation
             WHERE d.event=w.event AND d.scope=i.scope AND e.scope=d.scope
             AND e.kind='Revocation' AND e.state='Applied' AND i.state='Committed'
             AND w.generation=i.generation))",
        [], |r| r.get(0),
    )
}
trait Clock: Send + Sync {
    fn now(&self) -> Result<i64>;
}
struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> Result<i64> {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::CapabilityUnavailable)?
            .as_secs();
        i64::try_from(n).map_err(|_| Error::CapabilityUnavailable)
    }
}
#[derive(Clone, Copy)]
struct Actor {
    scope: Scope,
    principal: Id,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Memory(Id),
    Source(Id),
}
impl Target {
    fn parts(self) -> (&'static str, Id) {
        match self {
            Self::Memory(x) => ("Memory", x),
            Self::Source(x) => ("Source", x),
        }
    }
}
#[derive(Clone, PartialEq, Eq)]
struct Member {
    operation: Id,
    scope: Scope,
    principal: Id,
    request: Id,
    memory: Id,
    revision: Id,
    payload: Id,
    key: Id,
    evidence: Id,
    evidence_gen: i64,
    generation: i64,
    incarnation: Id,
    epoch: i64,
    dispatch: [u8; 32],
    source: Id,
    source_gen: i64,
    parent: Option<Id>,
}
impl Member {
    // Exact existing content-format-1 dispatch encoding; tombstones/availability
    // are deliberately not used to shrink the retained historical structure.
    fn original_dispatch(&self, installation: Id) -> Result<[u8; 32]> {
        let mut nodes = vec![
            (self.memory, 1, false),
            (self.revision, 1, false),
            (self.payload, self.generation, false),
            (self.key, self.generation, false),
            (self.evidence, self.evidence_gen, false),
            (self.source, self.source_gen, false),
        ];
        nodes.sort();
        let graph = gates::live_graph_binding(self.revision, nodes).ok_or(Error::Integrity)?;
        let mut bytes = b"YIXU/P10/R4/TRUSTED-INPROCESS-WRITE/v1\0".to_vec();
        for x in [
            installation,
            self.incarnation,
            self.scope[..16].try_into().map_err(|_| Error::Integrity)?,
            self.scope[16..32]
                .try_into()
                .map_err(|_| Error::Integrity)?,
            self.scope[32..].try_into().map_err(|_| Error::Integrity)?,
            self.principal,
            self.request,
            self.memory,
            self.revision,
            self.evidence,
            self.operation,
            self.payload,
            self.key,
        ] {
            bytes.extend(x);
        }
        for x in [self.epoch, self.generation, self.evidence_gen] {
            bytes.extend(x.to_be_bytes());
        }
        bytes.extend(graph);
        Ok(Sha256::digest(bytes).into())
    }
    fn marker(&self, installation: Id, event: i64) -> Vec<u8> {
        let mut context = b"YIXU/P10/UTF8/FORMAT-1/INSTALLATION/MEMORY\0".to_vec();
        context.extend(installation);
        context.extend(self.memory);
        let scope = SecurityScope {
            realm: self.scope[..16].try_into().expect("fixed scope"),
            tenant: self.scope[16..32].try_into().expect("fixed scope"),
            space: self.scope[32..].try_into().expect("fixed scope"),
        };
        let binding = PayloadBinding {
            scope,
            owner_kind: OwnerKind::Revision,
            owner: self.revision,
            payload: self.payload,
            key: self.key,
            seal_generation: self.generation as u64,
            vault_incarnation: 1,
            protection: Sha256::digest(context).into(),
        };
        let mut bytes = b"YIXU/B3/REVOKED/v1\0".to_vec();
        bytes.extend(binding.aad(32));
        bytes.extend(self.operation);
        bytes.extend(self.incarnation);
        bytes.extend(event.to_be_bytes());
        bytes
    }
}

/// Only the trusted in-process host owns this capability. Not an OS/process lock,
/// a public constructor, a lease, a native output proof, or a restore admission.
#[derive(Clone)]
struct RealmOrder {
    gate: Arc<Mutex<()>>,
    blocked: Arc<AtomicBool>,
    // Private test observation only. None retains the original lock() path.
    contention_observer: Arc<Mutex<Option<std::sync::mpsc::Sender<LockObservation>>>>,
    #[cfg(target_os = "windows")]
    marker_health: Arc<Mutex<Option<Arc<marker::OwnerHealth>>>>,
}
#[derive(Debug, PartialEq, Eq)]
enum LockObservation {
    Contended,
    Acquired,
}
impl RealmOrder {
    fn new() -> Self {
        Self {
            gate: Arc::new(Mutex::new(())),
            blocked: Arc::new(AtomicBool::new(false)),
            contention_observer: Arc::new(Mutex::new(None)),
            #[cfg(target_os = "windows")]
            marker_health: Arc::new(Mutex::new(None)),
        }
    }
    fn enter(&self) -> Result<std::sync::MutexGuard<'_, ()>> {
        let observer = self
            .contention_observer
            .lock()
            .map_err(|_| Error::CapabilityUnavailable)?
            .clone();
        if let Some(observer) = observer {
            let guard = match self.gate.try_lock() {
                Ok(guard) => guard,
                Err(std::sync::TryLockError::WouldBlock) => {
                    // An actual RealmOrder mutex contention result, not "about to call"
                    // or arrival at a scheduling barrier.
                    observer
                        .send(LockObservation::Contended)
                        .map_err(|_| Error::CapabilityUnavailable)?;
                    self.gate.lock().map_err(|_| Error::CapabilityUnavailable)?
                }
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(Error::CapabilityUnavailable);
                }
            };
            observer
                .send(LockObservation::Acquired)
                .map_err(|_| Error::CapabilityUnavailable)?;
            return Ok(guard);
        }
        self.gate.lock().map_err(|_| Error::CapabilityUnavailable)
    }
}
struct Application {
    db: Connection,
    installation: Id,
    realm: Id,
    incarnation: Id,
    order: RealmOrder,
    clock: Arc<dyn Clock>,
}
// Trusted management assembly only; never an ordinary deletion/client command.
enum SecurityChange {
    Grant {
        actor: Actor,
        memory: Id,
        enabled: bool,
    },
    Principal {
        actor: Actor,
        enabled: bool,
    },
    Space {
        scope: Scope,
        enabled: bool,
    },
    Source {
        scope: Scope,
        source: Id,
        enabled: bool,
    },
    Key {
        scope: Scope,
        memory: Id,
        enabled: bool,
    },
    Copy {
        scope: Scope,
        memory: Id,
        enabled: bool,
    },
    Epoch {
        scope: Scope,
    },
}
impl Application {
    fn security_change(&mut self, change: SecurityChange) -> Result<i64> {
        self.transaction(|app,db|{
            app.owner(db)?;app.graph_guard(db)?;
            if sql(gates::closing(db))? {return Err(Error::CapabilityUnavailable);}
            let scope=match &change {SecurityChange::Grant{actor,..}|SecurityChange::Principal{actor,..}=>actor.scope,
                SecurityChange::Space{scope,..}|SecurityChange::Source{scope,..}|SecurityChange::Key{scope,..}|
                SecurityChange::Copy{scope,..}|SecurityChange::Epoch{scope}=>*scope};
            require(scope[..16]==app.realm)?;
            sql(db.execute("INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",[scope]))?;
            let event=db.last_insert_rowid();
            if sql(gates::make_effective(db,event))?!=1 {return Err(Error::CapabilityUnavailable);}
            let changed=match change {
                SecurityChange::Grant{actor,memory,enabled}=>sql(db.execute("UPDATE authority SET grant_ok=?1 WHERE scope=?2 AND principal=?3 AND target=?4",params![enabled,scope,actor.principal,memory]))?,
                SecurityChange::Principal{actor,enabled}=>sql(db.execute("UPDATE authority SET principal_ok=?1 WHERE scope=?2 AND principal=?3",params![enabled,scope,actor.principal]))?,
                SecurityChange::Space{enabled,..}=>sql(db.execute("UPDATE authority SET space_ok=?1 WHERE scope=?2",params![enabled,scope]))?,
                SecurityChange::Source{source,enabled,..}=>sql(db.execute("UPDATE source SET valid=?1 WHERE scope=?2 AND id=?3",params![enabled,scope,source]))?,
                SecurityChange::Key{memory,enabled,..}=>sql(db.execute("UPDATE authority SET key_ok=?1 WHERE scope=?2 AND target=?3",params![enabled,scope,memory]))?,
                SecurityChange::Copy{memory,enabled,..}=>sql(db.execute("UPDATE authority SET copy_ok=?1 WHERE scope=?2 AND target=?3",params![enabled,scope,memory]))?,
                SecurityChange::Epoch{..}=>1,
            };
            require(changed>0)?;
            sql(db.execute("UPDATE runtime SET epoch=epoch+1",[]))?;
            sql(db.execute("UPDATE authority SET epoch=(SELECT epoch FROM runtime)",[]))?;
            sql(db.execute("INSERT INTO security_receipt(event,epoch) SELECT ?1,epoch FROM runtime",[event]))?;
            sql(db.execute("UPDATE security_event SET state='Applied' WHERE id=?1",[event]))?;Ok(event)
        })
    }
    fn attach(
        db: Connection,
        installation: Id,
        realm: Id,
        incarnation: Id,
        order: RealmOrder,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        configure(&db)?;
        let this = Self {
            db,
            installation,
            realm,
            incarnation,
            order,
            clock,
        };
        this.reconcile()?;
        Ok(this)
    }
    fn owner(&self, db: &Connection) -> Result<i64> {
        if self.order.blocked.load(Ordering::SeqCst) {
            return Err(Error::CapabilityUnavailable);
        }
        if let Err(original) = attest(db) {
            // Attestation also diagnoses data FKs. Refine a known persisted
            // intent-link fault to Integrity without ever turning a denial into
            // admission. Missing schema/query failures retain the original error.
            return match persisted_intent_links_valid(db) {
                Ok(false) => Err(Error::Integrity),
                _ => Err(original),
            };
        }
        require(sql(persisted_intent_links_valid(db))?)?;
        // S1's readback gap is additionally closed for these S2 connections only.
        require(sql(db.pragma_query_value(None, "temp_store", |r| r.get::<_, i64>(0)))? == 2)?;
        require(
            sql(db.pragma_query_value(None, "defer_foreign_keys", |r| r.get::<_, i64>(0)))? == 0,
        )?;
        let (install, realm, owner, epoch, blocked, clean): (Id, Id, Id, i64, bool, bool) =
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
        if install != self.installation
            || realm != self.realm
            || owner != self.incarnation
            || blocked
            || clean
        {
            return Err(Error::CapabilityUnavailable);
        }
        require(
            sql(
                db.query_row("SELECT schema_hash FROM runtime WHERE id=1", [], |r| {
                    r.get::<_, Vec<u8>>(0)
                }),
            )? == super::schema_hash(),
        )?;
        // Distinguish a normal unfinished intent from a corrupt intent that
        // already owns a committed receipt before applying the pending gate.
        validate_global_ledger(db)?;
        if sql(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM write_intent WHERE state<>'Committed')",
            [],
            |r| r.get::<_, bool>(0),
        ))? {
            return Err(Error::CapabilityUnavailable);
        }
        Ok(epoch)
    }
    fn ordered<'a>(&self, order: &'a RealmOrder) -> Result<std::sync::MutexGuard<'a, ()>> {
        match order.enter() {
            Ok(guard) => Ok(guard),
            Err(e) => {
                order.blocked.store(true, Ordering::SeqCst);
                #[cfg(target_os = "windows")]
                if let Some(h) = self.marker_health()? {
                    h.block(self, marker::Cause::CommitUnknown);
                }
                Err(e)
            }
        }
    }
    fn transaction<T>(&mut self, work: impl FnOnce(&Self, &Connection) -> Result<T>) -> Result<T> {
        let order = self.order.clone();
        let _guard = self.ordered(&order)?;
        if order.blocked.load(Ordering::SeqCst) {
            return Err(Error::CapabilityUnavailable);
        }
        #[cfg(target_os = "windows")]
        let health = self.marker_health()?;
        if !self.db.is_autocommit() {
            order.blocked.store(true, Ordering::SeqCst);
            #[cfg(target_os = "windows")]
            if let Some(h) = &health {
                h.block(self, marker::Cause::CommitUnknown);
            }
            return Err(Error::CapabilityUnavailable);
        }
        // Native observation finishes before BEGIN IMMEDIATE. Retained Pins,
        // rather than a write transaction, protect objects through commit.
        #[cfg(target_os = "windows")]
        let _pins = health.as_ref().map(|h| h.admit(self)).transpose()?;
        if self.db.execute_batch("BEGIN IMMEDIATE").is_err() {
            #[cfg(target_os = "windows")]
            if let Some(h) = &health {
                h.block(self, marker::Cause::MetadataFault);
            }
            return Err(Error::Database);
        }
        let result = work(self, &self.db);
        match result {
            Ok(value) => match self.db.execute_batch("COMMIT") {
                Ok(()) => Ok(value),
                Err(_) => {
                    // A failed COMMIT can leave a live transaction. Never reuse it
                    // without confirmed rollback; a commit error is conservatively unknown.
                    if !self.db.is_autocommit() {
                        let _ = self.db.execute_batch("ROLLBACK");
                    }
                    order.blocked.store(true, Ordering::SeqCst);
                    #[cfg(target_os = "windows")]
                    if let Some(h) = &health {
                        h.block(self, marker::Cause::CommitUnknown);
                    }
                    Err(Error::CapabilityUnavailable)
                }
            },
            Err(e) => {
                let rollback = self.db.execute_batch("ROLLBACK");
                if rollback.is_err() || !self.db.is_autocommit() {
                    order.blocked.store(true, Ordering::SeqCst);
                    #[cfg(target_os = "windows")]
                    if let Some(h) = &health {
                        h.block(self, marker::Cause::CommitUnknown);
                    }
                    return Err(Error::CapabilityUnavailable);
                }
                if matches!(
                    e,
                    Error::Integrity | Error::Database | Error::IncompatibleSchema
                ) {
                    order.blocked.store(true, Ordering::SeqCst);
                    #[cfg(target_os = "windows")]
                    if let Some(h) = &health {
                        h.block(self, marker::Cause::MetadataFault);
                    }
                }
                Err(e)
            }
        }
    }
    fn close(&mut self, actor: Actor, request: Id, target: Target) -> Result<i64> {
        if request == [0; 16] || target.parts().1 == [0; 16] {
            return Err(Error::InvalidInput);
        }
        self.transaction(|app, db| {
            let epoch = app.owner(db)?;
            app.graph_guard(db)?;
            require(actor.scope[..16] == app.realm)?;
            let selected = app.authoritative_closure(db, &actor.scope, target)?;
            app.authorize(db, actor, &selected, epoch)?;
            let old: Option<(i64, String, Id)> = sql(db
                .query_row(
                    "SELECT event,kind,target FROM deletion_intent
                WHERE scope=?1 AND principal=?2 AND request=?3",
                    params![actor.scope, actor.principal, request],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional())?;
            if let Some((event, kind, id)) = old {
                if (kind.as_str(), id) != target.parts() {
                    return Err(Error::IdempotencyConflict);
                }
                app.frozen(db, event)?;
                return Ok(event);
            }
            sql(db.execute(
                "INSERT INTO security_event(scope,kind) VALUES(?1,'Revocation')",
                [actor.scope],
            ))?;
            let event = db.last_insert_rowid();
            let (kind, id) = target.parts();
            sql(db.execute(
                "INSERT INTO deletion_intent VALUES(?1,?2,?3,?4,?5,?6)",
                params![event, actor.scope, actor.principal, request, kind, id],
            ))?;
            for member in &selected {
                sql(db.execute(
                    "INSERT INTO deletion_closure VALUES(?1,?2)",
                    params![event, member.operation],
                ))?;
            }
            sql(db.execute("INSERT INTO deletion_closure_seal VALUES(?1)", [event]))?;
            app.frozen(db, event)?;
            Ok(event)
        })
    }
    fn fence(&mut self, actor: Actor, event: i64) -> Result<()> {
        self.transaction(|app, db| {
            let epoch = app.owner(db)?;
            app.graph_guard(db)?;
            let (bound, _, state) = app
                .intent_optional(db, event)?
                .ok_or(Error::ResourceUnavailable)?;
            if bound.scope != actor.scope || bound.principal != actor.principal {
                return Err(Error::AuthorizationDenied);
            }
            let selected = app.frozen(db, event)?;
            app.authorize(db, actor, &selected, epoch)?;
            if state != "Closing" {
                return Err(Error::StateConflict);
            }
            if sql(gates::make_effective(db, event))? != 1 {
                return Err(Error::CapabilityUnavailable);
            }
            Ok(())
        })
    }
    fn apply(&mut self, actor: Actor, event: i64) -> Result<()> {
        self.apply_checked(actor, event, |_| Ok(()))
    }
    fn apply_checked(
        &mut self,
        actor: Actor,
        event: i64,
        checkpoint: impl FnOnce(&Connection) -> Result<()>,
    ) -> Result<()> {
        self.transaction(|app, db| {
            let epoch = app.owner(db)?;
            app.graph_guard(db)?;
            let (bound, target, state) = app
                .intent_optional(db, event)?
                .ok_or(Error::ResourceUnavailable)?;
            if bound.scope != actor.scope || bound.principal != actor.principal {
                return Err(Error::AuthorizationDenied);
            }
            let selected = app.frozen(db, event)?;
            app.authorize(db, actor, &selected, epoch)?;
            if state == "Applied" {
                return Ok(());
            }
            if state != "EffectiveFence" {
                return Err(Error::StateConflict);
            }
            for m in &selected {
                sql(db.execute(
                    "INSERT INTO deletion_member VALUES(?1,?2)",
                    params![event, m.operation],
                ))?;
                for object in [m.revision, m.payload, m.key] {
                    app.tombstone(db, actor.scope, object, m.generation, event)?;
                }
                app.ensure_work(db, m, event)?;
            }
            match target {
                Target::Memory(id) => app.tombstone(db, actor.scope, id, 1, event)?,
                Target::Source(id) => {
                    let mut q = sql(db.prepare(
                        "SELECT id,generation FROM source WHERE scope=?1 AND id=?2
                        UNION SELECT id,generation FROM evidence WHERE scope=?1 AND source=?2",
                    ))?;
                    let rows = sql(sql(q.query_map(params![actor.scope, id], |r| {
                        Ok((r.get::<_, Id>(0)?, r.get::<_, i64>(1)?))
                    }))?
                    .collect::<rusqlite::Result<Vec<_>>>())?;
                    for (id, generation) in rows {
                        app.tombstone(db, actor.scope, id, generation, event)?;
                    }
                }
            }
            // Fault checkpoint is private test assembly only, never an Application callback.
            checkpoint(db)?;
            app.equal_sets(db, event)?;
            // Fresh server time sample at the authorization linearization point.
            app.authorize(db, actor, &selected, epoch)?;
            require(app.authoritative_closure(db, &actor.scope, target)? == selected)?;
            sql(db.execute("UPDATE runtime SET epoch=epoch+1", []))?;
            sql(db.execute("UPDATE authority SET epoch=(SELECT epoch FROM runtime)", []))?;
            sql(db.execute(
                "INSERT INTO security_receipt(event,epoch) SELECT ?1,epoch FROM runtime",
                [event],
            ))?;
            sql(db.execute(
                "UPDATE security_event SET state='Applied' WHERE id=?1",
                [event],
            ))?;
            app.frozen(db, event)?;
            Ok(())
        })
    }
    fn intent_optional(
        &self,
        db: &Connection,
        event: i64,
    ) -> Result<Option<(Actor, Target, String)>> {
        let row: Option<(Scope, Id, String, Id)> = sql(db
            .query_row(
                "SELECT scope,principal,kind,target FROM deletion_intent WHERE event=?1",
                [event],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional())?;
        let Some((scope, principal, kind, target)) = row else {
            return Ok(None);
        };
        let parent: Option<(Scope, String, String)> = sql(db
            .query_row(
                "SELECT scope,kind,state FROM security_event WHERE id=?1",
                [event],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional())?;
        let (event_scope, event_kind, state) = parent.ok_or(Error::Integrity)?;
        require(event_scope == scope && event_kind == "Revocation")?;
        require(scope[..16] == self.realm)?;
        let target = match kind.as_str() {
            "Memory" => Target::Memory(target),
            "Source" => Target::Source(target),
            _ => return Err(Error::Integrity),
        };
        Ok(Some((Actor { scope, principal }, target, state)))
    }
    fn intent_required(&self, db: &Connection, event: i64) -> Result<(Actor, Target, String)> {
        self.intent_optional(db, event)?.ok_or(Error::Integrity)
    }
    fn operations(db: &Connection, table: &str, event: i64) -> Result<BTreeSet<Id>> {
        let mut q = sql(db.prepare(&format!(
            "SELECT operation FROM {table} WHERE event=?1 ORDER BY operation"
        )))?;
        sql(sql(q.query_map([event], |r| r.get(0)))?.collect())
    }
    fn equal_sets(&self, db: &Connection, event: i64) -> Result<()> {
        require(!sql(db.query_row(
            "SELECT EXISTS(SELECT operation FROM deletion_closure WHERE event=?1
            EXCEPT SELECT operation FROM deletion_member WHERE event=?1)
            OR EXISTS(SELECT operation FROM deletion_member WHERE event=?1
            EXCEPT SELECT operation FROM deletion_closure WHERE event=?1)",
            [event],
            |r| r.get::<_, bool>(0),
        ))?)
    }
    fn frozen(&self, db: &Connection, event: i64) -> Result<Vec<Member>> {
        let (actor, target, state) = self.intent_required(db, event)?;
        require(sql(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM deletion_closure_seal WHERE event=?1)",
            [event],
            |r| r.get(0),
        ))?)?;
        let selected = self.authoritative_closure(db, &actor.scope, target)?;
        let stored = Self::operations(db, "deletion_closure", event)?;
        require(stored == selected.iter().map(|x| x.operation).collect())?;
        if state == "Applied" {
            self.equal_sets(db, event)?;
            for m in &selected {
                self.validate_work(db, m, event)?;
                for object in [m.revision, m.payload, m.key] {
                    self.require_tombstone(db, m.scope, object, m.generation)?;
                }
            }
            match target {
                Target::Memory(id) => self.require_tombstone(db, actor.scope, id, 1)?,
                Target::Source(id) => {
                    let mut q = sql(db.prepare(
                        "SELECT id,generation FROM source WHERE scope=?1 AND id=?2
                        UNION SELECT id,generation FROM evidence WHERE scope=?1 AND source=?2",
                    ))?;
                    let rows = sql(sql(q.query_map(params![actor.scope, id], |r| {
                        Ok((r.get::<_, Id>(0)?, r.get::<_, i64>(1)?))
                    }))?
                    .collect::<rusqlite::Result<Vec<_>>>())?;
                    for (object, generation) in rows {
                        self.require_tombstone(db, actor.scope, object, generation)?;
                    }
                }
            }
            require(sql(db.query_row(
                "SELECT EXISTS(SELECT 1 FROM security_receipt WHERE event=?1 AND phase='Applied')",
                [event],
                |r| r.get(0),
            ))?)?;
        } else {
            require(Self::operations(db, "deletion_member", event)?.is_empty())?;
        }
        Ok(selected)
    }
    fn authoritative_closure(
        &self,
        db: &Connection,
        scope: &Scope,
        target: Target,
    ) -> Result<Vec<Member>> {
        let mut selected = Vec::new();
        // A Memory target is an explicit identity namespace. Source selection must
        // examine every scoped intent to avoid dropping unknown Source relations.
        let memory_filter = match target {
            Target::Memory(id) => Some(id),
            Target::Source(_) => None,
        };
        // Enumerate committed *intent identities*, never an inner-join projection that
        // can silently discard a corrupt/quarantined dependency.
        let mut q=sql(db.prepare("SELECT operation,principal,request,memory,revision,payload,key_id,evidence,evidence_generation,
            generation,incarnation,epoch,dispatch_binding,state,expected_current FROM write_intent
            WHERE scope=?1 AND (?2 IS NULL OR memory=?2) ORDER BY operation"))?;
        let mut rows = sql(q.query(params![scope, memory_filter]))?;
        while let Some(r) = sql(rows.next())? {
            if sql(r.get::<_, String>(13))? != "Committed" {
                return Err(Error::CapabilityUnavailable);
            }
            let mut m = Member {
                operation: sql(r.get(0))?,
                scope: *scope,
                principal: sql(r.get(1))?,
                request: sql(r.get(2))?,
                memory: sql(r.get(3))?,
                revision: sql(r.get(4))?,
                payload: sql(r.get(5))?,
                key: sql(r.get(6))?,
                evidence: sql(r.get(7))?,
                evidence_gen: sql(r.get(8))?,
                generation: sql(r.get(9))?,
                incarnation: sql(r.get(10))?,
                epoch: sql(r.get(11))?,
                dispatch: sql(r.get(12))?,
                source: [0; 16],
                source_gen: 0,
                parent: sql(r.get(14))?,
            };
            let source:Option<(Id,i64)>=sql(db.query_row("SELECT e.source,e.source_generation FROM evidence e
                JOIN source s ON s.scope=e.scope AND s.id=e.source AND s.generation=e.source_generation
                WHERE e.scope=?1 AND e.id=?2 AND e.generation=?3",params![scope,m.evidence,m.evidence_gen],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
            (m.source, m.source_gen) = source.ok_or(Error::Integrity)?;
            let complete:bool=sql(db.query_row("SELECT EXISTS(SELECT 1 FROM memory mem JOIN revision v
                ON v.scope=mem.scope AND v.memory=mem.id JOIN revision_evidence re ON re.scope=v.scope AND re.memory=v.memory AND re.revision=v.id
                JOIN payload p ON p.scope=v.scope AND p.memory=v.memory AND p.revision=v.id AND p.id=v.payload
                JOIN local_key k ON k.scope=p.scope AND k.payload=p.id AND k.id=p.key_id AND k.generation=p.generation
                JOIN publication pub ON pub.operation=p.operation JOIN copy_manifest_b3 copy ON copy.operation=p.operation
                JOIN receipt r ON r.operation=p.operation JOIN outbox o ON o.operation=r.operation
                WHERE mem.scope=?1 AND mem.id=?2 AND mem.state='active' AND v.id=?3 AND p.id=?4 AND p.key_id=?5
                AND p.generation=?6 AND p.operation=?7 AND re.evidence=?8 AND re.generation=?9
                AND copy.generation=p.generation AND r.scope=mem.scope AND r.memory=mem.id AND r.revision=v.id AND o.knowledge_seq=r.knowledge_seq
                AND ((v.number=1 AND ?10 IS NULL) OR (v.number>1 AND EXISTS(SELECT 1 FROM revision prev
                    WHERE prev.scope=v.scope AND prev.memory=v.memory AND prev.number=v.number-1 AND prev.id=?10)))
                AND EXISTS(SELECT 1 FROM revision cur WHERE cur.scope=mem.scope AND cur.memory=mem.id AND cur.id=mem.current_revision
                    AND cur.number=(SELECT max(number) FROM revision WHERE scope=mem.scope AND memory=mem.id)))",
                params![scope,m.memory,m.revision,m.payload,m.key,m.generation,m.operation,m.evidence,m.evidence_gen,m.parent],|r|r.get(0)))?;
            require(
                complete
                    && m.generation == 1
                    && m.source_gen > 0
                    && m.dispatch == m.original_dispatch(self.installation)?,
            )?;
            if match target {
                Target::Memory(id) => m.memory == id,
                Target::Source(id) => m.source == id,
            } {
                selected.push(m);
            }
        }
        // Reverse completeness: no undeclared Revision/Payload or receipt identity
        // may hide outside the intent enumeration. No COUNT-based set substitute.
        require(!sql(db.query_row("SELECT EXISTS(SELECT 1 FROM revision v WHERE v.scope=?1 AND (?2 IS NULL OR v.memory=?2) AND NOT EXISTS(
            SELECT 1 FROM write_intent i WHERE i.scope=v.scope AND i.memory=v.memory AND i.revision=v.id AND i.state='Committed'))
            OR EXISTS(SELECT 1 FROM payload p WHERE p.scope=?1 AND (?2 IS NULL OR p.memory=?2) AND NOT EXISTS(SELECT 1 FROM write_intent i
                WHERE i.operation=p.operation AND i.scope=p.scope AND i.memory=p.memory AND i.revision=p.revision AND i.payload=p.id))
            OR EXISTS(SELECT 1 FROM memory m WHERE m.scope=?1 AND (?2 IS NULL OR m.id=?2) AND NOT EXISTS(SELECT 1 FROM revision v WHERE v.scope=m.scope AND v.memory=m.id))
            OR EXISTS(SELECT 1 FROM revision v WHERE v.scope=?1 AND (?2 IS NULL OR v.memory=?2) AND v.number>1 AND NOT EXISTS(
                SELECT 1 FROM revision prev WHERE prev.scope=v.scope AND prev.memory=v.memory AND prev.number=v.number-1))",params![scope,memory_filter],|r|r.get::<_,bool>(0)))?)?;
        if selected.is_empty() {
            return Err(Error::ResourceUnavailable);
        }
        Ok(selected)
    }
    fn authorize(
        &self,
        db: &Connection,
        actor: Actor,
        members: &[Member],
        epoch: i64,
    ) -> Result<()> {
        let now = self.clock.now()?;
        for memory in members.iter().map(|m| m.memory).collect::<BTreeSet<_>>() {
            if sql(gates::authorized_snapshot(
                db,
                &actor.scope,
                &actor.principal,
                &memory,
                epoch,
                now,
            ))?
            .is_none()
            {
                return Err(Error::AuthorizationDenied);
            }
        }
        Ok(())
    }
    fn tombstone(
        &self,
        db: &Connection,
        scope: Scope,
        object: Id,
        generation: i64,
        event: i64,
    ) -> Result<()> {
        let old: Option<i64> = sql(db
            .query_row(
                "SELECT event FROM r5_tombstone WHERE scope=?1 AND object=?2 AND generation=?3",
                params![scope, object, generation],
                |r| r.get(0),
            )
            .optional())?;
        if let Some(old) = old {
            require(sql(db.query_row("SELECT EXISTS(SELECT 1 FROM security_event e JOIN security_receipt r ON e.id=r.event
                WHERE e.id=?1 AND e.scope=?2 AND e.state='Applied')",params![old,scope],|r|r.get(0)))?)?;
        } else {
            sql(db.execute(
                "INSERT INTO r5_tombstone VALUES(?1,?2,?3,?4)",
                params![scope, object, generation, event],
            ))?;
        }
        Ok(())
    }
    fn require_tombstone(
        &self,
        db: &Connection,
        scope: Scope,
        object: Id,
        generation: i64,
    ) -> Result<()> {
        require(sql(db.query_row("SELECT EXISTS(SELECT 1 FROM r5_tombstone t JOIN security_event e ON e.id=t.event
            JOIN security_receipt r ON r.event=e.id WHERE t.scope=?1 AND t.object=?2 AND t.generation=?3
            AND e.scope=t.scope AND e.state='Applied')",params![scope,object,generation],|r|r.get(0)))?)
    }
    fn validate_work(&self, db: &Connection, m: &Member, event: i64) -> Result<()> {
        let (canonical, generation, marker): (i64, i64, Vec<u8>) = sql(db
            .query_row(
                "SELECT event,generation,marker FROM deletion_work WHERE operation=?1",
                [m.operation],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional())?
        .ok_or(Error::Integrity)?;
        require(generation == m.generation && marker == m.marker(self.installation, canonical))?;
        let (actor, _, state) = self.intent_required(db, canonical)?;
        require(actor.scope == m.scope && state == "Applied")?;
        self.equal_sets(db, canonical)?;
        require(Self::operations(db, "deletion_member", canonical)?.contains(&m.operation))?;
        require(Self::operations(db, "deletion_closure", canonical)?.contains(&m.operation))?;
        if canonical != event {
            require(canonical < event)?;
        }
        Ok(())
    }
    fn ensure_work(&self, db: &Connection, m: &Member, event: i64) -> Result<()> {
        let exists: bool = sql(db.query_row(
            "SELECT EXISTS(SELECT 1 FROM deletion_work WHERE operation=?1)",
            [m.operation],
            |r| r.get(0),
        ))?;
        if exists {
            self.validate_work(db, m, event)
        } else {
            sql(db.execute(
                "INSERT INTO deletion_work(operation,event,generation,marker) VALUES(?1,?2,?3,?4)",
                params![
                    m.operation,
                    event,
                    m.generation,
                    m.marker(self.installation, event)
                ],
            ))?;
            Ok(())
        }
    }
    fn graph_guard(&self, db: &Connection) -> Result<()> {
        let mut q = sql(db.prepare("SELECT event FROM deletion_intent ORDER BY event"))?;
        let events = sql(
            sql(q.query_map([], |r| r.get::<_, i64>(0)))?.collect::<rusqlite::Result<Vec<_>>>()
        )?;
        for event in events {
            self.frozen(db, event)?;
        }
        // Reverse traversal starts at every Work, never at only selected members.
        // Even a well-formed extra Work must not hide outside an event's closure.
        let mut q =
            sql(db.prepare("SELECT operation,event FROM deletion_work ORDER BY operation"))?;
        let work = sql(
            sql(q.query_map([], |r| Ok((r.get::<_, Id>(0)?, r.get::<_, i64>(1)?))))?
                .collect::<rusqlite::Result<Vec<_>>>(),
        )?;
        for (operation, canonical) in work {
            let (scope, memory): (Scope, Id) = sql(db
                .query_row(
                    "SELECT scope,memory FROM write_intent WHERE operation=?1",
                    [operation],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional())?
            .ok_or(Error::Integrity)?;
            let members = self.authoritative_closure(db, &scope, Target::Memory(memory))?;
            let member = members
                .iter()
                .find(|m| m.operation == operation)
                .ok_or(Error::Integrity)?;
            self.validate_work(db, member, canonical)?;
        }
        Ok(())
    }
    fn reconcile(&self) -> Result<()> {
        let order = self.order.clone();
        let _guard = self.ordered(&order)?;
        #[cfg(target_os = "windows")]
        let health = self.marker_health()?;
        #[cfg(target_os = "windows")]
        let _pins = health.as_ref().map(|h| h.admit(self)).transpose()?;
        // Read-only snapshot, never repairs or advances an event, never produces Ready.
        if self.db.execute_batch("BEGIN").is_err() {
            #[cfg(target_os = "windows")]
            if let Some(h) = &health {
                h.block(self, marker::Cause::MetadataFault);
            }
            return Err(Error::Database);
        }
        let result = self
            .owner(&self.db)
            .and_then(|_| self.graph_guard(&self.db));
        let rollback = self.db.execute_batch("ROLLBACK");
        if result.is_err() || rollback.is_err() {
            order.blocked.store(true, Ordering::SeqCst);
            #[cfg(target_os = "windows")]
            if let Some(h) = &health {
                h.block(self, marker::Cause::MetadataFault);
            }
        }
        rollback.map_err(|_| Error::CapabilityUnavailable)?;
        result
    }
    fn admit_metadata(&mut self, actor: Actor, memory: Id) -> Result<()> {
        self.transaction(|app,db|{
            let epoch=app.owner(db)?;app.graph_guard(db)?;
            if sql(gates::closing(db))? {return Err(Error::CapabilityUnavailable);}
            let selected=app.authoritative_closure(db,&actor.scope,Target::Memory(memory))?;
            app.authorize(db,actor,&selected,epoch)?;
            for m in &selected {
                if !sql(db.query_row("SELECT e.valid=1 AND s.valid=1 AND k.revoked=0 FROM evidence e
                    JOIN source s ON s.scope=e.scope AND s.id=e.source AND s.generation=e.source_generation
                    JOIN local_key k ON k.scope=e.scope AND k.id=?4 AND k.payload=?5 AND k.generation=?6
                    WHERE e.scope=?1 AND e.id=?2 AND e.generation=?3",
                    params![actor.scope,m.evidence,m.evidence_gen,m.key,m.payload,m.generation],|r|r.get::<_,bool>(0)))? {return Err(Error::AuthorizationDenied);}
                for (id,generation) in [(m.revision,1),(m.payload,m.generation),(m.key,m.generation),(m.evidence,m.evidence_gen),(m.source,m.source_gen)] {
                    if sql(db.query_row("SELECT EXISTS(SELECT 1 FROM r5_tombstone WHERE scope=?1 AND object=?2 AND generation=?3)",params![actor.scope,id,generation],|r|r.get::<_,bool>(0)))? {return Err(Error::AuthorizationDenied);}
                }
            }
            if sql(db.query_row("SELECT EXISTS(SELECT 1 FROM r5_tombstone WHERE scope=?1 AND object=?2)
                OR EXISTS(SELECT 1 FROM object_quarantine q JOIN write_intent i USING(operation) WHERE i.scope=?1 AND i.memory=?2)",
                params![actor.scope,memory],|r|r.get::<_,bool>(0)))? {return Err(Error::AuthorizationDenied);} Ok(())
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(target_os = "windows")]
mod marker;
mod marker_state;
