//! Private R4 synthetic assembly. It never receives business credentials or content.
use super::*;
use rusqlite::{Connection as Sqlite, OptionalExtension, Transaction, TransactionBehavior, params};
use std::path::Path;

const SCHEMA: &str = include_str!("r4/synthetic.sql");
const TARGET: Id = [42; 16];
const PROFILE: &str = "R4-SYNTHETIC-1";

#[derive(Debug)]
enum Failure {
    Database(rusqlite::Error),
    Protocol(Error),
    Denied,
    Blocked,
    Conflict,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(f, "synthetic database: {error}"),
            Self::Protocol(error) => write!(f, "IPC: {error:?}"),
            other => write!(f, "{other:?}"),
        }
    }
}

impl From<rusqlite::Error> for Failure {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}

impl From<Error> for Failure {
    fn from(error: Error) -> Self {
        Self::Protocol(error)
    }
}

type Result<T> = std::result::Result<T, Failure>;

#[derive(Debug)]
struct Armed {
    operation: i64,
    binding: [u8; 32],
}

// Kept inside the executor module so the broker can consume but cannot construct evidence.
mod executor {
    use super::Armed;

    #[derive(Debug, Clone, Copy, Eq, PartialEq)]
    pub(super) enum Outcome {
        Accepted,
        Cancelled,
    }

    impl Outcome {
        pub(super) fn as_str(self) -> &'static str {
            match self {
                Self::Accepted => "SyntheticAccepted",
                Self::Cancelled => "SyntheticCancelled",
            }
        }
    }

    pub(super) struct SyntheticCompletion {
        operation: i64,
        binding: [u8; 32],
        outcome: Outcome,
    }

    impl SyntheticCompletion {
        pub(super) fn identity(&self) -> (i64, &[u8; 32], Outcome) {
            (self.operation, &self.binding, self.outcome)
        }
    }

    pub(super) fn complete(armed: Armed, cancelled: bool) -> SyntheticCompletion {
        SyntheticCompletion {
            operation: armed.operation,
            binding: armed.binding,
            outcome: if cancelled {
                Outcome::Cancelled
            } else {
                Outcome::Accepted
            },
        }
    }
}

#[derive(Clone, Copy)]
enum Change {
    Grant,
    Revocation,
    Delete,
    Revision,
    Source,
    Key,
    Copy,
    Principal,
    Space,
}

impl Change {
    fn record(self) -> (&'static str, &'static str) {
        match self {
            Self::Grant => ("Grant", "grant_ok=1"),
            Self::Revocation => ("Revocation", "grant_ok=0"),
            Self::Delete => ("Delete", "deletion_gen=deletion_gen+1"),
            Self::Revision => ("Revision", "revision=revision+1"),
            Self::Source => ("Source", "source=source+1,source_ok=0"),
            Self::Key => ("Key", "key_gen=key_gen+1,key_ok=0"),
            Self::Copy => ("Copy", "copy_gen=copy_gen+1,copy_ok=0"),
            Self::Principal => ("Principal", "principal_ok=0"),
            Self::Space => ("Space", "space_ok=0"),
        }
    }

    fn from_record(value: &str) -> Option<Self> {
        [
            Self::Grant,
            Self::Revocation,
            Self::Delete,
            Self::Revision,
            Self::Source,
            Self::Key,
            Self::Copy,
            Self::Principal,
            Self::Space,
        ]
        .into_iter()
        .find(|change| change.record().0 == value)
    }
}

struct Broker {
    db: Sqlite,
    core: Core,
    epoch: Rc<Cell<Option<u64>>>,
    active: Option<(i64, Connection, Id)>,
    blocked: bool,
}

fn create(path: &Path) -> Result<Broker> {
    std::fs::File::create_new(path).map_err(|_| Failure::Conflict)?;
    let mut db = Sqlite::open(path)?;
    db.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA recursive_triggers=ON;",
    )?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    tx.execute_batch(SCHEMA)?;
    let incarnation: Id = tx.query_row("SELECT randomblob(16)", [], |row| row.get(0))?;
    tx.execute(
        "INSERT INTO runtime(id,profile,incarnation) VALUES(1,?1,?2)",
        params![PROFILE, incarnation],
    )?;
    tx.execute(
        "INSERT INTO authority(scope,principal,target,epoch,expiry) VALUES(?1,?2,?3,7,100)",
        params![SCOPE.encode(), PRINCIPAL, TARGET],
    )?;
    tx.commit()?;
    let (core, epoch) = fixture(incarnation);
    Ok(Broker {
        db,
        core,
        epoch,
        active: None,
        blocked: false,
    })
}

