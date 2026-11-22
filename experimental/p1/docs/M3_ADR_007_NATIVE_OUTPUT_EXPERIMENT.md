# ADR-M3-007: Profile9 and native output termination experiment

Status: A1 PASS; A2 CONDITIONAL PASS; A3 NOT RUN. Synthetic Windows/NTFS only.
W3 baseline f836fbd74ae5417b9e50c41051f3b07e4c7d56b1 is frozen. This decision implements
only the first gates of the approved W4 report/contracts (2026-11-05), not Protected Witness.

## Decision

Add a fresh experimental Profile9. Reopen verifies compiled schema/checksum/FKs and never
upgrades Profile8. output_operation, terminal_evidence, connection_binding, security_mutation
and output_runtime are independent of W3 deletion SecurityIntent. Runtime begins
RecoveryBlocked. No issuer or recovery shortcut changes it to Ready.

The pure Operation reducer binds operation/connection/incarnation/attempt; cancel and timeout
remain Unsettled, only a privately constructed native terminal observation allows NoPending.
Exact counts select Accepted/Partial/Failed, error count ranges remain Unknown even if the
native operation is terminal. Unknown/Terminal cannot resubmit. Ordinary/all-features clients
cannot import the private auth/output module or construct terminal evidence.

SQLite constraints validate tenant/Space/principal/incarnation FKs, one operation per intent
and connection, one active operation, one open connection, immutable evidence, monotonic
phases and retained history. Terminal requires a matching evidence row; insertion and phase
transition must occur in one transaction. Exact-range validity is checked. A completed native
error is not a fabricated zero. Every security mutation category is blocked while an operation
is Submitted/CancelRequested/Stuck. These are storage contracts, not production authorization.

## Native mechanism

Isolated private Win32 module: first-instance local byte named pipe, FILE_FLAG_OVERLAPPED,
PIPE_REJECT_REMOTE_CLIENTS, explicit protected DACL for current process token's user SID,
noninheritable server/event, one manual-reset event and boxed OVERLAPPED per write, locked
synthetic body <=4096. Pipe names are OS-random-ID based; no caller-selected path or handle.
No metadata is emitted by this experiment (zero <=512). Real separate control thread uses
a bounded metadata-only channel capacity16; it cannot certify termination.

One-byte requested pipe quota with a 4096-byte synthetic write demonstrably yields real
ERROR_IO_PENDING on this host without exceeding the body cap. Buffer quota is not assumed
portable or a hard capacity guarantee; tests assert observed pending. CancelIoEx records only
an observation. GetOverlappedResult after the private event proves matching completion;
ERROR_IO_INCOMPLETE/raw STATUS_PENDING is not terminal. Errors record Range(0,B), not Exact(0).
No FlushFileBuffers/client ACK is used. Replay/wrong operation identity is refused.

Resources include pipe/event, Box<OVERLAPPED>, VirtualLock buffer and owned read endpoint.
If Drop cannot harvest completion after requesting cancellation and waiting, it intentionally
retains all heap/OS resources through ManuallyDrop and leaves the process-local BUSY budget
occupied. It never wipes or frees a kernel-referenced buffer. A late terminal harvested during
Drop can safely release experiment resources, but is NOT a persisted core receipt. A3 must not
reuse this destructor as permission to release a real security sequencer.

## Gate limitation and explicit stop

Native live-operation cancellation/completion works in measured cases. But the killed process
test leaves no matching terminal receipt; acquiring its now-free NTFS lock is not evidence of
all old I/O termination. Profile9 remains RecoveryBlocked. No reliable recovery isolation
capability has been established. Pipe/event lifetime under every crash/duplicate/old-instance
schedule is not proved. Native short/zero completion and kernel Stuck beyond cancel deadline
were not observed; corresponding fabricated results are not reported as native passes.

Therefore A2 is CONDITIONAL PASS, not unconditional acceptance of the adapter. Under the user's
gate A3 must remain NOT RUN. W1/W2/W3 source mechanisms, derived-key profiles, DPAPI wrapping,
content admission and NoContentRuntime are unchanged. No key/content integration is attempted.
No installation, production identity, cross-SID ACL, external IPC authentication, actual
Protected Witness, full rollback resistance or cross-platform support is claimed.

## Verification and alternatives

Real SQLite schema/rollback/negative tests; native pending, cancel995, completed-before-cancel
1168/normal0, disconnect109, partial receive then close, duplicate reader retaining pending,
cross-process lock refusal and kill with RecoveryBlocked, wrong completion identity, repeated
bounded requests and metadata queue rejection. Simulated Exact short/zero and Stuck reducer
branches are distinct from actual OS observations. No physical power-loss test.

Rejected shortcuts: cancellation return as terminal, zero count on error, forced close as a
receipt, lease/PID/lock-based restart admission, new-connection budget escape, using a fake
terminal to pass the A3 gate. Remaining recovery proof requires focused architecture/OS
evidence before integration. The cost is retained unavailability rather than unsafe recovery.

References: Microsoft CancelIoEx, GetOverlappedResult, CreateNamedPipeW, WriteFile; W4 local
architecture report and contracts. No third-party code copied or new dependency introduced.
