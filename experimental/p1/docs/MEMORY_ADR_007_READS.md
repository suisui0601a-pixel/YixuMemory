# ADR-007 (local private series) — metadata read publication and integrity

This numbering belongs to private implementation ADRs, not public licensing ADR-007.
Status: local M2 proposal awaiting review.

Only metadata envelopes are returned. source_blocked is NOT an available fact; no body, content
Payload/key/locator reference, URI, raw provenance or readable-content flag can be requested.
The production NoContentRuntime remains CapabilityUnavailable. The concrete Auth/Memory/Query
adapter is cfg(test), and test-support stays outside normal dependency graphs under all features.

SQLite BEGIN IMMEDIATE covers live scope/Grant/epoch checks, current graph integrity, snapshot
selection, candidate filtering, materialization, minimal metadata read audit and final live checks.
The final check under the writer lock is publication's linearization point; revocation/deletion
writers serialize against it across connections/processes. A read holding the lock may linearize
BEFORE a queued revoke. No promise is made to retract bytes after publication or to keep a network
stream authorized indefinitely. Future network/content adapters require bounded publication checks.

This deliberately serializes metadata reads with SQLite writers, sacrificing throughput for a
verifiable small-core boundary. A WAL deferred-read plus fresh epoch validation alternative is
reserved until its race and rollback protocol is independently designed. No process-local gate.
Final clock sampling keeps the already conditional Auth cutoff, not an fsync-time expiry guarantee.

Before filters/as_of, check every Memory in the authorized Space: same-Memory current exists and
is latest, contiguous number chain, same-Memory parents, required time/primary Evidence/source
relationships and blocked Payloads. Missing/stale/wrong pointers produce Integrity. No auto repair.
The same snapshot feeds the returned rows. Schema attestation is startup structure checking, not
this row invariant checking; neither proves hostile OS/file control resistance.

Limits: 1000 Memories, 10000 Revisions/Evidence/decisions per Space, page size 1..100. Budget excess
rejects ResourceUnavailable, never fake-complete partial results. Stable order is knowledge_seq,
Memory UUID, Revision.number, Revision UUID. Pagination offsets index the immutable bounded result
at a fixed tenant knowledge_seq; future commits cannot add rows into it. Current fences still win.

Server-MAC cursors bind realm, tenant, Space, principal, both auth epochs, query mode, filters,
validity policy, page size, snapshot and offset. Explicit as_of must agree with cursor snapshot.
Invalid/tampered/re-scoped/stale cursors reject. Tokens are not authority. Metadata snapshots may
be reused only with current reauthorization. Ephemeral test MAC keys are not production credentials.

Index capability enum reserves FTS/BM25/graph/vector/temporal-score, all unavailable. Future index
ports must carry scope, snapshot/watermark, generation and budget, then authoritative revalidation.
Rebuild requires a new generation plus catch-up/fence replays; missing deletion ACK cannot complete
erasure. No plaintext term index is created. CJK/FTS diagnostics use only synthetic :memory: tables.
