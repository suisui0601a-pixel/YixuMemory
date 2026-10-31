# P1-Memory-M1: local no-content orchestration

## Actual capabilities

| Layer | Implemented | Not implemented |
|---|---|---|
| Domain / Ports | Existing immutable envelopes; MemoryTarget, MemoryWrite, MemoryReceipt, AuthorizedMemory | Public serialization, temporal/Conflict commands |
| Application | MemoryApplication narrow facade; shared live authorization predicate | Production issuer or production SQLite assembly |
| Private SQLite cfg(test) | BEGIN IMMEDIATE, exact Grant, create/revise, CAS, replay, graph version, transactional audit/Outbox | Active Memory, readable content, background worker |
| Separate test-support | Opaque blocked slots and SyntheticSource fixture | PayloadStore, cryptography, proof of source validity |
| NoContentRuntime | Every Memory command returns CapabilityUnavailable | Any body ingestion or test-adapter injection |

## Payload and source matrix

| Object / field | Requirement | M1 representation |
|---|---|---|
| Event.payload_id | Optional, same Space | Only system_action with NULL payload; no source text |
| Evidence.payload_id | Required content Payload | Blocked opaque slot, no bytes |
| Evidence.locator_payload_id | Required locator Payload | Blocked opaque slot, no URI or path |
| Revision.content_payload_id | Required independent content Payload | New unused blocked slot per Revision |
| IDs, numbers, state, timestamps, assertion kind, relationship | Non-content metadata | Typed opaque IDs / fixed enum / server time |
| User body, URI, locator text, provenance text | Requires verified protection before persistence | No input field, database content column or success path |

The synthetic Event/Evidence graph is structurally valid, not evidence of a true statement. Memory
state stays source_blocked. No original Revision is made readable or rehabilitated. The fixture
profile is explicitly NOT encrypted. Synthetic, NoModel and personal declarations confer no bypass.

Ordinary MemoryWrite refers to existing, scope-authorized Evidence and a Payload slot. It cannot
create a source, choose a global creation ID, choose a parent or control the ID generator. Only
private test assembly can set a one-shot SyntheticSource fixture before execution; its seed runs
inside the same transaction on a NON-replay, after authorization and CommitRecord creation. The
helper lives in test-support and accepts no text. This proves the complete graph can commit or
roll back together; it does NOT implement authenticated event ingestion.

## Call and transaction flow

```mermaid
sequenceDiagram
    participant T as Controlled test assembly
    participant A as MemoryApplication
    participant S as Private SQLite adapter
    participant DB as C NTFS SQLite
    T->>A: Opaque internally issued Context + MemoryWrite
    A->>S: Atomic AuthorizedMemory.apply_memory
    S->>DB: BEGIN IMMEDIATE
    S->>DB: Read realm, live identity, states, epochs, exact operation Grant
    S->>S: Canonical evidence-set digest, bounded to 64 entries
    S->>DB: Lookup tenant/principal/key receipt
    alt Committed replay
        S->>DB: Compare digest and check fences; no allocation or new rows
    else New command
        S->>DB: Read scoped current + CAS; derive parent and number
        S->>S: OS UUIDv4 allocation with bounded private collision retries
        S->>DB: Increment tenant knowledge_seq + insert CommitRecord
        opt Private source fixture only
            S->>DB: Insert blocked slots, Event, Evidence
        end
        S->>DB: Verify scoped sources and unused independent Payload
        S->>DB: Insert Memory if first + immutable Revision + Evidence relations
        S->>DB: CAS current/version if revision; require latest current
        S->>DB: Insert receipt + audit + pending Outbox; increment graph_version
    end
    S->>DB: Recheck live authority and time; COMMIT with deferred FKs
    S-->>A: Opaque resource IDs, version, number, knowledge_seq, replayed
```

All failures unwind the rusqlite transaction; a failed consuming COMMIT rolls back. No intermediate
Memory/current pointer is committed. The old Revision rows and relation rows are immutable.
Memory.version is an aggregate CAS counter; Revision.number derives from the current Revision,
not from version. A synthetic version-only fixture demonstrates version 8 / number 2.

