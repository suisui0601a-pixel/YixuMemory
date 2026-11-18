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
}
