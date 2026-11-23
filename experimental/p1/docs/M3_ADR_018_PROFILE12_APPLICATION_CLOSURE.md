# ADR-M3-018: P12 metadata Application closure and transaction ordering

2026-11-23. S2 implementation only. Parent: eb0dabf66f9a17ee9456abae5f7d0b50cf78b3c5.

## Decision

Keep the S1 Schema Package bytes/hash and Profiles 9/10/11 unchanged. Add a
private metadata-only Application beneath the existing cfg(test) P12 module.
Do not teach the historical content Engine to accept Profile 12: that would also
require S3 Marker health and S4 filesystem/Owner admission, neither authorized.
There is no new production Application, SQL connection, identity issuer, content
API, or feature flag. Synthetic actors and management commands belong to the
trusted test host and cannot be obtained through a public dependency.

Use the existing R4/R5 authorized_snapshot, closing, make_effective and graph
binding helpers. One private RealmOrder capability is shared by the trusted
single-process host's connections. It serializes deletion and security changes;
BEGIN IMMEDIATE provides a stable database transaction in addition to this domain
ordering, not in its place. This is not a cross-process singleton or Windows
identity/ACL proof. Arbitrary raw database writers remain outside the trusted host
boundary. Revalidation detects inconsistent retained graphs rather than repairing
them. Revision writers are not added; inherited Closing admission triggers reject
new writes, and the retained graph is checked again at each phase.

## Structural closure

Enumerate every write_intent in the requested scope before projecting a target.
Unknown/uncommitted operations block. Independently require each operation's
Memory, historical Revision, predecessor, Current pointer, Evidence/Source,
RevisionEvidence, Payload/Key, publication, receipt/Outbox and copy registration.
Reverse checks refuse unregistered revisions/payloads and gaps. Recompute the
existing format-1 dispatch binding from retained immutable identity/generation
metadata to detect source rebinding, without treating unavailable keys or retired
sources as absent. Quarantine is never a closure filter. No content is decrypted.

Closing authorizes every distinct affected Memory using the current authority
projection and current server time, then atomically inserts event/intent/closure
and seal. Fence revalidates identity, complete graph, frozen equality, live policy
and earlier-event ordering. Applied constructs members/work/tombstones from the
same selected closure, performs both SQL EXCEPT checks, samples time and checks
policy again, then updates epoch/receipt/Applied in one transaction. It does not
increment knowledge_seq, rewrite Revision/Current, or emit business Outbox rows.

The final policy sample before Applied is the authorization linearization point.
Grant expiry after that sample is not a nanosecond wall-clock COMMIT deadline.
Earlier security changes commit before this check; later changes wait for the
same ordering capability. Later changes cannot pass an unresolved Closing.
No native/file/DPAPI/output I/O occurs inside these production-shaped SQL paths.
Test-only checkpoints permit deterministic transaction faults/barriers; they are
not ordinary caller callbacks or native completion evidence.

## Work, rollback and reconciliation

An overlapping event may reuse earlier work only after checking its canonical
event is earlier, same-scope Applied, membership matches, generation and exact
existing format-1 marker bytes match. It never blindly ignores a conflict or
rewrites marker identity. Marker bytes are metadata only here: S2 publishes,
observes and erases no marker, wrapper, ciphertext or backup file.

Failures explicitly ROLLBACK. A failed COMMIT never reports Applied; it attempts
rollback and conservatively latches the shared in-memory admission block even if
rollback succeeds. Failure to confirm rollback prohibits connection reuse.
This is not S3's durable shared-health/Marker recovery implementation.

Reconcile only reads a SQLite snapshot: it validates seals, authoritative/frozen
sets, Applied members/work/tombstones/receipts and Owner metadata. It never fills
missing members, invents authorization, advances Closing or produces Ready.
Historical Applied events are not undone when the original principal loses a
Grant; any new command/idempotent response still checks current authorization.
Opening a connection in the synthetic host is not S4 restore admission.

## Validation and remaining gates

Real NTFS temporary SQLite tests cover multiple memories/revisions/sources,
quarantine, corrupt/missing links, expired/disabled grants, epochs, seals,
two-connection thread ordering, exact-set failures, deferred COMMIT rollback,
overlap, replay and conservative reconciliation. S2 connections additionally
read back temp_store=MEMORY and reject defer_foreign_keys; S1 standalone behavior
is unchanged. Parent REPLACE assertions are now permanent Rust SQLite tests.

S3/S4/S5, actual content/file cleanup, DPAPI/KeyVault, Native Terminal, Windows
service isolation, A3/R12, production content, cross-process/physical crashes and
cross-platform operation are NOT validated by this decision. NoContentRuntime
continues to return CapabilityUnavailable. No schema migration is introduced.
