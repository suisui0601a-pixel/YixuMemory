use super::super::tests::{Fixture, TestClock};
use super::*;
fn id(n: u8) -> Id {
    Id::from_bytes([n; 16])
}
fn identity() -> Identity {
    Identity {
        operation: id(10),
        connection: id(11),
        incarnation: id(12),
        attempt: 1,
    }
}
#[test]
fn cancel_timeout_are_not_terminal_and_identity_is_exact() {
    let mut op = Operation::new(identity(), 4096, 512).unwrap();
    op.submit().unwrap();
    op.cancel().unwrap();
    op.timeout().unwrap();
    assert_eq!(op.phase, Phase::Stuck);
    for n in 0..4 {
        let mut wrong = identity();
        match n {
            0 => wrong.operation = id(80),
            1 => wrong.connection = id(80),
            2 => wrong.incarnation = id(80),
            _ => wrong.attempt = 2,
        };
        assert!(
            op.settle(TerminalEvidence {
                identity: wrong,
                count: Count::Exact(0),
                status: 995
            })
            .is_err()
        );
        assert_eq!(op.phase, Phase::Stuck);
    }
    assert_eq!(
        op.settle(TerminalEvidence {
            identity: identity(),
            count: Count::Range(0, 4096),
            status: 995
        })
        .unwrap(),
        Outcome::Unknown
    );
    assert_eq!(op.phase, Phase::Terminal);
    assert!(op.submit().is_err());
    assert!(op.cancel().is_err());
}
#[test]
fn exact_and_range_never_fabricate_zero_or_allow_replay() {
    for (count, want) in [
        (Count::Exact(0), Outcome::Failed),
        (Count::Exact(1), Outcome::Partial),
        (Count::Exact(4), Outcome::Accepted),
        (Count::Range(0, 4), Outcome::Unknown),
    ] {
        let mut op = Operation::new(identity(), 4, 0).unwrap();
        op.submit().unwrap();
        assert_eq!(
            op.settle(TerminalEvidence {
                identity: identity(),
                count,
                status: 0
            })
            .unwrap(),
            want
        );
        assert!(
            op.settle(TerminalEvidence {
                identity: identity(),
                count,
                status: 0
            })
            .is_err()
        );
    }
    assert!(Operation::new(identity(), 4097, 0).is_err());
    assert!(Operation::new(identity(), 4, 513).is_err());
}
fn seed(f: &Fixture) {
    f.db.connection.execute("INSERT INTO release_intent VALUES(?1,?2,?3,?4,?5,zeroblob(32),?6,zeroblob(163),1,1,'synthetic.read',?7,500,?8,0,'Pending',0)",params![id(20).to_string(),id(1).to_string(),id(3).to_string(),id(2).to_string(),id(21).to_string(),id(22).to_string(),id(23).to_string(),id(12).to_string()]).unwrap();
    f.db.connection
        .execute(
            "INSERT INTO connection_binding VALUES(?1,?2,?3,?4,?5,?6,0)",
            params![
                id(11).to_string(),
                id(1).to_string(),
                id(3).to_string(),
                id(2).to_string(),
                id(12).to_string(),
                id(23).to_string()
            ],
        )
        .unwrap();
    f.db.connection
        .execute(
            "INSERT INTO output_operation VALUES(?1,?2,?3,?4,?5,?6,?7,1,4096,512,'Prepared')",
            params![
                id(10).to_string(),
                id(1).to_string(),
                id(3).to_string(),
                id(2).to_string(),
                id(20).to_string(),
                id(11).to_string(),
                id(12).to_string()
            ],
        )
        .unwrap();
}
#[test]
fn profile9_actual_schema_blocks_forged_terminal_and_early_effective() {
    let f = Fixture::profile(9);
    seed(&f);
    let c = &f.db.connection;
    let state: String = c
        .query_row("SELECT state FROM output_runtime", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "RecoveryBlocked");
    assert!(
        c.execute("UPDATE output_operation SET phase='Submitted'", [])
            .is_err()
    );
    // Direct SQL fixture setup ONLY to exercise storage constraints, never a runtime issuer.
    c.execute(
        "UPDATE output_runtime SET state='Ready',incarnation=?1",
        [id(12).to_string()],
    )
    .unwrap();
    c.execute(
        "INSERT INTO security_mutation VALUES('pre','Grant','Closing',?1)",
        [id(12).to_string()],
    )
    .unwrap();
    assert!(
        c.execute("UPDATE output_operation SET phase='Submitted'", [])
            .is_err()
    );
    c.execute(
        "UPDATE security_mutation SET state='EffectiveFence' WHERE id='pre'",
        [],
    )
    .unwrap();
    c.execute(
        "UPDATE security_mutation SET state='Applied' WHERE id='pre'",
        [],
    )
    .unwrap();
    c.execute("UPDATE output_operation SET phase='Submitted'", [])
        .unwrap();
    for kind in [
        "Grant",
        "Principal",
        "Space",
        "Revision",
        "Source",
        "Key",
        "Deletion",
        "Copy",
    ] {
        c.execute(
            "INSERT INTO security_mutation VALUES(?1,?1,'Closing',?2)",
            params![kind, id(12).to_string()],
        )
        .unwrap();
        assert!(
            c.execute(
                "UPDATE security_mutation SET state='EffectiveFence' WHERE id=?1",
                [kind]
            )
            .is_err()
        );
    }
    c.execute("UPDATE output_operation SET phase='CancelRequested'", [])
        .unwrap();
    c.execute("UPDATE output_operation SET phase='Stuck'", [])
        .unwrap();
    assert!(
        c.execute("UPDATE output_operation SET phase='Terminal'", [])
            .is_err()
    );
    assert!(
        c.execute("UPDATE output_operation SET incarnation='old'", [])
            .is_err()
    );
    assert!(c.execute("INSERT INTO terminal_evidence VALUES('wrong','Cancelled','Range',0,4096,995,'NoPending')",[]).is_err());
    c.execute(
        "INSERT INTO terminal_evidence VALUES(?1,'Cancelled','Range',0,4096,995,'NoPending')",
        [id(10).to_string()],
    )
    .unwrap();
    c.execute("UPDATE output_operation SET phase='Terminal'", [])
        .unwrap();
    c.execute(
        "UPDATE security_mutation SET state='EffectiveFence' WHERE state='Closing'",
        [],
    )
    .unwrap();
    assert!(
        c.execute("UPDATE terminal_evidence SET accepted_max=0", [])
            .is_err()
    );
    assert!(
        c.execute("UPDATE output_operation SET phase='Prepared'", [])
            .is_err()
    );
    drop(f.db);
    assert!(AuthDatabase::open_profile(&f.path, TestClock::new(), 9).is_ok());
    assert!(AuthDatabase::open_profile(&f.path, TestClock::new(), 8).is_err());
}
#[test]
fn profile9_drift_rejects_and_does_not_auto_activate() {
    let f = Fixture::profile(9);
    seed(&f);
    f.db.connection
        .execute_batch("DROP TRIGGER evidence_valid")
        .unwrap();
    assert!(AuthDatabase::open_profile(&f.path, TestClock::new(), 9).is_err());
}

