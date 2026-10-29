# LOCAL ADR AUTH-002 — server-assigned creation IDs and scoped updates

Status: R1 remediation proposal/implementation for Luna independent review; no production activation.
Reviewed parent: 9e3e44225c82e2afcb85f7d745783e03125fa10e.

Confirmed: a caller selected tenant-global marker/event IDs. Other-Space marker occupancy caused
AuthorizationDenied vs success; Event occupancy caused Constraint vs success. Two independent
old-version assertions reproduce these exact differences with two principals and disjoint grants.

Decision:
- MarkerWrite carries target=Create OR Existing(marker), expected_version, Scope, idempotency and
  capability. There is no client-chosen creation marker/event ID or generator. Create requires
  expected_version=0. Existing is a target reference returned by a prior creation, not ID allocation.
- Every new marker and every new event uses OS CSPRNG via pinned getrandom 0.4.3, UUIDv4 layout.
  Allocation occurs inside the trusted private adapter AFTER live Policy and replay lookup. Never
  generate IDs from time, counters, identity or Space; no insecure RNG fallback. This is not a new
  authentication credential, and entropy never replaces authorization.
- A private, test-only ID source seam can force collision/entropy failure. No public constructor,
  Port injection or Cargo feature exposes it; cfg(test) identity/bootstrap boundaries stay closed.
- Check candidate availability under BEGIN IMMEDIATE (same tenant, relevant resource namespace),
  silently discard collisions and retry up to 8 candidates per ID. Discarded candidates, collision
  count/type/Space and SQL diagnostics are never outputs. Entropy failure and exhaustion return
  the same ResourceUnavailable. No hidden caller-provided ID mutator remains in the active path.
- Updates query marker by tenant AND Space AND id, BEFORE version comparison. Absent and foreign
  markers both return AuthorizationDenied, regardless of expected_version. Events are always
  server allocated. Keep (tenant,id) PKs and existing FKs/unique constraints unchanged.
- A same-principal replay returns committed marker+event+version+seq without allocating new IDs,
  but only after current authorization and final clock check. Generated IDs are not Create digest
  inputs. Different valid operations/targets/requests conflict on the same key. Retry key remains
  principal-scoped, so another principal cannot acquire that receipt by guessing/reusing its key.
- New disposable Auth profile 3 composes immutable Foundation and Auth profile 2 plus receipt
  definition delta. Receipt stores marker_id with a same-Space FK and a redundant scoped unique index; PKs are unchanged. v1/v2 opening is rejected BEFORE migration; no online
  upgrade, data loss or automatic repair. New physical DBs only. Formal PK/FK identity architecture
  is unchanged. Existing target refs stay legal; preallocating creation IDs is not a requirement.

Alternatives rejected: remapping errors alone leaves occupied-vs-free success differences; relying
on random IDs while retaining public creation IDs leaves the oracle; replacing all PK/FK namespaces
with (tenant,space,id) would expand scope and is unnecessary for server creation plus scoped lookup.

Validation: old failures retained as evidence; new DTO/external compile negatives; real SQLite
A/B principals; occupied/absent scoped targets; forced marker/event collisions, retries/exhaustion,
OS entropy UUID shape, entropy failure; replay before generator, response loss, cross-principal
key reuse, CAS races and rollback. No constant-time or arbitrary host-attacker claim is made.
Original F1/F2 negative tests remain. Revocation/concurrency remains CONDITIONAL runtime acceptance:
last server clock sample semantics and true Auth crash windows remain unresolved/unexecuted gates.

Primary implementation references: RFC 9562 sections 5.4 and 6.9
https://www.rfc-editor.org/rfc/rfc9562.html#section-5.4
https://docs.rs/getrandom/0.4.3/getrandom/ (also read locally from pinned, checksummed registry source).
Production crypto/identity/KeyVault/deletion/backup and public licensing gates are unchanged.
