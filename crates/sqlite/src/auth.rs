//! Private real SQLite authorization adapter. Compiled ONLY by Rust's test harness.
//! No feature, public factory, network credential issuer or production ingress exists.
use crate::storage::{APPLICATION_ID, MIGRATION, MIGRATION_RECORD};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
use yixu_application::metadata::DirectMetadataPolicy;
use yixu_domain::{Error, Id, Result, Scope};
use yixu_ports::{
    Clock,
    authorized_metadata::{AuthorizationState, AuthorizedMetadata, MarkerWrite, MetadataReceipt},
};
const AUTH_DDL: &str = include_str!("../migrations/0002_auth_profile.sql");
const VERSION: i64 = 2;
#[derive(Clone)]
struct AuthContext {
    realm: Id,
    subject: Id,
    principal: Id,
    scope: Scope,
    tenant_epoch: i64,
    space_epoch: i64,
    issued_at_ms: i64,
}
struct BootstrapAuthority {
    realm: Id,
}
// This is a trusted internal mapping key, NOT a request principal/tenant/admin claim.
struct TrustedIdentity {
    subject: Id,
}
struct AuthDatabase<C: Clock> {
    connection: Connection,
    clock: C,
    before_commit: Option<Box<dyn FnOnce() + Send>>,
}
fn sql_error(e: rusqlite::Error) -> Error {
    match e {
        rusqlite::Error::SqliteFailure(e, _) => match e.code {
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => Error::Busy,
            rusqlite::ErrorCode::ConstraintViolation => Error::Constraint,
            _ => Error::Database,
        },
        _ => Error::Database,
    }
}
fn natural(n: i64) -> Result<u64> {
    u64::try_from(n).map_err(|_| Error::Constraint)
}
fn checksum() -> Vec<u8> {
    Sha256::digest(format!("{MIGRATION}\n{AUTH_DDL}").as_bytes()).to_vec()
}
fn checksum_text() -> String {
    checksum().iter().map(|b| format!("{b:02x}")).collect()
}
fn time<C: Clock>(clock: &C) -> Result<i64> {
    let value = clock.utc_millis().map_err(|_| Error::AuthorizationDenied)?;
    if value < 0 {
        return Err(Error::AuthorizationDenied);
    }
    Ok(value)
}
fn realm(c: &Connection) -> Result<Id> {
    let value: String = c
        .query_row(
            "SELECT realm_id FROM auth_instance WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(sql_error)?
        .ok_or(Error::AuthorizationDenied)?;
    value.parse()
}
impl<C: Clock> AuthDatabase<C> {
    fn open(path: &Path, clock: C) -> Result<Self> {
        let mut c = Connection::open(path).map_err(sql_error)?;
        c.busy_timeout(Duration::from_secs(5)).map_err(sql_error)?;
        let version: i64 = c
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(sql_error)?;
        let app: i64 = c
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(sql_error)?;
        let count: i64 = c
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        if !((version == 0 && app == 0 && count == 0)
            || (version == VERSION && app == APPLICATION_ID))
        {
            return Err(Error::IncompatibleSchema);
        }
        c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA trusted_schema=OFF; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA temp_store=MEMORY;").map_err(sql_error)?;
        let fk: i64 = c
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .map_err(sql_error)?;
        let journal: String = c
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .map_err(sql_error)?;
        if fk != 1 || journal != "wal" {
            return Err(Error::Database);
        }
        let tx = c
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        if version == 0 {
            tx.execute_batch(MIGRATION_RECORD).map_err(sql_error)?;
            tx.execute_batch(MIGRATION).map_err(sql_error)?;
            tx.execute_batch(AUTH_DDL).map_err(sql_error)?;
            tx.execute(
                "INSERT INTO migration_record VALUES(?1,?2,'applied')",
                params![VERSION, checksum_text()],
            )
            .map_err(sql_error)?;
            tx.pragma_update(None, "user_version", VERSION)
                .map_err(sql_error)?;
            tx.pragma_update(None, "application_id", APPLICATION_ID)
                .map_err(sql_error)?;
        }
        let history: Vec<(i64, String, String)> = {
            let mut q = tx
                .prepare("SELECT version,checksum,phase FROM migration_record ORDER BY version")
                .map_err(sql_error)?;
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
                .map_err(sql_error)?
                .collect::<std::result::Result<_, _>>()
                .map_err(sql_error)?
        };
        if history.len() != 1 || history[0].0 != VERSION || history[0].2 != "applied" {
            return Err(Error::IncompatibleSchema);
        }
        if history[0].1 != checksum_text() {
            return Err(Error::MigrationChecksum);
        }
        crate::schema::validate_definitions(&tx, MIGRATION, AUTH_DDL)?;
        let integrity: String = tx
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(sql_error)?;
        let violations: i64 = tx
            .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })
            .map_err(sql_error)?;
        if integrity != "ok" || violations != 0 {
            return Err(Error::Constraint);
        }
        tx.commit().map_err(sql_error)?;
        Ok(Self {
            connection: c,
            clock,
            before_commit: None,
        })
    }
    // Only protected internal test assembly can invoke these constructors.
    fn bootstrap(&mut self, id: Id) -> Result<BootstrapAuthority> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let count: i64 = tx
            .query_row(
                "SELECT (SELECT count(*) FROM auth_instance)+(SELECT count(*) FROM tenant)",
                [],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        if count != 0 {
            return Err(Error::AuthorizationDenied);
        }
        tx.execute("INSERT INTO auth_instance VALUES(1,?1)", [id.to_string()])
            .map_err(sql_error)?;
        tx.commit().map_err(sql_error)?;
        Ok(BootstrapAuthority { realm: id })
    }
    fn administer(&mut self, authority: &BootstrapAuthority, change: AdminChange) -> Result<()> {
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        if realm(&tx)? != authority.realm {
            return Err(Error::AuthorizationDenied);
        }
        let recorded_at = time(&self.clock)?;
        let action = match &change {
            AdminChange::Tenant { .. } => "tenant.bootstrap",
            AdminChange::CreateSpace { .. } => "space.create",
            AdminChange::Principal { .. } => "principal.state",
            AdminChange::TenantState { .. } => "tenant.state",
            AdminChange::Space { .. } => "space.state",
            AdminChange::Grant { .. } => "grant.change",
            AdminChange::Binding { .. } => "identity.state",
        };
        let (tenant, space) = match change {
            AdminChange::Tenant {
                tenant,
                principal,
                subject,
                space,
            } => {
                tx.execute(
                    "INSERT INTO tenant VALUES(?1,'active',1,0)",
                    [tenant.to_string()],
                )
                .map_err(sql_error)?;
                tx.execute(
                    "INSERT INTO principal VALUES(?1,?2,'user','active')",
                    params![tenant.to_string(), principal.to_string()],
                )
                .map_err(sql_error)?;
                tx.execute(
                    "INSERT INTO identity_binding VALUES(?1,?2,?3,1)",
                    params![
                        subject.to_string(),
                        tenant.to_string(),
                        principal.to_string()
                    ],
                )
                .map_err(sql_error)?;
                tx.execute(
                    "INSERT INTO space VALUES(?1,?2,'personal',?3,NULL,1,0,'sensitive')",
                    params![tenant.to_string(), space.to_string(), principal.to_string()],
                )
                .map_err(sql_error)?;
                tx.execute(
                    "INSERT INTO auth_space VALUES(?1,?2,1)",
                    params![tenant.to_string(), space.to_string()],
                )
                .map_err(sql_error)?;
                (tenant, Some(space))
            }
            AdminChange::CreateSpace { scope, owner } => {
                tx.execute(
                    "INSERT INTO space VALUES(?1,?2,'personal',?3,NULL,1,0,'sensitive')",
                    params![
                        scope.tenant.to_string(),
                        scope.space.to_string(),
                        owner.to_string()
                    ],
                )
                .map_err(sql_error)?;
                tx.execute(
                    "INSERT INTO auth_space VALUES(?1,?2,1)",
                    params![scope.tenant.to_string(), scope.space.to_string()],
                )
                .map_err(sql_error)?;
                (scope.tenant, Some(scope.space))
            }
            AdminChange::Principal {
                tenant,
                principal,
                enabled,
            } => {
                let changed = tx
                    .execute(
                        "UPDATE principal SET status=?3 WHERE tenant_id=?1 AND id=?2",
                        params![
                            tenant.to_string(),
                            principal.to_string(),
                            if enabled { "active" } else { "disabled" }
                        ],
                    )
                    .map_err(sql_error)?;
                if changed != 1 {
                    return Err(Error::AuthorizationDenied);
                }
                (tenant, None)
            }
            AdminChange::TenantState { tenant, enabled } => {
                let changed = tx
                    .execute(
                        "UPDATE tenant SET status=?2 WHERE id=?1",
                        params![
                            tenant.to_string(),
                            if enabled { "active" } else { "disabled" }
                        ],
                    )
                    .map_err(sql_error)?;
                if changed != 1 {
                    return Err(Error::AuthorizationDenied);
                }
                (tenant, None)
            }
            AdminChange::Space { scope, enabled } => {
                let changed = tx
                    .execute(
                        "UPDATE auth_space SET enabled=?3 WHERE tenant_id=?1 AND space_id=?2",
                        params![scope.tenant.to_string(), scope.space.to_string(), enabled],
                    )
                    .map_err(sql_error)?;
                if changed != 1 {
                    return Err(Error::AuthorizationDenied);
                }
                (scope.tenant, Some(scope.space))
            }
            AdminChange::Grant {
                scope,
                principal,
                enabled,
                expires_at_ms,
            } => {
                tx.execute("INSERT INTO direct_grant VALUES(?1,?2,?3,'metadata.append',?4,?5) ON CONFLICT(tenant_id,space_id,principal_id,capability) DO UPDATE SET enabled=excluded.enabled,expires_at_ms=excluded.expires_at_ms",params![scope.tenant.to_string(),scope.space.to_string(),principal.to_string(),enabled,expires_at_ms]).map_err(sql_error)?;
                (scope.tenant, Some(scope.space))
            }
            AdminChange::Binding { subject, enabled } => {
                let tenant: String = tx
                    .query_row(
                        "SELECT tenant_id FROM identity_binding WHERE subject_id=?1",
                        [subject.to_string()],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(sql_error)?
                    .ok_or(Error::AuthorizationDenied)?;
                tx.execute(
                    "UPDATE identity_binding SET enabled=?2 WHERE subject_id=?1",
                    params![subject.to_string(), enabled],
                )
                .map_err(sql_error)?;
                (tenant.parse()?, None)
            }
        };
        let changed=tx.execute("UPDATE tenant SET auth_epoch=auth_epoch+1 WHERE id=?1 AND auth_epoch<9223372036854775807",[tenant.to_string()]).map_err(sql_error)?;
        if changed != 1 {
            return Err(Error::Constraint);
        }
        if let Some(space) = space {
            let changed=tx.execute("UPDATE space SET policy_epoch=policy_epoch+1 WHERE tenant_id=?1 AND id=?2 AND policy_epoch<9223372036854775807",params![tenant.to_string(),space.to_string()]).map_err(sql_error)?;
            if changed != 1 {
                return Err(Error::Constraint);
            }
        }
        let epoch: i64 = tx
            .query_row(
                "SELECT auth_epoch FROM tenant WHERE id=?1",
                [tenant.to_string()],
                |r| r.get(0),
            )
            .map_err(sql_error)?;
        tx.execute(
            "INSERT INTO auth_audit VALUES(?1,?2,?3,?4)",
            params![tenant.to_string(), epoch, action, recorded_at],
        )
        .map_err(sql_error)?;
        if let Some(hook) = self.before_commit.take() {
            hook();
        }
        tx.commit().map_err(sql_error)
    }
    fn identify(&self, identity: TrustedIdentity, space: Id) -> Result<AuthContext> {
        type Mapping = (String, String, String, i64, i64);
        let mapping:Option<Mapping>=self.connection.query_row("SELECT a.realm_id,b.tenant_id,b.principal_id,t.auth_epoch,s.policy_epoch FROM auth_instance a JOIN identity_binding b ON b.subject_id=?1 JOIN tenant t ON t.id=b.tenant_id JOIN principal p ON p.tenant_id=t.id AND p.id=b.principal_id JOIN space s ON s.tenant_id=t.id AND s.id=?2 JOIN auth_space z ON z.tenant_id=t.id AND z.space_id=s.id WHERE a.singleton=1 AND b.enabled=1 AND t.status='active' AND p.status='active' AND z.enabled=1",params![identity.subject.to_string(),space.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(sql_error)?;
        let (realm, tenant, principal, tenant_epoch, space_epoch) =
            mapping.ok_or(Error::AuthorizationDenied)?;
        Ok(AuthContext {
            realm: realm.parse()?,
            subject: identity.subject,
            principal: principal.parse()?,
            scope: Scope {
                tenant: tenant.parse()?,
                space,
            },
            tenant_epoch,
            space_epoch,
            issued_at_ms: time(&self.clock)?,
        })
    }
}
enum AdminChange {
    Tenant {
        tenant: Id,
        principal: Id,
        subject: Id,
        space: Id,
    },
    Principal {
        tenant: Id,
        principal: Id,
        enabled: bool,
    },
    CreateSpace {
        scope: Scope,
        owner: Id,
    },
    TenantState {
        tenant: Id,
        enabled: bool,
    },
    Space {
        scope: Scope,
        enabled: bool,
    },
    Grant {
        scope: Scope,
        principal: Id,
        enabled: bool,
        expires_at_ms: i64,
    },
    Binding {
        subject: Id,
        enabled: bool,
    },
}
fn authorization(
    tx: &Transaction<'_>,
    ctx: &AuthContext,
    cmd: &MarkerWrite,
    now: i64,
) -> Result<()> {
    if ctx.scope != cmd.scope || realm(tx)? != ctx.realm {
        return Err(Error::AuthorizationDenied);
    }
    type Live = (bool, bool, bool, bool, i64, i64, Option<bool>, Option<i64>);
    let live:Option<Live>=tx.query_row("SELECT t.status='active',p.status='active',z.enabled,b.enabled,t.auth_epoch,s.policy_epoch,g.enabled,g.expires_at_ms FROM tenant t JOIN principal p ON p.tenant_id=t.id AND p.id=?3 JOIN space s ON s.tenant_id=t.id AND s.id=?2 JOIN auth_space z ON z.tenant_id=t.id AND z.space_id=s.id JOIN identity_binding b ON b.subject_id=?4 AND b.tenant_id=t.id AND b.principal_id=p.id LEFT JOIN direct_grant g ON g.tenant_id=t.id AND g.space_id=s.id AND g.principal_id=p.id AND g.capability='metadata.append' WHERE t.id=?1",params![ctx.scope.tenant.to_string(),ctx.scope.space.to_string(),ctx.principal.to_string(),ctx.subject.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?))).optional().map_err(sql_error)?;
    let (
        tenant_active,
        principal_active,
        space_active,
        binding_active,
        live_tenant_epoch,
        live_space_epoch,
        grant_enabled,
        expires,
    ) = live.ok_or(Error::AuthorizationDenied)?;
    DirectMetadataPolicy::check(
        &AuthorizationState {
            realm_matches: true,
            scope_matches: true,
            tenant_active,
            principal_active,
            space_active,
            binding_active,
            context_tenant_epoch: ctx.tenant_epoch,
            live_tenant_epoch,
            context_space_epoch: ctx.space_epoch,
            live_space_epoch,
            grant_enabled: grant_enabled.unwrap_or(false),
            grant_expires_at_ms: expires.unwrap_or(0),
            identity_issued_at_ms: ctx.issued_at_ms,
        },
        cmd.capability,
        now,
    )
}
fn request_digest(cmd: MarkerWrite) -> Vec<u8> {
    Sha256::digest(
        format!(
            "metadata.append/v1|{}|{}|{}|{}|{}|{}",
            cmd.scope.tenant,
            cmd.scope.space,
            cmd.marker,
            cmd.event,
            cmd.expected_version,
            cmd.idempotency
        )
        .as_bytes(),
    )
    .to_vec()
}
impl<C: Clock> AuthorizedMetadata for AuthDatabase<C> {
    type Context = AuthContext;
    fn apply(
        &mut self,
        context: Option<&AuthContext>,
        cmd: MarkerWrite,
    ) -> Result<MetadataReceipt> {
        let ctx = context.ok_or(Error::AuthorizationDenied)?;
        let tx = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let start = time(&self.clock)?;
        authorization(&tx, ctx, &cmd, start)?;
        let digest = request_digest(cmd);
        type Replay = (Vec<u8>, i64, i64, String);
        let replay:Option<Replay>=tx.query_row("SELECT request_digest,version,knowledge_seq,event_id FROM metadata_retry WHERE tenant_id=?1 AND principal_id=?2 AND key_id=?3",params![ctx.scope.tenant.to_string(),ctx.principal.to_string(),cmd.idempotency.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(sql_error)?;
        let receipt = if let Some((old_digest, version, knowledge_seq, event)) = replay {
            if digest != old_digest {
                return Err(Error::IdempotencyConflict);
            }
            MetadataReceipt {
                version: natural(version)?,
                knowledge_seq: natural(knowledge_seq)?,
                event: event.parse()?,
                replayed: true,
            }
        } else {
            let current: Option<(String, i64)> = tx
                .query_row(
                    "SELECT space_id,version FROM metadata_marker WHERE tenant_id=?1 AND id=?2",
                    params![ctx.scope.tenant.to_string(), cmd.marker.to_string()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(sql_error)?;
            if current
                .as_ref()
                .is_some_and(|(space, _)| space != &ctx.scope.space.to_string())
            {
                return Err(Error::AuthorizationDenied);
            }
            let version = current.map_or(0, |(_, v)| v);
            if version != i64::try_from(cmd.expected_version).map_err(|_| Error::InvalidInput)? {
                return Err(Error::CasConflict);
            }
            let version = version.checked_add(1).ok_or(Error::Constraint)?;
            let changed=tx.execute("UPDATE tenant SET knowledge_seq=knowledge_seq+1 WHERE id=?1 AND knowledge_seq<9223372036854775807",[ctx.scope.tenant.to_string()]).map_err(sql_error)?;
            if changed != 1 {
                return Err(Error::Constraint);
            }
            let seq: i64 = tx
                .query_row(
                    "SELECT knowledge_seq FROM tenant WHERE id=?1",
                    [ctx.scope.tenant.to_string()],
                    |r| r.get(0),
                )
                .map_err(sql_error)?;
            tx.execute(
                "INSERT INTO commit_record VALUES(?1,?2,?3)",
                params![ctx.scope.tenant.to_string(), seq, start],
            )
            .map_err(sql_error)?;
            tx.execute(
                "INSERT INTO event VALUES(?1,?2,?3,'system_action',NULL,?4,?5,?5,?6,'1.0')",
                params![
                    ctx.scope.tenant.to_string(),
                    ctx.scope.space.to_string(),
                    cmd.event.to_string(),
                    ctx.principal.to_string(),
                    start,
                    seq
                ],
            )
            .map_err(sql_error)?;
            if version == 1 {
                tx.execute(
                    "INSERT INTO metadata_marker VALUES(?1,?2,?3,1,?4)",
                    params![
                        ctx.scope.tenant.to_string(),
                        ctx.scope.space.to_string(),
                        cmd.marker.to_string(),
                        cmd.event.to_string()
                    ],
                )
                .map_err(sql_error)?;
            } else {
                let changed=tx.execute("UPDATE metadata_marker SET version=?3,current_event_id=?4 WHERE tenant_id=?1 AND id=?2 AND space_id=?5 AND version=?6",params![ctx.scope.tenant.to_string(),cmd.marker.to_string(),version,cmd.event.to_string(),ctx.scope.space.to_string(),i64::try_from(cmd.expected_version).map_err(|_|Error::InvalidInput)?]).map_err(sql_error)?;
                if changed != 1 {
                    return Err(Error::CasConflict);
                }
            }
            tx.execute(
                "INSERT INTO metadata_retry VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    ctx.scope.tenant.to_string(),
                    ctx.scope.space.to_string(),
                    ctx.principal.to_string(),
                    cmd.idempotency.to_string(),
                    digest,
                    version,
                    seq,
                    cmd.event.to_string()
                ],
            )
            .map_err(sql_error)?;
            tx.execute(
                "INSERT INTO metadata_audit VALUES(?1,?2,?3,'metadata.append')",
                params![
                    ctx.scope.tenant.to_string(),
                    ctx.scope.space.to_string(),
                    cmd.event.to_string()
                ],
            )
            .map_err(sql_error)?;
            tx.execute(
                "INSERT INTO metadata_outbox VALUES(?1,?2,?3,?4,'pending')",
                params![
                    ctx.scope.tenant.to_string(),
                    ctx.scope.space.to_string(),
                    cmd.event.to_string(),
                    seq
                ],
            )
            .map_err(sql_error)?;
            MetadataReceipt {
                version: natural(version)?,
                knowledge_seq: natural(seq)?,
                event: cmd.event,
                replayed: false,
            }
        };
        if let Some(hook) = self.before_commit.take() {
            hook();
        }
        let final_time = time(&self.clock)?;
        if final_time < start {
            return Err(Error::AuthorizationDenied);
        }
        authorization(&tx, ctx, &cmd, final_time)?;
        tx.commit().map_err(sql_error)?;
        Ok(receipt)
    }
}
#[cfg(test)]
mod tests;
