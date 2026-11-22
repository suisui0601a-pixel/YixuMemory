//! Metadata ONLY. No body, payload/key/locator references or text query is returned or accepted.
use crate::memory::{MemoryReceipt, MemoryWrite};
use yixu_domain::{
    Id, MemoryState, Result, Scope,
    temporal::{TemporalMatch, Validity},
};
#[derive(Clone, Debug)]
pub struct TemporalWrite {
    pub write: MemoryWrite,
    pub validity: Validity,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnowledgeToken(pub Vec<u8>);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor(pub Vec<u8>);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryMode {
    One(Id),
    Current,
    History(Id),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retrieval {
    Metadata,
    FullText,
    Bm25,
    Graph,
    Vector,
    TemporalScore,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssertionKind {
    ExtractedFact,
    ModelInference,
    FictionalSetting,
    UserAssertion,
}
impl AssertionKind {
    pub const fn text(self) -> &'static str {
        match self {
            Self::ExtractedFact => "extracted_fact",
            Self::ModelInference => "model_inference",
            Self::FictionalSetting => "fictional_setting",
            Self::UserAssertion => "user_assertion",
        }
    }
}
#[derive(Clone, Debug)]
pub struct MetadataQuery {
    pub scope: Scope,
    pub mode: QueryMode,
    pub as_of: Option<KnowledgeToken>,
    pub valid_at: Option<i64>,
    pub include_uncertain: bool,
    pub evidence: Option<Id>,
    pub assertion_kind: Option<AssertionKind>,
    pub state: MemoryState,
    pub retrieval: Retrieval,
    pub limit: u16,
    pub cursor: Option<Cursor>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidenceMetadata {
    pub id: Id,
    pub event: Id,
    pub occurred_at_ms: i64,
    pub recorded_at_ms: i64,
    pub knowledge_seq: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionMetadata {
    pub memory: Id,
    pub revision: Id,
    pub number: u64,
    pub memory_version_at_revision: u64,
    pub knowledge_seq: u64,
    pub recorded_at_ms: i64,
    pub validity: Validity,
    pub temporal_match: Option<TemporalMatch>,
    pub assertion_kind: AssertionKind,
    pub primary_evidence: Id,
    pub evidence: Vec<EvidenceMetadata>,
    pub state: MemoryState,
    pub content_available: bool,
    pub conflicts: Vec<Id>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetadataPage {
    pub rows: Vec<RevisionMetadata>,
    pub snapshot: KnowledgeToken,
    pub next_cursor: Option<Cursor>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictTarget {
    Create {
        left: Id,
        right: Id,
    },
    Resolve {
        conflict: Id,
        expected_version: u64,
        resolution_revision: Id,
    },
    Reopen {
        conflict: Id,
        expected_version: u64,
        new_revision: Id,
    },
}
#[derive(Clone, Debug)]
pub struct ConflictWrite {
    pub scope: Scope,
    pub target: ConflictTarget,
    pub idempotency: Id,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictReceipt {
    pub conflict: Id,
    pub decision: Id,
    pub version: u64,
    pub knowledge_seq: u64,
    pub replayed: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictStatus {
    Open,
    Resolved,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictDecision {
    pub id: Id,
    pub number: u64,
    pub status: ConflictStatus,
    pub resolution_revision: Option<Id>,
    pub basis_revision: Option<Id>,
    pub event: Id,
    pub knowledge_seq: u64,
}
#[derive(Clone, Debug)]
pub struct ConflictQuery {
    pub scope: Scope,
    pub conflict: Id,
    pub as_of: Option<KnowledgeToken>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictHistory {
    pub conflict: Id,
    pub left: Id,
    pub right: Id,
    pub decisions: Vec<ConflictDecision>,
    pub snapshot: KnowledgeToken,
}
pub trait MemoryM2 {
    type Context;
    fn write_temporal(
        &mut self,
        ctx: Option<&Self::Context>,
        command: TemporalWrite,
    ) -> Result<MemoryReceipt>;
    fn query_metadata(
        &mut self,
        ctx: Option<&Self::Context>,
        query: MetadataQuery,
    ) -> Result<MetadataPage>;
    fn write_conflict(
        &mut self,
        ctx: Option<&Self::Context>,
        command: ConflictWrite,
    ) -> Result<ConflictReceipt>;
    fn conflict_history(
        &mut self,
        ctx: Option<&Self::Context>,
        query: ConflictQuery,
    ) -> Result<ConflictHistory>;
}

/// Candidate-only future index boundary; never authorization or a content-hydration result.
/// Production query terms and protected index lifecycle require a later security design.
pub struct IndexRead {
    pub scope: Scope,
    pub snapshot_seq: u64,
    pub auth_epoch: u64,
    pub index_generation: u64,
    pub candidate_budget: u16,
    pub retrieval: Retrieval,
}
pub struct IndexCandidate {
    pub scope: Scope,
    pub revision: Id,
    pub knowledge_seq: u64,
    pub deletion_generation: u64,
}
pub struct IndexBatch {
    pub candidates: Vec<IndexCandidate>,
    pub watermark: u64,
    pub index_generation: u64,
    pub complete: bool,
}
pub trait RetrievalIndex {
    /// Caller must reject insufficient watermark/generation and revalidate every candidate.
    /// Pair with the existing Index rebuild Port; no second authoritative database is implied.
    fn candidates(&self, request: IndexRead) -> Result<IndexBatch>;
}
