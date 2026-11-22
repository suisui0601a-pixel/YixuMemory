# Two-repository integration review

Public base: `85f033e646b5446d02a0fdc9a3bf92a0ee8dd0e0`.
P1 source: `78eb835990cff0086204326d5c2fc91907c353c8`.
Roots: public `4be93e2974460691677e40f8dc1db488b71178e9`,
P1 `b89ad5f06d2a94574d3e31392d4fadcc27438cc2`; no common ancestor.

The integration merge has public main as first parent and P1 as second parent. It preserves both
histories without rebase/squash/filtering. The P1 tree is placed under experimental/p1 using git
read-tree --prefix, keeping internal structure and exact source bytes. This avoids the three root
collisions (.gitattributes/.gitignore/README.md). A file/hash map verifies every imported file.
Existing public design, contract and test files are retained; only integration documentation,
README/ignore rules and an additional CI workflow are added. The original LICENSE is unchanged.

F1 source closure and F2 marker fixes (aabe486, 2aa7ba6, 78eb835) are retained. P9/P10/P11 DDL
and all original Rust source/tests remain exact copies. Old P11 is explicitly incompatible and
refused; there is no automatic migration. The nested historical documents are not new production
capability declarations. Current status is in ../../IMPLEMENTATION_STATUS.md.

## Publication audit

33 P1 commits, 311 unique blobs, 176 historical paths, 171 current files audited. Full-history
Gitleaks: zero findings. No binary blobs, databases, runtime logs, credentials files or special file
modes found. A manual key-pattern hit was an authorized_keys test function name, not a credential.
One project-only author identity uses an invalid-domain email. Absolute-path findings were three
historical locations for D:/YixuMemory/work/p1-tooling examples and one agent identifier; no user
home path or server private key was found. Historical records are retained, not rewritten.

Dependency SPDX declarations and upstream LICENSE/NOTICE hashes are inventoried separately.
No downloaded registry source/toolchain is vendored. Research citations are references, not imports.
Two crates lack root notice files in their downloaded packages; their declared licenses are recorded,
not silently replaced with MIT. This is a source/manifest audit, not proof of all copyright ownership
or of future binary distribution compliance. Future distribution must retain required notices.
The owner authorized public integration under the existing repository license; PR #2 remains a
separate unimplemented proposal. No custom license or retroactive license revocation is introduced.

## Development policy

Only YixuMemory is the intended development destination after approval. This PR must remain open
until the owner approves merging main. Do not create another development line in the P1 repository.
Keep the private repository private and unchanged while review is pending. Archive it read-only only
after the integration is approved/merged and both histories are verified; do not delete it.
Security acceptance of R6, production content and services remains separate from repository integration.

## Validation

P0 baseline CI remains intact. Additional Windows CI runs the pinned Rust toolchain, default and
all-features tests (excluding the previously out-of-scope Native output experiments), Clippy/fmt,
Python contracts, API compile-negative probes, dependency directions and source-hash checks.
No Linux/macOS runtime claim is derived from P0 Python CI on Ubuntu. No Native output, Windows
service registration, real users, production deployment or physical power-loss experiment is run.
See the PR's actual checks and the local integration report for results; planned jobs are not passed jobs.
