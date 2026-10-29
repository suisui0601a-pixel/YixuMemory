# P1 Auth Foundation — trusted test assembly and transactional direct policy

This is an isolated, no-content local engineering prototype. It implements and tests real SQLite
policy behavior; no public authentication, production identity issuer or content service is active.
The reviewed Foundation parent is cedf6ee9e3674e05f79b67d80d3a7899086167bf.
ADR AUTH-001 defines compatibility deltas BEFORE this implementation. No production P0 promises
are replaced by this synthetic test profile. No remote, public branch, PR or license change.

## Modules and trust

```mermaid
flowchart TD
  T[Private cfg test assembly] --> B[Private bootstrap authority]
  T --> I[Private trusted identity mapping]
  I --> C[Opaque AuthContext]
  C --> A[Application MetadataApplication]
  A --> P[Ports AuthorizedMetadata atomic command]
  P --> S[Private SQLite AuthDatabase]
  S --> D[Fixed Application DirectMetadataPolicy]
  S --> DB[(SQLite authoritative state)]
  B --> M[Private administrative transaction]
  M --> DB
```

Domain remains database/OS/model independent; Ports and Application depend only on domain/Ports.
SQLite's test adapter uses Application via an existing dev-dependency. No new library is introduced.
Public facade assembly is an infrastructure composition operation; no production composition root
accepts a request-selected Store or Policy. A fake external Port implementation can fake its own
result, but cannot acquire the real private backend or its Context. Backend fields, raw connection,
constructors, administrative methods and identity issuer are private, not exports or test features.
`cfg(test)` is not activated in a dependency by Cargo feature unification or --all-features.
The fixture does not test a network credential: TrustedIdentity is a key accepted ONLY from this
protected test assembly. No boolean broker proof, requested admin/owner or raw Principal ID is proof.

Old ports::AuthContext/BootstrapAuthority stay unconstructible, inert compatibility markers for
NoContentRuntime; they are not convertible to the backend-associated opaque identity. This avoids
adding a public privileged constructor solely to bridge crates. Consolidating them for an actual
production identity issuer needs separate review. All new trusted constructors remain unavailable
outside the Rust unit-test harness, even when a caller enables both existing lab features.
The lab stays explicitly experimental. It cannot open Auth v2 as a v1 store or be injected into
NoContentRuntime. OS principals with arbitrary file/code writes remain outside API encapsulation.

## Identity and management

The server-side SELECT maps subject -> Tenant/Principal and verifies active binding, active tenant,
active principal, existing same-tenant Space and enabled auth_space. Target Space is a selector only.
AuthContext's private fields: realm, subject, principal, scope, tenant_epoch, space_epoch, issued_at.
No embedded Grant or admin flag; Clone copies observations, not durable authorization. No serializer,
public constructor or conversion from claims exists. The realm prevents contexts from another DB
instance with coincidentally equal tenant/principal/epoch values being accepted. Restored/cloned
backups retaining the same realm are NOT protected from rollback by this mechanism.

Bootstrap starts only in an empty realm/tenant DB. Private BootstrapAuthority is independently
issued, bound to the realm, and never accepted by an ordinary marker command. Tenant initialization,
first principal/binding, new personal Space, direct Grant change and state changes use this private
administrative path. They execute BEGIN IMMEDIATE, validate authority, mutate, increment tenant
Auth epoch (and affected Space policy epoch), append an immutable minimal auth audit and COMMIT.
Failure at any stage rolls back all rows/epoch/audit, including partial bootstrap inserts.
The test-only authority does not implement production admin login, persistence, recovery or approval.

Supported Grant is exactly metadata.append for a direct same-Tenant principal and Space, with
strict expires_at_ms and enabled state. Unsupported capability/group inheritance is denied, not
inferred from ownership, admin claims, P0 role matrices or Group existence. Tenant, principal,
binding and Space disablement invalidate requests. Re-enabling never revives an old context because
epochs only advance. Grant changes conservatively invalidate all contexts in the affected tenant.
There is no grant-cache decision to bypass current state. Tenant-wide invalidation is intentional
for a small first kernel; a future finer epoch scheme must preserve this rejection behavior.

## Atomic application command

MarkerWrite contains only opaque scope/marker/event/idempotency IDs, expected CAS version and an
enum capability. No body, classification, claimed actor, owner or administrator field exists.
Success creates a synthetic system_action Event with NULL payload, marker version, retry receipt,
commit_record, metadata audit and pending metadata Outbox row. It does not create valid Memory,
Revision, Evidence or content Payload. These are intentionally not content grants.

