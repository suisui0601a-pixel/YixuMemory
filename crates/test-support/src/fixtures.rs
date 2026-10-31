//! Privileged synthetic fixtures. Only opaque identifiers are persisted; NO content is accepted.
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use yixu_domain::{Error, Id, Result, Scope};

fn opaque(scope: Scope, payload: Id, purpose: &str) -> Id {
    let digest = Sha256::digest(format!(
        "fixture-only/{}/{}/{payload}/{purpose}",
        scope.tenant, scope.space
    ));
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    Id::from_bytes(bytes)
}
pub fn blocked_payload(c: &Connection, scope: Scope, payload: Id, locator: bool) -> Result<()> {
    c.execute("INSERT INTO payload VALUES(?1,?2,?3,?4,NULL,'sensitive',1,?5,?6,'metadata-fixture-not-encrypted',?7,1,'blocked')",params![scope.tenant.to_string(),scope.space.to_string(),payload.to_string(),if locator {"locator"} else {"content"},opaque(scope, payload,"ref").to_string(),opaque(scope, payload,"key").to_string(),opaque(scope, payload,"seal").to_string()]).map_err(|_|Error::Constraint)?;
    Ok(())
}
pub struct SyntheticSource {
    pub event: Id,
    pub evidence: Id,
    pub body: Id,
    pub locator: Id,
}
impl SyntheticSource {
    /// Caller owns the fixture transaction and commit; this never disables an admission trigger.
    pub fn seed(
        &self,
        c: &Connection,
        scope: Scope,
        principal: Id,
        knowledge_seq: i64,
    ) -> Result<()> {
        blocked_payload(c, scope, self.body, false)?;
        blocked_payload(c, scope, self.locator, true)?;
        c.execute(
            "INSERT INTO event VALUES(?1,?2,?3,'system_action',NULL,?4,0,0,?5,'1.0')",
            params![
                scope.tenant.to_string(),
                scope.space.to_string(),
                self.event.to_string(),
                principal.to_string(),
                knowledge_seq
            ],
        )
        .map_err(|_| Error::Constraint)?;
        c.execute(
            "INSERT INTO evidence VALUES(?1,?2,?3,?4,?5,?6,'user_statement',0)",
            params![
                scope.tenant.to_string(),
                scope.space.to_string(),
                self.evidence.to_string(),
                self.event.to_string(),
                self.body.to_string(),
                self.locator.to_string()
            ],
        )
        .map_err(|_| Error::Constraint)?;
        Ok(())
    }
}
