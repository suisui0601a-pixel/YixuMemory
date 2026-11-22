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
    /// Product profile selection is not evidence of installed, accepted security backends.
    pub fn security_capabilities(
        &self,
        profile: yixu_domain::security_m3::Profile,
    ) -> SecurityCapabilities {
        SecurityCapabilities {
            profile,
            content: false,
            strong_erasure: false,
            restore: false,
        }
    }
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

pub struct SecurityCapabilities {
    profile: yixu_domain::security_m3::Profile,
    content: bool,
    strong_erasure: bool,
    restore: bool,
}
impl SecurityCapabilities {
    pub fn profile(&self) -> yixu_domain::security_m3::Profile {
        self.profile
    }
    pub fn content(&self) -> bool {
        self.content
    }
    pub fn strong_erasure(&self) -> bool {
        self.strong_erasure
    }
    pub fn restore(&self) -> bool {
        self.restore
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_name_cannot_upgrade_actual_assembly_capabilities() {
        for profile in [
            yixu_domain::security_m3::Profile::Local,
            yixu_domain::security_m3::Profile::Protected,
        ] {
            let caps = NoContentRuntime::default().security_capabilities(profile);
            assert_eq!(caps.profile(), profile);
            assert!(!caps.content());
            assert!(!caps.strong_erasure());
            assert!(!caps.restore());
        }
    }
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

pub mod memory;
pub mod memory_m2;
/// Direct authorization logic and the unified metadata facade.
pub mod metadata;
