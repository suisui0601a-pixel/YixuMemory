//! Every concrete implementation is trusted composition, never a request-injected backend.
use yixu_domain::{Error, Result};
use yixu_ports::{memory::MemoryReceipt, memory_m2::*};
pub struct MemoryM2Application<S: MemoryM2> {
    store: S,
}
impl<S: MemoryM2> MemoryM2Application<S> {
    pub fn assemble(store: S) -> Self {
        Self { store }
    }
    pub fn write(&mut self, ctx: Option<&S::Context>, c: TemporalWrite) -> Result<MemoryReceipt> {
        self.store.write_temporal(ctx, c)
    }
    pub fn query(&mut self, ctx: Option<&S::Context>, q: MetadataQuery) -> Result<MetadataPage> {
        self.store.query_metadata(ctx, q)
    }
    pub fn conflict(
        &mut self,
        ctx: Option<&S::Context>,
        c: ConflictWrite,
    ) -> Result<ConflictReceipt> {
        self.store.write_conflict(ctx, c)
    }
    pub fn conflict_history(
        &mut self,
        ctx: Option<&S::Context>,
        q: ConflictQuery,
    ) -> Result<ConflictHistory> {
        self.store.conflict_history(ctx, q)
    }
}
impl MemoryM2 for super::NoContentRuntime {
    type Context = yixu_ports::AuthContext;
    fn write_temporal(
        &mut self,
        _: Option<&Self::Context>,
        _: TemporalWrite,
    ) -> Result<MemoryReceipt> {
        Err(Error::CapabilityUnavailable)
    }
    fn query_metadata(
        &mut self,
        _: Option<&Self::Context>,
        _: MetadataQuery,
    ) -> Result<MetadataPage> {
        Err(Error::CapabilityUnavailable)
    }
    fn write_conflict(
        &mut self,
        _: Option<&Self::Context>,
        _: ConflictWrite,
    ) -> Result<ConflictReceipt> {
        Err(Error::CapabilityUnavailable)
    }
    fn conflict_history(
        &mut self,
        _: Option<&Self::Context>,
        _: ConflictQuery,
    ) -> Result<ConflictHistory> {
        Err(Error::CapabilityUnavailable)
    }
}

pub struct UnavailableRetrievalIndex;
impl RetrievalIndex for UnavailableRetrievalIndex {
    fn candidates(&self, _: IndexRead) -> Result<IndexBatch> {
        Err(Error::CapabilityUnavailable)
    }
}
