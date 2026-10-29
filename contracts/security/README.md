# P0-SC design contracts

Proposal only. deletion-intent.schema.json can represent unsupported broad requests so they can be rejected explicitly; it is not a capability declaration. Accepted DeletionRequest and production Payload metadata schemas live in ../jsonschema. policy.json is a design matrix, not trusted caller input or an implemented enforcement system.

examples.json contains only synthetic IDs/graph expectations. checks/security_closure.py is a pure review oracle: no SQLite, storage, crypto, KeyVault, authorization, network or erasure runs. Tests validate graph consequences, scope/coverage honesty, classification joins and simulated failure decisions. True-valued observations are not proof; actual acceptance is documented in docs/P0_SECURITY_ACCEPTANCE.md.

Root MIT license remains unchanged. No full production core or encryption adapter is supplied. No public API field enables a plaintext test backend.
