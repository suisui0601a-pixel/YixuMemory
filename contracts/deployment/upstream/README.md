# Official MCP schema selections

Source: https://github.com/modelcontextprotocol/modelcontextprotocol/tree/271ecc9accafdd9b83a3c869fa67c22953b2af80/schema

These files contain only the transitive $defs closure of the roots in provenance.json. All selected definition objects are unchanged. The wrapper $id/$comment and selection manifest are YixuMemory packaging, not a new official MCP release. Source-byte SHA256 and canonical selected-definition hashes are recorded for reproducibility; tests check local integrity without network access. Resolve local refs only.

Copyright and licensing: preserve the upstream UPSTREAM_LICENSE.txt verbatim, including its Apache-2.0/MIT transition notice and applicable texts. No root YixuMemory LICENSE is replaced, and these third-party definitions are not claimed to be exclusively YixuMemory-owned or uniformly relicensed as MIT. This is specification validation data, not a bundled runtime SDK or memory engine.

Regeneration: fetch each pinned source URL, verify source_sha256, traverse #/$defs refs from the declared roots, retain each selected object unchanged, and verify definitions_sha256 using sorted-key compact UTF-8 JSON. Review upstream changes in a separate PR. Full official files are not needed at test/runtime and are not fetched by CI.
