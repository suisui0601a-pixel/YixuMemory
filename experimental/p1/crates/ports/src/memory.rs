//! Envelope orchestration contract. Payload references are not admission proofs.
use yixu_domain::{Id, Result, Scope};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryTarget {
    Create,
    Revise { memory: Id, expected_version: u64 },
}
/// Creation IDs, revision numbers and parent pointers are assigned by trusted storage.
/// Evidence is a SET (duplicates rejected); primary must belong to it.
/// No body, URI, locator, provenance or classification override is accepted here.
/// ```compile_fail
/// fn forge(c: &mut yixu_ports::memory::MemoryWrite) { c.revision_id = todo!(); }
/// ```
/// ```compile_fail
/// fn forge(c: &mut yixu_ports::memory::MemoryWrite) { c.memory_id = todo!(); }
/// ```
#[derive(Clone, Debug)]
pub struct MemoryWrite {
    pub scope: Scope,
    pub target: MemoryTarget,
    pub evidence: Vec<Id>,
    pub primary_evidence: Id,
    pub content_payload: Id,
    pub idempotency: Id,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryReceipt {
    pub memory: Id,
    pub revision: Id,
    pub version: u64,
    pub revision_number: u64,
    pub knowledge_seq: u64,
    pub replayed: bool,
}
impl MemoryTarget {
    pub const fn capability(self) -> &'static str {
        match self {
            Self::Create => "memory.append",
            Self::Revise { .. } => "memory.revise",
        }
    }
}
/// Trusted adapter MUST authorize the specific operation in its write transaction,
/// including replays. A payload UUID alone never permits production content admission.
pub trait AuthorizedMemory {
    type Context;
    fn apply_memory(
        &mut self,
        context: Option<&Self::Context>,
        command: MemoryWrite,
    ) -> Result<MemoryReceipt>;
}
