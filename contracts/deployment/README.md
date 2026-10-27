# Deployment and agent contract suite v1

Design-only artifacts, not installable configuration, binaries, an installer or a memory engine. Profile examples use opaque path/secret refs; a future trusted runtime resolves and validates them. Synthetic hashes, signatures, IDs and receipts are expectations, not real authentication, artifacts or execution evidence.

- config.schema.json + profiles/: safe NoModel desktop/container/server/postgres presets.
- plan.schema.json, approval.schema.json, action-policy.json: typed bounded actions and plan/approval relationships; a JSON approval is not proof of human authorization.
- result.schema.json + exit-codes.json: CLI output/exit mapping. stdout secret prohibition needs runtime redaction too, not just schema.
- client-config.schema.json: client-neutral manifest with credential refs and one tenant/Space; not a ready-to-paste universal MCP client config or a grant.
- capabilities.schema.json, tools.json, tool-result.schema.json, context-bundle.schema.json: filtered discovery, MCP descriptors, resource receipts and provenance-aware context projection.
- audit-record.schema.json: minimal deployment action journal envelope, no content or secrets.
- agent-extension.openapi.json: new paths and new component schemas merged into the base API. Existing search path is copied only to add optional X-Min-Commit-Token header; baseline resource DTOs and security schemes remain unchanged. It is a merge fragment, not independently resolvable until base components are included; tests validate the merged OpenAPI without network refs.
- examples/: bounded plan/approval, reader config, audit and synthetic payload→event/evidence→memory→search→context expectation fixtures. A receipt goes into MCP structuredContent; the authorized operation response is an additional JSON resource block. Transport adapters must validate both.

Run python -m pytest -q tests/test_self_hosting_contracts.py. Pure policy oracles validate contract relationships against synthetic trusted observations; they do not implement authorization, cryptography or deployment. Domain cross-object constraints, path realness, token secrecy, UTF-8 byte limits and runtime transactions need the real acceptance suite in docs/SELF_HOSTED_ACCEPTANCE.md.

This extension is published under the repository's current MIT license. It does not implement the unmerged licensing proposal or release a complete commercial-targeted core engine.
