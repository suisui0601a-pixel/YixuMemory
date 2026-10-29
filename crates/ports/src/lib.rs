//! Interfaces, not evidence that a cryptographic or security operation ran.
use yixu_domain::{Id, Result, Scope};

pub struct AuthContext {
    pub principal: Id,
    pub scope: Scope,
}
pub trait Policy {
    fn authorize(&self, ctx: &AuthContext, capability: Capability) -> Result<()>;
}
#[derive(Clone, Copy)]
pub enum Capability {
    ContentWrite,
    ContentRead,
    Erase,
    Recover,
}
pub trait Clock {
    fn utc_millis(&self) -> Result<i64>;
    fn monotonic_ticks(&self) -> u64;
}

/// No successful sealed material constructor is supplied by this foundation.
pub struct SealedMaterial {
    _private: (),
}
pub struct OpenedContent {
    _private: (),
}
pub struct KeyHandle {
    pub id: Id,
    pub seal_generation: u64,
}
pub struct CopyReceipt {
    pub copy: Id,
    pub deletion_generation: u64,
    pub closure_version: u64,
}
pub trait KeyVault {
    fn readiness(&self) -> Result<()>;
    fn allocate_or_lookup(&self, scope: Scope, intent: Id) -> Result<KeyHandle>;
    fn seal(&self, key: &KeyHandle, content: &[u8]) -> Result<SealedMaterial>;
    fn open(&self, scope: Scope, payload: Id) -> Result<OpenedContent>;
    fn revoke(
        &self,
        scope: Scope,
        key: &KeyHandle,
        deletion_generation: u64,
    ) -> Result<CopyReceipt>;
}
pub trait PayloadStore {
    fn publish_no_replace(&self, scope: Scope, payload: Id, sealed: &SealedMaterial) -> Result<()>;
    fn erase(
        &self,
        scope: Scope,
        payload: Id,
        seal_generation: u64,
        deletion_generation: u64,
    ) -> Result<CopyReceipt>;
}
pub trait SecurityWitness {
    fn require_fresh(&self, scope: Scope, vault_epoch: u64) -> Result<()>;
    fn prepare(&self, scope: Scope, intent: Id) -> Result<()>;
    fn finalize(&self, scope: Scope, intent: Id, knowledge_seq: u64) -> Result<()>;
}
pub trait MetadataTransaction {
    fn knowledge_seq(&self) -> u64;
    fn commit(self) -> Result<()>;
}
pub trait UnitOfWork {
    type Transaction<'a>: MetadataTransaction
    where
        Self: 'a;
    fn begin(&mut self, tenant: Id) -> Result<Self::Transaction<'_>>;
}
pub trait MemoryRepository {
    fn memory_version(&self, scope: Scope, memory: Id) -> Result<u64>;
}
pub trait Outbox {
    fn claim(&mut self, scope: Scope, job: Id, now_ms: i64) -> Result<u64>;
}
pub trait Index {
    fn rebuild(&self, scope: Scope, generation: u64) -> Result<()>;
}
pub trait DeletionCoordinator {
    fn resume(&self, scope: Scope, deletion: Id) -> Result<()>;
}
