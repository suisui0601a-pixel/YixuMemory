//! M3 proposed versioned Ports. No production issuer or adapter is installed.
//! Old foundation ports remain unavailable; these do not upgrade old data/profile.
use yixu_domain::security_m3::{Digest, Identity, PayloadBinding, ProtocolResult, SecurityIntent};

/// Constructible only by a future audited transaction-bound authorization authority.
/// ```compile_fail
/// let ctx = yixu_ports::security_m3::ProtectionContext { binding: todo!() };
/// ```
pub struct ProtectionContext {
    binding: PayloadBinding,
}
impl ProtectionContext {
    pub fn binding(&self) -> &PayloadBinding {
        &self.binding
    }
}
/// Single-use, non-Clone permission; no public issuer, serde, Default, or raw constructor.
/// ```compile_fail
/// let permit = yixu_ports::security_m3::ReleasePermit { _private: () };
/// ```
pub struct ReleasePermit {
    _private: (),
}
pub struct KeyHandle {
    _private: (),
}
pub struct PreparedIntent {
    _private: (),
}
pub struct VerifiedCompletion {
    _private: (),
}
pub struct SealedPayload {
    _private: (),
}
pub struct LockedContent {
    _private: (),
}
pub struct CurrentWitness {
    _private: (),
}
pub trait KeyVaultV1 {
    fn allocate(
        &mut self,
        context: &ProtectionContext,
        idempotency: Identity,
    ) -> ProtocolResult<KeyHandle>;
    fn seal(
        &mut self,
        context: &ProtectionContext,
        key: &KeyHandle,
        content: LockedContent,
    ) -> ProtocolResult<SealedPayload>;
    fn open_for_release(
        &mut self,
        context: &ProtectionContext,
        sealed: &SealedPayload,
        permit: ReleasePermit,
    ) -> ProtocolResult<LockedContent>;
    fn revoke(
        &mut self,
        intent: &PreparedIntent,
        key: &KeyHandle,
    ) -> ProtocolResult<VerifiedCompletion>;
}
pub trait WitnessV1 {
    fn prepare(
        &mut self,
        context: &ProtectionContext,
        intent: &SecurityIntent,
        request_sequence: u64,
    ) -> ProtocolResult<PreparedIntent>;
    /// Includes a linearizable barrier invalidating all earlier in-flight requests.
    fn current_after_barrier(
        &mut self,
        realm: Identity,
        nonce: Digest,
        old_incarnation: u64,
    ) -> ProtocolResult<CurrentWitness>;
    fn finalize(
        &mut self,
        intent: PreparedIntent,
        receipts: &[VerifiedCompletion],
    ) -> ProtocolResult<CurrentWitness>;
}
pub trait PayloadStoreV1 {
    fn publish_no_replace(
        &mut self,
        context: &ProtectionContext,
        sealed: SealedPayload,
    ) -> ProtocolResult<Digest>;
    fn erase_fenced(
        &mut self,
        intent: &PreparedIntent,
        binding: &PayloadBinding,
        worker_fence: u64,
    ) -> ProtocolResult<VerifiedCompletion>;
}
pub trait ReleaseAuthorityV1 {
    /// Authorization, source/current status and current Witness are rechecked under the release gate.
    fn authorize(
        &mut self,
        context: &ProtectionContext,
        witness: &CurrentWitness,
    ) -> ProtocolResult<ReleasePermit>;
}
