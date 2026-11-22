# ADR-M3-005: Windows Local KeyVault — DPAPI user-scoped wrapped DEKs

Date 2026-11-04. Status: implemented for isolated synthetic assembly; not production admission. W1 base 0ed6265d56a03c0401f59e2d971822cc5a9d166c. No Local/Protected trust-model change.

## Decision and alternatives

Use Windows CryptProtectData/CryptUnprotectData current-user scope, UI_FORBIDDEN, null prompt/description. Never LOCAL_MACHINE, never interactive prompts or automatic weaker backend. Each Payload gets 32 random bytes via getrandom's Windows OS RNG in checked VirtualLock memory. Canonical PayloadBinding AAD(32) is DPAPI additional entropy AND part of the protected plaintext with the DEK. It binds realm/Tenant/Space/owner/Payload/KeyId/SealGeneration/vault incarnation/protection context. Public entropy is context, not a secret password. Payload AEAD remains XChaCha20-Poly1305 with its original content AAD.

CNG software KSP was considered for a non-exportable asymmetric wrapping key: useful future broker/service isolation, but requires persistent key-container identity, provider availability, backup/ACL/rotation and separate acceptance. Platform KSP/TPM would add hardware/recovery constraints, not automatically supply rollback-resistant revocation. DPAPI-NG/domain descriptors are not needed for offline personal use. A plaintext/test-derived DEK backend is forbidden for this profile.

