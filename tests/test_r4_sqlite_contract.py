"""Build-free verification for the actual R4 synthetic SQLite schema."""

import sqlite3
import subprocess
import sys
from contextlib import closing
from pathlib import Path

import pytest

SCHEMA = (
    Path(__file__).resolve().parents[1] / "crates/ipc/src/tests/r4/synthetic.sql"
).read_text(encoding="utf-8")
SCOPE = bytes(range(1, 49))
PRINCIPAL = b"p" * 16
TARGET = b"t" * 16
REQUEST = b"r" * 16
BINDING = b"b" * 32
COMPLETION = (1, 1, BINDING, "SyntheticAccepted")


def init(db):
    db.executescript("PRAGMA foreign_keys=ON; PRAGMA recursive_triggers=ON;" + SCHEMA)
    db.execute("INSERT INTO runtime(id,profile,incarnation) VALUES(1,?,?)",
               ("R4-SYNTHETIC-1", b"i" * 16))
    db.execute("INSERT INTO authority(scope,principal,target,epoch,expiry) VALUES(?,?,?,7,100)",
               (SCOPE, PRINCIPAL, TARGET))
    db.commit()


def arm(db, operation=1, request=REQUEST):
    db.execute(
        "INSERT INTO release_intent(operation,scope,principal,target,request,binding) VALUES(?,?,?,?,?,?)",
        (operation, SCOPE, PRINCIPAL, TARGET, request, BINDING),
    )


def counts(db):
    return db.execute(
        "SELECT (SELECT count(*) FROM release_intent),"
        "(SELECT count(*) FROM observation),(SELECT count(*) FROM receipt),"
        "(SELECT count(*) FROM audit)"
    ).fetchone()


def test_intent_before_executor_and_one_durable_active_slot():
    with closing(sqlite3.connect(":memory:")) as db:
        init(db)
        arm(db)
        with pytest.raises(sqlite3.IntegrityError):
            arm(db, 2, b"2" * 16)
        with pytest.raises(sqlite3.IntegrityError):
            db.execute("UPDATE release_intent SET state='Settled'")
        with pytest.raises(sqlite3.IntegrityError):
            db.execute("UPDATE release_intent SET binding=?", (bytes(32),))
        assert counts(db) == (1, 0, 0, 1)


def test_receipt_requires_exact_matching_observation_and_is_immutable():
    with closing(sqlite3.connect(":memory:")) as db:
        init(db)
        arm(db)
        with pytest.raises(sqlite3.IntegrityError):
            db.execute("INSERT INTO receipt VALUES(?,?,?,?)", COMPLETION)
        for attempt, binding in [(2, BINDING), (1, bytes(32))]:
            with pytest.raises(sqlite3.IntegrityError):
                db.execute("INSERT INTO observation VALUES(?,?,?,?)",
                           (1, attempt, binding, COMPLETION[-1]))
        db.execute("INSERT INTO observation VALUES(?,?,?,?)", COMPLETION)
        assert counts(db) == (1, 1, 0, 2)
        with pytest.raises(sqlite3.IntegrityError):
            db.execute("INSERT INTO receipt VALUES(?,?,?,?)",
                       (*COMPLETION[:-1], "SyntheticCancelled"))
        assert db.execute("SELECT state FROM release_intent").fetchone() == ("Armed",)
        db.execute("INSERT INTO receipt VALUES(?,?,?,?)", COMPLETION)
        assert counts(db) == (1, 1, 1, 3)
        for sql in ["DELETE FROM receipt", "UPDATE receipt SET attempt=2",
                    "INSERT OR REPLACE INTO receipt SELECT * FROM receipt",
                    "DELETE FROM observation", "DELETE FROM release_intent", "DELETE FROM audit"]:
            with pytest.raises(sqlite3.IntegrityError):
                db.execute(sql)
        assert db.execute("SELECT state FROM release_intent").fetchone() == ("Settled",)
        arm(db, 2, b"2" * 16)
        assert counts(db) == (2, 1, 1, 4)


def test_rollback_and_reopen_leave_observation_without_receipt(tmp_path):
    path = tmp_path / "r4-synthetic.sqlite"
    with closing(sqlite3.connect(path)) as db:
        init(db)
        arm(db)
        db.rollback()
        assert counts(db) == (0, 0, 0, 0)
        arm(db)
        db.execute("INSERT INTO observation VALUES(?,?,?,?)", COMPLETION)
        db.commit()
        db.execute("CREATE TEMP TRIGGER fail BEFORE INSERT ON audit WHEN NEW.event='Settled'"
                   " BEGIN SELECT RAISE(ABORT,'fault'); END")
        with pytest.raises(sqlite3.IntegrityError):
            db.execute("INSERT INTO receipt VALUES(?,?,?,?)", COMPLETION)
        db.rollback()
    with closing(sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)) as db:
        assert counts(db) == (1, 1, 0, 2)
        assert db.execute("SELECT state FROM release_intent").fetchone() == ("Armed",)
        with pytest.raises(sqlite3.OperationalError):
            db.execute("INSERT INTO receipt VALUES(?,?,?,?)", COMPLETION)


