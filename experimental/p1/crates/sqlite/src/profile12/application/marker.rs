//! Private S3 assembly. No content, external output, old-root admission or Ready issuer.
use super::marker_state::{self, FileView, Ledger, State};
use super::*;
use crate::auth::marker_io::{MarkerPin, MarkerStore};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cause {
    MetadataFault,
    StorageUnknown,
    CommitUnknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Unconfirmed {
    KnownNotCommitted,
    CommitUnknown,
    Unsettled,
    WrongOwner,
    UntrustedSchema,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Report {
    Ready,
    Durable,
    NotConfirmed(Unconfirmed),
}
struct DurableBlockReceipt {
    installation: Id,
    incarnation: Id,
}
enum BlockState {
    Ready,
    Durable(DurableBlockReceipt),
    NotConfirmed(Unconfirmed),
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockFault {
    None,
    BeforeWrite,
    AfterCommitReplyLost,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Point {
    Published,
    IdentityCommitted,
    ObservationCommitted,
    ReceiptCommitted,
}
#[derive(Clone, Copy)]
struct Hooks {
    stop: Option<Point>,
    block: BlockFault,
    deferred_failure: bool,
}
impl Default for Hooks {
    fn default() -> Self {
        Self {
            stop: None,
            block: BlockFault::None,
            deferred_failure: false,
        }
    }
}
struct FreshSchema {
    database: String,
    roots: Vec<u8>,
}
#[derive(Clone, PartialEq, Eq)]
struct Work {
    member: Member,
    event: i64,
    ledger: Ledger,
    bytes: Vec<u8>,
    name: String,
}
struct PublishPermit {
    work: Work,
    incarnation: Id,
    roots: Vec<u8>,
}
struct Publisher<'a>(&'a OwnerHealth);
impl Publisher<'_> {
    fn publish(&self, permit: PublishPermit) -> Result<MarkerPin> {
        require(permit.incarnation == self.0.incarnation)?;
        require(
            self.0
                .io
                .binding()
                .map_err(|_| Error::ResourceUnavailable)?
                == permit.roots,
        )?;
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce).map_err(|_| Error::CapabilityUnavailable)?;
        let temp = format!("pending-revoke-{}", yixu_domain::Id::from_bytes(nonce));
        self.0
            .io
            .publish_once(&temp, &permit.work.name, &permit.work.bytes)
            .map_err(|_| Error::ResourceUnavailable)
    }
}
fn database(db: &Connection) -> Result<String> {
    sql(db.query_row(
        "SELECT file FROM pragma_database_list WHERE name='main'",
        [],
        |r| r.get(0),
    ))
}
fn read<T>(app: &Application, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    require(app.db.is_autocommit())?;
    sql(app.db.execute_batch("BEGIN"))?;
    let result = f(&app.db);
    let rollback = app.db.execute_batch("ROLLBACK");
    if rollback.is_err() || !app.db.is_autocommit() {
        return Err(Error::CapabilityUnavailable);
    }
    result
}

pub(super) struct OwnerHealth {
    io: MarkerStore,
    installation: Id,
    incarnation: Id,
    roots: Vec<u8>,
    state: Mutex<BlockState>,
    cause: Mutex<Option<Cause>>,
    uncertain_commit: AtomicBool,
    publish_calls: AtomicI64,
}
use std::sync::atomic::AtomicI64;
impl OwnerHealth {
    // Trusted synthetic host only. A new empty SQL database and empty controlled
    // Marker directory are necessary; this does not inspect/admit old roots.
    fn initialize_fresh(db: &mut Connection, io: &MarkerStore) -> Result<FreshSchema> {
        require(
            io.names()
                .map_err(|_| Error::ResourceUnavailable)?
                .is_empty(),
        )?;
        let roots = io.binding().map_err(|_| Error::ResourceUnavailable)?;
        super::super::install_fresh(db)?;
        let database = database(db)?;
        require(!database.is_empty())?;
        Ok(FreshSchema { database, roots })
    }
    fn bind_fresh(app: &Application, io: MarkerStore, permit: FreshSchema) -> Result<Arc<Self>> {
        let order = app.order.clone();
        let _guard = order.enter()?;
        require(database(&app.db)? == permit.database)?;
        require(io.binding().map_err(|_| Error::ResourceUnavailable)? == permit.roots)?;
        require(
            io.names()
                .map_err(|_| Error::ResourceUnavailable)?
                .is_empty(),
        )?;
        read(app, |db| {
            app.owner(db)?;
            app.graph_guard(db)?;
            let roots: Vec<u8> = sql(db.query_row(
                "SELECT store_identity FROM runtime WHERE id=1",
                [],
                |r| r.get(0),
            ))?;
            require(roots == permit.roots)?;
            require(sql(db.query_row("SELECT NOT EXISTS(SELECT 1 FROM deletion_intent) AND NOT EXISTS(SELECT 1 FROM stop_receipt) AND NOT EXISTS(SELECT 1 FROM resume_receipt)",[],|r|r.get(0)))?)
        })?;
        let h = Arc::new(Self {
            io,
            installation: app.installation,
            incarnation: app.incarnation,
            roots: permit.roots,
            state: Mutex::new(BlockState::Ready),
            cause: Mutex::new(None),
            uncertain_commit: AtomicBool::new(false),
            publish_calls: AtomicI64::new(0),
        });
        let mut slot = order
            .marker_health
            .lock()
            .map_err(|_| Error::CapabilityUnavailable)?;
        require(slot.is_none())?;
        *slot = Some(h.clone());
        Ok(h)
    }
    fn report(&self) -> Report {
        match self.state.lock() {
            Ok(s) => match &*s {
                BlockState::Ready => Report::Ready,
                BlockState::Durable(r)
                    if r.installation == self.installation && r.incarnation == self.incarnation =>
                {
                    Report::Durable
                }
                BlockState::Durable(_) => Report::NotConfirmed(Unconfirmed::WrongOwner),
                BlockState::NotConfirmed(u) => Report::NotConfirmed(*u),
            },
            Err(_) => Report::NotConfirmed(Unconfirmed::Unsettled),
        }
    }
    fn schema_trusted(db: &Connection) -> Result<()> {
        let expected = sql(Connection::open_in_memory())?;
        super::super::configure(&expected)?;
        super::super::write_schema(&expected)?;
        require(super::super::definitions(db)? == super::super::definitions(&expected)?)
    }
    fn persist(&self, app: &Application, fault: BlockFault) -> BlockState {
        if !app.db.is_autocommit() {
            return BlockState::NotConfirmed(Unconfirmed::Unsettled);
        }
        if Self::schema_trusted(&app.db).is_err() {
            return BlockState::NotConfirmed(Unconfirmed::UntrustedSchema);
        }
        if fault == BlockFault::BeforeWrite {
            return BlockState::NotConfirmed(Unconfirmed::KnownNotCommitted);
        }
        if app.db.execute_batch("BEGIN IMMEDIATE").is_err() {
            return BlockState::NotConfirmed(Unconfirmed::KnownNotCommitted);
        }
        let changed=app.db.execute("UPDATE runtime SET blocked=1 WHERE id=1 AND installation=?1 AND realm=?2 AND incarnation=?3 AND clean=0 AND schema_hash=?4",
            params![self.installation,app.realm,self.incarnation,super::super::schema_hash()]);
        if changed != Ok(1) {
            let rb = app.db.execute_batch("ROLLBACK");
            return BlockState::NotConfirmed(if rb.is_err() || !app.db.is_autocommit() {
                Unconfirmed::Unsettled
            } else if changed == Ok(0) {
                Unconfirmed::WrongOwner
            } else {
                Unconfirmed::KnownNotCommitted
            });
        }
        if app.db.execute_batch("COMMIT").is_err() {
            if !app.db.is_autocommit() {
                let _ = app.db.execute_batch("ROLLBACK");
            }
            return BlockState::NotConfirmed(if app.db.is_autocommit() {
                Unconfirmed::CommitUnknown
            } else {
                Unconfirmed::Unsettled
            });
        }
        if fault == BlockFault::AfterCommitReplyLost {
            return BlockState::NotConfirmed(Unconfirmed::CommitUnknown);
        }
        let confirmed:rusqlite::Result<bool>=app.db.query_row("SELECT blocked=1 AND installation=?1 AND realm=?2 AND incarnation=?3 AND clean=0 AND schema_hash=?4 FROM runtime WHERE id=1",
            params![self.installation,app.realm,self.incarnation,super::super::schema_hash()],|r|r.get(0));
        if confirmed == Ok(true) && app.db.is_autocommit() {
            BlockState::Durable(DurableBlockReceipt {
                installation: self.installation,
                incarnation: self.incarnation,
            })
        } else {
            BlockState::NotConfirmed(Unconfirmed::CommitUnknown)
        }
    }
    fn block_with(&self, app: &Application, cause: Cause, fault: BlockFault) {
        // Latch BEFORE any attempt at SQL persistence or error return.
        app.order.blocked.store(true, Ordering::SeqCst);
        if let Ok(mut c) = self.cause.lock()
            && c.is_none()
        {
            *c = Some(cause);
        }
        if let Ok(mut s) = self.state.lock()
            && matches!(*s, BlockState::Ready)
        {
            *s = BlockState::NotConfirmed(Unconfirmed::Unsettled);
            *s = self.persist(app, fault);
        }
    }
    pub(super) fn block(&self, app: &Application, cause: Cause) {
        self.block_with(app, cause, BlockFault::None);
    }
    fn retry_block_record(&self, app: &Application) -> Report {
        app.order.blocked.store(true, Ordering::SeqCst);
        let order = app.order.clone();
        let Ok(_guard) = order.enter() else {
            return Report::NotConfirmed(Unconfirmed::Unsettled);
        };
        if let Ok(mut s) = self.state.lock()
            && matches!(*s, BlockState::NotConfirmed(_))
        {
            *s = self.persist(app, BlockFault::None);
        }
        self.report()
    }
    fn load(&self, app: &Application) -> Result<Vec<Work>> {
        read(app, |db| self.load_rows(app, db))
    }
    fn load_rows(&self, app: &Application, db: &Connection) -> Result<Vec<Work>> {
        app.owner(db)?;
        app.graph_guard(db)?;
        let roots: Vec<u8> = sql(db.query_row(
            "SELECT store_identity FROM runtime WHERE id=1",
            [],
            |r| r.get(0),
        ))?;
        require(roots == self.roots)?;
        let mut q=sql(db.prepare("SELECT w.operation,w.event,i.scope,i.memory FROM deletion_work w JOIN write_intent i USING(operation) ORDER BY w.operation"))?;
        let keys = sql(sql(q.query_map([], |r| {
            Ok((
                r.get::<_, Id>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Scope>(2)?,
                r.get::<_, Id>(3)?,
            ))
        }))?
        .collect::<rusqlite::Result<Vec<_>>>())?;
        let mut works = Vec::new();
        for (operation, event, scope, memory) in keys {
            let member = app
                .authoritative_closure(db, &scope, Target::Memory(memory))?
                .into_iter()
                .find(|m| m.operation == operation)
                .ok_or(Error::Integrity)?;
            app.validate_work(db, &member, event)?;
            let raw: Option<Vec<u8>> = sql(db.query_row(
                "SELECT marker_identity FROM deletion_work WHERE operation=?1",
                [operation],
                |r| r.get(0),
            ))?;
            let identity = raw
                .map(|b| b.try_into().map_err(|_| Error::Integrity))
                .transpose()?;
            let observation=sql(db.query_row("SELECT generation FROM deletion_observation WHERE operation=?1 AND action='KeyUnavailable'",[operation],|r|r.get(0)).optional())?;
            let receipt=sql(db.query_row("SELECT generation FROM deletion_receipt WHERE operation=?1 AND action='KeyUnavailable'",[operation],|r|r.get(0)).optional())?;
            // S3 cannot certify or import unimplemented physical-cleanup outcomes.
            require(!sql(db.query_row("SELECT EXISTS(SELECT 1 FROM deletion_observation WHERE operation=?1 AND action<>'KeyUnavailable') OR EXISTS(SELECT 1 FROM deletion_receipt WHERE operation=?1 AND action<>'KeyUnavailable')",[operation],|r|r.get::<_,bool>(0)))?)?;
            let bytes = member.marker(self.installation, event);
            let name = format!("revoked-{}", yixu_domain::Id::from_bytes(member.key));
            works.push(Work {
                ledger: Ledger {
                    generation: member.generation,
                    identity,
                    observation,
                    receipt,
                },
                member,
                event,
                bytes,
                name,
            });
        }
        Ok(works)
    }
    fn view(w: &Work, pin: Option<&MarkerPin>) -> Result<State> {
        marker_state::classify(
            &w.ledger,
            &w.bytes,
            match pin {
                Some(p) => FileView::Present {
                    bytes: p.bytes(),
                    identity: p.identity(),
                },
                None => FileView::Absent,
            },
        )
        .map_err(|e| match e {
            marker_state::Fault::Integrity => Error::Integrity,
            marker_state::Fault::StorageUnknown => Error::ResourceUnavailable,
        })
    }
    fn inspect(&self, app: &Application) -> Result<Vec<(Work, Option<MarkerPin>, State)>> {
        require(self.report() == Report::Ready)?;
        require(self.io.binding().map_err(|_| Error::ResourceUnavailable)? == self.roots)?;
        let work = self.load(app)?;
        let expected: BTreeSet<_> = work.iter().map(|w| w.name.as_str()).collect();
        for name in self.io.names().map_err(|_| Error::ResourceUnavailable)? {
            require(expected.contains(name.as_str()))?;
        }
        let mut result = Vec::new();
        for w in work {
            let pin = self
                .io
                .observe(&w.name)
                .map_err(|_| Error::ResourceUnavailable)?;
            let state = Self::view(&w, pin.as_ref())?;
            result.push((w, pin, state));
        }
        Ok(result)
    }
    pub(super) fn admit(&self, app: &Application) -> Result<Vec<MarkerPin>> {
        if app.order.blocked.load(Ordering::SeqCst) || self.report() != Report::Ready {
            return Err(Error::CapabilityUnavailable);
        }
        match self.inspect(app) {
            Ok(v) => Ok(v.into_iter().filter_map(|(_, p, _)| p).collect()),
            Err(e) => {
                self.block(
                    app,
                    if e == Error::ResourceUnavailable {
                        Cause::StorageUnknown
                    } else {
                        Cause::MetadataFault
                    },
                );
                Err(e)
            }
        }
    }
    fn current(&self, app: &Application, expected: &Work) -> Result<()> {
        let now = self
            .load(app)?
            .into_iter()
            .find(|w| w.member.operation == expected.member.operation)
            .ok_or(Error::Integrity)?;
        require(&now == expected)
    }
    fn stage(
        &self,
        app: &Application,
        w: &Work,
        pin: &mut MarkerPin,
        point: Point,
        hooks: Hooks,
    ) -> Result<()> {
        // File work and Pin verification occur BEFORE opening the write transaction.
        self.io
            .verify(pin)
            .map_err(|_| Error::ResourceUnavailable)?;
        self.current(app, w)?;
        sql(app.db.execute_batch("BEGIN IMMEDIATE"))?;
        let result = (|| {
            // Recheck the complete persistent D binding in the same short
            // transaction as the transition, in addition to ordered admission.
            let current = self
                .load_rows(app, &app.db)?
                .into_iter()
                .find(|now| now.member.operation == w.member.operation)
                .ok_or(Error::Integrity)?;
            require(&current == w)?;
            let changed=match point {
                Point::IdentityCommitted=>sql(app.db.execute("UPDATE deletion_work SET marker_identity=?1 WHERE operation=?2 AND marker_identity IS NULL",params![pin.identity(),w.member.operation]))?,
                Point::ObservationCommitted=>sql(app.db.execute("INSERT INTO deletion_observation VALUES(?1,'KeyUnavailable',?2)",params![w.member.operation,w.member.generation]))?,
                Point::ReceiptCommitted=>sql(app.db.execute("INSERT INTO deletion_receipt VALUES(?1,'KeyUnavailable',?2)",params![w.member.operation,w.member.generation]))?,
                Point::Published=>return Err(Error::Integrity),
            };
            require(changed == 1)?;
            if hooks.deferred_failure {
                // Controlled SQLite fixture fault: actual deferred FK COMMIT
                // failure, not a fake native completion or disk-full claim.
                sql(app.db.execute_batch("PRAGMA defer_foreign_keys=ON"))?;
                sql(app.db.execute(
                    "INSERT INTO resume_receipt VALUES(?1,?2,1,?3)",
                    params![[201u8; 16], [202u8; 16], [203u8; 32]],
                ))?;
            }
            Ok(())
        })();
        if let Err(e) = result {
            let rb = app.db.execute_batch("ROLLBACK");
            if rb.is_err() || !app.db.is_autocommit() {
                self.uncertain_commit.store(true, Ordering::SeqCst);
                return Err(Error::CapabilityUnavailable);
            }
            return Err(e);
        }
        if app.db.execute_batch("COMMIT").is_err() {
            self.uncertain_commit.store(true, Ordering::SeqCst);
            if !app.db.is_autocommit() {
                let _ = app.db.execute_batch("ROLLBACK");
            }
            return Err(Error::CapabilityUnavailable);
        }
        if hooks.stop == Some(point) {
            self.uncertain_commit.store(true, Ordering::SeqCst);
            return Err(Error::CapabilityUnavailable);
        }
        Ok(())
    }
    fn settle_inner(&self, app: &Application, op: Id, hooks: Hooks) -> Result<State> {
        let mut all = self.inspect(app)?;
        let index = all
            .iter()
            .position(|(w, _, _)| w.member.operation == op)
            .ok_or(Error::ResourceUnavailable)?;
        let (mut w, mut pin, state) = all.swap_remove(index);
        if state == State::Settled {
            return Ok(State::Settled);
        }
        if state == State::Unpublished {
            self.current(app, &w)?;
            require(
                w.ledger.identity.is_none()
                    && w.ledger.observation.is_none()
                    && w.ledger.receipt.is_none(),
            )?;
            let permit = PublishPermit {
                work: w.clone(),
                incarnation: app.incarnation,
                roots: self.roots.clone(),
            };
            self.publish_calls.fetch_add(1, Ordering::SeqCst);
            pin = Some(Publisher(self).publish(permit)?);
            if hooks.stop == Some(Point::Published) {
                return Err(Error::ResourceUnavailable);
            }
        }
        let mut pin = pin.ok_or(Error::Integrity)?;
        if w.ledger.identity.is_none() {
            self.stage(app, &w, &mut pin, Point::IdentityCommitted, hooks)?;
            w.ledger.identity = Some(pin.identity());
        }
        if w.ledger.observation.is_none() {
            self.stage(app, &w, &mut pin, Point::ObservationCommitted, hooks)?;
            w.ledger.observation = Some(w.member.generation);
        }
        if w.ledger.receipt.is_none() {
            self.stage(app, &w, &mut pin, Point::ReceiptCommitted, hooks)?;
        }
        // Pin and the other observed Marker Pins remain owned until all phases finish.
        self.io
            .verify(&mut pin)
            .map_err(|_| Error::ResourceUnavailable)?;
        Ok(State::Settled)
    }
    fn clean_stop_allowed(&self, app: &Application) -> Result<()> {
        if app.order.blocked.load(Ordering::SeqCst) || self.report() != Report::Ready {
            return Err(Error::CapabilityUnavailable);
        }
        // Successful P12 Owner stop/recovery belongs to S4, never inferred here.
        Err(Error::CapabilityUnavailable)
    }
}
impl Application {
    pub(super) fn marker_health(&self) -> Result<Option<Arc<OwnerHealth>>> {
        match self.order.marker_health.lock() {
            Ok(h) => Ok(h.clone()),
            Err(_) => {
                self.order.blocked.store(true, Ordering::SeqCst);
                Err(Error::CapabilityUnavailable)
            }
        }
    }
    fn settle_marker(&mut self, op: Id) -> Result<State> {
        self.settle_marker_with_hooks(op, Hooks::default())
    }
    fn settle_marker_with_hooks(&mut self, op: Id, hooks: Hooks) -> Result<State> {
        let order = self.order.clone();
        let _guard = self.ordered(&order)?;
        if order.blocked.load(Ordering::SeqCst) {
            return Err(Error::CapabilityUnavailable);
        }
        let health = self.marker_health()?.ok_or(Error::CapabilityUnavailable)?;
        let result = health.settle_inner(self, op, hooks);
        if result.is_err() {
            health.block_with(
                self,
                if health.uncertain_commit.load(Ordering::SeqCst) {
                    Cause::CommitUnknown
                } else {
                    Cause::StorageUnknown
                },
                hooks.block,
            );
        }
        result
    }
}

#[cfg(test)]
mod tests;
