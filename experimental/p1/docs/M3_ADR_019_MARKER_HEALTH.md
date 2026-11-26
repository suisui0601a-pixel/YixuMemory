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

Until final S3 integration and regression evidence exists, later staged items in
this ADR are requirements, not completion claims. The final report must list any
remaining NOT TESTED/NOT IMPLEMENTED items and retain the independent review gate.
