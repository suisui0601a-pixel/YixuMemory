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

`MetadataLab` is a low-level infrastructure laboratory, NOT an authenticated production repository.
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
On this machine source remains on D:, but D: reports an unknown filesystem and cannot preserve
some package archive timestamps. Build/cache and actual SQLite temporary tests use C: NTFS.
This is not an acceptance of D: for production SQLite or trusted file operations.
For the prepared local toolchain, dot-source `scripts/enter-toolchain.ps1 -BuildRoot <private-NTFS-build-directory>`.

```text
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --workspace --doc --locked
```

`fault-injection` only enables a process-kill child binary; it cannot enable content or keys.
`Cargo.lock` pins transitive dependencies; compiler, bundled SQLite version and audit results belong
in the verification report. Dependencies must be re-audited before any approved publication.

See `docs/DOMAIN_MODEL_MAP.md`, `docs/LOCAL_DECISIONS.md`, and `docs/PUBLICATION_GATE.md`.
