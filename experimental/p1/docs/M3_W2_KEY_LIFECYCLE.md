# W2 Local key lifecycle and acceptance boundaries

Implementation decision and threat model: [ADR-M3-005](M3_ADR_005_LOCAL_KEYVAULT.md).

| Phase / failure | Durable authority | Required result |
|---|---|---|
| No secure backend / profile unavailable | No approved key capability | Refuse; never switch to derived key or plaintext |
| Tstage / one-use claim | Existing content_intent + attempted | Persist first, no cryptographic work under SQLite writer lock |
| DPAPI protection failure | Claim consumed, no registered key | Locked/application OS buffers wiped; no Tseal/T1 |
| Wrapped publication unknown | W1 no-replace object may exist | Refuse adoption/regeneration without authoritative registry; retain evidence |
| Registry commit succeeds / cipher absent | Key row may exist, claim consumed | No reseal/generation fallback; new authorized intent needed |
| Cipher published / Tseal or T1 fails | Key registry + immutable ciphertext receipts | Authorized exact retry; no partial graph/Outbox/committed sequence |
| DPAPI unwrap | Current pre/post Policy, gate, registry and marker | Exact binding/hash; key stays internal locked memory |
| Grant/key revoke race | Final DB writer gate | No new external content release after winning revocation commit |
| DB revocation committed / marker fails | Monotonic tombstone denies | Keep blocked, return unavailable, marker retry idempotent |
| Wrapper deleted/corrupt/wrong valid blob | Registry hash + DPAPI/AAD/AEAD | Fail closed; no automatic key replacement |
| Unknown security gate / recovery state | Persisted unknown | Refuse; no automatic reset/restore |
| Same-user restart | Current profile, current registry and wrapped file | Legitimate exact key can unwrap; permissions and revocation reread |
| SQLite-only old copy, current marker | Vault marker | Refuse revoked KeyId |
| DB + Vault + OS profile historical snapshot | Same Local rollback domain | Not detectable in general; no strong deletion claim |
| Rotation / migration | Unimplemented | No in-place fallback, import/export or re-enable API |

The key-bearing W2 path is private Windows cfg(test) assembly with real OS protection and synthetic content. This is not a production SDK/REST service and not a complete implementation of public KeyVaultV1 authority issuance. Public ports remain unavailable until production authorization/process/ACL and recovery acceptance. The persisted Local gate and conservative Space deletion/invalidation checks are not a complete production SecurityIntent/Tguard coordinator; that integration remains unaccepted.

尚未执行不同 Windows 账户切换、无交互登录的服务、域恢复、口令重置、休眠或转储检查、物理断电及独立 Witness 验收。本轮没有修改系统账户、操作系统权限或安全配置。Windows 管理的 DPAPI 内部状态和 profile 存储不属于应用的临时目录；正常 DPAPI 调用可以访问这些系统管理的存储。

Linux/macOS remain NOT TESTED, exFAT unaccepted. The W1 NTFS file layer is reused without modification. Real DEKs are not exported, logged or stored in fixtures; DPAPI-wrapped material and current registry are stored only under the test's private scoped directories. NoContentRuntime remains closed regardless of backend success.
