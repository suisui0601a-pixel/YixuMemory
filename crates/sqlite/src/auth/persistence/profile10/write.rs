use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Point {
    Pending,
    Staged,
    BeforeKey,
    Wrapped,
    BeforeCipher,
    Published,
    Sealed,
    BeforeCommit,
    Committed,
}

impl Engine {
    pub(super) fn plan_row(
        db: &Connection,
        identity: Identity,
        request: Id,
    ) -> Result<Option<(Plan, String)>> {
        sql(db
            .query_row(
                "SELECT operation,memory,revision,evidence,evidence_generation,generation,
             expected_current,payload,key_id,incarnation,epoch,dispatch_binding,state
             FROM write_intent WHERE scope=?1 AND principal=?2 AND request=?3",
                params![identity.scope_bytes(), identity.principal, request],
                |r| {
                    Ok((
                        Plan {
                            operation: r.get(0)?,
                            identity,
                            request: WriteRequest {
                                request,
                                memory: r.get(1)?,
                                revision: r.get(2)?,
                                evidence: r.get(3)?,
                                evidence_generation: r.get(4)?,
                                generation: r.get(5)?,
                                expected_current: r.get(6)?,
                            },
                            payload: r.get(7)?,
                            key: r.get(8)?,
                            incarnation: r.get(9)?,
                            epoch: r.get(10)?,
                            dispatch: r.get(11)?,
                        },
                        r.get(12)?,
                    ))
                },
            )
            .optional())
    }

