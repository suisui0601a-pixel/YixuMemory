# Foundation F1/F2 remediation — local only

Status: implementation for independent review, not production authorization/security acceptance.
Reviewed parent: abb7180f4056d104c9b189e07d150f419c8ea182. Public main unchanged.
Original D: source is preserved. C: NTFS remediation clone has no remote. Never publish this
implementation until the owner resolves the exact file/license approval gate. Existing MIT
rights are not revoked; no LICENSE, public branch, tag or PR #2 changes are authorized here.

## F1: assembly boundary

- SQLite implementation is in private `storage`. Root MetadataLab/MetadataTx exports are removed.
- Only opt-in, non-default `metadata-lab` exposes `experimental::MetadataLab/MetadataTx`.
  `fault-injection` implies this lab feature. External default builds cannot import the lab;
  even all-feature builds cannot import storage or the old root write API.
- Lab open/bootstrap/UoW writes are intentionally unauthenticated synthetic infrastructure tests,
  not safe ingress. All existing SQL tests continue through this explicit experimental surface.
- Application has no SQLite dependency, adapter setter, Policy injection or successful metadata
  write path. `NoContentRuntime::write_metadata` always returns CapabilityUnavailable. Bootstrap
  takes a separate, unconstructible BootstrapAuthority and is also unavailable. Ordinary memory
  writer rights must not grant tenant, principal or space administration.
- AuthContext has private fields, read-only accessors, no constructor, Default, deserializer,
  conversion from request claims or production issuer. BootstrapAuthority is independently sealed.
  Unsafe code is forbidden within this workspace. External safe Rust cannot mint either value.
- Future production path must add a trusted authentication issuer and transaction-bound, server-
  owned Policy. Authenticate credentials -> mint context privately -> begin IMMEDIATE UoW ->
  read live principal, Grant, tenant/Space and revocation/policy epochs -> authorize operation
  and scope -> mutate -> commit. Read authorization at the transaction linearization point,
  never accept client-supplied epochs/verified booleans as authority. No public allow-all Policy
  or caller-injected Policy may be the production assembly. This requires later implementation
  and real unauthenticated/no-Grant/revoked/forged-claims tests before opening writes.
- Today only unavailable-entry and compile-time forgery rejection are tested. Live authentication,
  missing Grant and revocation behavior are NOT implemented or accepted. Do not count labeled
  request cases as real authorization verification. A future authorized success path is absent.

Rust visibility is an assembly safeguard, not a sandbox. Enabling the lab deliberately grants
experiment code database access; it must not process untrusted requests. An OS principal with
arbitrary DB file writes, injected code or unsafe process control is outside this protection.
Production needs separate file permissions, process identity/isolation and trusted VFS acceptance.

## F2: trusted schema attestation, not automatic repair

In the existing BEGIN IMMEDIATE migration transaction:
1. Validate version and compiled migration SHA256/history; new empty DB migration is transactional.
2. Independently build an in-memory expected DB from the compiled migration and compiled
   migration_record definition, using the same pinned/bundled SQLite implementation.
3. Compare all main.sqlite_schema objects: type, name, owning table and normalized SQL, including
   implicit sqlite_autoindex objects (SQL NULL). Root page numbers and physical layout are ignored.
4. Compare SQLite-interpreted table_xinfo, foreign_key_list, index_list and index_xinfo for all
   relevant objects. This covers composite same-Space FK targets, index uniqueness/origin/columns,
   primary keys and generated/hidden column metadata. CHECK, trigger bodies, deferred FK semantics,
   views and ordering details also remain protected by CREATE SQL comparison.
5. Execute integrity_check and foreign_key_check, then commit. A failure returns no lab handle.
   Formal Application writes remain unavailable regardless. No modified production DB is rebuilt,
   repaired, imported or partially migrated by this validator.

Expected definitions never come from a DB-stored manifest. Schema version/checksum prove migration
provenance only; object comparison detects drift; integrity_check detects structural inconsistency;
foreign_key_check validates current relationships; per-connection foreign_keys=ON is separately set
and verified. None alone proves all database security. No DDL or migration version/checksum changes
were needed, and the original migration bytes remain unchanged.

Normalizer tokenizes and removes comments/whitespace and folds only unquoted ASCII tokens. It
preserves quoted literals and identifier tokens; token boundaries are not collapsed. Trailing
statement semicolons are ignored. It does not attempt arbitrary semantic SQL equivalence; rewrites
outside this documented normalization are conservatively rejected for offline review. Implicit
indexes are compared, not ignored by sqlite_* prefix. ANALYZE statistics tables are allowed only
when their definitions can be independently regenerated by ANALYZE in the trusted expected DB.
Stats contents are not authority. Unknown extra persistent tables/views/indexes/triggers are rejected.
No cached manifest allows a changed DB to reuse a previously successful result.

The IMMEDIATE transaction prevents another ordinary SQLite writer changing the schema during
validation. This is startup validation, not continuous detection after handle admission and not
protection against arbitrary external file/code modification. WAL/journal configuration can create
normal connection artifacts; failure must not be presented as a byte-for-byte read-only open.
Future production ownership/ACL, handle-safe operations, integrity recovery and active-process
protection are separate gates. Malformed DBs may fail earlier with Database/Constraint errors;
they are never accepted just because the checksum still matches.

## Executable evidence

`verification/old-counterexamples.txt` records both original assertions failing on the unmodified
reviewed source: an external caller created a tenant with no auth; dropped admission trigger was
accepted at reopen. The C clone's initial 23 tracked files were byte-compared to D reviewed source.

Original 44 Rust tests are preserved (38 integration + 6 unit). Additions: 13 real SQLite/assembly
tests, 1 unavailable-ingress unit test, 3 compile-fail doctests and 5 external cargo-check probes.
The new SQLite cases cover missing/weakened trigger, missing implicit unique index, weakened UNIQUE,
missing named index, weakened composite FK, unexpected objects, wrong object type, weakened parent
NULL guard, repeat valid open, harmless formatting/ANALYZE and literal-change rejection. Every drift
case preserves the migration checksum; no automatic repair is performed. The pre-existing actual
parent_number=NULL insertion rejection test remains. Controlled writable_schema appears only in
synthetic destructive test fixtures, never the runtime implementation.

External compilation checks assert specific Rust diagnostics, not merely any build failure.
Run both default production-surface checks and all-feature lab tests. Runtime tests use synthetic
C: NTFS temporary databases and SQLite 3.54.1, not D: exFAT. No tests prove production encryption,
key revocation, secure erase, backup anti-rollback, real Grant/revocation or safe D: deployment.

P1 follow-ups: trusted auth/Policy issuer; ACL/process isolation; production KeyVault and AEAD/AAD;
SQLite WAL/TEMP/log/backup plaintext inspections; generation receipts; deletion coordinator;
crash/restore/old-worker anti-resurrection and actual production filesystem race tests. pytest
8.4.2 security update remains a separate maintenance task; public Python lockfile is unchanged.
No fundamental Rust/Ports/SQLite redesign or 6A escalation is identified by these two fixes.