1. BEGIN IMMEDIATE on the authoritative SQLite connection (5-second busy timeout).
2. Sample trusted server Clock; validate live realm, scope, binding/principal/tenant/Space states,
   matching context epochs, direct capability, enabled flag and expires_at_ms > now.
3. Look up (tenant, principal, operation implicitly fixed, idempotency key). Matching digest returns
   the same receipt ONLY after authorization; mismatched digest returns IdempotencyConflict.
4. Otherwise read marker in the authorized scope. A marker in another Space is denied even when
   this principal has a valid direct Grant in that other Space. Compare expected version, allocate
   tenant knowledge_seq in the same transaction, append Event, perform CAS and persist receipt,
   minimal audit and Outbox. All SQL conversion/constraint failures roll back the whole command.
5. Re-sample server time and revalidate policy before COMMIT, including replays. Clock errors or
   backward motion within the command reject. Return success only after a successful COMMIT.

No externally usable raw transaction or commit callback crosses the application boundary. Private
before_commit hooks are barriers in this test-only adapter, used to schedule real SQLite races.
Application has no caller-injected Policy parameter; the adapter uses fixed DirectMetadataPolicy.
Policy observation structs are plain data, not security capabilities; only live trusted DB reads
inside this adapter may supply them for actual writes. Receipts cannot be converted into authority.

## Linearization and evidence

Administration and writes contend for SQLite's real BEGIN IMMEDIATE lock, including across
processes. If a writer already holds the lock and passes policy, its commit may precede concurrent
revocation. If revocation/disable commits first, queued or later requests fail live state/epoch
checks; they cannot replay an old receipt as authorization. No in-process gate is used as evidence.
Authorization expiration is defined at the last server policy sample under the held write lock,
not wall time at the physical COMMIT instruction. This follows P0 recorded_at's non-physical-time
semantics. Sustained contention returns Busy; it never relaxes Policy. No automatic semantic retry.

Tests cover both grant-revoke/write orders, disable/write, grant-add/write, same-version competing
CAS, same-key competing replay and expiry after provisional writes. The cross-process test issues
an old context before revocation, holds a real admin write transaction, makes a child observe
SQLITE_BUSY with a short real timeout, then commits revocation; the child's normal queued command
is denied and knowledge_seq remains 0. This is an actual second process and SQLite file, not mock.
It does not prove correctness on another filesystem, across hosts, backups or arbitrary OS attackers.

## Schema, time and acceptance limits

Fresh v2 composes unchanged Foundation migration plus 0002_auth_profile.sql and stores the combined
checksum. Expected definitions come from compiled code, never the DB manifest. Full object and
interpreted-constraint attestation, integrity_check and foreign_key_check run inside startup's
IMMEDIATE transaction; foreign_keys=ON and WAL are checked per connection. v1 is refused, not
silently migrated. The v1 Foundation attestation entry and all its counterexamples still run.
There is no generic migration/repair tool or validated production import path in this release.

Clock is an injected infrastructure Port from trusted TEST assembly; tests control time to reproduce
expiry deterministically and perform real DB mutations. Network credentials, trusted wall-clock
operation, clock rollback across restart and protected epoch/backup witnesses remain unaccepted.
SHA256 digests cover only synthetic opaque commands here; production HMAC/key and 30-day retention
requirements are NOT implemented. Audit is a local minimal append-only ledger, not production
administrator attribution, denied-request telemetry or tamper-proof storage. Outbox rows have no
worker/lease execution. Admin authorization state orders by epoch, independently of Event knowledge
sequence; current authorization is always applied, including to historical/replayed results.

No public OAuth, REST, MCP, real user identity, complete Group model, encrypted content, KeyVault,
SecurityWitness, deletion coordinator, irreversible erase, backup anti-rollback or D: deployment
acceptance is claimed. NoContentRuntime still refuses all content classes and NoModel operations.
Read/export/history/search endpoints are absent and confer no implemented permission.

## Reproduce

Use C: NTFS source and build/temp/cache directories with enough free space. Never clean unknown
files or reuse D: exFAT for SQLite acceptance. In this machine's prepared environment:

```powershell
. ./scripts/enter-toolchain.ps1 -BuildRoot <C-cache> -ToolRoot D:/YixuMemory/work/p1-tooling
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo clippy --workspace --all-targets --no-default-features --locked -- -D warnings
cargo test --workspace --all-targets --all-features --locked
cargo test --workspace --no-default-features --locked
cargo test --workspace --doc --all-features --locked
python scripts/check_surface.py
```

Python 611 regression runs against a fresh C: copy of public main 85f033e..., with bytecode writes
disabled and caches on C:. No D: source, license, tag, public branch or authorization PR is modified.
