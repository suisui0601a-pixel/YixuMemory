# LOCAL ADR AUTH-001 — transactional metadata authorization prototype

Status: historical Auth Foundation proposal; production activation NOT accepted.
R1 creation-ID/receipt/profile details are superseded by AUTH_ADR_002. Retained to record the
reviewed design history, not to authorize the removed caller-selected creation API.
Parent: cedf6ee9e3674e05f79b67d80d3a7899086167bf. No public remote/push.

Changes before implementation:
- Add a narrow AuthorizedMetadata Port with backend-associated opaque Context. Application delegates
  one atomic command to this Port. Its real SQLite implementation always invokes the fixed server
  DirectMetadataPolicy inside BEGIN IMMEDIATE, including idempotent retries and final time check.
  No exposed unguarded transaction or caller-injected Policy belongs to this path.
- The SQLite Auth adapter and its identity/bootstrap constructors are PRIVATE and cfg(test), not
  a Cargo feature. Tests execute real SQL and real authorization; no AllowAllPolicy exists. No
  public/default/all-features build can mint its identity or bootstrap capability. External auth
  issuance is unavailable. Existing ports::AuthContext/BootstrapAuthority and NoContentRuntime
  remain inert compatibility markers; they do not open a second production ingress. The backend
  associated Context is deliberately not constructible from those markers or request claims.
- Use independent Auth profile v2 on fresh test DBs, composed from unchanged Foundation migration
  plus auth-specific DDL. It rejects existing Foundation v1 DBs, does not auto-upgrade them, and
  cannot be opened by MetadataLab v1. Reuse the SAME schema attestation algorithm with trusted
  compiled definitions. Foundation profile/checksum/58 tests remain unchanged.
- Direct grants only: one metadata.append allow-list capability, tenant/principal/Space FK,
  enabled + strict expiry. Every grant/admin change increments tenant auth_epoch; affected Space
  policy_epoch increments too. Conservative tenant-wide invalidation is intentional. Contexts
  carry realm/tenant/Space/principal and observed epochs, never cached permissions or admin flags.
- Bootstrap authority is distinct, privately issued only by trusted test assembly and bound to a
  database realm. It is never accepted by ordinary metadata commands. Admin mutations are atomic.
- Linearization: SQLite's cross-connection writer lock serializes administration and request
  transactions. Validate live state after lock acquisition, before replay/CAS; recheck server time
  and policy immediately before commit. A write already holding the lock can precede a concurrent
  revocation; after revocation commits, no new write/replay using revoked authority may succeed.
  Expiration is evaluated at the last policy sample under that lock, not an impossible guarantee
  about wall clock at the physical disk commit. Request time is never used. Clock failure/backward
  movement within a request fails closed. Trusted-clock persistence/rollback defenses remain P1.
- First business use case is a synthetic opaque metadata marker plus system Event, commit_record,
  CAS version and idempotency receipt. It does not fabricate valid Memory/Revision/Payload content.
  knowledge_seq is incremented only in the same successful transaction; failures roll back all.

Alternatives: exposing a context constructor or allow-all injection was rejected (F1 regression).
Public test-support Cargo feature was rejected (feature unification/all-features). Migrating the
accepted v1 DB was deferred until a separately reviewed real upgrade contract. Group inheritance,
network credentials, content encryption, deletion and production filesystem protections stay closed.

Validation: real temporary SQLite, two connections and process contention, commit-order evidence,
expired/revoked/disabled/foreign identities, idempotency/CAS/rollback, compile-negative public surfaces,
all 58 Foundation tests and 611 public Python tests from a C: copy. No D: writes/builds/DB tests.


Profile compatibility limits (do not silently transfer to production):
- Epoch-stamped, append-only auth_audit rows commit with admin mutations. They are a minimal local
  synthetic ledger, not authenticated production administrator attribution or a tamper-proof sink.
  Admin commits serialize under the same DB lock but do not consume the knowledge sequence of
  memory/Event transactions; authorization always reads CURRENT state independently of as_of.
- Business Event, marker CAS, commit_record, metadata audit/outbox and retry receipt are one atomic
  transaction. Outbox rows are inert: no worker/lease/recovery service is supplied in this phase.
- Synthetic-only idempotency hashes fixed opaque command metadata with SHA256 and retains receipts
  without expiry in this disposable profile. This is NOT production P0 HMAC + 30-day receipt
  retention, is not acceptable for content/real identities, and requires future protected-key and
  retention integration. No production contract or privacy guarantee is weakened or activated.
- Only Append exists; no implicit owner/admin/membership/group rights. Existing P0 owner matrices
  are future production policy requirements, not an excuse to grant unimplemented paths here.
