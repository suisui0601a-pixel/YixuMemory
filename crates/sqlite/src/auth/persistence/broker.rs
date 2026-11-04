//! Synthetic-only single-owner actor. No public factory, feature or arbitrary sink callback.
//! The process harness owns this actor; ordinary applications have no DB/key/plaintext API.
use super::*;
use std::{fs::File, io::Write};

enum Sink {
    Synthetic { limit: usize },
    // Only inherited stdout of the dedicated test child; synchronous, no cancellation/takeover.
    InheritedPipe,
}
struct Broker<C: Clock> {
    db: AuthDatabase<C>,
    store: Store,
    vault: LocalVault,
    _lock: File,
    _anchor: winfs::Anchor,
    incarnation: Id,
    blocked: bool,
    events: Vec<&'static str>,
}
impl<C: Clock> Broker<C> {
    fn start(db: AuthDatabase<C>, store: Store, vault: LocalVault) -> Result<Self> {
        if db.profile != 8 {
            return Err(Error::CapabilityUnavailable);
        }
        // Lock is anchored in the same pinned ciphertext directory for this assembly.
        let anchor = winfs::Anchor::open(&store.root).map_err(|_| Error::CapabilityUnavailable)?;
        let lock = anchor.broker_lock().map_err(|_| Error::Busy)?;
        let incarnation = fresh()?;
        let mut b = Self {
            db,
            store,
            vault,
            _lock: lock,
            _anchor: anchor,
            incarnation,
            blocked: false,
            events: Vec::new(),
        };
        let tx =
            b.db.connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sql_error)?;
        tx.execute(
            "UPDATE release_intent SET state='Unknown' WHERE state='Armed'",
            [],
        )
        .map_err(sql_error)?;
        tx.execute(
            "UPDATE release_intent SET state='Quarantined' WHERE state='Pending'",
            [],
        )
        .map_err(sql_error)?;
        // Old permits are never reconstructed. Unknown outcomes remain terminal, not replays.
        tx.execute(
            "UPDATE broker_runtime SET incarnation=?1,blocked=0",
            [incarnation.to_string()],
        )
        .map_err(sql_error)?;
        tx.commit().map_err(sql_error)?;
        b.events.push("started");
        Ok(b)
    }
    fn check(tx: &Transaction<'_>, ctx: &AuthContext, now: i64) -> Result<()> {
        AuthDatabase::<C>::content_check(tx, ctx, now, "memory.read")?;
        let blocked:bool=tx.query_row("SELECT (SELECT blocked FROM broker_runtime WHERE singleton=1) OR EXISTS(SELECT 1 FROM release_fence WHERE tenant_id=?1 AND space_id=?2)",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string()],|r|r.get(0)).map_err(sql_error)?;
        if blocked {
            return Err(Error::CapabilityUnavailable);
        }
        Ok(())
    }
    fn request(
        &mut self,
        ctx: &AuthContext,
        content: Id,
        request: Id,
        destination: Id,
        expires: i64,
    ) -> Result<Id> {
        if self.blocked {
            return Err(Error::CapabilityUnavailable);
        }
        let p = self.db.plan(ctx, content)?;
        let (payload, key, owner, kind, _) = p.copies.last().ok_or(Error::Constraint)?;
        let binding = binding(ctx, *payload, *key, *owner, *kind);
        let digest =
            hash(format!("release-v1|{content}|{destination}|{expires}|synthetic.read").as_bytes());
        let tx = self
            .db
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let now = time(&self.db.clock)?;
        Self::check(&tx, ctx, now)?;
        if expires <= now || expires > now + 1000 {
            return Err(Error::InvalidInput);
        }
        let old:Option<(String,Vec<u8>)>=tx.query_row("SELECT id,digest FROM release_intent WHERE tenant_id=?1 AND space_id=?2 AND principal_id=?3 AND request_id=?4",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),ctx.principal.to_string(),request.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional().map_err(sql_error)?;
        if let Some((id, d)) = old {
            if d != digest {
                return Err(Error::IdempotencyConflict);
            }
            return id.parse();
        }
        let id = fresh()?;
        tx.execute("INSERT INTO release_intent VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'synthetic.read',?11,?12,?13,0,'Pending',0)",params![id.to_string(),ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),ctx.principal.to_string(),request.to_string(),digest,content.to_string(),binding.aad(32),ctx.tenant_epoch,ctx.space_epoch,destination.to_string(),expires,self.incarnation.to_string()]).map_err(sql_error)?;
        tx.execute(
            "INSERT INTO release_event(intent_id,event) VALUES(?1,'Pending')",
            [id.to_string()],
        )
        .map_err(sql_error)?;
        tx.commit().map_err(sql_error)?;
        self.events.push("Pending");
        Ok(id)
    }
    fn terminal(&mut self, id: Id, state: &str, n: usize) -> Result<()> {
        let tx = self
            .db
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let changed=tx.execute("UPDATE release_intent SET state=?2,accepted_bytes=?3 WHERE id=?1 AND incarnation=?4",params![id.to_string(),state,n as i64,self.incarnation.to_string()]).map_err(sql_error)?;
        if changed != 1 {
            return Err(Error::CapabilityUnavailable);
        }
        tx.execute(
            "INSERT INTO release_event(intent_id,event) VALUES(?1,?2)",
            params![id.to_string(), state],
        )
        .map_err(sql_error)?;
        tx.commit().map_err(sql_error)
    }
    fn release(
        &mut self,
        ctx: &AuthContext,
        id: Id,
        destination: Id,
        sink: Sink,
        stop: u8,
    ) -> Result<()> {
        if self.blocked {
            return Err(Error::CapabilityUnavailable);
        }
        // No callback/queue can acquire the actor during this synchronous command.
        let row:(String,Vec<u8>,i64,String,String,i64)=self.db.connection.query_row("SELECT content_id,binding,expires,state,incarnation,attempt FROM release_intent WHERE id=?1 AND tenant_id=?2 AND space_id=?3 AND principal_id=?4 AND destination=?5",params![id.to_string(),ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),ctx.principal.to_string(),destination.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).map_err(sql_error)?;
        if row.3 != "Pending" || row.4 != self.incarnation.to_string() || row.5 != 0 {
            return Err(Error::CapabilityUnavailable);
        }
        if time(&self.db.clock)? >= row.2 {
            self.terminal(id, "Expired", 0)?;
            return Err(Error::AuthorizationDenied);
        }
        let content = row.0.parse()?;
        // W2 validates all graph copies and current authority; no plaintext leaves the actor.
        if self
            .db
            .release_backend(ctx, content, &self.store, Some(&self.vault))
            .is_err()
        {
            self.terminal(id, "Denied", 0)?;
            return Err(Error::CapabilityUnavailable);
        }
        let p = self.db.plan(ctx, content)?;
        let (payload, key, owner, kind, _) = p.copies.last().ok_or(Error::Constraint)?;
        let b = binding(ctx, *payload, *key, *owner, *kind);
        if b.aad(32) != row.1 {
            self.terminal(id, "Quarantined", 0)?;
            return Err(Error::Constraint);
        }
        let bytes = self.store.read(*payload)?;
        let k = self
            .db
            .local_key(ctx, &b, &self.vault, "memory.read", false)?;
        let plain = decrypt_locked(&b, &bytes, &k)?;
        drop(k);
        self.events.push("verified");
        let tx = self
            .db
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        Self::check(&tx, ctx, time(&self.db.clock)?)?;
        AuthDatabase::<C>::check_local_keys(&tx, ctx, content)?;
        let n=tx.execute("UPDATE release_intent SET state='Armed',attempt=1 WHERE id=?1 AND state='Pending' AND incarnation=?2 AND expires>?3 AND tenant_epoch=?4 AND space_epoch=?5",params![id.to_string(),self.incarnation.to_string(),time(&self.db.clock)?,ctx.tenant_epoch,ctx.space_epoch]).map_err(sql_error)?;
        if n != 1 {
            return Err(Error::AuthorizationDenied);
        }
        tx.execute(
            "INSERT INTO release_event(intent_id,event) VALUES(?1,'Armed')",
            [id.to_string()],
        )
        .map_err(sql_error)?;
        if tx.commit().is_err() {
            self.blocked = true;
            return Err(Error::CapabilityUnavailable);
        }
        self.events.push("Armed");
        if stop == 3 {
            eprintln!("W3-ARMED");
            loop {
                std::thread::park();
            }
        }
        if stop == 1 {
            self.blocked = true;
            return Err(Error::CapabilityUnavailable);
        }
        // No SQLite transaction spans this I/O. The actor remains exclusively occupied.
        let accepted = match sink {
            Sink::Synthetic { limit } => Ok(limit.min(plain.bytes().len())),
            Sink::InheritedPipe => {
                if !winfs::inherited_stdout_is_pipe() {
                    self.blocked = true;
                    let _ = self.terminal(id, "Unknown", 0);
                    return Err(Error::CapabilityUnavailable);
                }
                let mut pipe = std::io::stdout().lock();
                // stdout can buffer without a newline. Acceptance requires synchronous flush.
                pipe.write(plain.bytes())
                    .and_then(|n| pipe.flush().map(|()| n))
                    .map_err(|_| Error::CapabilityUnavailable)
            }
        };
        let n = match accepted {
            Ok(n) => n,
            Err(e) => {
                self.blocked = true;
                let _ = self.terminal(id, "Unknown", 0);
                return Err(e);
            }
        };
        self.events.push("sink-accepted");
        if stop == 4 {
            eprintln!("W3-OUTPUT");
            loop {
                std::thread::park();
            }
        }
        let state = if n == plain.bytes().len() {
            "Accepted"
        } else if n == 0 {
            "Failed"
        } else {
            "Partial"
        };
        if stop == 2 {
            self.blocked = true;
            return Err(Error::CapabilityUnavailable);
        }
        if self.terminal(id, state, n).is_err() {
            self.blocked = true;
            return Err(Error::CapabilityUnavailable);
        }
        self.events.push(state);
        Ok(())
    }
    fn administer(&mut self, a: &BootstrapAuthority, change: AdminChange) -> Result<()> {
        if self.blocked {
            return Err(Error::CapabilityUnavailable);
        }
        self.db.administer(a, change)?;
        self.events.push("revoke-commit");
        Ok(())
    }
    fn revoke_key(&mut self, a: &BootstrapAuthority, binding: &PayloadBinding) -> Result<()> {
        if self.blocked {
            return Err(Error::CapabilityUnavailable);
        }
        if let Err(e) = self.db.revoke_local(a, binding, &self.vault) {
            // DB tombstone may already be committed. Never assume marker failure rolled it back.
            self.blocked = true;
            return Err(e);
        }
        self.events.push("key-revoke-commit");
        Ok(())
    }
    fn fence(&mut self, ctx: &AuthContext) -> Result<Id> {
        if self.blocked {
            return Err(Error::CapabilityUnavailable);
        }
        let id = fresh()?;
        let tx = self
            .db
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        AuthDatabase::<C>::content_check(&tx, ctx, time(&self.db.clock)?, "memory.read")?;
        tx.execute(
            "INSERT INTO release_fence VALUES(?1,?2,?3)",
            params![
                id.to_string(),
                ctx.scope.tenant.to_string(),
                ctx.scope.space.to_string()
            ],
        )
        .map_err(sql_error)?;
        tx.commit().map_err(sql_error)?;
        self.events.push("Tguard");
        Ok(id)
    }
}
#[cfg(test)]
mod tests;