fn closing(db: &Sqlite) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM security_event WHERE state<>'Applied')",
        [],
        |r| r.get(0),
    )
}

impl Broker {
    fn block(&mut self) {
        self.blocked = true;
        self.core.realm_blocked = true;
        self.core.invalidate_authority();
        // If durable recording itself fails, this owner stops and reopen remains blocked.
        let _ = self
            .db
            .execute("UPDATE runtime SET blocked=1 WHERE id=1", []);
    }

    fn transact<T>(&mut self, action: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let result = (|| {
            let tx = self
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let result = action(&tx)?;
            tx.commit()?;
            Ok(result)
        })();
        if matches!(&result, Err(Failure::Database(_) | Failure::Blocked)) {
            self.block();
        }
        result
    }

    fn accepting(&mut self) -> Result<bool> {
        if self.blocked {
            return Err(Failure::Blocked);
        }
        match closing(&self.db) {
            Ok(pending) => Ok(!pending),
            Err(error) => {
                self.block();
                Err(error.into())
            }
        }
    }

    fn connect(&mut self, channel: [u8; 32], now: u64) -> Result<Connection> {
        if !self.accepting()? {
            return Err(Failure::Blocked);
        }
        Ok(self.core.connect(channel, now)?)
    }

    fn receive(
        &mut self,
        connection: Connection,
        bytes: &[u8],
        now: u64,
    ) -> Result<super::Outcome> {
        if !self.accepting()? {
            return Err(Failure::Blocked);
        }
        let result = self
            .core
            .receive(connection, bytes, now)
            .map_err(Failure::from);
        self.sync_active()?;
        result
    }

    fn sync_active(&mut self) -> Result<()> {
        let Some((operation, connection, request)) = self.active else {
            return Ok(());
        };
        let transition = match self.core.request_state(connection, request) {
            Some(RequestState::CancelRequested) => ("CancelRequested", "state='Armed'"),
            Some(RequestState::Unknown) => ("Unknown", "state IN ('Armed','CancelRequested')"),
            Some(RequestState::Dispatched) => return Ok(()),
            _ => {
                self.block();
                return Err(Failure::Blocked);
            }
        };
        self.transact(|tx| {
            let (next, from) = transition;
            tx.execute(
                &format!("UPDATE release_intent SET state=?1 WHERE operation=?2 AND {from}"),
                params![next, operation],
            )?;
            Ok(())
        })
    }

    fn disconnect(&mut self, connection: Connection) -> Result<()> {
        self.core.disconnect(connection)?;
        self.sync_active()?;
        if !self.active.is_some_and(|(_, owner, _)| connection == owner) {
            self.core.retire(connection)?;
        }
        Ok(())
    }

    fn arm(&mut self, now: u64) -> Result<Option<Armed>> {
        self.arm_before_commit(now, || {})
    }

