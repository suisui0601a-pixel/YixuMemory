# Foundation local decisions — implementation notes, not public accepted ADRs

1. Keep all source under the existing public repository's ignored `work/p1-foundation` directory.
   A local-only source snapshot may be committed, but there must be no remote and no push.
   This explicitly follows the owner's current publication restriction over normal sync policy.
2. Pin Rust 1.99.1 and exact direct crate versions; preserve Cargo.lock. Domain uses only std.
3. Use bundled rusqlite to ensure the actual tested SQLite version is recorded, not host Python's.
4. Nonempty Evidence uses a deferred association witness; SQLite FK proof is run on real files.
5. Use BEGIN IMMEDIATE; allocate knowledge sequence in the same transaction as envelopes/outbox.
   Any failed mutation poisons the UoW, even if callers ignore the original error.
6. No production content backend is installed or injectable. Ports retain future KeyVault,
   PayloadStore, SecurityWitness and erasure contracts without pretending operations succeeded.
7. Blocked metadata fixtures may refer to opaque nonexistent key/blob identifiers. Their profile
   explicitly denies encryption. No production import path, active assertion or availability result exists.
8. Standard SQLite VFS is a lab limitation. File ACL/handle race acceptance is not implied.
9. Test-only process-kill entrypoint is gated by `fault-injection` in the separate unpublished
   `yixu-sqlite-test-support` crate. The formal SQLite crate has no such feature or API, and neither
   Application nor its normal dependency graph includes test-support. The helper only appends opaque
   synthetic metadata. This is actual process termination, not physical power loss.

No new fundamental architecture requires 6A escalation for this restricted foundation. Before
production integration, KeyVault/witness anti-rollback, Tguard, read release gates and native VFS
still require the architecture decisions identified in the prior P1 proposal. They are not silently solved here.