References: [LocalSize](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-localsize), [CryptProtectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata), [CryptUnprotectData](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptunprotectdata), [CNG key storage](https://learn.microsoft.com/en-us/windows/win32/seccng/key-storage-and-retrieval). No third-party implementation copied, no crypto algorithm invented.

DPAPI binds OS credentials/profile, NOT one application process. Other code under the same Windows identity can invoke DPAPI. Same-identity full process/file control, administrator, raw disk, kernel, crash dumps and memory inspection are not defended. Owner/private ACL provisioning and a production broker are separate gates. Normal DPAPI can access Windows-managed profile/masterkey state; we do not alter accounts, policies, global ACLs or delete OS-managed keys. Application code requires no official cloud/online service; domain-profile Windows recovery can have domain/enterprise dependencies and is not validated here.

## Fresh Profile 7 and trusted path

Old Profile 6 remains test-derived and unchanged. New Profile 7 is created fresh from compiled migrations plus a local key registry; existing profiles cannot be silently upgraded. Production public KeyVaultV1/ReleasePermit issuers remain unavailable. The real crypto backend is private cfg(test) with existing Auth/Bootstrap assembly; normal/all-features dependency users cannot construct it. It is real OS protection executed on synthetic data, not a production service.

Profile7 operations require an explicitly supplied LocalVault and never fall back to lab_key. New key registry binds exact AAD, wrapped-file hash and a monotonic revoked bit; FK binds same-Space sealed_copy. Updates cannot change binding/hash or undo revocation, deletes retain tombstones. local_key_gate ready/unknown is a persisted Local gate, not a fake independent Witness. Missing/unknown gate refuses key use. Any deletion/invalidation in the Space is conservatively fenced by existing Policy logic.

Call path: trusted AuthContext → short BEGIN IMMEDIATE live Policy + current gate/binding/one-use claim checks → external DPAPI/W1 file I/O outside SQLite write tx → second BEGIN IMMEDIATE Policy/gate/binding/hash/revocation revalidation → internal locked DEK use → Tseal/T1 checks all four registered nonrevoked keys → final read authorization/key-state gate before synthetic output. A concurrent committed grant/key revoke wins at the next check; output release is ordered by the final SQLite writer transaction. In-flight computation may hold a DEK, but no raw key export API exists and no output is allowed using stale authority. This is not a production network ReleaseAuthority.

## Key and content lifecycle

1. Tstage records server-chosen IDs. Durable attempted=1 commits BEFORE DEK generation, Seal or external publication.
2. Generate independent random DEK in locked pages. DPAPI wraps DEK + AAD. Publish wrapped bytes under KeyId via the exact W1 Anchor (exclusive temp, file sync, readback, no-replace native rename, identity checks). No unwrapped key is persisted.
3. Revalidate authority and record hash/binding. A crash after wrapped publication but before registration is UNKNOWN: do not import a guessed wrapper or regenerate/reseal the KeyId. Orphan objects remain blocked; automatic recovery is not implemented for this window.
4. Seal ciphertext with that key and immutable generation, then existing Tseal/T1. T1 rollback preserves key/cipher staging but publishes no partial business fact, Outbox or knowledge_seq. Existing one-use rules prohibit reseal after uncertain missing ciphertext.
5. Before each unwrap, check current authority/gates/registry. Reject mismatched hash, scope, generation, KeyId, revoked state and DPAPI integrity failure. Recheck after external operations. No retained DEK cache or lower-generation retry.
6. Trusted Bootstrap-only key revocation commits a permanent DB tombstone, then writes a no-replace Vault revocation marker outside the tx. Every load checks the marker before and after unwrap. Marker failure leaves DB revoked and returns unavailable; this is not successful physical erasure. Existing markers always deny, even malformed. API cannot remove/re-enable them.

Independent Vault markers survive a SQLite-only old-copy experiment when current Vault files are retained, but both reside in the same Local recovery domain. Restoring the DB AND Vault/profile snapshot can resurrect historical usable material; this is NOT an independent anti-rollback authority. A crash between DB revocation and marker creation retains the committed DB tombstone; marker retries are idempotent. No supported backup import/restore API is provided; known restore attempts require separate validation, not automatic resurrection. Manual replacement of all authority files cannot be reliably distinguished without independent authority. Local does not claim strong deletion against historical snapshots.

## Sensitive memory and failures

DEKs, protection input and content use the accepted lock-before-copy/zeroize-on-drop buffer. DPAPI unprotect allocates a Windows-owned buffer outside application locking control; a RAII owner queries LocalSize to validate the actual Windows allocation, wipes that allocation then LocalFree on success and application validation errors. It never trusts a failed API cbData for slice length; unverifiable allocation pointers are neither dereferenced nor freed, and the operation refuses. No complete cleanup guarantee is made for invalid OS allocation metadata. It is immediately copied to checked locked pages before use. This cannot prove DPAPI's internal copies, cipher registers/stack, paging during OS allocation, dump, hibernation, forced process termination or firmware safety. Failure tests observe application-owned wipe before LocalFree, not OS internal memory cleansing.

No Debug/Clone/serde/export for DEKs; errors contain no key bytes. Test assertions compare secrets as boolean conditions, never formatted byte slices. Disk scans use fixed synthetic content and transient generated keys without printing them. Wrapped DPAPI files are sensitive protected material and must remain private; filenames and binding metadata are not content encryption substitutes.

## Identity, recovery, migration and rotation limits

Actual tests use the current logged-in Windows user and a separate child process under that same identity. Different application principal and scope are rejected. Different OS SID/service account, password reset, domain recovery, logged-out profile, gMSA, unattended boot and credential migration have NOT been tested; no accounts are created/changed to fake such proof. Unavailable DPAPI/profile refuses, never prompts or falls back.

DPAPI may permit roaming/domain recovery; user password resets can lose access. No portable key export, backup recovery key, migration wizard, machine-wide override or revoked-key recovery backdoor is provided. W1 directory power-loss limitations remain. New Payload/new KeyId can use a new independent DEK; same-Payload rewrap/rotation and generation migration are unimplemented. Read generation is exact, never searched backwards. Protected Witness is not implemented.

## Verification plan and boundary

Real DPAPI protect/unprotect, random independence, wrong DEK/AAD/scope/generation, tamper/missing wrapper, same-user process restart, rollback/replay, revocation races using real SQLite connections, monotonic tombstones, unknown gate, application buffers wiped, disk key scan, and real/fake profile isolation. Original W1 native filesystem and M3-B/Foundation/Auth/M1/M2 suites remain required. Physical power loss, actual OS-identity switch and production KeyVault/Broker certification remain unexecuted. NoContentRuntime continues CapabilityUnavailable.
