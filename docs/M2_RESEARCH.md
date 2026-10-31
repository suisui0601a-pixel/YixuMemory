# M2 source research — 2026-11-01

Fixed source commits, actual license-file blob hashes and inspected ranges are in M2_RESEARCH_SOURCES.json.
No surveyed repository source was copied, vendored or executed. The new HMAC test dependency is reviewed separately below. API license metadata is a hint; actual root
texts were inspected. These are engineering observations, not legal reuse clearance or upstream
security/performance certification. Marketing claims and repository popularity are not evidence.

## xtdb/xtdb

Commit: `b3d0a252c07e7573e49afa90b77daf78d5196d4a`. License observation: MPL-2.0.

QueryBasis separates snapshot token/time and current-time context; the resolver test compares real node as-of results with full resolution across valid/system coordinates.

Adopt independent knowledge and valid axes plus a reference-oracle test. Do not import its distributed SQL/storage stack or substitute system timestamps for tenant sequence.

Substantial database implementation; inspected tests, not executed or independently certified.

Inspected source:

- [core/src/main/kotlin/xtdb/api/query/QueryBasis.kt](https://github.com/xtdb/xtdb/blob/b3d0a252c07e7573e49afa90b77daf78d5196d4a/core/src/main/kotlin/xtdb/api/query/QueryBasis.kt)
- [core/src/test/kotlin/xtdb/operator/scan/AsOfResolverNodeTest.kt](https://github.com/xtdb/xtdb/blob/b3d0a252c07e7573e49afa90b77daf78d5196d4a/core/src/test/kotlin/xtdb/operator/scan/AsOfResolverNodeTest.kt)

## getzep/graphiti

Commit: `72b5c59c06607e09ea855be61b6ff5a3446fc951`. License observation: Apache-2.0.

EntityEdge has episode provenance and distinct created/expired/valid/invalid dates; retrieval combines scoped full-text/vector arms. A temporal test normalizes future-intent assertions to reference time.

Adopt explicit provenance and candidate-stage composition. Reject treating every future effective fact as valid at acceptance time; no LLM temporal inference in core.

Working graph adapters and tests, with model-dependent semantic behavior; no Yixu security equivalence.

Inspected source:

- [graphiti_core/edges.py](https://github.com/getzep/graphiti/blob/72b5c59c06607e09ea855be61b6ff5a3446fc951/graphiti_core/edges.py)
- [tests/utils/maintenance/test_temporal_edge_utils.py](https://github.com/getzep/graphiti/blob/72b5c59c06607e09ea855be61b6ff5a3446fc951/tests/utils/maintenance/test_temporal_edge_utils.py)
- [graphiti_core/search/search.py](https://github.com/getzep/graphiti/blob/72b5c59c06607e09ea855be61b6ff5a3446fc951/graphiti_core/search/search.py)

## Roenbaeck/positorium

Commit: `c4cf913f5682f17c8af06b8e252c7c3b25390443`. License observation: Apache-2.0 / MIT license files; reuse expression not legally assessed.

Constructs distinguish proposition identity, appearances, time, and assertion envelopes; the married-snapshot test asserts a nonzero result count.

Keep evidence/assertion distinct and test nonempty snapshots. Avoid expanding the Rust prototype into its proposition algebra or adopting numeric public identities.

Source calls internal APIs unstable/beta; production maturity not established by this inspection.

Inspected source:

- [src/construct.rs](https://github.com/Roenbaeck/positorium/blob/c4cf913f5682f17c8af06b8e252c7c3b25390443/src/construct.rs)
- [tests/single_married_snapshot.rs](https://github.com/Roenbaeck/positorium/blob/c4cf913f5682f17c8af06b8e252c7c3b25390443/tests/single_married_snapshot.rs)
- [src/datatype.rs](https://github.com/Roenbaeck/positorium/blob/c4cf913f5682f17c8af06b8e252c7c3b25390443/src/datatype.rs)

## terminusdb/terminusdb

Commit: `ba32d880664cf9ede2e34c7c20043dd60000ebdd`. License observation: Apache-2.0.

Document history resolves commit metadata and paged change history; transaction/validate commits validated objects in dependency order.

Borrow append-only lineage and explicit validation-before-commit. Commit graph history does not supply Yixu valid-time policy or erase guarantees.

Established codebase with explicit transaction machinery; no upstream tests run.

Inspected source:

- [src/core/document/history.pl](https://github.com/terminusdb/terminusdb/blob/ba32d880664cf9ede2e34c7c20043dd60000ebdd/src/core/document/history.pl)
- [src/core/transaction.pl](https://github.com/terminusdb/terminusdb/blob/ba32d880664cf9ede2e34c7c20043dd60000ebdd/src/core/transaction.pl)
- [src/core/transaction/validate.pl](https://github.com/terminusdb/terminusdb/blob/ba32d880664cf9ede2e34c7c20043dd60000ebdd/src/core/transaction/validate.pl)

## dolthub/dolt

Commit: `46d8805d8556599eb0a6472892e67e3bb1e0de03`. License observation: Apache-2.0.

HistoryTable exposes commit-linked historical rows with integration queries; merge.go refuses overwriting unresolved conflict sets.

Preserve unresolved conflicts and stable knowledge snapshots. Do not adopt Git-style branching/merge truth or a second versioned database.

Real SQL integration tests inspected, not run; VCS history is not automatically bitemporal fact history.

Inspected source:

- [go/libraries/doltcore/sqle/history_table.go](https://github.com/dolthub/dolt/blob/46d8805d8556599eb0a6472892e67e3bb1e0de03/go/libraries/doltcore/sqle/history_table.go)
- [go/libraries/doltcore/sqle/integration_test/history_table_test.go](https://github.com/dolthub/dolt/blob/46d8805d8556599eb0a6472892e67e3bb1e0de03/go/libraries/doltcore/sqle/integration_test/history_table_test.go)
- [go/libraries/doltcore/merge/merge.go](https://github.com/dolthub/dolt/blob/46d8805d8556599eb0a6472892e67e3bb1e0de03/go/libraries/doltcore/merge/merge.go)

## vectorize-io/hindsight

Commit: `bc5ea195a0fed1346ff171a5780bbb72a38c89c8`. License observation: MIT.

Schema separates event date, occurred range and mentioned time; fusion caps per-source candidates before combining semantic/BM25/graph/temporal ranks. The temporal-window test records the store boundary type.

Adopt explicit time dimensions and retrieval budgets. RRF/reranking must follow authorization; score is not truth. Its window transport test is not proof of storage temporal correctness.

Implemented backend/search pipeline and tests; selected store test is an adapter test, not database certification.

Inspected source:

- [hindsight-api-slim/hindsight_api/engine/search/fusion.py](https://github.com/vectorize-io/hindsight/blob/bc5ea195a0fed1346ff171a5780bbb72a38c89c8/hindsight-api-slim/hindsight_api/engine/search/fusion.py)
- [hindsight-api-slim/tests/test_full_recall_temporal_window.py](https://github.com/vectorize-io/hindsight/blob/bc5ea195a0fed1346ff171a5780bbb72a38c89c8/hindsight-api-slim/tests/test_full_recall_temporal_window.py)
- [hindsight-api-slim/hindsight_api/alembic/versions/5a366d414dce_initial_schema.py](https://github.com/vectorize-io/hindsight/blob/bc5ea195a0fed1346ff171a5780bbb72a38c89c8/hindsight-api-slim/hindsight_api/alembic/versions/5a366d414dce_initial_schema.py)

## mem0ai/mem0

Commit: `753496cf1435d76c3a711f3af51303f7fdcb6420`. License observation: Apache-2.0.

SQLite history migration copies intersecting columns transactionally; tests inspect history schema and stored rows. History includes old/new values.

Adopt real schema/rollback tests. Reject plaintext history values and automatic old-table cleanup/migration in the protected Yixu profile.

Concrete SQLite code and tests; incompatible plaintext/content boundary, not reused.

Inspected source:

- [mem0/memory/storage.py](https://github.com/mem0ai/mem0/blob/753496cf1435d76c3a711f3af51303f7fdcb6420/mem0/memory/storage.py)
- [tests/memory/test_storage.py](https://github.com/mem0ai/mem0/blob/753496cf1435d76c3a711f3af51303f7fdcb6420/tests/memory/test_storage.py)

## topoteretes/cognee

Commit: `525b1d1179c550cd5b5661bcbf8f00f57a676928`. License observation: Apache-2.0.

QueryInterval represents LLM-extracted temporal windows; hint tests distinguish explicit, inferred and missing-year dates.

Retain uncertainty and precision. Do not let extraction or context inference silently become core validity, identity or authorization.

Implemented extraction/retrieval utilities; inspected unit tests do not establish ACID or multi-tenant security.

Inspected source:

- [cognee/modules/retrieval/temporal_hybrid/models.py](https://github.com/topoteretes/cognee/blob/525b1d1179c550cd5b5661bcbf8f00f57a676928/cognee/modules/retrieval/temporal_hybrid/models.py)
- [cognee/tests/unit/modules/engine/test_temporal_hints.py](https://github.com/topoteretes/cognee/blob/525b1d1179c550cd5b5661bcbf8f00f57a676928/cognee/tests/unit/modules/engine/test_temporal_hints.py)

## langchain-ai/langmem

Commit: `839c3fa0fc2c118734611d5d86503e52fc3a3dd1`. License observation: MIT.

Knowledge tools route configurable namespaces through BaseStore; graph auth prefixes store namespaces with authenticated identity.

Keep one trusted identity/application layer. A namespace or model-selected tool argument is not a Grant, transaction boundary or deletion fence.

Framework-specific orchestration inspected; no selected persistent database implementation or upstream test execution.

Inspected source:

- [src/langmem/knowledge/tools.py](https://github.com/langchain-ai/langmem/blob/839c3fa0fc2c118734611d5d86503e52fc3a3dd1/src/langmem/knowledge/tools.py)
- [src/langmem/graphs/auth.py](https://github.com/langchain-ai/langmem/blob/839c3fa0fc2c118734611d5d86503e52fc3a3dd1/src/langmem/graphs/auth.py)

## letta-ai/letta-code

Commit: `59df5424838419e083de0691fa4d08524f348937`. License observation: Apache-2.0.

Memory repair claims are tied to Git conflict state and worker attempts; tests prevent repeated repair of unchanged states.

Borrow bounded retries and auditable attempts, not its Git-conflict repair as factual Conflict semantics. Agent repair cannot select truth.

Agent/runtime implementation with Git subprocess tests; materially different conflict domain.

Inspected source:

- [src/agent/memory-conflict-repair.ts](https://github.com/letta-ai/letta-code/blob/59df5424838419e083de0691fa4d08524f348937/src/agent/memory-conflict-repair.ts)
- [src/agent/memory-conflict-repair.test.ts](https://github.com/letta-ai/letta-code/blob/59df5424838419e083de0691fa4d08524f348937/src/agent/memory-conflict-repair.test.ts)

## cozodb/cozo

Commit: `481af058abac9444ea8c9c52c78f096ed4b5bfc4`. License observation: MPL-2.0.

SessionTx explicitly commits its store transaction; validity tests exercise assertion/retraction and time queries.

Use explicit validity and transactional tests. Do not replace tenant commit order, authentication or erasure with Datalog/storage semantics.

Implemented query/database engine; no performance or security claims transferred.

Inspected source:

- [cozo-core/src/runtime/transact.rs](https://github.com/cozodb/cozo/blob/481af058abac9444ea8c9c52c78f096ed4b5bfc4/cozo-core/src/runtime/transact.rs)
- [cozo-core/src/data/tests/validity.rs](https://github.com/cozodb/cozo/blob/481af058abac9444ea8c9c52c78f096ed4b5bfc4/cozo-core/src/data/tests/validity.rs)

## quickwit-oss/tantivy

Commit: `360a5ebbbb6fa493d06b777a3c6890fbb1afbe7b`. License observation: MIT.

BM25 uses document/term statistics; IndexWriter separates pending segments, commit opstamp, deletes and rollback, with inline tests.

Future index Port needs commit watermark, generation, rebuild and deletion acknowledgement. Persisted terms/statistics remain protected content even if stored fields are disabled.

Real Rust index implementation; introduces a separate commit domain, deliberately not added.

Inspected source:

- [src/query/bm25.rs](https://github.com/quickwit-oss/tantivy/blob/360a5ebbbb6fa493d06b777a3c6890fbb1afbe7b/src/query/bm25.rs)
- [src/indexer/index_writer.rs](https://github.com/quickwit-oss/tantivy/blob/360a5ebbbb6fa493d06b777a3c6890fbb1afbe7b/src/indexer/index_writer.rs)

## autumn-foundation/AletheiaDB

Commit: `e3ef79ff0f1497ffd567959f5653f0f6b8359c6f`. License observation: MIT / Apache-2.0 files; API metadata NOASSERTION.

Temporal types distinguish valid/transaction ranges; inspected regression traverses historical versions. Its nonempty-result assertion is conditional, so empty success can evade that assertion.

Borrow interval naming, not HLC as a replacement for tenant sequence. Add unconditional cardinality checks in Yixu tests; source comments are not proof of maturity.

Experimental/evolving implementation by this limited evidence; upstream test quality and claims not accepted wholesale.

Inspected source:

- [src/core/temporal.rs](https://github.com/autumn-foundation/AletheiaDB/blob/e3ef79ff0f1497ffd567959f5653f0f6b8359c6f/src/core/temporal.rs)
- [tests/regressions/temporal_query_visibility.rs](https://github.com/autumn-foundation/AletheiaDB/blob/e3ef79ff0f1497ffd567959f5653f0f6b8359c6f/tests/regressions/temporal_query_visibility.rs)

## basicmachines-co/basic-memory

Commit: `d7b16128351e8eb6570fe38f7b006d433f5e8d62`. License observation: AGPL-3.0 root text; no reuse.

SQLite query preparation explicitly scopes project predicates and addresses punctuation/CJK analysis. Scope tests require empty scopes to match nothing.

Borrow scope-first query shape and negative scope tests. Plaintext note/FTS architecture and license require separate review before any reuse; no source copied.

Concrete local-first repositories and tests; Yixu encrypted-content policy intentionally differs.

Inspected source:

- [src/basic_memory/repository/sqlite_search_query.py](https://github.com/basicmachines-co/basic-memory/blob/d7b16128351e8eb6570fe38f7b006d433f5e8d62/src/basic_memory/repository/sqlite_search_query.py)
- [tests/repository/test_search_scope.py](https://github.com/basicmachines-co/basic-memory/blob/d7b16128351e8eb6570fe38f7b006d433f5e8d62/tests/repository/test_search_scope.py)

## SQLite FTS5 and cryptography

[SQLite official FTS5 documentation](https://www.sqlite.org/fts5.html) describes contentless tables,
term/doclist indexes, BM25 and Unicode61/trigram tokenizers. Contentless hides stored source fields,
not indexed terms. M2 uses only an in-memory synthetic Chinese diagnostic to demonstrate this;
no production FTS tables, token terms, text query or index files are enabled.

[HMAC 0.13.0 upstream documentation](https://docs.rs/hmac/0.13.0/hmac/) supplies HMAC-SHA256 and
constant-time verify_slice. M2 uses that crate only in private test assembly for signed metadata
knowledge/cursor tokens. No custom MAC algorithm, production signing backend or Payload encryption
is introduced. Dependencies and actual lockfile changes must be audited.

## Decision table

| Area | Selected | Deliberately deferred |
|---|---|---|
| Knowledge snapshots | Tenant sequence, server MAC, current authorization/fences | Wall-clock as-of, HLC/distributed DB |
| Effective time | Explicit unknown or normalized uncertain bounds + precision | Natural-language/calendar parsing in core |
| Fact history | Immutable revision selection at seq, then valid_at | Automatic resurrection of an older revision |
| Conflict | Explicit open/resolved/open decisions retaining sources | LLM contradiction detection or winner inference |
| Query | Scoped authoritative metadata, bounded deterministic pages | Full-text/vector/graph engines and rank fusion |
| Index | Port capability and watermark/deletion contract | Persisted plaintext terms, untested CJK claims |
