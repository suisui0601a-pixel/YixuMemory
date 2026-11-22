# ADR-004: private blocked-envelope Memory orchestration

Status: implemented locally for independent M1 review; not a production admission design.
Accepted parent: 17d6a62e6b7589f0f8b7b840361d36440d3d23eb.
Public main: 85f033e646b5446d02a0fdc9a3bf92a0ee8dd0e0 (unchanged).

## Boundary and decision

P0 requires an Evidence content Payload plus a locator Payload, and an independent Revision
content Payload. No secure KeyVault, PayloadStore, classifier or production issuer exists.
Consequently an active, content-bearing Memory cannot safely be admitted in M1.

Retain every Foundation admission/state/immutability trigger unchanged. Test-support alone can
create fixed blocked opaque Payload placeholders, carrying the explicit profile
`metadata-fixture-not-encrypted`. They contain neither plaintext nor ciphertext and never prove
successful encryption. NoContentRuntime rejects all Memory commands as CapabilityUnavailable.

Add the narrow Memory Port and Application facade. The only SQLite implementation is within the
existing private cfg(test) Auth module. It is absent in distributable builds regardless of feature
unification. Test-support is still absent from normal Application/SQLite dependency graphs.
Application exposes neither Repository nor bootstrap nor fixture configuration.

A fresh private Profile v4 composes the unchanged Foundation/v2/v3 SQL plus 0004_memory_lab.sql.
V3 remains accepted by its original open path; v4 requires its separate private constructor.
Both refuse the other's files before migration. This is NOT an upgrade/import migration.
Checksum, actual schema attestation, integrity check and FK check remain separate checks.

Use existing P0 names memory.append and memory.revise, selected by operation, not caller flags.
A dedicated memory_grant laboratory projection avoids rewriting immutable v2 SQL or broadening
metadata.append. Protected bootstrap mutation bumps tenant auth_epoch and Space policy_epoch,
and appends the existing grant.change audit action atomically. Group/inherited authorization is
not implemented. Future production Grant projection must be reviewed separately.

## Alternatives

| Alternative | Reason not selected |
|---|---|
| Nullable Evidence/Revision Payloads or removing admission triggers | Weakens accepted relationships and production encryption boundary |
| Fake successful seal or plaintext synthetic store | Synthetic is not encryption authorization |
| Public MetadataLab Memory ingress / feature flag | Reintroduces F1 and R2 packaging bypass |
| Map metadata.append to Memory rights | Expands authority without an explicit Grant |
| Change PK to tenant/Space/id | Unnecessary; generated IDs and scoped reference checks preserve R2 |
| Upgrade v3 files in place | Not required or reviewed; risks accepted private snapshots |
| Wait for full cryptography before testing any relationship | Needlessly delays real transaction/FK/CAS validation; blocked envelopes suffice for this limited goal |

## Compatibility and tests

No existing migration SQL, public P0 contract, Memory PK/FK or published license changes.
New local DTOs have no creation IDs, parent pointer, Revision.number or content bytes. Old tests
are preserved unmodified. New tests cover transactional Memory/Revision creation, source fixture
atomicity, CAS races, replay ownership, revoke ordering, independent counters, deferred-FK
failures, shared evidence and unavailable content. External probes check the package boundary.

This records a capability-restricted local implementation choice, not a change to P0 deletion,
MCP or protection semantics. No identified conflict currently requires 6A arbitration. A future
proposal to make blocked placeholders valid facts, unify test/production admission, or silently
upgrade this profile WOULD require a new security ADR and review.