    fn stage(
        &mut self,
        identity: Identity,
        request: WriteRequest,
        now: i64,
    ) -> Result<(Plan, bool)> {
        if self.recovery_only {
            return Err(Error::CapabilityUnavailable);
        }
        if request.request == [0; 16]
            || request.revision == [0; 16]
            || request.memory == [0; 16]
            || request.generation != 1
            || now < 0
        {
            return Err(Error::InvalidInput);
        }
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let epoch = Self::authorize(
            &tx,
            identity,
            request,
            self.installation,
            self.incarnation,
            now,
        )?;
        if let Some((plan, state)) = Self::plan_row(&tx, identity, request.request)? {
            let p = plan.request;
            if p.memory != request.memory
                || p.revision != request.revision
                || p.evidence != request.evidence
                || p.evidence_generation != request.evidence_generation
                || p.generation != request.generation
                || p.expected_current != request.expected_current
            {
                return Err(Error::IdempotencyConflict);
            }
            if state != "Committed" {
                return Err(Error::CapabilityUnavailable);
            }
            sql(tx.commit())?;
            return Ok((plan, true));
        }
        let unfinished: bool = sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM write_intent WHERE state<>'Committed')",
            [],
            |r| r.get(0),
        ))?;
        if unfinished {
            return Err(Error::CapabilityUnavailable);
        }
        let existing: Option<Id> = sql(tx
            .query_row(
                "SELECT current_revision FROM memory WHERE scope=?1 AND id=?2",
                params![identity.scope_bytes(), request.memory],
                |r| r.get(0),
            )
            .optional())?;
        if existing != request.expected_current {
            return Err(Error::CasConflict);
        }
        let operation = fresh()?;
        let payload = fresh()?;
        let key = fresh()?;
        let mut plan = Plan {
            operation,
            identity,
            request,
            payload,
            key,
            incarnation: self.incarnation,
            epoch,
            dispatch: [0; 32],
        };
        plan.dispatch = plan.expected_dispatch(&tx, self.installation)?;
        sql(tx.execute(
            "INSERT INTO write_intent VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,'Pending')",
            params![operation,identity.scope_bytes(),identity.principal,request.request,
                request.memory,request.revision,payload,key,request.evidence,request.evidence_generation,
                request.generation,request.expected_current,self.incarnation,epoch,plan.dispatch],
        ))?;
        sql(tx.commit())?;
        Ok((plan, false))
    }

    pub(super) fn write(
        &mut self,
        identity: Identity,
        request: WriteRequest,
        body: &str,
        now: i64,
        mut hook: impl FnMut(Point, &mut Self) -> Result<()>,
    ) -> Result<WriteReceipt> {
        if body.is_empty() || body.len() > 4096 {
            return Err(Error::InvalidInput);
        }
        let (plan, duplicate) = self.stage(identity, request, now)?;
        if duplicate {
            let (plain, _) = self.verify_files(&plan)?;
            if plain.bytes() != body.as_bytes() {
                return Err(Error::IdempotencyConflict);
            }
            Self::authorize(
                &self.db,
                identity,
                request,
                self.installation,
                self.incarnation,
                now,
            )?;
            return self.receipt(plan.operation);
        }
        let result = self.finish_write(&plan, body, now, &mut hook);
        if result.is_err() {
            // Any failure after durable intent retains evidence and shuts this owner.
            // An already committed receipt remains committed even if its response was lost.
            let _ = self.db.execute(
                "UPDATE write_intent SET state='Unknown'
                WHERE operation=?1 AND state IN ('Pending','Staged','Sealing','Published')",
                [plan.operation],
            );
            let _ = self.db.execute("UPDATE runtime SET blocked=1", []);
        }
        result
    }
    fn finish_write(
        &mut self,
        plan: &Plan,
        body: &str,
        now: i64,
        hook: &mut impl FnMut(Point, &mut Self) -> Result<()>,
    ) -> Result<WriteReceipt> {
        hook(Point::Pending, self)?;
        sql(self.db.execute(
            "UPDATE write_intent SET state='Staged' WHERE operation=?1",
            [plan.operation],
        ))?;
        hook(Point::Staged, self)?;
        {
            let tx = sql(self
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate))?;
            Self::authorize(
                &tx,
                plan.identity,
                plan.request,
                self.installation,
                self.incarnation,
                now,
            )?;
            sql(tx.execute(
                "UPDATE write_intent SET state='Sealing' WHERE operation=?1",
                [plan.operation],
            ))?;
            sql(tx.commit())?;
        }
        hook(Point::BeforeKey, self)?;
        let binding = plan.binding(self.installation);
        // W2 allocates its own independent OS-random DEK and publishes its DPAPI wrapper by W1.
        let (key, _) = self.vault.create(&binding)?;
        hook(Point::Wrapped, self)?;
        let envelope = content_crypto::seal(&binding, body.as_bytes(), &key)?;
        drop(key);
        hook(Point::BeforeCipher, self)?;
        self.files
            .publish(
                &format!("pending-{}", filename(fresh()?)),
                &filename(plan.payload),
                &envelope,
                0,
                |_| {},
            )
            .map_err(|_| Error::ResourceUnavailable)?;
        hook(Point::Published, self)?;
        let (_, publication) = self.verify_files(plan)?;
        {
            let tx = sql(self
                .db
                .transaction_with_behavior(TransactionBehavior::Immediate))?;
            Self::authorize(
                &tx,
                plan.identity,
                plan.request,
                self.installation,
                self.incarnation,
                now,
            )?;
            sql(tx.execute(
                "INSERT INTO publication VALUES(?1,?2,?3,?4,?5,?6)",
                params![
                    plan.operation,
                    publication.cipher_hash,
                    publication.wrapper_hash,
                    publication.cipher_identity,
                    publication.wrapper_identity,
                    binding.aad(32)
                ],
            ))?;
            sql(tx.execute(
                "UPDATE write_intent SET state='Published' WHERE operation=?1",
                [plan.operation],
            ))?;
            sql(tx.commit())?;
        }
        hook(Point::Sealed, self)?;
        // Read-back with DPAPI and tag validation is outside the final SQLite writer transaction.
        let (_, publication) = self.verify_files(plan)?;
        hook(Point::BeforeCommit, self)?;
        self.commit(plan, &publication, now)?;
        hook(Point::Committed, self)?;
        self.receipt(plan.operation)
    }

    pub(super) fn verify_files(&self, plan: &Plan) -> Result<(LockedBuffer, Publication)> {
        self.verify_files_with(plan, |_| Ok(()))
    }

    pub(super) fn verify_files_with(
        &self,
        plan: &Plan,
        mut checkpoint: impl FnMut(super::read::ReadPoint) -> Result<()>,
    ) -> Result<(LockedBuffer, Publication)> {
        let binding = plan.binding(self.installation);
        let cipher_name = filename(plan.payload);
        let key_name = filename(plan.key);
        let cipher_id = identity(&self.files, &cipher_name)?;
        let key_id = identity(&self.vault.anchor, &key_name).map_err(|e| {
            if e == Error::ResourceUnavailable {
                Error::CapabilityUnavailable
            } else {
                e
            }
        })?;
        let envelope = self
            .files
            .read(&cipher_name)
            .map_err(|_| Error::ResourceUnavailable)?;
        checkpoint(super::read::ReadPoint::CipherRead)?;
        checkpoint(super::read::ReadPoint::BeforeUnwrap)?;
        let (key, wrapped_hash) = self.vault.load(&binding)?;
        let plain = content_crypto::open(&binding, &envelope, &key)?;
        drop(key);
        checkpoint(super::read::ReadPoint::Decrypted)?;
        if std::str::from_utf8(plain.bytes()).is_err()
            || identity(&self.files, &cipher_name)? != cipher_id
            || identity(&self.vault.anchor, &key_name)? != key_id
        {
            return Err(Error::Integrity);
        }
        let publication = Publication {
            cipher_hash: digest(&envelope),
            wrapper_hash: wrapped_hash.try_into().map_err(|_| Error::Integrity)?,
            cipher_identity: cipher_id,
            wrapper_identity: key_id,
        };
        let expected: Option<(Publication, Vec<u8>)> = sql(self
            .db
            .query_row(
                "SELECT cipher_hash,wrapper_hash,cipher_identity,wrapper_identity,binding
             FROM publication WHERE operation=?1",
                [plan.operation],
                |r| {
                    Ok((
                        Publication {
                            cipher_hash: r.get(0)?,
                            wrapper_hash: r.get(1)?,
                            cipher_identity: r.get(2)?,
                            wrapper_identity: r.get(3)?,
                        },
                        r.get(4)?,
                    ))
                },
            )
            .optional())?;
        if let Some((expected, b)) = expected
            && (expected != publication || b != binding.aad(32))
        {
            return Err(Error::Integrity);
        }
        Ok((plain, publication))
    }
    fn commit(&mut self, plan: &Plan, publication: &Publication, now: i64) -> Result<()> {
        let callback = self.before_commit.take();
        self.commit_before(plan, publication, now, |_| match callback {
            Some(callback) => callback(),
            None => Ok(()),
        })
    }
    fn commit_before(
        &mut self,
        plan: &Plan,
        publication: &Publication,
        now: i64,
        before_commit: impl FnOnce(&Connection) -> Result<()>,
    ) -> Result<()> {
        let tx = sql(self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate))?;
        let epoch = Self::authorize(
            &tx,
            plan.identity,
            plan.request,
            self.installation,
            self.incarnation,
            now,
        )?;
        if plan.expected_dispatch(&tx, self.installation)? != plan.dispatch {
            return Err(Error::Integrity);
        }
        if epoch != plan.epoch || self.incarnation != plan.incarnation {
            return Err(Error::AuthorizationDenied);
        }
        let confirmed:bool=sql(tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM write_intent i JOIN publication p USING(operation)
             WHERE i.operation=?1 AND i.state='Published' AND i.dispatch_binding=?2
              AND p.cipher_hash=?3 AND p.wrapper_hash=?4 AND p.cipher_identity=?5 AND p.wrapper_identity=?6)",
            params![plan.operation,plan.dispatch,publication.cipher_hash,publication.wrapper_hash,
                publication.cipher_identity,publication.wrapper_identity],|r|r.get(0),
        ))?;
        if !confirmed {
            return Err(Error::Integrity);
        }
        let scope = plan.identity.scope_bytes();
        let req = plan.request;
        let existing: Option<Id> = sql(tx
            .query_row(
                "SELECT current_revision FROM memory WHERE scope=?1 AND id=?2",
                params![scope, req.memory],
                |r| r.get(0),
            )
            .optional())?;
        if existing != req.expected_current {
            return Err(Error::CasConflict);
        }
        sql(tx.execute(
            "INSERT INTO security_event(scope,kind) VALUES(?1,'Write')",
            [&scope],
        ))?;
        let event = tx.last_insert_rowid();
        if sql(gates::make_effective(&tx, event))? != 1 {
            return Err(Error::CapabilityUnavailable);
        }
        let number: i64 = match existing {
            Some(parent) => sql(tx.query_row(
                "SELECT number+1 FROM revision WHERE scope=?1 AND memory=?2 AND id=?3",
                params![scope, req.memory, parent],
                |r| r.get(0),
            ))?,
            None => {
                sql(tx.execute(
                    "INSERT INTO memory VALUES(?1,?2,?3,'active')",
                    params![scope, req.memory, req.revision],
                ))?;
                1
            }
        };
        sql(tx.execute(
            "INSERT INTO revision VALUES(?1,?2,?3,?4,?5)",
            params![scope, req.memory, req.revision, number, plan.payload],
        ))?;
        if let Some(parent) = existing
            && sql(tx.execute(
                "UPDATE memory SET current_revision=?1 WHERE scope=?2 AND id=?3 AND current_revision=?4",
                params![req.revision,scope,req.memory,parent],
            ))? != 1 {
                return Err(Error::CasConflict);
            }
        sql(tx.execute(
            "INSERT INTO local_key VALUES(?1,?2,?3,?4,1,0)",
            params![scope, plan.payload, plan.key, req.generation],
        ))?;
        sql(tx.execute(
            "INSERT INTO payload VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                scope,
                req.memory,
                req.revision,
                plan.payload,
                plan.key,
                req.generation,
                plan.operation
            ],
        ))?;
        sql(tx.execute(
            "INSERT INTO revision_evidence VALUES(?1,?2,?3,?4,?5)",
            params![
                scope,
                req.memory,
                req.revision,
                req.evidence,
                req.evidence_generation
            ],
        ))?;
        sql(tx.execute(
            "UPDATE runtime SET epoch=epoch+1,knowledge_seq=knowledge_seq+1",
            [],
        ))?;
        sql(tx.execute("UPDATE authority SET epoch=(SELECT epoch FROM runtime)", []))?;
        sql(tx.execute(
            "INSERT INTO security_receipt(event,epoch) SELECT ?1,epoch FROM runtime",
            [event],
        ))?;
        sql(tx.execute(
            "UPDATE security_event SET state='Applied' WHERE id=?1",
            [event],
        ))?;
        sql(tx.execute(
            "INSERT INTO receipt SELECT ?1,?2,?3,?4,knowledge_seq,?5 FROM runtime",
            params![plan.operation, scope, req.memory, req.revision, event],
        ))?;
        sql(tx.execute("INSERT INTO outbox SELECT operation,'MemoryCreated',knowledge_seq FROM receipt WHERE operation=?1",[plan.operation]))?;
        sql(tx.execute(
            "UPDATE write_intent SET state='Committed' WHERE operation=?1",
            [plan.operation],
        ))?;
        before_commit(&tx)?;
        sql(tx.commit())
    }
    pub(super) fn receipt(&self, operation: Id) -> Result<WriteReceipt> {
        sql(self.db.query_row(
            "SELECT memory,revision,knowledge_seq FROM receipt WHERE operation=?1",
            [operation],
            |r| {
                Ok(WriteReceipt {
                    operation,
                    memory: r.get(0)?,
                    revision: r.get(1)?,
                    knowledge_seq: r.get(2)?,
                })
            },
        ))
    }
}
