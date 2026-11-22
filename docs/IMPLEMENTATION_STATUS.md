# Implementation and validation status

Publication is not security acceptance. Source baseline: P1 `78eb835990cff0086204326d5c2fc91907c353c8`.
The P0 public contracts remain authoritative design contracts; the P1 profiles are isolated experimental
formats, not production migrations. F1/F2 have implementation self-tests; specified Luna signoff is NOT TESTED.

| Area | Location | Status |
|---|---|---|
| Memory/domain/ports/application | experimental/p1/crates/{domain,ports,application} | Domain/metadata prototypes; production content CapabilityUnavailable |
| SQLite | experimental/p1/crates/sqlite, crates/test-support | Real experimental transactions/constraints; content assembly is private cfg(test) |
| Authorization | experimental/p1/crates/sqlite/src/auth* | Synthetic trusted-host identities/direct Grants; production identity NOT IMPLEMENTED |
| KeyVault | experimental/p1/crates/sqlite/src/auth/persistence/local_keys.rs | Windows CurrentUser DPAPI experiment, NOT READY as production KeyVault |
| PayloadStore | experimental/p1/crates/sqlite/src/auth/persistence/winfs.rs | Windows NTFS synthetic tests; production assembly NOT READY |
| Deletion/recovery | experimental/p1/crates/sqlite/src/auth/persistence/profile10/b3.rs | Fenced explicit same-Space prototype; no full snapshot anti-rollback/absolute destruction guarantee |
| IPC | experimental/p1/crates/ipc | Protocol and synthetic dispatch/settlement; authenticated Windows service NOT IMPLEMENTED |
| REST API / MCP | contracts/openapi; docs/AGENT_INTEGRATION.md | Contract designs only; production servers NOT IMPLEMENTED |
| Python / TypeScript SDK | future adapters under architecture ports | NOT IMPLEMENTED |
| PostgreSQL backend | docs/COMPATIBILITY.md | Planned, NOT IMPLEMENTED; no SQLite equivalence claim |
| Deployment CLI/container/server | docs/DEPLOYMENT_CONTRACT.md; contracts/deployment | Design contracts; runnable product deployer NOT IMPLEMENTED |
| Schema/ADR/threat model | contracts; docs; experimental/p1/docs | P0 baseline + P1 experimental records; no silent contract replacement |
| Native Terminal / A3 / R12 / Witness | experimental research only | NOT TESTED / NOT READY, no successful production release or recovery |

Windows x64 NTFS is the only previously exercised prototype platform. Windows exFAT, Linux and macOS
remain NOT TESTED. The Linux reference source is not a Linux compatibility claim. CI reports add only
what the actual jobs execute. Process termination tests are not physical power-loss tests.

Profiles 9/10 SQL remain byte-identical to the source snapshot. Profile 11's F1 revision freezes the
delete operation set and refuses incompatible old P11 files; fresh-only, no automatic migration.
History reads still require current authorization and source/key/generation/fence eligibility.
Do not treat examples, cache, old receipts, as_of or fake authentication as permission to release content.
