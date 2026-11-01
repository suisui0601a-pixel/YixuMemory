# M3 ADR-002 — Tguard model, uncertain outcomes and release

Status: synthetic protocol implementation; real Witness/SQLite transaction integration blocked.
Implements challenge ADR candidates 02/04/08/09 without claiming a durable external authority.

## Model and ordering

```
begin -> OutcomeUnknown -> Guarded -> DbFenced -> Receipted -> Finalized
              deny           deny       deny         deny       target tombstones
```

`begin` records the full explicit closure and request identity before sending PREPARE. Unknown
never auto-aborts. Same intent and exact same fields replay without consuming sequence; changed
input conflicts. Empty/cross-Space closure or zero generations are rejected before mutation.
The pure model's scope fence denies all content in the Space until every pending intent completes;
final tombstones deny each payload/key even if a caller changes generation. Distinct overlapping
requests have distinct records: completing one cannot clear the other. Scope closure is supplied,
not discovered; production must read/freeze the complete R1 graph in the authoritative transaction.

Tguard in Protected is the independent durable PREPARE acknowledgement; the model's earlier
OutcomeUnknown state already conservatively denies. Local's future equivalent requires a durable
local intent/fence commit under the same gate; this lab has only an in-memory journal, not that proof.
Tdel models the database tombstone transition. It does not execute real SQLite deletion. Failed
transition validation leaves the state unchanged; recovery mismatch deliberately disables availability.

Worker claim increments its fence and clears observations from the previous claimant. Each receipt
must match intent/digest/scope/binding/seal generation/deletion generation/closure version/copy/fence.
The current model uses an explicit Cartesian target x copy set (key and ciphertext replicas in the
lab). A future sparse replica registry must preserve exact coverage, not infer missing copies.
Only all expected observations permit finalize. In-memory fakes atomically check their fence at
the receiving map; their tombstones are not durable or externally authenticated.

## Reconcile and release

Protected reconciliation requires a simulated current external ledger, equal realm/profile,
nondecreasing sequence/state/fence, exact local intent identities and a newer incarnation. It
replays intents absent from an old local snapshot. Missing known intents, stale ledger, unavailable
authority or conflicting identity leaves the caller unavailable. Local restore returns Unsupported.

Important unimplemented precondition: an actual authenticated Witness barrier must fence all old
senders and settle in-flight requests before this pure reducer can consume a snapshot. A NONE query
does not prove that a delayed PREPARE cannot arrive. The prototype deliberately has no UNKNOWN->NONE
release transition. Full request barriers/commitments/signature freshness remain production gates.

The concentrated Luna review found a fake-only ledger overwrite in the initial prototype. A new
negative test reproduced it before repair. Fake completion now checks matching realm/profile,
incarnation/sequence and exact immutable intent inventory BEFORE cleanup/claim. It starts from the
remote ledger and projects only the selected intent's simulated receipts/finalize, preserving peer
phases and remote-only facts. Divergent inventory is rejected for reconciliation, never overwritten.
Lost finalize acknowledgement can retry without changing an already finalized remote record.
The selected remote phase/fence and same-fence receipt subset are also preflighted before revoking
any key or removing any blob. A remote-ahead state requires reconciliation, not a cleanup followed
by a late error. A separately reproduced negative test verifies both ledgers and resources remain
unchanged on that rejection.

Private LabPermit is single-use and binds payload, authorization epoch, incarnation and security_seq.
The test release function rechecks those values and live membership under the same Mutex as deletion
and revocation. Its sink is a SHA256 digest of a fixed synthetic body; no network body queue exists.
Two-thread scheduling tests prove only this single-process lab ordering. They do not prove partial
socket writes, process failover, remote admin revocation or delayed external side effects.

Future production: authorization sampling through durable PREPARE must hold the same sequencer and
DB writer lock; unknown RPC outcomes require a persistent deny and an external request barrier.
Release is a bounded output acceptance under the gate, not the time a remote user receives bytes.
No lease-only automatic Broker takeover, cached TTL permission or check-then-send optimization.

## Alternatives, costs and failure tests

- DB-first deletion rejected: old DB restoration can lose the only fence (F05/F08).
- Witness prepare then timed auto-abort rejected: unknown outcome can reopen reads (F09).
- Independent lease-only workers rejected: recipient must enforce fence (F11/F13).
- Coarse Space fence chosen initially: availability loss for unrelated content until pending work
  completes, simpler overlap reasoning (F12). Tombstones remain object-specific afterwards.
- 5^5 bounded operation schedules test rejection immutability and no release after begin. This is
  an exhaustive small reducer test, not a full distributed model proof or physical crash test.
