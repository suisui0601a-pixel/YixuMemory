# ADR-005 — knowledge snapshots and explicit validity

Status: local M2 implementation proposal, awaiting independent review. P0 retained.

M1 actually wrote acceptance time into revision.valid_from_ms and NULL into valid_to_ms. Those
bytes cannot be reinterpreted as reality validity. Profile 5 is a FRESH-only profile, never an
upgrade: its compiled Foundation DDL renames those legacy slots to accepted_placeholder_from_ms /
accepted_placeholder_to_ms and adds a deferred circular FK requiring one immutable revision_time
row per Revision. The compiled composition and actual SQLite schema are checked. Profiles 1–4
remain unchanged and incompatible; any future import must explicitly map old validity to Unknown,
retain acceptance provenance, and separately authorize source-backed validity. No automatic import.

occurred_at is source event time; recorded_at is acceptance under the write lock, NOT physical
commit time. knowledge_seq is the tenant committed order and rolls back. Revision.number is local
history; Memory.version is independent CAS. Neither wall clock nor revision number is a snapshot.

Validity is Unknown OR Known(from,to). A boundary has inclusive earliest/latest possible UTC ms
positions and an explicit millisecond/day/month/year/uncertain precision. Known facts use [from,to).
NULL end is explicitly unbounded, not permanently true. Unknown never means unbounded. Already
normalized numeric bounds are accepted; calendar/timezone/NL parsing is not implemented. Coarse
precision is retained, not fabricated from acceptance time. Invalid/empty/inverted bounds reject.

For uncertain bounds a point is Certain only inside all possible intervals, Possible if inside
some, otherwise Outside. Unknown remains separate. valid_at defaults to Certain only; explicit
include_uncertain can return Possible/Unknown with labels, never as proven valid content.

P0 as_of is retained as a signed tenant/knowledge_seq token, not arbitrary time or sequence input.
M2 test assembly uses ephemeral in-memory HMAC-SHA256 authority, scoped to database realm and tenant.
This is authentic signing/verification, not a production key backend or backup anti-rollback proof.
Restart/key rotation invalidates old tokens; no database key/claim can authenticate a token.

At a knowledge snapshot choose the latest known Revision per Memory, THEN apply validity. Do not
fallback to an older Revision if the latest is future/unknown/expired: that could revive superseded
facts. Independently valid facts should be separate Memories; History explicitly returns previous
assertions as history. Retrospective revision changes future knowledge of earlier validity but
never changes earlier snapshot results. Temporal portion-update semantics would need a new ADR.

Rows return immutable memory_version_at_revision, not today's mutable aggregate version; otherwise
future writes would change old pages. New writes still CAS current Memory.version. Current/latest
integrity is checked today before any historical selection, in the same SQLite transaction.

Alternatives rejected: reinterpret M1 slots; wall-clock as_of; silently treat unknown as forever;
choose most recent valid historical revision; implement distributed temporal storage in M2.
Tests: late event vs acceptance, rollback sequence, retroactive/future validity, exact boundaries,
uncertainty/precision, old snapshot stability, invalid token/tenant, and missing temporal rows.

Current database integrity also compares tenant root with the committed frontier and rejects a
parent/source committed after its dependent Revision. This detects an internally inconsistent
snapshot, not a fully restored old backup; an old self-consistent DB still needs SecurityWitness
and external protected deletion/key state before any production restore.
