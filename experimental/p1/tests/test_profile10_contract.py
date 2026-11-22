"""Profile 10 DDL contracts; Windows runtime behavior is tested by the Rust assembly."""

from pathlib import Path
import sqlite3


DDL = (
    Path(__file__).resolve().parents[1]
    / "crates/sqlite/migrations/0010_local_content.sql"
).read_text(encoding="utf-8")


def test_profile10_installs_its_complete_schema_in_an_isolated_database(tmp_path):
    with sqlite3.connect(tmp_path / "profile10-contract.sqlite") as connection:
        connection.execute("PRAGMA foreign_keys=ON")
        connection.executescript(DDL)
        assert connection.execute("PRAGMA application_id").fetchone() == (1498952246,)
        assert connection.execute("PRAGMA user_version").fetchone() == (10,)
        assert connection.execute("PRAGMA integrity_check").fetchone() == ("ok",)
        assert connection.execute("PRAGMA foreign_key_check").fetchall() == []
        tables = {
            row[0]
            for row in connection.execute(
                "SELECT name FROM sqlite_schema WHERE type='table'"
            )
        }
        assert {
            "runtime", "authority", "write_intent", "publication", "memory",
            "revision", "payload", "local_key", "source", "evidence",
            "revision_evidence", "receipt", "outbox", "security_event",
            "security_receipt", "r5_tombstone", "stop_receipt", "resume_receipt",
        } <= tables
        for table in tables:
            columns = {
                row[1]
                for row in connection.execute(
                    "SELECT * FROM pragma_table_info(?)", (table,)
                )
            }
            assert not {"body", "plaintext", "content", "title"} & columns


def test_profile10_ddl_contains_immutable_and_atomic_commit_guards():
    with sqlite3.connect(":memory:") as connection:
        connection.executescript(DDL)
        triggers = {
            row[0]
            for row in connection.execute(
                "SELECT name FROM sqlite_schema WHERE type='trigger'"
            )
        }
        assert {
            "receipt_atomic", "security_receipt_fenced", "revision_fenced",
            "authority_ordered", "intent_progress", "commit_requires_graph",
            "seal_requires_receipt", "tombstone_fenced",
            "immutable_revision_UPDATE", "immutable_revision_DELETE",
            "immutable_stop_receipt_UPDATE", "immutable_resume_receipt_DELETE",
        } <= triggers
        references = connection.execute(
            "SELECT * FROM pragma_foreign_key_list('revision')"
        ).fetchall()
        assert {"memory", "payload", "revision_evidence"} <= {
            row[2] for row in references
        }
