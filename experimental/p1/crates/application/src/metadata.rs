//! No database, network, model, OS identity or caller-injected authorization logic.
use yixu_domain::{Error, Result};
use yixu_ports::authorized_metadata::{
    AuthorizationState, AuthorizedMetadata, MarkerWrite, MetadataCapability, MetadataReceipt,
};

pub struct DirectMetadataPolicy;
impl DirectMetadataPolicy {
    /// Observations are not proof by themselves. Real adapters must collect them under their UoW.
    pub fn check(
        state: &AuthorizationState,
        capability: MetadataCapability,
        now_ms: i64,
    ) -> Result<()> {
        if capability != MetadataCapability::Append {
            return Err(Error::AuthorizationDenied);
        }
        LiveAuthorizationPolicy::check(state, now_ms)
    }
}
/// Shared live-state checks, AFTER the trusted adapter selects the exact operation's Grant.
/// These observations do not mint identity or authorize any operation by themselves.
pub struct LiveAuthorizationPolicy;
impl LiveAuthorizationPolicy {
    pub fn check(state: &AuthorizationState, now_ms: i64) -> Result<()> {
        if now_ms < 0
            || now_ms < state.identity_issued_at_ms
            || !state.realm_matches
            || !state.scope_matches
            || !state.tenant_active
            || !state.principal_active
            || !state.space_active
            || !state.binding_active
            || state.context_tenant_epoch <= 0
            || state.context_space_epoch <= 0
            || state.context_tenant_epoch != state.live_tenant_epoch
            || state.context_space_epoch != state.live_space_epoch
            || !state.grant_enabled
            || now_ms >= state.grant_expires_at_ms
        {
            return Err(Error::AuthorizationDenied);
        }
        Ok(())
    }
}
/// Public facade has no direct storage mutation API and accepts no Policy override.
pub struct MetadataApplication<S: AuthorizedMetadata> {
    store: S,
}
impl<S: AuthorizedMetadata> MetadataApplication<S> {
    /// Trusted composition root supplies infrastructure, not request parameters.
    pub fn assemble(store: S) -> Self {
        Self { store }
    }
    pub fn execute(
        &mut self,
        context: Option<&S::Context>,
        command: MarkerWrite,
    ) -> Result<MetadataReceipt> {
        self.store.apply(context, command)
    }
}