def test_closing_cannot_advance_unknown_or_overtake_another_fence():
    with closing(sqlite3.connect(":memory:")) as db:
        init(db)
        arm(db)
        db.execute("UPDATE release_intent SET state='Unknown'")
        for name in ("Delete", "Revocation"):
            db.execute("INSERT INTO security_event(kind,scope) VALUES(?,?)", (name, SCOPE))
        with pytest.raises(sqlite3.IntegrityError):
            db.execute("UPDATE security_event SET state='EffectiveFence' WHERE id=1")
        db.execute("INSERT INTO observation VALUES(?,?,?,?)", COMPLETION)
        db.execute("INSERT INTO receipt VALUES(?,?,?,?)", COMPLETION)
        with pytest.raises(sqlite3.IntegrityError):
            db.execute("UPDATE security_event SET state='EffectiveFence' WHERE id=2")
        db.execute("UPDATE security_event SET state='EffectiveFence' WHERE id=1")
        with pytest.raises(sqlite3.IntegrityError):
            arm(db, 2, b"2" * 16)
        db.execute("UPDATE security_event SET state='Applied' WHERE id=1")
        with pytest.raises(sqlite3.IntegrityError):
            arm(db, 2, b"2" * 16)
        db.execute("UPDATE security_event SET state='EffectiveFence' WHERE id=2")
        db.execute("UPDATE security_event SET state='Applied' WHERE id=2")
        arm(db, 2, b"2" * 16)


def test_persistent_block_never_admits_new_work_or_fence():
    with closing(sqlite3.connect(":memory:")) as db:
        init(db)
        db.execute("UPDATE runtime SET blocked=1")
        with pytest.raises(sqlite3.IntegrityError):
            arm(db)
        with pytest.raises(sqlite3.IntegrityError):
            db.execute("UPDATE runtime SET blocked=0")
        with pytest.raises(sqlite3.IntegrityError):
            db.execute("INSERT INTO security_event(kind,scope) VALUES('Grant',?)", (SCOPE,))
        assert counts(db) == (0, 0, 0, 0)


@pytest.mark.parametrize(("stage", "expected"), [
    ("before_arm_commit", (0, 0, 0, 0)),
    ("armed", (1, 0, 0, 1)),
    ("observed", (1, 1, 0, 2)),
    ("settled", (1, 1, 1, 3)),
])
def test_owned_process_loss_at_durable_boundaries(tmp_path, stage, expected):
    path = tmp_path / "r4-process.sqlite"
    with closing(sqlite3.connect(path)) as db:
        init(db)
        db.executescript(
            "CREATE TABLE knowledge_seq(value INTEGER); INSERT INTO knowledge_seq VALUES(7);"
            "CREATE TABLE business_write(id INTEGER); CREATE TABLE outbox(id INTEGER);"
        )
    program = """
import os
import sqlite3
import sys
db = sqlite3.connect(sys.argv[1])
stage = sys.argv[2]
db.executescript("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")
db.execute(
    "INSERT INTO release_intent(operation,scope,principal,target,request,binding) "
    "SELECT 1,scope,principal,target,?,? FROM authority",
    (b"r" * 16, b"b" * 32),
)
if stage != "before_arm_commit":
    db.commit()
if stage in ("observed", "settled"):
    db.execute("INSERT INTO observation VALUES(1,1,?,'SyntheticAccepted')", (b"b" * 32,))
    db.commit()
if stage == "settled":
    db.execute("INSERT INTO receipt VALUES(1,1,?,'SyntheticAccepted')", (b"b" * 32,))
    db.commit()
os._exit(77)
"""
    child = subprocess.run([sys.executable, "-c", program, str(path), stage],
                           check=False, capture_output=True, timeout=15)
    assert child.returncode == 77, child.stderr
    # Opening with SQLite permits journal recovery, not Broker admission.
    with closing(sqlite3.connect(path)) as db:
        assert counts(db) == expected
        assert db.execute("SELECT value FROM knowledge_seq").fetchone() == (7,)
        assert db.execute("SELECT count(*) FROM business_write").fetchone() == (0,)
        assert db.execute("SELECT count(*) FROM outbox").fetchone() == (0,)
        assert db.execute("PRAGMA integrity_check").fetchone() == ("ok",)