    fn arm_before_commit(
        &mut self,
        now: u64,
        before_commit: impl FnOnce(),
    ) -> Result<Option<Armed>> {
        let accepting = self.accepting()?;
        self.core.authorization_changed(now)?;
        self.sync_active()?;
        if !accepting || self.active.is_some() {
            return Ok(None);
        }
        let Some(scheduled) = self.core.dispatch(now)? else {
            return Ok(None);
        };
        let slot = &self.core.sessions[&scheduled.connection];
        let digest = slot.requests[&scheduled.request_id].digest;
        let session = slot.id;
        let deadline = slot.deadline;
        let channel = slot.channel;
        let result = self.transact(|tx| {
            let Request::Inspect(target) = scheduled.request else { return Err(Failure::Denied); };
            let scope = scheduled.scope.encode();
            let now = i64::try_from(now).map_err(|_| Failure::Denied)?;
            let epoch = i64::try_from(scheduled.epoch).map_err(|_| Failure::Denied)?;
            let snapshot = tx.query_row(
                "SELECT epoch, expiry, revision, source, key_gen, copy_gen, deletion_gen
                 FROM authority WHERE scope=?1 AND principal=?2 AND target=?3 AND epoch=?4
                 AND expiry>?5 AND grant_ok=1 AND principal_ok=1 AND space_ok=1 AND source_ok=1
                 AND key_ok=1 AND copy_ok=1 AND deletion_gen=0",
                params![scope, scheduled.principal, target, epoch, now],
                |row| Ok([
                    row.get::<_, i64>(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
                    row.get(4)?, row.get(5)?, row.get(6)?,
                ]),
            ).optional()?.ok_or(Failure::Denied)?;
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM release_intent WHERE scope=?1 AND principal=?2 AND request=?3)",
                params![scope, scheduled.principal, scheduled.request_id], |row| row.get(0),
            )?;
            if exists { return Err(Failure::Conflict); }
            let operation: i64 = tx.query_row(
                "SELECT coalesce(max(operation),0)+1 FROM release_intent", [], |row| row.get(0),
            )?;
            let mut hash = Sha256::new();
            hash.update(b"YXIP/r4-synthetic-operation/v1\0");
            hash.update(scope);
            hash.update(scheduled.principal);
            hash.update(target);
            hash.update(scheduled.request_id);
            hash.update(digest);
            hash.update(session);
            hash.update(channel);
            hash.update(deadline.to_be_bytes());
            hash.update(scheduled.connection.incarnation);
            hash.update(scheduled.connection.ordinal.to_be_bytes());
            hash.update(operation.to_be_bytes());
            hash.update([1]); // Attempt.
            for field in snapshot { hash.update(field.to_be_bytes()); }
            let binding: [u8; 32] = hash.finalize().into();
            tx.execute(
                "INSERT INTO release_intent(operation,scope,principal,target,request,binding)
                 VALUES(?1,?2,?3,?4,?5,?6)",
                params![operation, scope, scheduled.principal, target, scheduled.request_id, binding],
            )?;
            before_commit();
            Ok(Armed { operation, binding })
        });
        match result {
            Ok(armed) => {
                self.active = Some((armed.operation, scheduled.connection, scheduled.request_id));
                Ok(Some(armed))
            }
            Err(error @ (Failure::Denied | Failure::Conflict)) => {
                // The only drop path is a definitive pre-arm rejection.
                self.core
                    .sessions
                    .get_mut(&scheduled.connection)
                    .unwrap()
                    .requests
                    .get_mut(&scheduled.request_id)
                    .unwrap()
                    .state = RequestState::Dropped;
                self.core.active = None;
                Err(error)
            }
            Err(error) => Err(error),
        }
    }

    fn observe(&mut self, completion: &executor::SyntheticCompletion) -> Result<()> {
        let (operation, binding, outcome) = completion.identity();
        if self.blocked {
            return Err(Failure::Blocked);
        }
        if self.active.map(|active| active.0) != Some(operation) {
            return Err(Failure::Conflict);
        }
        self.transact(|tx| {
            let matched: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM release_intent WHERE operation=?1 AND attempt=1
                 AND binding=?2 AND state<>'Settled')",
                params![operation, binding],
                |r| r.get(0),
            )?;
            if !matched {
                return Err(Failure::Conflict);
            }
            let seen = receipt_like(tx, "observation", operation, binding)?;
            if let Some(previous) = seen {
                return if previous == outcome.as_str() {
                    Ok(())
                } else {
                    Err(Failure::Conflict)
                };
            }
            tx.execute(
                "INSERT INTO observation VALUES(?1,1,?2,?3)",
                params![operation, binding, outcome.as_str()],
            )?;
            Ok(())
        })
    }

    fn settle(&mut self, completion: executor::SyntheticCompletion) -> Result<executor::Outcome> {
        let (operation, binding, outcome) = completion.identity();
        if self.blocked {
            return Err(Failure::Blocked);
        }
        let Some((active, connection, request)) = self.active else {
            return Err(Failure::Conflict);
        };
        if active != operation {
            return Err(Failure::Conflict);
        }
        self.transact(|tx| {
            if receipt_like(tx, "observation", operation, binding)?.as_deref()
                != Some(outcome.as_str())
            {
                return Err(Failure::Conflict);
            }
            tx.execute(
                "INSERT INTO receipt VALUES(?1,1,?2,?3)",
                params![operation, binding, outcome.as_str()],
            )?;
            Ok(())
        })?;
        self.core
            .sessions
            .get_mut(&connection)
            .unwrap()
            .requests
            .get_mut(&request)
            .unwrap()
            .state = RequestState::SettledMetadata;
        self.active = None;
        self.core.active = None;
        if self.core.state(connection) == State::Closed {
            self.core.retire(connection)?;
        }
        Ok(outcome)
    }

    fn close_for(&mut self, scope: Scope, change: Change) -> Result<i64> {
        if scope != SCOPE {
            self.block();
            return Err(Failure::Protocol(Error::Binding));
        }
        if self.blocked {
            return Err(Failure::Blocked);
        }
        let slot = self.active;
        let id = self.transact(|tx| {
            tx.execute("INSERT INTO security_event(kind,scope) VALUES(?1,?2)", params![change.record().0, scope.encode()])?;
            let id = tx.last_insert_rowid();
            if let Some((operation, _, _)) = slot {
                tx.execute("UPDATE release_intent SET state='CancelRequested' WHERE operation=?1 AND state='Armed'", [operation])?;
            }
            Ok(id)
        })?;
        for session in self
            .core
            .sessions
            .values_mut()
            .filter(|session| session.accepting())
        {
            session.invalidate(State::Closing, false);
        }
        Ok(id)
    }

    fn make_effective(&mut self, id: i64) -> Result<()> {
        if self.blocked || self.active.is_some() {
            return Err(Failure::Blocked);
        }
        self.transact(|tx| {
            let changed = tx.execute(
                "UPDATE security_event SET state='EffectiveFence'
                WHERE id=?1 AND state='Closing'
                AND NOT EXISTS(SELECT 1 FROM security_event WHERE id<?1 AND state<>'Applied')",
                [id],
            )?;
            if changed != 1 {
                return Err(Failure::Conflict);
            }
            Ok(())
        })
    }

    fn apply(&mut self, id: i64) -> Result<()> {
        if self.blocked || self.active.is_some() {
            return Err(Failure::Blocked);
        }
        let epoch = self.transact(|tx| {
            let kind: String = tx
                .query_row(
                    "SELECT kind FROM security_event
                WHERE id=?1 AND state='EffectiveFence'",
                    [id],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or(Failure::Conflict)?;
            let change = Change::from_record(&kind).ok_or(Failure::Blocked)?;
            let changed = tx.execute(
                &format!(
                    "UPDATE authority SET epoch=epoch+1,{} WHERE scope=?1",
                    change.record().1
                ),
                [SCOPE.encode()],
            )?;
            if changed != 1 {
                return Err(Failure::Blocked);
            }
            tx.execute(
                "UPDATE security_event SET state='Applied' WHERE id=?1",
                [id],
            )?;
            let epoch: u64 = tx.query_row("SELECT epoch FROM authority", [], |r| r.get(0))?;
            Ok(epoch)
        })?;
        self.epoch.set(Some(epoch));
        Ok(())
    }
}

