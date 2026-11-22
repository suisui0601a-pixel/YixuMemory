# Local verification — 2026-10-30, Asia/Shanghai

Status: LOCAL_ONLY / PUBLICATION_PENDING. This is Foundation readiness, not full P1 acceptance.
Public baseline remains `85f033e646b5446d02a0fdc9a3bf92a0ee8dd0e0`.

## Executed

- Rust/Cargo 1.99.1, Windows GNU; w64devkit 2.11.0 / GCC 16.2.0.
- Actual adapter uses rusqlite 0.41.1 with bundled SQLite **3.54.1**.
- `cargo fmt --all --check`: pass.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`: pass.
- `cargo test --workspace --all-targets --all-features --locked`: **44 passed**.
  38 real temporary-file SQLite integration tests, 2 actual adapter migration/capacity fault tests,
  and 4 domain/no-content assembly tests. No mock SQLite adapter is used.
- `cargo test --workspace --doc --locked`: success, zero doctest examples (not extra coverage).
- Existing public Python baseline: **611 pytest passed**, mypy 14 files, ruff and supplemental
  repository scan (110 paths, zero findings) passed. Source is unchanged by this prototype.
- Local audit helper: strict mypy and ruff passed; dependency direction validated.
- Live OSV batch audit: exact versions of **42 registry packages**, zero returned advisories.
- Additional audit of the unchanged public Python lock: **29 packages**, pytest 8.4.2 matches
  GHSA-6w46-j5rx-g56g / PYSEC-2026-1845 (the same UNIX temporary-directory issue,
  fixed in pytest 9.0.3). This Windows run does not reproduce that exploit. The public dependency
  lock is unchanged; a separately approved maintenance update and regression run are required.
  Do not describe the combined dependency audit as having no findings.
- Gitleaks source directory scan: zero findings. Final diff and local history checks are recorded
  in the delivery report after snapshot creation.

## Actual assertions

Composite tenant/Space FK rejection; foreign Event/Payload/current/association rejection;
first Memory/current/Revision cycle commits atomically; nonempty Evidence is enforced at COMMIT;
parent identity/number checks; current-latest check; immutable envelopes; independent Revision
Payload units; legal shared Evidence; two-client CAS gives one success and one conflict;
BEGIN IMMEDIATE blocks second writer and times out; failed statement poisons UoW;
rollback removes envelopes/outbox and does not consume sequence; committed sequence is durable;
reopen checks integrity/FKs; repeated migration is a no-op; altered checksum/future version/unknown
history/unknown existing database reject; actual migrator DDL failure rolls back;
SQLite max_page_count triggers a real capacity failure and rollback/recovery;
child process is forcibly killed both before and after COMMIT and a fresh connection verifies data;
Space-only deletion structures and exact copy/store/generation receipt FKs reject invalid rows;
SQL flags cannot promote content or erasure completion.

An additional regression caught a real weakness in the prior DDL sketch: a NULL parent_number
could pass CHECK as UNKNOWN and skip the composite FK. The new regression was first observed
failing, then passed after requiring non-NULL parent_number for later revisions.

## Environment and evidence limits

D: reports FileSystemType=Unknown and HealthStatus=Warning. Cargo failed to set a zero archive
mtime there. Source stays in the ignored D: directory, but dependency cache/build products and
actual temporary SQLite databases are on C: NTFS. No production filesystem support is inferred.

The first Rust bootstrap could not create proxy hardlinks on D:. The exact compiler binaries are
used directly through a process-local setup script. Rust mirror components were compared with
SHA256 values from the official Rust manifest. The portable compiler archive was checked against
the upstream release asset SHA256. No global PATH, system compiler or alternate GitHub credential
was installed/selected.

An exploratory DDL syntax run on host Python SQLite 3.50.4 was supplemental only; the Rust
integration assertions above ran on bundled SQLite 3.54.1.

The SQL capacity failure and process termination tests are real. They are NOT physical power loss,
hardware disk-full I/O fault, filesystem corruption, OS race, key destruction or backup restore tests.
No authentication service, cryptography, production Payload, real secret, model, network service,
deletion coordinator, external writer fencing or anti-rollback backend is provided.

OSV results cover known advisories for exact Rust registry versions; they are not a complete
audit of native SQLite, compiler toolchains or supply chains. Dependency declared licenses include
MIT/Apache alternatives, Unicode-3.0, Zlib, and optional LLVM/LGPL alternatives. Record the actual
license choices and third-party notices before approved distribution. Toolchain binaries are excluded
from deliverables. This report does not replace rights/contributor/legal review.

## Next implementation tasks

1. Review the exact source manifest and decide the licensing/publication treatment. Do not push
   this source or a public work branch until expressly approved.
   Separately approve an update of the existing pytest development dependency to a fixed version,
   then rerun the public contract suite; do not hide that issue inside a core implementation PR.
2. Independently review the real migration, evidence witness, NULL regression, UoW poisoning
   and metadata-only restrictions. Preserve the existing P0 contract suite.
3. Before production integration, implement trusted authentication/Policy/UoW wiring and finalize
   KeyVault AAD/security context, SecurityWitness freshness/Tguard and native VFS contracts.
4. Select and validate a real key/witness backend before enabling any content capability.

No fundamental architecture conflict requires 6A escalation for this phase. The local NULL fix
preserves the intended P0 invariant. KeyVault/witness/VFS decisions remain explicit future gates.
