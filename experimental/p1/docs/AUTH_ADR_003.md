# ADR-003: isolate synthetic SQLite writes in unpublished test support

Status: implemented locally for P1-Auth-R2; awaiting independent review. No public sync is
authorized by this ADR.

## Context

P1-Auth-R1 removed caller-selected Marker/Event creation IDs from the formal Auth Application.
However, the opt-in `metadata-lab` feature still exposed `MetadataLab::append_system_event` with a
caller-supplied globally unique Event ID. An external caller could distinguish an Event owned by a
different Space (constraint error) from an unused ID (success). A warning or runtime flag would not
remove that capability from a distributable build.

## Decision

Move synthetic Foundation storage, migration/schema helpers and process-fault test facilities into
the separate workspace crate `yixu-sqlite-test-support`, marked `publish = false`. The formal
`yixu-sqlite-foundation` crate has no MetadataLab API and no feature that enables it. Its normal
dependency graph excludes test-support; formal SQLite unit tests may use it as a dev-dependency.
Application has no normal or transitive dependency on test-support.

Keep deterministic caller-supplied opaque IDs in the test-support fixture API because test fixtures
need stable graph construction. The crate is not a production ingress or a supported distributable
adapter. Release API probes check ordinary dependencies, `--all-features`, explicit support-feature
unification, and direct transitive-import attempts.

This separate crate is chosen over changing fixture IDs to generated values because it establishes
a package boundary for all experimental write and fault-injection capabilities, avoids relying on
feature selection in a distributed artifact, and retains deterministic SQL fixture tests with
limited reorganization. The formal Auth creation path remains server-issued UUIDv4 and is unchanged.

## Consequences

- Downstream users of the old `metadata-lab` feature or `experimental::MetadataLab` API will no
  longer compile. This is an intentional API break; no compatibility alias is provided.
- No production data format or schema migration is introduced by this move.
- `publish = false` prevents routine Cargo publication of the helper, but does not secure a local
  path dependency or decide source ownership/licensing. Production packaging must exclude the helper
  and verify the selected package/dependency graph.
- AuthContext, BootstrapAuthority, direct Grant checks, CAS, idempotency, knowledge sequence,
  Profile v3 and fail-closed production content operations remain unchanged.
- This ADR does not implement production credentials, trusted clocks, KeyVault, encryption,
  deletion coordination, crash-after-commit recovery or D: filesystem acceptance.

## Verification

Run formatting, Clippy, unit/integration/documentation tests, default and all-feature builds,
external compile-negative API probes, package-graph checks, the Python baseline, and local security
scans on the isolated C: NTFS workspace. Synthetic SQLite tests do not constitute production
authentication or storage certification.
