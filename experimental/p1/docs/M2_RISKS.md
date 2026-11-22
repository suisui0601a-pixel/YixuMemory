# M2 review counterexamples and remaining gates

| Counterexample | Present response | Assurance / next gate |
|---|---|---|
| M1 acceptance slot treated as fact validity | Profile 5 renames placeholders, mandatory explicit time row; v4 files refuse | Actual SQL/FK and checksum tests; future import is unimplemented |
| Unknown time treated as forever | Default valid_at excludes Unknown; explicit uncertain query labels it | Real SQLite + domain comparison; NL/calendar normalization deferred |
| Future correction leaks into old knowledge | Latest known Revision chosen by signed tenant seq; immutable projected version | Actual old/new snapshot tests; whole-old-backup restoration still needs Witness |
| Old Revision linked to later source Event | Causal knowledge-seq invariant rejects before historical output | Privileged bad-data fixture; not arbitrary hostile host resistance |
| Current pointer stale, wrong Memory or missing | Integrity, including history; no repair | Real corrupt-row fixtures on same DB snapshot |
| Candidate returned before permission check | Exact Grant and current epoch first; final check under SQLite writer lock | Real queued revoke and Busy writer tests; no production bearer service |
| Cursor moved to another scope/user/query/epoch | Server-MAC binding, snapshot agreement and live reauthorization | Actual HMAC/tamper/epoch tests; persistent signing/TTL/rotation backend deferred |
| New assertion silently closes Conflict | Decisions are explicit and append-only; lineage refs remain | Real resolution/reopen and source-union tests; semantic detection not implemented |
| Resolution discards other source | Both Evidence sets required in resolution Revision | Actual refusal/rollback; truth is not established by source presence |
| Delete source then query past/cursor | Current deletion/invalidation/state refusal overrides as_of | Lab conservative Space refusal; no real coordinator/erasure receipts |
| cfg features reopen MetadataLab/signer | Concrete adapter remains cfg(test), separate test-support | External compile failures and package graph checks; OS DB-write control excluded |
| contentless FTS assumed safe/CJK capable | No persistent FTS; in-memory synthetic vocabulary/CJK diagnostic | A small diagnostic, not full Chinese retrieval quality or encrypted search |
| Index says complete but watermark stale | Reserved candidate Port; no available implementation | Future authoritative watermark/generation check + rebuild/erase tests required |
| Read query exceeds bounds | ResourceUnavailable, no partial fake completeness | 1001-Memory real database refusal test; production budgets/performance unmeasured |
| Read serializes all SQLite writers | Deliberate BEGIN IMMEDIATE publication protocol | Correctness before throughput; optimized MVCC read protocol needs separate ADR |
| Restart changes token key | Old tokens reject | Expected private ephemeral behavior; not a production service migration plan |

Accepted P0 design is retained. Implemented assurance is only the local no-content profile and
its actual tests. Production content/identity, secure signer/KeyVault, permanent source deletion,
external indexing, backups/anti-rollback, group inheritance, M2 crash windows and hostile filesystem
operations remain unavailable or unaccepted. Existing Auth time/revocation cutoff remains conditional.

No surveyed source is copied; new unmodified Cargo registry dependencies have license/provenance
inventory. Publication still requires the owner's explicit file/license decision. The known pytest
8.4.2 advisory is a separate maintenance item, not suppressed or fixed by M2.

No identified conflict currently requires 6A arbitration. Changing latest-then-validity to temporal
portion updates, making source_blocked metadata projection a production read exception, activating
protected content/indexing, or replacing the publication lock with a weaker check would require a
new reviewed ADR. ADR-007 in this private series does not change public licensing ADR-007.
