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

PR #3 remediation: MCP_COMPATIBILITY.md defines the independent legacy/modern wire profiles; mcp-protocols.json, mcp-wire.schema.json and mcp-wire-examples.json are review fixtures only. The 13 business tools are shared; server/discover and initialize are protocol methods, not business tools. Modern complete results include resultType; legacy absence is normalized only internally. client-config explicitly selects a mode, and capabilities labels support as design-only.

FILESYSTEM_OPERATIONS.md defines trusted anchors, handle-relative no-reparse access and exclusive/no-replace publication. filesystem-operation.schema.json records intent, never live handle authority; filesystem-policy.json is a target capability checklist. Existing absolute_path is display-only. Single-plan approval requires exact duplicate-free sets and elevation equality; no implicit broad preauthorization. Caller-provided proof booleans have no authentication meaning.

Wire example case.request is the JSON-RPC body, case.headers/http_method are transport inputs, and expected is an oracle decision rather than a wire response. expected.http_status applies only to HTTP; stdio uses the JSON-RPC error code without an HTTP status. Separate results examples illustrate legacy and modern response bodies.

A1 round 3: mcp-response.schema.json validates outgoing results against unchanged selected official definitions under upstream/ (pinned commit/source digests and original upstream licensing in provenance.json/UPSTREAM_LICENSE.txt), plus the product complete/zero-TTL/private profile. That third-party material retains its upstream terms; the preceding repository MIT statement does not relicense it. mcp-cache-policy.json disables HTTP/proxy/auth-context cache reuse; tools/call/ping are noncacheable, and optional prompts/resources are not advertised. mcp-fallback-cases.json is the structured parser/fail-closed decision fixture: unknown peers, generic errors and stale legacy hints cannot prove legacy; explicit legacy configuration is required. response_kind labels are not evidence. Run the full pytest suite, including test_pr3_a1_round3.py; no runtime support is claimed.

P0-SC aligns staging protection metadata and the synthetic loop with independent source/locator/Revision Payload ownership; it does not change MCP transport or approval/filesystem policies. See contracts/security and ADR-010/011 for scope and production encryption requirements.
