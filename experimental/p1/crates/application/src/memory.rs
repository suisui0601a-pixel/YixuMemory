//! Shared facade without a Repository escape hatch or caller-supplied Policy.
use yixu_domain::{Error, Result};
use yixu_ports::memory::{AuthorizedMemory, MemoryReceipt, MemoryWrite};

pub struct MemoryApplication<S: AuthorizedMemory> {
    store: S,
}
impl<S: AuthorizedMemory> MemoryApplication<S> {
    pub fn assemble(store: S) -> Self {
        Self { store }
    }
    pub fn execute(
        &mut self,
        context: Option<&S::Context>,
        command: MemoryWrite,
    ) -> Result<MemoryReceipt> {
        self.store.apply_memory(context, command)
    }
}
impl AuthorizedMemory for super::NoContentRuntime {
    type Context = yixu_ports::AuthContext;
    fn apply_memory(&mut self, _: Option<&Self::Context>, _: MemoryWrite) -> Result<MemoryReceipt> {
        Err(Error::CapabilityUnavailable)
    }
}