#[test]
fn sqlite_terminal_receipt_rolls_back_atomically_and_recovery_stays_blocked() {
    let f = Fixture::profile(9);
    seed(&f);
    let c = &f.db.connection;
    // Fresh synthetic storage fixture; no API admits or recovers a real output.
    c.execute(
        "UPDATE output_runtime SET state='Ready',incarnation=?1",
        [id(12).to_string()],
    )
    .unwrap();
    c.execute("UPDATE output_operation SET phase='Submitted'", [])
        .unwrap();
    c.execute_batch("BEGIN IMMEDIATE;").unwrap();
    c.execute(
        "INSERT INTO terminal_evidence VALUES(?1,'Error','Range',0,4096,109,'NoPending')",
        [id(10).to_string()],
    )
    .unwrap();
    c.execute("UPDATE output_operation SET phase='Terminal'", [])
        .unwrap();
    c.execute_batch("ROLLBACK;").unwrap();
    let count: i64 = c
        .query_row("SELECT count(*) FROM terminal_evidence", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let phase: String = c
        .query_row("SELECT phase FROM output_operation", [], |r| r.get(0))
        .unwrap();
    assert_eq!(phase, "Submitted");
    assert!(
        c.execute("UPDATE connection_binding SET closed=1", [])
            .is_err()
    );
    drop(f.db);
    let db = AuthDatabase::open_profile(&f.path, TestClock::new(), 9).unwrap();
    let state: String = db
        .connection
        .query_row("SELECT state FROM output_runtime", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "RecoveryBlocked");
}
