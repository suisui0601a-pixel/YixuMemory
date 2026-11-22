//! Source-shared only by private Windows tests, absent from the production module tree.
pub(super) trait ContentWrite {
    type Request;
    type Receipt;
    type Error;
    fn persist_utf8(
        &mut self,
        request: Self::Request,
        body: &str,
    ) -> Result<Self::Receipt, Self::Error>;
}

/// The trusted caller can inspect a borrow only during the ordered handoff.
/// Implementors must not return a reference to their secret allocation.
pub(super) trait ContentRead {
    type Query;
    type Error;
    fn borrow_utf8<T>(
        &mut self,
        query: Self::Query,
        inspect: impl FnOnce(&str) -> T,
    ) -> Result<T, Self::Error>;
}
pub(super) struct InternalContentApplication<'a, W: ContentWrite> {
    writer: &'a mut W,
}
impl<'a, W: ContentWrite> InternalContentApplication<'a, W> {
    pub(super) fn new(writer: &'a mut W) -> Self {
        Self { writer }
    }
    pub(super) fn create_memory(
        &mut self,
        request: W::Request,
        body: &str,
    ) -> Result<W::Receipt, W::Error> {
        // &str fixes UTF-8 at ingress. The private adapter checks length and live authority.
        self.writer.persist_utf8(request, body)
    }

    pub(super) fn revise_memory(
        &mut self,
        request: W::Request,
        body: &str,
    ) -> Result<W::Receipt, W::Error> {
        self.writer.persist_utf8(request, body)
    }
}

impl<W: ContentWrite + ContentRead> InternalContentApplication<'_, W> {
    pub(super) fn read_revision<T>(
        &mut self,
        query: W::Query,
        inspect: impl FnOnce(&str) -> T,
    ) -> Result<T, <W as ContentRead>::Error> {
        self.writer.borrow_utf8(query, inspect)
    }
}
