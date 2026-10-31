# YixuMemory P1 Foundation — private local metadata prototype

Verified public baseline: `85f033e646b5446d02a0fdc9a3bf92a0ee8dd0e0`.

This isolated directory has no public remote. `publish=false` prevents accidental Cargo publication;
it is not a license and does not decide ownership or supersede existing MIT grants.
Do not copy this implementation to a public branch until the owner approves the exact file set
and the applicable license. No LICENSE replacement or custom legal terms are included.

## What runs

- Rust domain → ports → application; SQLite implements metadata UoW and repository interfaces.
- A real, checksum-verified SQLite migration, deferred composite foreign keys, nonempty evidence,
  CAS, transactional tenant knowledge sequence, immutable envelopes and transactional Outbox rows.
- Temporary, synthetic metadata databases; process-kill, rollback, writer-lock and migration tests.
- A production-facing `NoContentRuntime` that ALWAYS refuses content, hydration, erasure and restore.

## Important boundary

The formal `yixu-sqlite-foundation` crate exports no experimental write adapter or lab feature.
Synthetic Foundation fixtures live in the separate `yixu-sqlite-test-support` workspace crate,
which is `publish = false`; it is a dev-dependency for controlled tests and is absent from the
normal dependency graphs of both the formal SQLite crate and Application. Its interfaces are
privileged synthetic test infrastructure, NEVER authenticated ingress. NoContentRuntime cannot
load this adapter.
Formal metadata writes and bootstrap remain unavailable. AuthContext and BootstrapAuthority have
private fields and no issuer or constructor; live Grant/revocation authorization is unimplemented.
Test Payload rows are blocked, opaque placeholders with the fixed profile
`metadata-fixture-not-encrypted`. There is no content column, encrypted body, DEK or successful
cryptographic observation. Memory envelopes remain `source_blocked`; they are not valid assertions.
These fixtures demonstrate relationships, NOT that an actual source deletion happened.

No plaintext test adapter exists. NoModel, `synthetic`, or a client flag cannot enable content.
All production content operations return `CapabilityUnavailable` without a storage dependency.
SQL guards reject available Payload/active Memory states and verified erasure/completed deletion.

Standard SQLite VFS, synthetic fixed timestamps and caller-supplied opaque test IDs are used.
Trusted-handle VFS, authentication, Policy implementation, KeyVault, SecurityWitness, PayloadStore,
graph deletion coordinator, backup anti-rollback, reliable erasure and OS race safety are NOT supplied.
This database is not a production migration/import format; never import its fixtures into production.

## Verification

Use the pinned compiler in `rust-toolchain.toml`, a C compiler, and a private Cargo cache.
The reviewed original remains on D:; remediation source and tests run on C:, but D: reports an unknown filesystem and cannot preserve
some package archive timestamps. Build/cache and actual SQLite temporary tests use C: NTFS.
This is not an acceptance of D: for production SQLite or trusted file operations.
For the prepared local toolchain, dot-source `scripts/enter-toolchain.ps1 -BuildRoot <private-NTFS-build-directory> -ToolRoot <prepared-private-tools>`.

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked -- --test-threads=1
cargo test --workspace --doc --all-features --locked
python scripts/check_surface.py
```

`fault-injection` exists only on the unpublished test-support crate and enables a process-kill
test helper. No feature on the formal SQLite crate enables test-support or experimental writes.
`Cargo.lock` pins transitive dependencies; compiler, bundled SQLite version and audit results belong
in the verification report. Dependencies must be re-audited before any approved publication.

See `docs/DOMAIN_MODEL_MAP.md`, `docs/LOCAL_DECISIONS.md`, and `docs/PUBLICATION_GATE.md`.


Startup compares actual sqlite_schema and interpreted constraints against a trusted in-memory DB
rebuilt from compiled migration SQL, then runs integrity/FK checks. Unknown drift is rejected,
not repaired. See docs/FOUNDATION_REMEDIATION.md for trust boundaries and normalization limits.


## P1 Auth local prototype

See docs/AUTH_ADR_001.md and docs/AUTH_FOUNDATION.md. The private `cfg(test)` Auth adapter
executes real SQLite identity mapping, direct Grant authorization, revocation epochs and atomic
metadata marker commands. It is not a public credential issuer or production authenticated service.
Application adds a narrow AuthorizedMetadata Port; production NoContentRuntime remains fail-closed.
Auth profile v3 uses fresh C: synthetic databases and refuses automatic v1/v2 upgrades. No public
feature (including all-features) can mint its AuthContext or BootstrapAuthority. External compilation
probes verify these boundaries. Existing Foundation migration and 58 tests remain preserved.

R1: creation IDs are server-assigned UUIDv4; MarkerWrite uses Create or a scoped Existing target,
never caller-selected creation marker/event IDs. Receipts preserve generated IDs for idempotent
replay. See docs/AUTH_ADR_002.md. Revocation/concurrency remains conditional runtime acceptance;
this R1 remediation does not assert a production or independent Auth PASS.

R2: synthetic metadata storage and its caller-supplied opaque fixture IDs are isolated in the
unpublished test-support crate. The released SQLite crate has no lab API under default,
all-features, or feature-unification builds. This is a local release-boundary fix, not a production
authentication or data safety certification. See docs/AUTH_ADR_003.md.

## P1 Memory M1 local extension

See docs/MEMORY_M1.md and docs/MEMORY_ADR_004.md. A separate fresh Profile v4 tests blocked-envelope
Memory creation/revision through the private cfg(test) authorization adapter. Profile v3 is unchanged
and old files are never automatically upgraded. memory.append and memory.revise are exact independent
Grants. The public Memory facade uses an atomic Port; its supplied NoContentRuntime always refuses
commands. There is no production Memory or source ingestion adapter, valid content or hydration.
The accepted Auth-R2 parent is 17d6a62e6b7589f0f8b7b840361d36440d3d23eb; this delta awaits Luna review.
