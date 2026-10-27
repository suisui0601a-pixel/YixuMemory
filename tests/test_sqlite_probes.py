"""SQLite feature probes, deliberately not production schema or engine tests."""
from __future__ import annotations

import sqlite3
from pathlib import Path

import pytest

DDL = """
CREATE TABLE source(tenant TEXT, id TEXT, PRIMARY KEY(tenant,id));
CREATE TABLE memory(tenant TEXT, id TEXT, version INTEGER NOT NULL,
 PRIMARY KEY(tenant,id));
CREATE TABLE evidence(tenant TEXT, id TEXT, source_id TEXT,
 PRIMARY KEY(tenant,id), FOREIGN KEY(tenant,source_id) REFERENCES source(tenant,id));
CREATE TABLE outbox(tenant TEXT, id TEXT, PRIMARY KEY(tenant,id));
"""


def db(path: Path) -> sqlite3.Connection:
    connection = sqlite3.connect(path, isolation_level=None, timeout=0.05)
    connection.execute("PRAGMA foreign_keys=ON")
    connection.execute("PRAGMA journal_mode=WAL")
    return connection


def test_composite_fk_blocks_foreign_tenant(tmp_path: Path) -> None:
    with db(tmp_path / "probe.db") as c:
        c.executescript(DDL)
        c.execute("INSERT INTO source VALUES(?,?)", ("A", "source"))
        with pytest.raises(sqlite3.IntegrityError):
            c.execute("INSERT INTO evidence VALUES(?,?,?)", ("B", "e", "source"))


def test_rollback_removes_memory_and_outbox(tmp_path: Path) -> None:
    path = tmp_path / "probe.db"
    c = db(path)
    c.executescript(DDL)
    c.execute("BEGIN IMMEDIATE")
    c.execute("INSERT INTO memory VALUES(?,?,?)", ("A", "m", 1))
    c.execute("INSERT INTO outbox VALUES(?,?)", ("A", "job"))
    # Closing an uncommitted connection simulates abrupt transaction abandonment.
    c.close()
    with db(path) as recovered:
        assert recovered.execute("SELECT count(*) FROM memory").fetchone() == (0,)
        assert recovered.execute("SELECT count(*) FROM outbox").fetchone() == (0,)


def test_cas_and_writer_contention(tmp_path: Path) -> None:
    path = tmp_path / "probe.db"
    a, b = db(path), db(path)
    try:
        a.executescript(DDL)
        a.execute("INSERT INTO memory VALUES(?,?,?)", ("A", "m", 1))
        a.execute("BEGIN IMMEDIATE")
        with pytest.raises(sqlite3.OperationalError, match="locked"):
            b.execute("BEGIN IMMEDIATE")
        assert a.execute(
            "UPDATE memory SET version=version+1 WHERE tenant=? AND id=? AND version=?",
            ("A", "m", 1),
        ).rowcount == 1
        a.execute("COMMIT")
        assert b.execute(
            "UPDATE memory SET version=version+1 WHERE tenant=? AND id=? AND version=?",
            ("A", "m", 1),
        ).rowcount == 0
    finally:
        a.close()
        b.close()
