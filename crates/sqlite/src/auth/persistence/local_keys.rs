//! Private transaction-bound Local key issuance. External DPAPI/file operations are outside tx.
use super::*;
impl<C: Clock> AuthDatabase<C> {
    fn key_guard(
        tx: &Transaction<'_>,
        ctx: &AuthContext,
        b: &PayloadBinding,
        cap: &str,
        now: i64,
    ) -> Result<()> {
        Self::content_check(tx, ctx, now, cap)?;
        if b.scope
            != (SecurityScope {
                realm: raw(ctx.realm),
                tenant: raw(ctx.scope.tenant),
                space: raw(ctx.scope.space),
            })
            || b.seal_generation != 1
        {
            return Err(Error::AuthorizationDenied);
        }
        let ready: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM local_key_gate WHERE singleton=1 AND state='ready')",
                [],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM sealed_copy s JOIN content_intent i ON i.tenant_id=s.tenant_id AND i.principal_id=s.principal_id AND i.id=s.intent_id WHERE s.tenant_id=?1 AND s.space_id=?2 AND s.payload_id=?3 AND s.key_id=?4 AND s.owner_id=?5 AND s.owner_kind=?6 AND s.seal_generation=?7 AND s.attempted=1 AND i.state<>'blocked')",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),Id::from_bytes(b.payload).to_string(),Id::from_bytes(b.key).to_string(),Id::from_bytes(b.owner).to_string(),match b.owner_kind{OwnerKind::Event=>1,OwnerKind::Evidence=>2,OwnerKind::Revision=>3},b.seal_generation as i64],|r|r.get(0)).map_err(sql_error)?;
        if !ready || !valid {
            return Err(Error::CapabilityUnavailable);
        }
        Ok(())
    }
    pub(super) fn local_key(
        &mut self,
        ctx: &AuthContext,
        b: &PayloadBinding,
        v: &LocalVault,
        cap: &str,
        create: bool,
    ) -> Result<LockedBuffer> {
        if self.profile != 7 {
            return Err(Error::CapabilityUnavailable);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        Self::key_guard(&tx, ctx, b, cap, time(&self.clock)?)?;
        type Row = (Vec<u8>, Vec<u8>, bool);
        let old:Option<Row>=tx.query_row("SELECT binding,wrapped_hash,revoked FROM local_key_registry WHERE tenant_id=?1 AND space_id=?2 AND key_id=?3",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),Id::from_bytes(b.key).to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional().map_err(sql_error)?;
        if create {
            if old.is_some() {
                return Err(Error::CapabilityUnavailable);
            }
        } else {
            let Some((ref bind, _, false)) = old else {
                return Err(Error::CapabilityUnavailable);
            };
            if bind != &b.aad(32) {
                return Err(Error::AuthorizationDenied);
            }
        }
        tx.commit().map_err(sql_error)?;
        let (key, wrapped) = if create { v.create(b)? } else { v.load(b)? };
        // Post-unwrap transaction is the internal key-use authorization point. No key is exported.
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        Self::key_guard(&tx, ctx, b, cap, time(&self.clock)?)?;
        if create {
            tx.execute(
                "INSERT INTO local_key_registry VALUES(?1,?2,?3,?4,?5,?6,0)",
                params![
                    ctx.scope.tenant.to_string(),
                    ctx.scope.space.to_string(),
                    Id::from_bytes(b.payload).to_string(),
                    Id::from_bytes(b.key).to_string(),
                    b.aad(32).to_vec(),
                    wrapped
                ],
            )
            .map_err(sql_error)?;
        } else {
            let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM local_key_registry WHERE tenant_id=?1 AND space_id=?2 AND payload_id=?3 AND key_id=?4 AND binding=?5 AND wrapped_hash=?6 AND revoked=0)",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),Id::from_bytes(b.payload).to_string(),Id::from_bytes(b.key).to_string(),b.aad(32).to_vec(),wrapped],|r|r.get(0)).map_err(sql_error)?;
            if !valid {
                return Err(Error::CapabilityUnavailable);
            }
        }
        tx.commit().map_err(sql_error)?;
        Ok(key)
    }
    pub(super) fn open_backend(
        &mut self,
        ctx: &AuthContext,
        b: &PayloadBinding,
        bytes: &[u8],
        v: Option<&LocalVault>,
        cap: &str,
    ) -> Result<Vec<u8>> {
        if let Some(v) = v {
            let k = self.local_key(ctx, b, v, cap, false)?;
            open_using_key(b, bytes, &k)
        } else {
            if self.profile != 6 {
                return Err(Error::CapabilityUnavailable);
            }
            open(b, bytes)
        }
    }
    pub(super) fn check_local_keys(
        tx: &Transaction<'_>,
        ctx: &AuthContext,
        intent: Id,
    ) -> Result<()> {
        let n:i64=tx.query_row("SELECT count(*) FROM local_key_registry k JOIN sealed_copy s ON s.tenant_id=k.tenant_id AND s.space_id=k.space_id AND s.payload_id=k.payload_id AND s.key_id=k.key_id WHERE s.tenant_id=?1 AND s.space_id=?2 AND s.principal_id=?3 AND s.intent_id=?4 AND k.revoked=0 AND EXISTS(SELECT 1 FROM local_key_gate WHERE singleton=1 AND state='ready')",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),ctx.principal.to_string(),intent.to_string()],|r|r.get(0)).map_err(sql_error)?;
        if n != 4 {
            return Err(Error::CapabilityUnavailable);
        }
        Ok(())
    }
    pub(super) fn revoke_local(
        &mut self,
        a: &BootstrapAuthority,
        b: &PayloadBinding,
        v: &LocalVault,
    ) -> Result<()> {
        if self.profile != 7 || raw(a.realm) != b.scope.realm {
            return Err(Error::AuthorizationDenied);
        }
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        if realm(&tx)? != a.realm {
            return Err(Error::AuthorizationDenied);
        }
        let n=tx.execute("UPDATE local_key_registry SET revoked=1 WHERE tenant_id=?1 AND space_id=?2 AND key_id=?3 AND binding=?4",params![Id::from_bytes(b.scope.tenant).to_string(),Id::from_bytes(b.scope.space).to_string(),Id::from_bytes(b.key).to_string(),b.aad(32).to_vec()]).map_err(sql_error)?;
        if n != 1 {
            return Err(Error::CapabilityUnavailable);
        }
        tx.commit().map_err(sql_error)?;
        v.revoke_marker(b)
    }
}
