//! Private trusted ID source; never a request argument or a public Cargo feature.
use yixu_domain::{Error, Id, Result};
pub(super) trait ResourceIds: Send {
    fn next(&mut self) -> Result<Id>;
}
pub(super) struct OsResourceIds;
impl ResourceIds for OsResourceIds {
    fn next(&mut self) -> Result<Id> {
        let mut bytes = [0; 16];
        // Use pinned OS entropy. On ANY failure discard the buffer and fail closed.
        getrandom::fill(&mut bytes).map_err(|_| Error::ResourceUnavailable)?;
        // RFC 9562 UUIDv4: version 0100, IETF variant 10. Remaining bits stay random.
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Ok(Id::from_bytes(bytes))
    }
}
