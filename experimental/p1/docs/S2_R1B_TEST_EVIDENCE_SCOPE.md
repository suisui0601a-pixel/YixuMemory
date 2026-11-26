# S2-R1b table-row evidence scope

2026-11-26. Follow-up to S2R1B-001 in the independent review of
`2c52031dff4b3e43aca43999179dda8af6449e18`.

## Acceptance decision

The snapshot-completeness task requires the failed-operation assertions to cover
the data rows of every table actually used by Profile 12. The R2 acceptance matrix
also distinguishes before/after SQLite row state from schema attestation and
external file identity evidence. It does not require this test helper to prove
that every aspect of a SQLite database is unchanged.

S2R1B-001 is therefore a limitation of this evidence, not a missing row-data
check. Describing these assertions as a complete database-state comparison would
be incorrect. This follow-up makes the narrower scope explicit in names, comments
and diagnostic output; it does not add a schema/PRAGMA snapshot or change the
Application, database format, authorization or deletion protocol.

## What the helper proves

`Case::table_rows_snapshot()` in `profile12/application/tests.rs` enumerates all
31 tables created by the P12 base and extension migrations. For each table it
reads `quote(rowid), *`, ordered by rowid, and compares typed SQLite values.

Equality means the enumerated tables have equal row values and rowids at the
two observations. The fixture must be quiescent while the per-table queries run;
this helper is not a concurrent database snapshot protocol.

The table-coverage test separately compares the names against all actual
`main.sqlite_schema` table objects, checks uniqueness, and checks stable rows.
This is a **table-name coverage check**, not schema-definition equality.

The three sensitivity tests temporarily modify one row of `publication`,
`object_quarantine` or `profile12_format` in an isolated transaction. They verify
that only that table's row snapshot changes, explicitly roll back, compare rows
again, check exact restoration of the selected immutable trigger, and run
attestation and foreign-key checks. Those are separate assertions, not additional
fields implicitly covered by the row helper.

## What it does not prove

- Complete schema equality: table definitions, constraints, indexes, triggers,
  views, or other schema objects are not compared by the row helper.
- PRAGMA/header equality, TEMP or attached namespaces, physical DB pages,
  WAL/SHM, or external ciphertext/key/file state are not compared.
- A row-only equality assertion does not establish authorization correctness,
  native I/O completion, production recovery, or absence of all side effects.

P12 attestation has its own trusted schema-definition comparison, configuration,
format and integrity checks. Existing schema tests exercise it. Neither that
mechanism nor a successful run of those tests means every failure-path row
assertion also compared all schema objects and PRAGMAs before and after.

If a future task requires complete database-state invariance, it must define the
exact schema objects, per-connection settings, persistent headers and external
artifacts to observe, including their concurrency and transaction boundaries.
That would be a separate acceptance extension, not an inferred guarantee here.

## Review and runtime evidence

The independent review of `2c52031` is CONDITIONAL PASS for table-row evidence and
records Rust execution as NOT TESTED in the reviewer's environment. Local reruns
by the implementer are self-validation; they cannot replace independent Rust
execution or silently upgrade that review conclusion. The original report and
its hash are retained with the local follow-up evidence.

This clarification does not grant S2 final approval or permission for S3, S4 or
S5. Production content remains closed and NoContentRuntime remains fail-closed.
Native output, A3/R12 and Windows service experiments remain outside this task.