Epoch checks and exact Grants run before resource lookup and again before commit, including
replays. memory.append does not imply memory.revise; metadata.append implies neither. Authorization
mutations and business writes use the actual SQLite writer lock on separate connections; a write
holding the lock may finish before revocation commits, while a queued write after revocation must
be denied. The final trusted-clock sample is still the accepted conditional cutoff, not a proof
of wall-clock expiry at physical fsync. No new process-local gate is used.

Cross-Space/Tenant and absent source/Payload/Memory references map uniformly to AuthorizationDenied.
Creation IDs are internal OS UUIDv4. Collisions are retried privately up to 8 candidates per ID;
entropy failure/exhaustion return the same ResourceUnavailable with no SQL detail or candidate.
High entropy never replaces Policy/FKs. No new globally unique client-controlled resource insert
exists; idempotency keys are principal-scoped.

Replay uses a canonical sorted Evidence set, target/expected version, primary and Payload references
plus scope. New IDs are not request input. Same principal/key/different digest conflicts; another
principal gets only its own receipt. A response-loss retry returns committed IDs even after reopen.
A revision replay can return its old receipt after newer revisions but cannot change current or
read old content. This laboratory digest is SHA256 over opaque metadata, not a production
HMAC/retention implementation. Receipts do not confer authorization or readable Memory status.

## Deletion, temporal and recovery restrictions

Any recorded DeletionRequest in the selected Space blocks new M1 operations and replay. This is a
conservative local refusal, not a replacement for the accepted graph deletion coordinator. A
non-source_blocked Memory or an invalidated prior revision cannot be revised here. SQL still rejects
active states, completed deletion and verified erasure. No source replacement can revive an old
Revision. Shared independent Evidence is retained; no erase command is implemented.

No valid_at/as_of reads, historical hydration, Conflict resolution or permanent source replacement
API is exposed. Compile-negative probes reject nonexistent history/resolve_conflict facade methods;
NoContentRuntime refuses hydration, erase and restore; actual fence/invalidation fixtures refuse
writes/replays. Those tests do not prove source_blocked enforcement on future reading surfaces.

Existing Foundation process-kill and Auth cross-process tests are retained. New M1 tests exercise
real rollback, deferred-FK failure, reopen/response loss, actual two-connection writer ordering and
CAS/idempotency races. M1-specific process-kill windows, physical power loss, hostile DB file
replacement, production trusted clocks, old-backup restoration, generation receipts, encryption,
KeyVault, SQLite plaintext inspection under real encrypted workloads and full deletion remain
UNACCEPTED. Standard VFS/OS file access is trusted in this local test harness; Rust visibility and
schema attestation do not protect against an adversary controlling the DB and process/code.

## Review and reproduction

Use the pinned toolchain with scripts/enter-toolchain.ps1 and existing C: build/cache. Never run
these SQLite tests on the original D: exFAT workspace. Do not clean files without user approval.
Run fmt, clippy all targets/features, full/default Rust suites, doc tests, both builds, external API
probes, audit_local.py, gitleaks source/history and unchanged P0 Python tests/mypy/ruff/security scan.
P0 pytest includes JSON Schema/OpenAPI, relation and security-contract verification.

The new Rust scenarios are in crates/sqlite/src/auth/memory/tests.rs. Fixture-only constraint tests
intentionally bypass orchestration to exercise real deferred SQLite FKs; they are not application
authorization tests. Compilation probes and package-graph checks are counted separately. Execution
counts, exact SQLite version, disk space and checked SHA belong to the local verification report.
Nothing in this change is pushed, published or independently approved by its author.

One final highly parallel run was stopped after the unchanged Foundation two-client CAS fixture
waited indefinitely at its unbounded Barrier. A pre-Barrier open/timeout is a suspected trigger,
not a proven diagnosis. The retained failed-attempt log is not counted as a passing suite.
Full final suites use --test-threads=1 for independent fixture scheduling; internal two-connection,
thread and process race tests remain enabled. Hardening that old harness with bounded readiness
signals is a separate non-production maintenance item; no original tests were removed or edited.
