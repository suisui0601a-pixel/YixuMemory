//! Metadata foundation only. No content, database, model or operating-system dependencies.
use std::{fmt, str::FromStr};
pub mod security_m3;
pub mod temporal;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Integrity,
    InvalidToken,
    InvalidCursor,
    UnsupportedQuery,
    StateConflict,
    InvalidInput,
    NotFound,
    Constraint,
    Busy,
    Database,
    CasConflict,
    MigrationChecksum,
    IncompatibleSchema,
    CapabilityUnavailable,
    UnsupportedScope,
    AuthorizationDenied,
    IdempotencyConflict,
    ResourceUnavailable,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never expose SQL diagnostics, parameters, paths, or request bodies.
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Id([u8; 16]);
impl Id {
    /// Caller supplies an opaque identity. Production random allocation is not implemented.
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }
}
impl FromStr for Id {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        if s.len() != 36 || [8, 13, 18, 23].iter().any(|&i| s.as_bytes()[i] != b'-') {
            return Err(Error::InvalidInput);
        }
        let mut out = [0; 16];
        let hex: Vec<_> = s.bytes().filter(|&b| b != b'-').collect();
        if hex.len() != 32 {
            return Err(Error::InvalidInput);
        }
        fn nibble(b: u8) -> Result<u8> {
            match b {
                b'0'..=b'9' => Ok(b - b'0'),
                b'a'..=b'f' => Ok(b - b'a' + 10),
                _ => Err(Error::InvalidInput),
            }
        }
        for (i, pair) in hex.as_chunks::<2>().0.iter().enumerate() {
            out[i] = nibble(pair[0])? * 16 + nibble(pair[1])?;
        }
        Ok(Self(out))
    }
}
impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if [4, 6, 8, 10].contains(&i) {
                write!(f, "-")?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Scope {
    pub tenant: Id,
    pub space: Id,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Classification {
    Synthetic,
    Personal,
    Sensitive,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryState {
    Active,
    Archived,
    SourceBlocked,
    DeletionPending,
    Purged,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PayloadState {
    Staged,
    Available,
    ErasurePending,
    Blocked,
    Erased,
}

/// These counters are not interchangeable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Generations {
    pub seal: u64,
    pub deletion: u64,
    pub lease: u64,
    pub vault_epoch: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteOperation {
    MemoryGraph,
    EvidenceGraph,
    SourceGraph,
    SourceAllCopies,
    SubjectAllCopies,
}
impl DeleteOperation {
    pub fn validate_space_scope(self) -> Result<()> {
        match self {
            Self::MemoryGraph | Self::EvidenceGraph | Self::SourceGraph => Ok(()),
            _ => Err(Error::UnsupportedScope),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EvidenceEnvelope {
    pub scope: Scope,
    pub id: Id,
    pub event: Id,
    pub payload: Id,
    pub locator: Id,
}
#[derive(Clone, Copy, Debug)]
pub struct RevisionEnvelope {
    pub scope: Scope,
    pub id: Id,
    pub memory: Id,
    pub number: u64,
    pub parent: Option<Id>,
    pub primary_evidence: Id,
    pub content_payload: Id,
    pub valid_from_ms: i64,
    pub valid_to_ms: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opaque_uuid_is_strict() {
        let v = "00000000-0000-4000-8000-000000000001";
        assert_eq!(v.parse::<Id>().unwrap().to_string(), v);
        for invalid in [
            "text",
            "00000000_0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-00000000000Z",
        ] {
            assert_eq!(invalid.parse::<Id>(), Err(Error::InvalidInput));
        }
    }
    #[test]
    fn broad_erasure_is_not_silently_narrowed() {
        assert_eq!(
            DeleteOperation::SourceAllCopies.validate_space_scope(),
            Err(Error::UnsupportedScope)
        );
        assert_eq!(
            DeleteOperation::SubjectAllCopies.validate_space_scope(),
            Err(Error::UnsupportedScope)
        );
        assert_eq!(DeleteOperation::MemoryGraph.validate_space_scope(), Ok(()));
    }
}
