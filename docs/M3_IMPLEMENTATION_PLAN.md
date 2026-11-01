# M3 implementation and acceptance map

This is the approved Local/Protected direction implemented as a SYNTHETIC CORE PROTOTYPE.
It is not a production KeyVault, independent Witness, encrypted Memory service or erasure coordinator.

## Delivered stages

1. ADR-001: shared-core profiles, threat/guarantee matrix and explicit recovery refusal.
2. ADR-002: pure intent/receipt/overlap/reconcile model with failure and schedule enumeration tests.
3. Versioned Ports: KeyVaultV1/WitnessV1/PayloadStoreV1/ReleaseAuthorityV1 and opaque unissued
   ProtectionContext/ReleasePermit. No public construction or normal adapter success path.
4. ADR-003: private cfg(test) FakeKeyVault/FakeWitness, Windows locked buffer and real XChaCha AEAD.
5. Fixed synthetic source -> locked plaintext -> AEAD -> ciphertext-only handle -> bounded validated
   read -> locked authenticated plaintext -> current gate -> digest sink -> intent -> fake revocation
   and store removal -> denied old ciphertext. Both profiles run without LLM; Local calls no Witness.
6. Whole workspace/contract regressions and external API probes; one concentrated Luna review.

The experiment intentionally does not bypass SQLite payload admission or insert a synthetic
available payload into the accepted DB. T1/Tdel are modeled transitions; old real SQLite regressions
remain separate evidence. This avoids silently pretending a fake key verifies production admission.

## Challenge F01–F23 mapping (no test-count inflation)

| Challenge | Current implementation/evidence | Still required |
|---|---|---|
| F01–F04 TPM sessions/cache/NV/power | no TPM dependency; rejected as implicit proof | actual TPM/swtpm command experiments, device records, power cycle and rate tests |
| F05 old DB/blob/key mix | model replays missing intents; tamper/AAD/generation tests | durable DB + external authenticated current-state recovery |
| F06 full VM rollback | explicitly unsupported without independent authority | two genuinely independent restore domains |
| F07 epoch rewrap failure | no global epoch rewrap implemented | backend-specific survivor/backup consistency experiment |
| F08 protocol cuts | injected PrepareAck/Tdel/Erase/FinalizeAck failures + pure transition schedules | real RPC, SQLite, process restart and durable journal cuts |
| F09 response loss | model holds Unknown, no abort; fake ACK loss | request-sequence barrier rejecting delayed PREPARE |
| F10 release race | single Mutex, epoch/current state recheck, digest-only synthetic sink | partial socket/IPC output accounting, no plaintext queues |
| F11 old Broker/two processes | NOT implemented; auto-takeover disallowed | proof of old process fencing and output sink validation |
| F12 overlap/registration | overlapping explicit closures and surviving independent target tests | actual R1 graph/copy-registry freeze under SQLite writer lock |
| F13 old worker/receipt | exact receipt matching, claim epoch and receiver-fence tests | persistent receiver fences and verified external receipts |
| F14 AEAD tamper | real tag/nonce/ciphertext/key/context/generation tests | cross-language complete AAD golden corpus, production adapter |
| F15 bounds | locked-buffer 1..4096 bounds, wire file exact length | hostile network parser/format decoder fuzzing (none exists yet) |
| F16 allocation/lock/RNG/error | injected failures + real checked VirtualLock + wipe test | process-wide dumps, swap/hibernation and cipher-copy audit |
| F17 hostile filesystem paths | handle-retained anonymous test files, no string reopen | native directory anchoring, symlink/junction/hardlink/mount race and VFS |
| F18 shortwrite/sync/ENOSPC | errors are propagated; synthetic Publish/T1 failure and corruption | actual ENOSPC/short-write/directory-sync fault injection |
| F19 Witness missing/stale | version, sequence, unavailable and rollback rejection in fake/model | nonce/signature/realm validation, network partitions and ledger commitment |
| F20 old HSM/KMS backups | no production backend selected | destroy/restore/import/administrator override tests per product/firmware |
| F21 multiple old backups | Local restore denied; model reconcile | registered backup manifests, survivor key mapping and tombstone retention |
| F22 old Revision/as_of/source | existing M1/M2 regression suite unchanged | integrated encrypted hydrate + current source closure enforcement |
| F23 physical power loss | NOT run | real platform/fs/device fault matrix, separate from process termination |

A loop over 3,125 schedules is ONE model test, not 3,125 security guarantees. Compiler probes,
doc-tests, real crypto/file tests and SQLite tests are reported separately. Existing Foundation's
process termination tests are not M3 durable-content recovery evidence.

## Next real implementation gates for Sol

1. Choose a Local OS key backend with explicit offline lifecycle/backup limitations. Exercise
   unavailable key, denied identity, device replacement and revoked ID import. Never use fake keys.
2. Protected Witness: private authenticated API, canonical immutable intent replay log, durable
   request sequence/barrier, realm/incarnation and checkpoint commitments, independent operator and
   backup domains. Run delayed requests, lost ACK, old DB and old Witness combinations on two hosts.
3. Linux Broker: dedicated account/private dirs, peer-authenticated IPC, checked lock/dump controls,
   one release authority. No production feature until old instance fencing and bounded output pass.
4. Persistent PayloadStore: trusted directory handles, no-replace publish, file+directory durability,
   immutable IDs/generations, registered copies and durable receiver fences. Preserve evidence graph
   semantics; implement sparse expected receipts explicitly rather than weaken completion checks.
5. SQLite content transaction integration: keep admission blocked until verified seal/key/material
   exists. T1 checks current grant/source/protection under BEGIN IMMEDIATE; Tdel freezes full graph;
   recovery journal and Witness reconcile precede all content routes. Scan WAL/TEMP/logs/backups for
   synthetic canaries and force failures at every commit/publish boundary.
6. Independent review and actual platform/power/backup tests before any real user content enablement.

No major product conflict found in this restricted implementation: Local can remain offline while
declining unverified strong rollback/restore claims. No guarantee was lowered to make tests pass.
