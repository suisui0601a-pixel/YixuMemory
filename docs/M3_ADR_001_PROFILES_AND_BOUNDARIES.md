# M3 ADR-001 — Local / Protected and the experimental boundary

Status: product direction approved 2026-11-01; implementation is a local synthetic prototype,
not permission to enable production content. Baseline 48ca49c251c76f5d132cc5f46c2f4874720e9f05.
Authority: YIXUMEMORY_M3_ARCHITECTURE_DIRECTION_APPROVED. The independent challenge and its
16-project/109-file fixed-source inventory remain in the external M3 challenge delivery.
No license change: personal noncommercial free is a product direction; existing MIT and PR #2
are untouched. No private P1 code may be published without separate authorization.

## Guarantee matrix

| Property | Local intended profile | Protected intended profile | This prototype |
|---|---|---|---|
| Official cloud/account dependency | none | none; user may host Witness | none |
| Third-party online dependency | none required | independently recoverable authority required, not necessarily third-party | Fake only |
| AEAD and server policy | mandatory | mandatory | real XChaCha test; fixture protection context only |
| Live authorization and source/history fences | mandatory | mandatory | accepted M1/M2 unchanged; isolated synthetic gate, no DB content integration |
| Witness loss | no remote Witness requirement; local unsafe/unknown state rejects | deny read/write/restore, never downgrade | explicit negative tests |
| Whole-host rollback resistance | NOT claimed without trusted independent anchor | must verify independent rollback/recovery domain | NOT demonstrated |
| Old backup restore | reject all currently unverified routes | current Witness + key authority reconciliation required | Local rejects; Protected model replay only |
| Key revocation | current trusted runtime denies revoked IDs | permanent independent tombstones and verified key lifecycle | volatile FakeKeyVault denies; snapshot of entire fake not protected |
| Production body ingress | requires independent security acceptance | requires independent security acceptance | NoContentRuntime remains CapabilityUnavailable |
| Strong erasure completion | cannot claim beyond actual backend evidence | receipt/closure/backup/key proof required | model Finalized only, not production completed |

Local and Protected share the domain, identity formats, AAD and Ports. A profile name is not
evidence. Local is usable as a future offline design; this stage does not ship a usable user-content
service in either profile. Linux is the first production candidate, Windows NTFS the current
experimental host; NAS/network-filesystem support is not implied by allowing self-hosted deployment.
NAS may eventually run the service with a validated local filesystem. D: exFAT is not a test target.

## Architecture and capability isolation

domain::security_m3 -> std only (pure transition model)
ports::security_m3 -> domain (opaque, unissued production contexts/permits)
application -> ports/domain (existing NoContentRuntime, unchanged)
m3-lab [cfg(test), Windows] -> domain + real AEAD/OS + private fakes

No normal dependency on m3-lab, no feature enabling its private implementation, no public test
body input, no serde/Default/Clone constructor for production ReleasePermit/ProtectionContext.
The lab's normal rlib is empty. External all-features compilation probes test this boundary.
Pure GuardModel accepts caller-provided observations by design: it is a protocol model and cannot
authenticate, issue a production permit, open a key, or write any content. Do not use it as Policy.

Existing metadata Profile v3, schema/migrations, AuthContext, grant logic and current_revision
semantics are untouched. No old profiles/data are rewritten. New ports are versioned proposals
beside unavailable foundation ports. Their lack of an issuer is intentional until production
authorization, key material and receipt validation have been audited.

## Accepted threat boundary

Malicious clients and old workers cannot be trusted; state, key, blob and receipt identities must
match. The present lab uses one Mutex and private fixture identity, not public authentication or
cross-process isolation. OS administrator/debugger/kernel, VM snapshot of memory, dump/hibernation,
and rollback of every fake component are outside what these tests demonstrate. Offline Local must
never be relabelled Protected. Unknown recovery state is unavailable, not permission to bootstrap
a new realm over old data. Previously released bytes cannot be revoked.

## Major alternatives, failures, costs and tests

1. Global TPM epoch KEK rejected as default deletion mechanism: cached sessions/keys, survivor
   rewrap and old backups remain unresolved (challenge F01–F07). Alternative: device-bound local
   key backend with explicit recovery loss; hardware experiment required, no new cloud requirement.
2. One common core selected instead of two engines: costs explicit capability contracts and
   rejection states; test normal dependency graph and both synthetic profiles.
3. Conservative no-restore Local selected until a backend is accepted: costs availability after
   backup restore, avoids falsely asserting independent freshness. Test Unsupported and no downgrade.
4. Source invalidation/R1 closure and classification/R14 remain mandatory: this lab does not
   implement a second graph resolver or encryption exception. It cannot admit payload into SQLite.

No P0 safety promise is relaxed. Operational denial, durable anti-resurrection, cryptographic
termination and physical media erasure remain four different acceptance claims.
