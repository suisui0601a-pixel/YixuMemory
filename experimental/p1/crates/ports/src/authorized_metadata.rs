//! Narrow atomic use case, not a publicly writable Repository or UoW.
use yixu_domain::{Id, Result, Scope};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataCapability {
    Append,
    GroupInherited,
}
/// Create carries NO resource ID. Existing is an authorized, Space-scoped reference,
/// not permission to assign a new globally unique marker or event ID.
#[derive(Clone, Copy, Debug)]
pub enum MarkerTarget {
    Create,
    Existing(Id),
}
/// Client resource allocation is not part of the request DTO.
/// ```compile_fail
/// use yixu_ports::authorized_metadata::MarkerWrite;
/// fn choose_id(command: &mut MarkerWrite) { command.marker = todo!(); }
/// ```
/// ```compile_fail
/// use yixu_ports::authorized_metadata::MarkerWrite;
/// fn choose_id(command: &mut MarkerWrite) { command.event = todo!(); }
/// ```
#[derive(Clone, Copy, Debug)]
pub struct MarkerWrite {
    pub scope: Scope,
    pub target: MarkerTarget,
    pub expected_version: u64,
    pub idempotency: Id,
    pub capability: MetadataCapability,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetadataReceipt {
    pub marker: Id,
    pub version: u64,
    pub knowledge_seq: u64,
    pub event: Id,
    pub replayed: bool,
}
/// Infrastructure implementors are trusted assembly, never caller-supplied policy plugins.
/// A fake implementation can affect its own result, not acquire a real adapter's opaque Context.
pub trait AuthorizedMetadata {
    type Context;
    /// Must reauthorize live state in the SAME write transaction on every call/retry.
    fn apply(
        &mut self,
        context: Option<&Self::Context>,
        command: MarkerWrite,
    ) -> Result<MetadataReceipt>;
}
/// Plain observations supplied by trusted storage, NOT an authentication capability.
pub struct AuthorizationState {
    pub realm_matches: bool,
    pub scope_matches: bool,
    pub tenant_active: bool,
    pub principal_active: bool,
    pub space_active: bool,
    pub binding_active: bool,
    pub context_tenant_epoch: i64,
    pub live_tenant_epoch: i64,
    pub context_space_epoch: i64,
    pub live_space_epoch: i64,
    pub grant_enabled: bool,
    pub grant_expires_at_ms: i64,
    pub identity_issued_at_ms: i64,
}
