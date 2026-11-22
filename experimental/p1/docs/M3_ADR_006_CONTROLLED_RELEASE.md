# ADR-M3-006: single-owner Local release broker

Status: implemented in private Windows cfg(test) synthetic assembly; no production admission.
Authority: approved W3 architecture, W2 baseline 78af78e5b4826fae6b14733da4a8aa6d232601f0.

## Decision

Use a single-owner synchronous broker actor in a dedicated test child process. It owns the
SQLite connection, W1 ciphertext directory, W2 DPAPI vault and locked plaintext buffers.
The actor's exclusive command execution is the security sequencer. Legitimate administration
and release commands use that actor; no release worker receives a DEK or an output capability.
No normal/all-features build includes this module, issuer, fixture or factory.

An NTFS handle-relative exclusive broker-lock file pins the instance for its lifetime. It is
not a lease and has no timeout takeover. Same-volume/type/link/reparse validation reuses W1.
After a process is confirmed terminated, Windows releases the handle. Tests retain/pin the
same configured root; this does not discover malicious copies or prevent an attacker with
full same-identity file/DB access from opening a copied database and starting another instance.
Private OS identity, ACL provisioning and authenticated production IPC remain unaccepted.

Fresh Profile8 adds ReleaseIntent independently of the existing deletion SecurityIntent.
The exact schema/checksum verifier covers the new objects. Profile7 files are never upgraded.
Rows bind server ID, Principal/Tenant/Space, request digest, exact PayloadBinding (including
KeyId/Generation), epochs, fixed purpose, destination, expiry, incarnation, attempt and result.
No body, key, content URI or locator is stored. Output budget is at most 4096 bytes; currently
the cipher parser accepts only the existing fixed synthetic body length.

## Transaction and output order

Pending commit -> W2 authorization/file/hash/DPAPI validation -> locked AEAD decrypt -> final
BEGIN IMMEDIATE live Policy/key/fence/expiry check -> Armed attempt=1 commit -> transaction
ends -> synchronous bounded output, actor still occupied -> result commit -> actor available.

No DB write transaction spans file, key or output I/O. No asynchronous sender queue exists.
The adapter accepts only inherited stdout in a dedicated synthetic child, not a supplied path,
callback or client-selected handle. Rust stdout may buffer: successful write AND synchronous
flush are required before accepting the returned count. No automatic retry of partial output.
Failed/ambiguous writes block the actor. A blocked write keeps the actor occupied indefinitely;
there is no cancellation or timeout claim. An administrator must not call a queued revoke
committed while that write remains unresolved. Loss of availability is intentional.

Armed is durable permission to ATTEMPT, never successful output. Zero accepted bytes -> Failed;
known subset -> Partial; full boundary acceptance -> Accepted (not proof of user receipt/read).
Error, crash, or result-commit failure -> Unknown/Armed pending reconciliation; never replay.
The final DB commit cannot be atomic with external output. Exactly-once delivery is not claimed.

Revoke commits only when the actor is free. Thus revoke-first denies output, while output-first
settles or blocks before a revoke can commit. No new accepted output occurs after a winning
revoke commit. OS-buffered bytes accepted earlier may reach the receiver later.
Grant expiry is sampled at final authorization; no retroactive withdrawal of accepted bytes.

Restart changes incarnation, quarantines Pending and converts Armed to Unknown. No old permit,
buffer or attempt is restored. A new explicitly authorized request can read again; it does not
replay an unknown prior request. Terminal requests remain retained. Unknown outcome alone does
not permanently disable unrelated reads after confirmed process death and fresh startup.

Deletion fences are independent append-only rows. Multiple fences coexist and none is cleared
by completing a release. The experimental fence command is NOT a production deletion service
or erasure coordinator. Existing source_blocked/history and R1/R14 constraints remain unchanged.

## Trust and alternatives

In-process public decryption was rejected: it exposes plaintext beyond the broker. Lease-only
takeover was rejected: a paused old process could resume output. Recording success before I/O
was rejected: it fabricates success. Auto-replay after a missing receipt was rejected: it may
duplicate released content. Async cancellation is not implemented or assumed reliable.

All legitimate security mutations must be routed through the actor. File exclusivity is NOT
a SQLite access-control mechanism. Test-only adversarial connections can bypass the actor;
they test revalidation before Armed, not a guarantee against arbitrary concurrent DB writers
after Armed. Production cannot open until such bypasses are excluded by assembly/process ACLs.

Local remains offline. Same recovery-domain DB/Vault/DPAPI-profile snapshot rollback remains
undetectable in general. No backup import, key reenable, Protected Witness, independent recovery
authority, active-active operation or production content route is introduced. The private test
runner is a process harness, not an installable production service or authenticated IPC server.

## Validation and costs

Real SQLite state constraints, checksum/schema tamper, W1 lock exclusion across child processes,
real DPAPI decrypt and inherited pipe, two command orders, forced child termination before and
after output, terminal request replay, receipt DB error, scope/destination mismatch, missing
cipher/wrapper, key tombstone despite missing marker, exact generation and W2 regressions.
Synthetic partial writes/Armed loss are fault injection, distinct from real OS failures.
No physical power loss, pipe saturation, overlapped-I/O cancellation, cross-SID isolation,
independent Witness or whole-host antirollback evidence is claimed. Serial commands reduce
throughput and a stuck output blocks administration; this is the explicit safety tradeoff.
