# ADR-M3-019: P12 Marker and Owner health (S3)

2026-11-26. Authorized S3 implementation while S2 remains AUDIT INCOMPLETE.
Parent: d8a42e62b339670c94ad7e745d88ff74f616448e. The acceptance-baseline draft
has not been approved. This ADR does not retroactively approve S2.

## Design source and boundary

Use R2_HANDOFF, R2_REPAIR_DESIGN and F2_MARKER_LIFECYCLE_DESIGN from the
2026-11-23 design delivery. Preserve their original files and hashes in the S3
evidence package. Only new P12 synthetic metadata assembly is in scope. P9/P10/P11
definitions, content format 1, DPAPI, existing W1 native policies, and the normal
NoContentRuntime remain unchanged. No S4 file-format/Owner recovery admission,
S5, Native Terminal, A3/R12 or production content is authorized.

## Staged implementation

First implement a pure D/F classifier: valid work plus no identity/observation/
receipt and definitely absent file is Unpublished; a matching file without a
recorded identity is Published; matching recorded identity is Recorded; matching
observation and receipt additionally make Settled. Invalid combinations are
Integrity. Unknown I/O remains StorageUnknown, never Absent. Both errors block
progress; neither is an additional successful lifecycle state.

Next add a private same-handle Marker observation Pin to W1, without changing
open/share/Volume/type/link/reparse/no-replace rules. Native identity evidence
must come from that adapter; pure classifier values cannot sign native evidence.

Only a healthy, ordered, never-recorded Unpublished observation may issue a
non-cloneable, consumed publish permit. Published, Recorded and Settled never
publish. A failed or unknown publish is not retried. Pins protect the verified
object across the short SQL identity/observation/receipt steps. Native I/O must
remain outside SQLite write transactions.

Owner health is monotonic: first latch current in-memory admission, then attempt
the incarnation-bound runtime.blocked transaction. A private Durable receipt
requires a confirmed commit and matching current Owner. KnownNotCommitted and
CommitUnknown are different NotConfirmed reasons. An unsettled connection is
retained and cannot be reused. A later durable block retry never re-enables work.

S3 does not create a new Ready or successful recovery authority. Initial test
admission is limited to a freshly constructed trusted synthetic host; unknown
prior Owner state remains RecoveryBlocked. S2 reference fixtures stay available
for their own tests. Once S3 health is attached to their shared RealmOrder, all
that host's metadata command and reconcile entry points must use it. Normal
authorization errors remain business denials, not fabricated shared corruption.

## Evidence

Each step needs its own real results. D/F value tests are pure synthetic tests.
SQLite tests prove database effects only. NTFS Pin/publication tests use fresh
temporary directories and actual Windows handles. Injected response loss and
commit failures are labelled separately from real OS errors. Process termination
is not physical power loss; no strong deletion, backup freshness, production ACL
or Windows service identity claim follows from this work.

## Concrete S3 assembly and remaining gates

`application/marker_state.rs` contains the pure classifier. The private
`application/marker.rs` coordinator shares the trusted host's RealmOrder across
SQLite connections, consumes PublishPermit by value, retains real Marker Pins,
and revalidates persistent D inside each short transition transaction. Native
publication, observation and Pin verification happen outside write transactions.
`auth/persistence/marker_io.rs` wraps the additive W1 handle observation only.
The ordinary build, including all Cargo features, does not compile these private
test-host modules. There is no feature-gated substitute for production authority.

KeyUnavailable here records the applied synthetic work's local refusal stage;
there is no real key, decryptor, content callback or physical cleanup adapter in
this host. It is not evidence of DPAPI revocation or cryptographic erasure.
Other physical-cleanup observations/receipts are explicitly unavailable in S3;
they cannot be imported as trusted outcomes. S3 never erases a legal Marker.

Only a trusted fresh SQL construction and the exact newly pinned roots can bind
the test host. This is not old-root validation or an OS single-instance proof.
Successful P12 clean stop, reopened-root admission, blocked-owner recovery and
process-level isolation remain unavailable. Report::Ready describes only that
fresh host's in-memory status; it is not a recovery admission proof. A lock,
FileId, schema hash or confirmed blocked bit cannot issue Ready.

On a newly attached S3 host, a failed reconcile may persist only the shared
`runtime.blocked` health fact after ending its read transaction. It does not
repair business history, rewrite Marker identity, issue a cleanup receipt or
advance a deletion. S2-only reference hosts retain their original read-only
reconcile path. S3 failure snapshots explicitly exclude this one permitted
health field and assert it separately; they prove row stability, not full
SQLite schema/PRAGMA/file-state immutability.

Source/NTFS/SQLite/self-validation results belong in the external S3 evidence
package. They do not retroactively approve S2, establish production capability,
or replace independent S3 review. Real OS errors and injected commit/reply-loss
faults must remain distinguishable. No new process-kill or physical-power-loss
claim follows from those injected faults.

## S3 P1 follow-up: main persistence and connection trust

Independent review found a false Durable result in commit 62c0fa4. A real
SQLite TEMP runtime table could shadow both the unqualified UPDATE and its
confirmation SELECT: temp.blocked became 1 while a separate connection still
read main.runtime.blocked=0. Main-only sqlite_schema equality did not prevent it.
The regression is first run against that implementation and retains the failure.

Blocking still latches the host in memory before doing SQL. Its persistence
path now reuses unchanged P12 attest/configured/namespace_clear, covering the
exact main schema and format, integrity, TEMP objects, attached databases,
foreign_keys and recursive_triggers. It additionally reads temp_store=MEMORY
and defer_foreign_keys=OFF; it never configures, normalizes, drops or repairs
the caller's connection. Deferred enforcement is checked before other database
statements. A data-integrity failure can consequently also make this path
NotConfirmed rather than allowing a best-effort Durable claim.

Validation occurs before BEGIN IMMEDIATE, again under its write lock, and
after a confirmed COMMIT before confirmation. Failed in-transaction validation
requires explicit ROLLBACK; unresolved transaction state remains NotConfirmed.
All persistent table references in the Marker coordinator, especially blocking
UPDATE and readback, explicitly target main. Only a confirmed main commit,
trusted readback with matching Owner, and ended transaction issue Durable.

Regression coverage includes a TEMP shadow, a TEMP trigger on main.runtime,
an attached database, disabled/incorrect protection pragmas, and an independent
read-only reopen of main after normal persistence. Invalid connections retain
their original TEMP/ATTACH/pragma state and never become Ready. This is a
localized S3 correction; P12 package/DDL and historical attestation, W1,
Marker encoding/canonical reuse, production API, and S4/S5 remain unchanged.
Implementation self-validation and automated CI do not constitute independent
S3 approval. Durable still means a confirmed SQLite main blocking fact, not
physical-power-loss certification, an OS ownership proof or Ready admission.
