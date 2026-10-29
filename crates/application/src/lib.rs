//! Production-facing no-content assembly. No configurable backend or plaintext fallback.
use yixu_domain::{Classification, Error, Id, Result, Scope};
use yixu_ports::{CopyReceipt, KeyHandle, KeyVault, OpenedContent, SealedMaterial};

pub struct UnavailableKeyVault;
impl KeyVault for UnavailableKeyVault {
    fn readiness(&self) -> Result<()> {
        Err(Error::CapabilityUnavailable)
    }
    fn allocate_or_lookup(&self, _: Scope, _: Id) -> Result<KeyHandle> {
        Err(Error::CapabilityUnavailable)
    }
    fn seal(&self, _: &KeyHandle, _: &[u8]) -> Result<SealedMaterial> {
        Err(Error::CapabilityUnavailable)
    }
    fn open(&self, _: Scope, _: Id) -> Result<OpenedContent> {
        Err(Error::CapabilityUnavailable)
    }
    fn revoke(&self, _: Scope, _: &KeyHandle, _: u64) -> Result<CopyReceipt> {
        Err(Error::CapabilityUnavailable)
    }
}

#[derive(Default)]
pub struct NoContentRuntime {
    _private: (),
}
impl NoContentRuntime {
    /// Formal metadata ingress is unavailable until a trusted issuer, live Grant/
    /// revocation Policy and transaction-bound authorization are implemented.
    /// No policy or laboratory adapter can be injected into this assembly.
    pub fn write_metadata(&self, _auth: Option<&yixu_ports::AuthContext>) -> Result<()> {
        Err(Error::CapabilityUnavailable)
    }
    /// Bootstrap is distinct from ordinary memory writing and is also unavailable.
    pub fn bootstrap(&self, _authority: &yixu_ports::BootstrapAuthority) -> Result<()> {
        Err(Error::CapabilityUnavailable)
    }
    pub const fn content_available(&self) -> bool {
        false
    }
    pub fn stage(&self, _scope: Scope, _declared: Classification, _body: &[u8]) -> Result<Id> {
        // No database, file, logger, test adapter or pluggable success path is reachable.
        Err(Error::CapabilityUnavailable)
    }
    pub fn hydrate(&self, _scope: Scope, _payload: Id) -> Result<OpenedContent> {
        Err(Error::CapabilityUnavailable)
    }
    pub fn erase(&self, _scope: Scope, _target: Id) -> Result<()> {
        Err(Error::CapabilityUnavailable)
    }
    pub fn restore(&self, _scope: Scope) -> Result<()> {
        Err(Error::CapabilityUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            tenant: Id::from_bytes([1; 16]),
            space: Id::from_bytes([2; 16]),
        }
    }
    #[test]
    fn every_classification_and_no_model_fail_closed() {
        let runtime = NoContentRuntime::default();
        assert!(!runtime.content_available());
        for classification in [
            Classification::Synthetic,
            Classification::Personal,
            Classification::Sensitive,
        ] {
            assert_eq!(
                runtime.stage(scope(), classification, b"synthetic-canary"),
                Err(Error::CapabilityUnavailable)
            );
        }
    }
    #[test]
    fn unavailable_keys_cannot_allocate_or_claim_erasure() {
        let vault = UnavailableKeyVault;
        assert_eq!(vault.readiness(), Err(Error::CapabilityUnavailable));
        assert!(matches!(
            vault.allocate_or_lookup(scope(), scope().space),
            Err(Error::CapabilityUnavailable)
        ));
        assert_eq!(
            NoContentRuntime::default().erase(scope(), scope().space),
            Err(Error::CapabilityUnavailable)
        );
        assert_eq!(
            NoContentRuntime::default().restore(scope()),
            Err(Error::CapabilityUnavailable)
        );
    }
}

#[cfg(test)]
mod ingress_tests {
    #[test]
    fn unauthenticated_metadata_ingress_is_unavailable() {
        assert_eq!(
            super::NoContentRuntime::default().write_metadata(None),
            Err(yixu_domain::Error::CapabilityUnavailable)
        );
    }
}