fn receipt_like(
    db: &Sqlite,
    table: &'static str,
    operation: i64,
    binding: &[u8; 32],
) -> Result<Option<String>> {
    Ok(db
        .query_row(
            &format!("SELECT outcome FROM {table} WHERE operation=?1 AND attempt=1 AND binding=?2"),
            params![operation, binding],
            |row| row.get(0),
        )
        .optional()?)
}

struct RecoveryBlocked {
    db: Sqlite,
    unresolved: i64,
}

impl RecoveryBlocked {
    fn inspect(path: &Path) -> Result<Self> {
        let db = Sqlite::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let profile: String =
            db.query_row("SELECT profile FROM runtime WHERE id=1", [], |r| r.get(0))?;
        if profile != PROFILE {
            return Err(Failure::Blocked);
        }
        let (inconsistent, unresolved): (bool, i64) = db.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)
             OR EXISTS(SELECT 1 FROM release_intent AS i WHERE (i.state='Settled') <>
                 EXISTS(SELECT 1 FROM receipt WHERE operation=i.operation))
             OR EXISTS(SELECT 1 FROM release_intent AS i WHERE NOT EXISTS(
                 SELECT 1 FROM audit WHERE operation=i.operation AND event='Armed'))
             OR EXISTS(SELECT 1 FROM observation AS o WHERE NOT EXISTS(
                 SELECT 1 FROM audit WHERE operation=o.operation AND event='Observed'))
             OR EXISTS(SELECT 1 FROM receipt AS r WHERE NOT EXISTS(
                 SELECT 1 FROM audit WHERE operation=r.operation AND event='Settled')),
             (SELECT count(*) FROM release_intent WHERE state<>'Settled')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if inconsistent {
            return Err(Failure::Blocked);
        }
        Ok(Self { db, unresolved })
    }

    fn receipt(&self, operation: i64, binding: &[u8; 32]) -> Result<Option<String>> {
        receipt_like(&self.db, "receipt", operation, binding)
    }
}

mod cases;
