"""Fresh Profile 11 SQL checks; Windows behavior remains in native Rust tests."""
from pathlib import Path
import sqlite3

ROOT = Path(__file__).resolve().parents[1]
BASE = (ROOT / "crates/sqlite/migrations/0010_local_content.sql").read_text(encoding="utf-8")
EXT = (ROOT / "crates/sqlite/migrations/0011_object_recovery.sql").read_text(encoding="utf-8")


def test_profile11_extension_installs_and_does_not_alter_profile10_definition(tmp_path):
    with sqlite3.connect(tmp_path / "profile11.sqlite") as db:
        db.execute("PRAGMA foreign_keys=ON")
        db.executescript(BASE)
        base_objects = dict(db.execute("SELECT name,sql FROM sqlite_schema"))
        db.executescript(EXT)
        assert db.execute("PRAGMA user_version").fetchone() == (11,)
        current = dict(db.execute("SELECT name,sql FROM sqlite_schema"))
        assert all(current[name] == definition for name, definition in base_objects.items())
        assert db.execute("PRAGMA integrity_check").fetchone() == ("ok",)
        assert db.execute("PRAGMA foreign_key_check").fetchall() == []
        assert {"object_quarantine", "deletion_intent", "deletion_work", "deletion_receipt",
                "backup_registration", "backup_receipt", "copy_manifest_b3", "content_outbox_v2"} <= current.keys()


def test_profile11_foreign_keys_bind_every_work_and_receipt(tmp_path):
    with sqlite3.connect(tmp_path / "relations.sqlite") as db:
        db.executescript(BASE + EXT)
        def refs(table):
            return {row[2] for row in db.execute(f"PRAGMA foreign_key_list({table})")}
        assert refs("object_quarantine") == {"write_intent"}
        assert refs("deletion_member") == {"deletion_intent", "write_intent"}
        assert refs("deletion_receipt") == {"deletion_observation"}
        assert refs("backup_receipt") == {"backup_registration"}
        assert db.execute("SELECT count(*) FROM content_outbox_v2").fetchone() == (0,)


def test_profile10_still_has_its_original_version_and_no_b3_tables(tmp_path):
    with sqlite3.connect(tmp_path / "original.sqlite") as db:
        db.executescript(BASE)
        assert db.execute("PRAGMA user_version").fetchone() == (10,)
        assert db.execute("SELECT count(*) FROM sqlite_schema WHERE name='object_quarantine'").fetchone() == (0,)
