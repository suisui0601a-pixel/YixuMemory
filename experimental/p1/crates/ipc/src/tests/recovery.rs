use super::*;
use rusqlite::Connection as Sqlite;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn setup(db: &Sqlite) {
    db.execute_batch(
        "CREATE TABLE knowledge_seq(value INTEGER NOT NULL);
         INSERT INTO knowledge_seq VALUES(0);
         CREATE TABLE business_write(id INTEGER PRIMARY KEY);
         CREATE TABLE outbox(id INTEGER PRIMARY KEY);",
    )
    .unwrap();
}

fn counts(db: &Sqlite) -> (i64, i64, i64) {
    db.query_row(
        "SELECT (SELECT value FROM knowledge_seq),
                (SELECT count(*) FROM business_write), (SELECT count(*) FROM outbox)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

#[test]
fn rejected_requests_leave_sqlite_sentinel_unchanged() {
    let db = Sqlite::open_in_memory().unwrap();
    setup(&db);
    let (mut core, _) = fixture(INC);
    let (c, sid) = establish(&mut core, 11);
    for (i, invalid) in [
        request(
            sid,
            1,
            2,
            Scope {
                tenant: [23; 16],
                ..SCOPE
            },
            None,
        )
        .encode(),
        request(sid, 1, 3, SCOPE, None).encode(),
        vec![0; 5],
    ]
    .into_iter()
    .enumerate()
    {
        let (handle, current) = if i == 0 {
            (c, sid)
        } else {
            establish(&mut core, 12 + i as u8)
        };
        let mut invalid = invalid;
        if invalid.len() >= HEADER {
            invalid[8..24].copy_from_slice(&current);
        }
        assert!(core.receive(handle, &invalid, 1).is_err());
        assert_eq!(core.dispatch(1), Ok(None));
        assert_eq!(counts(&db), (0, 0, 0));
    }
}

const STAGE_ENV: &str = "YIXU_R3_FAULT_STAGE";
const DB_ENV: &str = "YIXU_R3_FAULT_DB";

fn park_for_kill() {
    println!("R3_FAULT_POINT_REACHED");
    std::io::stdout().flush().unwrap();
    let _ = std::io::stdin().read(&mut [0u8]);
}

#[test]
fn crash_child() {
    let Ok(stage) = std::env::var(STAGE_ENV) else {
        return;
    };
    let path = std::env::var_os(DB_ENV).expect("test-owned SQLite path");
    assert!(matches!(
        stage.as_str(),
        "handshake" | "before_submit" | "after_submit" | "after_commit"
    ));
    // Refuse an existing target, including an accidentally supplied application DB.
    std::fs::File::create_new(&path).unwrap();
    let mut db = Sqlite::open(path).unwrap();
    setup(&db);
    let (mut core, _) = fixture(INC);
    if stage == "handshake" {
        let c = core.connect([11; 32], 1).unwrap();
        challenge(&mut core, c, &initial_hello());
    } else {
        let (c, sid) = establish(&mut core, 11);
        assert_eq!(
            send(&mut core, c, &request(sid, 1, 2, SCOPE, None), 1),
            Ok(Outcome::Queued)
        );
        let _scheduled = core.dispatch(1).unwrap().unwrap();
        if matches!(stage.as_str(), "after_submit" | "after_commit") {
            // Independent SQLite test sink models submission and durability before the reply.
            // The IPC crate has no production application dispatcher.
            let tx = db.transaction().unwrap();
            tx.execute_batch(
                "UPDATE knowledge_seq SET value=1;
                 INSERT INTO business_write VALUES(1);
                 INSERT INTO outbox VALUES(1);",
            )
            .unwrap();
            if stage == "after_submit" {
                park_for_kill();
                return;
            }
            tx.commit().unwrap();
        }
    }
    park_for_kill();
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn process_loss_never_resumes_session_or_replays_committed_work() {
    for stage in ["handshake", "before_submit", "after_submit", "after_commit"] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-r3.sqlite");
        let mut child = OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "tests::recovery::crash_child", "--nocapture"])
                .env(STAGE_ENV, stage)
                .env(DB_ENV, &path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = child.0.stdout.take().unwrap();
        let (seen, wait) = mpsc::sync_channel(1);
        let observer = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if line.unwrap().contains("R3_FAULT_POINT_REACHED") {
                    seen.send(()).unwrap();
                    break;
                }
            }
        });
        wait.recv_timeout(Duration::from_secs(15))
            .expect("child reached fault point");
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        observer.join().unwrap();
        let db =
            Sqlite::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let wanted = if stage == "after_commit" {
            (1, 1, 1)
        } else {
            (0, 0, 0)
        };
        assert_eq!(counts(&db), wanted);

        let (mut former, _) = fixture(INC);
        let old_conn = former.connect([11; 32], 1).unwrap();
        let h = initial_hello();
        let old_challenge = challenge(&mut former, old_conn, &h);
        let old_proof = proof(&former, &h, &old_challenge, [11; 32]);
        let old_request = request(old_challenge.session, 1, 2, SCOPE, None);
        let (mut restarted, _) = fixture([81; 16]);
        let new_conn = restarted.connect([11; 32], 1).unwrap();
        let new_challenge = challenge(&mut restarted, new_conn, &h);
        assert_eq!(
            send(&mut restarted, old_conn, &old_request, 1),
            Err(Error::Binding)
        );
        let mut rebound = old_proof;
        rebound.session = new_challenge.session;
        assert_eq!(
            send(&mut restarted, new_conn, &rebound, 1),
            Err(Error::Denied)
        );
        assert_eq!(restarted.dispatch(1), Ok(None));
        assert_eq!(counts(&db), wanted);
    }
}
