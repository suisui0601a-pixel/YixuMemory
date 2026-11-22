# M2 private metadata contract

Design/review draft for the local synthetic Profile 5; no transport or serializer is implemented.
A request profile marker does NOT activate a laboratory backend. Production NoContentRuntime is
unavailable. JSON scopes select targets only; trusted AuthContext and current Grants are checked
in the actual transaction. Public P0 /v1 JSON Schemas and MCP contracts are unchanged.

- time.schema.json: explicit Unknown or normalized UTC boundary ranges and retained precision.
- write.schema.json: memory.append / memory.revise intent, no client creation IDs or content bytes.
- query.schema.json: current/one/history, bounded same-Space metadata only, signed opaque tokens.
- page.schema.json: structural source_blocked views, content_available=false, no Payload/key/locator.
- conflict.schema.json and conflict-history.schema.json: explicit source-preserving decisions.
- openapi.json: bundled independent HTTP DRAFT, including exact capability selection and unavailable states.

Run the local tests/test_m2_contracts.py for schema shape and OpenAPI validation. These are not FK,
authorization, cryptography or recovery tests. Numeric boundary ordering, known-time consistency,
parent/current integrity, source-causal sequence, true MAC verification and permission/fences are
asserted by the Rust domain and actual SQLite tests. Shape-only example tokens are deliberately
NOT trusted signatures. Future wire binding must strictly decode base64, validate selectors and
reject unknown fields; it cannot infer permissions from this JSON or release blocked content.

Effective fact intervals are [from,to). Uncertain boundary limits are inclusive possible positions;
precision is separate from uncertainty width. Inputs are already normalized, not raw date/NL text.
Calendar normalization and preserving original expressions in protected Payload are deferred.

Read fields are metadata, not factual content. The laboratory placeholder projection is not a
production source_blocked read exception. Recorded deletion/invalidations/state fences still reject
all old snapshots/cursors. New temporal/Conflict/audit references must enter future deletion and
retention inventory before any real user data is admitted. No erasure capability is implemented.
