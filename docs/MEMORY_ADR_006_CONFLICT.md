# ADR-006 — explicit append-only Conflict decisions

Status: local M2 implementation proposal, awaiting review. Extends the existing P0 Conflict.

Conflict links two distinct same-Space Revisions, including across Memories. Canonical endpoint
ordering prevents reversed duplicate pairs. Its identity and initial knowledge_seq are immutable.
Logical status is the projection of append-only ConflictDecision records. Allowed sequence:
initial open, open -> resolved, resolved -> open. Every transition has its own server ID,
monotonic decision number/CAS, system_action Event, knowledge_seq, audit, idempotent receipt and
pending Outbox, committed together. No side's Memory/current/Revision/Evidence is overwritten.

Resolution MUST reference a same-Space committed Revision containing the UNION of both endpoint
Evidence sets. It records a decision, not objective truth or an automatic winner. Reopening requires
a later new Revision carrying at least one new Evidence plus a generated explanation action Event.
No model, rank, insertion order or knowledge_seq decides whether facts conflict or which is true.
Explicit conflict can preserve purported contradictions; temporal overlap is inspectable, not proof
of contradiction. Automatic semantic discovery remains unimplemented.

Conflict operations require memory.revise and exact same-Space live authorization. History requires
memory.history; normal metadata rows expose only conflict references known by that snapshot.
This uses existing P0 capabilities, not metadata.append or a client-declared admin flag.

All decisions remain queryable as_of knowledge token; a resolved projection cannot erase prior
open history. Current fences/revocation and row integrity override every historical snapshot.
The laboratory remains source_blocked, so no content assertion, source repair, deletion or truth
verification is certified. Wrong/cross-Space endpoints and unknown endpoints have the same denial.

Alternatives rejected: overwrite status without decision history; last-write-wins facts; import
Git merge conflicts as semantic Conflict; allow resolution without both sources. No new physical
copy/deletion exemption is introduced. Future erasure must include decision/source references in
the reviewed graph and controlled-copy registry; until then any recorded Space deletion blocks
this lab rather than attempting partial operation.

Tests cover opposite sources, explicit overlap, union evidence, immutable sides, CAS/replay,
resolution/reopen history at old/new knowledge snapshots, scope isolation and rollback.
