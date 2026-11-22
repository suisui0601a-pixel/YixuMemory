# P0 mapping and explicit gaps

Public baseline: 85f033e646b5446d02a0fdc9a3bf92a0ee8dd0e0.

| P0 requirement | Foundation implementation | Boundary |
|---|---|---|
| tenant/Space isolation | PK(tenant,id), UNIQUE(tenant,space,id), composite FKs | Database integrity, not user authorization |
| Memory/current circular reference | current→same-memory Revision deferred FK | No intermediate committed NULL current |
| nonempty Revision evidence | mandatory primary_evidence_id→actual association row, deferred FK | Witness is not evidence ranking |
| parent same Memory, previous number | five-column parent FK + shape CHECK | Cycles/number gaps rejected |
| current is latest Revision | check at MetadataTx.commit | Arbitrary privileged SQL is not an application UoW |
| memory version vs revision number | separate counters, CAS on version only | Fixed-state metadata transitions only |
| immutable envelopes | update/delete triggers | Does not implement physical erasure |
| complete reverse Payload ownership | canonical payload_owner view | No DerivedLink or full deletion graph/coordinator |
| independent Revision deletion unit | SQL triggers forbid source/Revision Payload aliasing | Only blocked opaque test slots |
| T1/outbox | same real SQL transaction | Outbox claim/ACK/worker execution not implemented |
| tenant knowledge_seq | BEGIN IMMEDIATE + in-transaction increment + CommitRecord | Bootstrap tenant sequence starts at zero; timestamps are synthetic zero |
| failed command atomicity | poisoned UoW + rollback on drop/failed COMMIT | Real process kill tested; physical power loss is not tested |
| migration/checksum | single transaction, application_id, user_version, SHA256, history validation | One initial migration; no online expand/backfill/contract engine |
| Payload classification/encryption | no-content production assembly; blocked lab placeholders only | No seal/open/key, no trusted classification result or public DTO serialization |
| DeletionRequest scope | Space-only enum, typed target FKs, closure/version link | No accepted application deletion operation |
| receipts/generation | separate seal/delete/lease/vault types; copy/store/generation FKs | No verified receipts accepted; completion SQL guard |
| source_blocked | enum + lab-only blocked envelopes | No executed source invalidation |
| RevisionInvalidation | append-only table linked to deletion generation | No coordinator populates it |
| IdempotencyRecord | scope and actor uniqueness, fixed-size lookup/digest slots | No HMAC generation, 30-day scheduler or replay authorization |
| time DTOs | internal integer milliseconds | No UTC serializer, valid_at/as_of API or signed token |
| Principal/Group/Space owners | minimal identity and ownership FKs | No Membership/Grant/Policy implementation |
| filesystem guarantees | standard VFS on synthetic temp files only | Never production-safe A2/VFS support |

The lab intentionally rejects some states permitted by P0: no available/staged content, active
Memory, verified erasure or completed deletion. This is capability restriction, not a weakening of
R1/R14. Future production adapters require a new reviewed migration and real backend evidence;
removing triggers alone is not an implementation plan or an authorization mechanism.

One Evidence per Revision pair has one relation in this prototype. Multiple roles for the same
pair, additional Memory-owned copies, full provenance, conflict processing and temporal policy
projection remain unimplemented; future integration must explicitly resolve their exact schema.

The prior DDL sketch allowed number>1 with parent_number=NULL: SQL CHECK evaluates UNKNOWN
as admissible, and a composite FK with a NULL component is not checked. A new real SQLite
regression was observed failing before the fix. The migration now explicitly requires non-NULL
parent_number for number>1. The regression must pass with the corrected migration.
