use super::*;

#[derive(Clone, Copy)]
pub(super) struct ReadQuery {
    pub(super) memory: Id,
    pub(super) revision: Option<Id>,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum ReadPoint {
    CipherRead,
    BeforeUnwrap,
    Decrypted,
}

/// Borrow-only delivery. The trusted callback executes under the final SQLite writer
/// reservation and must not perform external I/O or return any reference to the body.
impl Engine {
    fn read_snapshot(&self, identity: Identity, query: ReadQuery) -> Result<(Plan, i64, Id)> {
        if self.recovery_only {
            return Err(Error::CapabilityUnavailable);
        }
        let scope = identity.scope_bytes();
        // Admission uses current authority before looking up object existence.
        let epoch: i64 = sql(self
            .db
            .query_row("SELECT epoch FROM runtime", [], |r| r.get(0)))?;
        if identity.scope.realm != self.installation.realm
            || sql(gates::authorized_snapshot(
                &self.db,
                &scope,
                &identity.principal,
                &query.memory,
                epoch,
                trusted_now()?,
            ))?
            .is_none()
        {
            return Err(Error::AuthorizationDenied);
        }
        let current: Id = sql(self
            .db
            .query_row(
                "SELECT current_revision FROM memory WHERE scope=?1 AND id=?2 AND state='active'",
                params![scope, query.memory],
                |r| r.get(0),
            )
            .optional())?
        .ok_or(Error::ResourceUnavailable)?;
        let current_valid: bool = sql(self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM revision v WHERE v.scope=?1 AND v.memory=?2
             AND v.id=?3 AND v.number=(SELECT max(number) FROM revision WHERE scope=?1 AND memory=?2))",
            params![scope,query.memory,current], |r|r.get(0),
        ))?;
        if !current_valid {
            return Err(Error::Integrity);
        }
        let selected = query.revision.unwrap_or(current);
        let (writer, request): (Id, Id) = sql(self.db.query_row(
            "SELECT i.principal,i.request FROM revision v JOIN payload p
              ON p.scope=v.scope AND p.memory=v.memory AND p.revision=v.id AND p.id=v.payload
             JOIN write_intent i ON i.operation=p.operation JOIN receipt r ON r.operation=i.operation
             JOIN local_key k ON k.scope=p.scope AND k.payload=p.id AND k.id=p.key_id AND k.generation=p.generation
             JOIN revision_evidence re ON re.scope=v.scope AND re.memory=v.memory AND re.revision=v.id
             WHERE v.scope=?1 AND v.memory=?2 AND v.id=?3 AND i.state='Committed'
              AND k.revoked=0 AND i.revision=v.id AND i.memory=v.memory
              AND re.evidence=i.evidence AND re.generation=i.evidence_generation
              AND r.revision=v.id AND r.memory=v.memory
              AND ((v.number=1 AND i.expected_current IS NULL) OR EXISTS(
                SELECT 1 FROM revision parent WHERE parent.scope=v.scope AND parent.memory=v.memory
                  AND parent.id=i.expected_current AND parent.number=v.number-1))",
            params![scope,query.memory,selected], |r| Ok((r.get(0)?,r.get(1)?)),
        ).optional())?.ok_or(Error::Integrity)?;
        let (plan, phase) = Self::plan_row(
            &self.db,
            Identity {
                principal: writer,
                ..identity
            },
            request,
        )?
        .ok_or(Error::Integrity)?;
        if phase != "Committed"
            || plan.request.revision != selected
            || plan.request.memory != query.memory
        {
            return Err(Error::Integrity);
        }
        let authorized = Self::authorize(
            &self.db,
            identity,
            plan.request,
            self.installation,
            self.incarnation,
            0,
        )?;
        if authorized != epoch
            || plan.expected_dispatch(&self.db, self.installation)? != plan.dispatch
        {
            return Err(Error::AuthorizationDenied);
        }
        Ok((plan, epoch, current))
    }

    pub(super) fn read_borrow<T>(
        &mut self,
        identity: Identity,
        query: ReadQuery,
        mut checkpoint: impl FnMut(ReadPoint) -> Result<()>,
        inspect: impl FnOnce(&str) -> T,
    ) -> Result<T> {
        self.b3_guard_read()?;
        // A consistent initial view is released before all file and DPAPI I/O.
        let initial = sql(rusqlite::Transaction::new_unchecked(
            &self.db,
            TransactionBehavior::Deferred,
        ))?;
        let snapshot = self.read_snapshot(identity, query);
        let end = sql(initial.rollback());
        let (plan, epoch, current) = snapshot?;
        end?;
        let (plain, publication) =
            self.verify_files_with(&plan, &mut checkpoint)
                .map_err(|error| {
                    if error == Error::Constraint {
                        Error::Integrity
                    } else {
                        error
                    }
                })?;
        self.b3_guard_read()?;
        // This reservation orders the synchronous in-process borrow before later security commits.
        // No file/DPAPI/output I/O occurs during this section.
        let final_guard = sql(rusqlite::Transaction::new_unchecked(
            &self.db,
            TransactionBehavior::Immediate,
        ))?;
        let result = (|| {
            let (confirmed, latest, pointer) = self.read_snapshot(identity, query)?;
            if latest != epoch || pointer != current || confirmed.operation != plan.operation {
                return Err(Error::AuthorizationDenied);
            }
            let stored: bool = sql(self.db.query_row(
                "SELECT EXISTS(SELECT 1 FROM publication WHERE operation=?1
                 AND cipher_hash=?2 AND wrapper_hash=?3 AND cipher_identity=?4 AND wrapper_identity=?5)",
                params![plan.operation,publication.cipher_hash,publication.wrapper_hash,
                    publication.cipher_identity,publication.wrapper_identity], |r|r.get(0),
            ))?;
            if !stored {
                return Err(Error::Integrity);
            }
            let text = std::str::from_utf8(plain.bytes()).map_err(|_| Error::Integrity)?;
            Ok(inspect(text))
        })();
        let end = sql(final_guard.rollback());
        drop(plain);
        // If the borrow already happened, cleanup failure cannot revoke those delivered bytes.
        // Block this owner before permitting any subsequent operation.
        if end.is_err() {
            let _ = self.db.execute("UPDATE runtime SET blocked=1", []);
            return Err(Error::CapabilityUnavailable);
        }
        result
    }
}

impl application::ContentRead for Application {
    type Query = ReadQuery;
    type Error = Error;
    fn borrow_utf8<T>(&mut self, query: ReadQuery, inspect: impl FnOnce(&str) -> T) -> Result<T> {
        self.engine
            .read_borrow(self.identity, query, |_| Ok(()), inspect)
    }
}

impl Application {
    pub(super) fn read_revision<T>(
        &mut self,
        query: ReadQuery,
        inspect: impl FnOnce(&str) -> T,
    ) -> Result<T> {
        application::InternalContentApplication::new(self).read_revision(query, inspect)
    }
    pub(super) fn revise_memory(
        &mut self,
        request: WriteRequest,
        body: &str,
    ) -> Result<WriteReceipt> {
        if request.expected_current.is_none() {
            return Err(Error::InvalidInput);
        }
        application::InternalContentApplication::new(self).revise_memory(request, body)
    }
}

#[cfg(test)]
pub(super) mod tests;
