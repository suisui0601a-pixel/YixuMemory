# Publication gate — owner approval required

This is a local engineering prototype, not a publicly delivered engine. No remote CI run is created.

## Proposed source set for a later review

- Cargo.toml, Cargo.lock, rust-toolchain.toml, .gitignore, README.md.
- crates/domain: manifest and opaque metadata/domain types.
- crates/ports: manifest and future security/storage/UoW contracts.
- crates/application: manifest and fail-closed no-content assembly.
- crates/sqlite: manifest, real adapter, migration, integration tests, test-only crash binary.
- docs: mapping, local decisions, this gate, verification summary.
- scripts: local validation/audit helpers, if included in the final source manifest.

The final manifest records exact files and SHA256 values. Exclude `.git`, target, tooling, Cargo
cache, downloaded compilers, databases, WAL/SHM, runtime diagnostics and unreviewed artifacts.

## Preconditions before any public branch or PR

1. Owner explicitly approves the exact source set and its public licensing treatment.
2. Rights/contributor/AI-assisted material review and dependency license inventory are complete.
3. Do not presume future commercial restrictions have taken effect: main remains MIT and PR #2
   remains separate. Already granted MIT rights cannot be retroactively revoked.
4. If any new files are intended to retain future commercial restrictions, hold them privately until
   an approved licensing/distribution plan exists. `publish=false` and a private folder are accident
   prevention, not legal terms or retroactive license changes.
5. Rebase/integrate only through an approved independent branch, rerun all checks, inspect diff,
   then commit/push/remote verification/CI. Local green tests do not establish remote acceptance.

No LICENSE file is created, changed or substituted by this prototype.
