use super::super::tests::{Fixture, TestClock};
use super::*;
fn id(n: u8) -> Id {
    Id::from_bytes([n; 16])
}
fn setup() -> (
    Broker<TestClock>,
    tempfile::TempDir,
    AuthContext,
    BootstrapAuthority,
    Id,
) {
    let mut f = Fixture::profile(8);
    let ctx = f.ctx();
    let cmd = f.cmd(70);
    std::fs::create_dir(f._dir.path().join("keys")).unwrap();
    let v = LocalVault::new(f._dir.path().join("keys")).unwrap();
    f.db.persist_backend(&ctx, &cmd, &f.store, Stop::None, Some(&v))
        .unwrap();
    let b = Broker::start(f.db, f.store, v).unwrap();
    (b, f._dir, ctx, f.authority, cmd.key)
}
fn state(b: &Broker<TestClock>, i: Id) -> String {
    b.db.connection
        .query_row(
            "SELECT state FROM release_intent WHERE id=?1",
            [i.to_string()],
            |r| r.get(0),
        )
        .unwrap()
}
fn revoke(b: &mut Broker<TestClock>, a: &BootstrapAuthority, ctx: &AuthContext) {
    b.administer(
        a,
        AdminChange::MemoryGrant {
            scope: ctx.scope,
            principal: ctx.principal,
            capability: "memory.read",
            enabled: false,
            expires_at_ms: 1000,
        },
    )
    .unwrap();
}
#[test]
fn bounded_output_idempotency_partial_and_order() {
    let (mut b, _d, ctx, a, c) = setup();
    let i = b.request(&ctx, c, id(1), id(8), 500).unwrap();
    assert_eq!(i, b.request(&ctx, c, id(1), id(8), 500).unwrap());
    assert!(b.request(&ctx, c, id(1), id(9), 500).is_err());
    b.release(&ctx, i, id(8), Sink::Synthetic { limit: 4096 }, 0)
        .unwrap();
    assert_eq!(state(&b, i), "Accepted");
    assert!(
        b.release(&ctx, i, id(8), Sink::Synthetic { limit: 4096 }, 0)
            .is_err()
    );
    let j = b.request(&ctx, c, id(2), id(8), 500).unwrap();
    b.release(&ctx, j, id(8), Sink::Synthetic { limit: 2 }, 0)
        .unwrap();
    assert_eq!(state(&b, j), "Partial");
    revoke(&mut b, &a, &ctx);
    assert_eq!(
        &b.events[b.events.len() - 4..],
        ["Armed", "sink-accepted", "Partial", "revoke-commit"]
    );
    assert!(b.request(&ctx, c, id(3), id(8), 500).is_err());
}
#[test]
fn revoke_before_unwrap_denies_without_sink_acceptance() {
    let (mut b, _d, ctx, a, c) = setup();
    let i = b.request(&ctx, c, id(1), id(8), 500).unwrap();
    revoke(&mut b, &a, &ctx);
    assert!(
        b.release(&ctx, i, id(8), Sink::Synthetic { limit: 4096 }, 0)
            .is_err()
    );
    assert_eq!(state(&b, i), "Denied");
    assert!(!b.events.contains(&"sink-accepted"));
}
#[test]
fn armed_and_output_receipt_loss_do_not_replay_after_restart() {
    for stop in [1, 2] {
        let (mut b, d, ctx, a, c) = setup();
        let i = b.request(&ctx, c, id(1), id(8), 500).unwrap();
        let path = d.path().join("content.sqlite");
        let root = d.path().join("cipher");
        assert!(
            b.release(&ctx, i, id(8), Sink::Synthetic { limit: 4096 }, stop)
                .is_err()
        );
        assert_eq!(state(&b, i), "Armed");
        assert!(
            b.administer(
                &a,
                AdminChange::MemoryGrant {
                    scope: ctx.scope,
                    principal: ctx.principal,
                    capability: "memory.read",
                    enabled: false,
                    expires_at_ms: 1000
                }
            )
            .is_err()
        );
        drop(b);
        let mut b = Broker::start(
            AuthDatabase::open_profile(&path, TestClock::new(), 8).unwrap(),
            Store::new(root).unwrap(),
            LocalVault::new(d.path().join("keys")).unwrap(),
        )
        .unwrap();
        assert_eq!(state(&b, i), "Unknown");
        assert!(
            b.release(&ctx, i, id(8), Sink::Synthetic { limit: 4096 }, 0)
                .is_err()
        );
        assert!(!b.events.contains(&"sink-accepted"));
    }
}
#[test]
fn restart_pending_scope_destination_and_schema_tamper() {
    let (mut b, d, ctx, _a, c) = setup();
    let i = b.request(&ctx, c, id(1), id(8), 500).unwrap();
    assert!(
        b.release(&ctx, i, id(9), Sink::Synthetic { limit: 4096 }, 0)
            .is_err()
    );
    let mut wrong = ctx.clone();
    wrong.scope.space = id(55);
    assert!(
        b.release(&wrong, i, id(8), Sink::Synthetic { limit: 4096 }, 0)
            .is_err()
    );
    assert!(
        b.db.connection
            .execute("UPDATE release_intent SET state='Accepted'", [])
            .is_err()
    );
    assert!(
        b.db.connection
            .execute("UPDATE release_intent SET binding=zeroblob(163)", [])
            .is_err()
    );
    assert!(
        b.db.connection
            .execute("DELETE FROM release_intent", [])
            .is_err()
    );
    let p = d.path().join("content.sqlite");
    drop(b);
    let b = Broker::start(
        AuthDatabase::open_profile(&p, TestClock::new(), 8).unwrap(),
        Store::new(d.path().join("cipher")).unwrap(),
        LocalVault::new(d.path().join("keys")).unwrap(),
    )
    .unwrap();
    assert_eq!(state(&b, i), "Quarantined");
    b.db.connection
        .execute_batch("DROP TRIGGER release_transition")
        .unwrap();
    assert!(AuthDatabase::open_profile(&p, TestClock::new(), 8).is_err());
}
#[test]
fn same_payload_distinct_requests_and_fence_never_cleared() {
    let (mut b, _d, ctx, _a, c) = setup();
    for n in 1..4 {
        let i = b.request(&ctx, c, id(n), id(8), 500).unwrap();
        b.release(&ctx, i, id(8), Sink::Synthetic { limit: 4096 }, 0)
            .unwrap();
    }
    let i = b.request(&ctx, c, id(4), id(8), 500).unwrap();
    let fence = b.fence(&ctx).unwrap();
    assert_ne!(b.fence(&ctx).unwrap(), fence);
    assert!(
        b.release(&ctx, i, id(8), Sink::Synthetic { limit: 4096 }, 0)
            .is_err()
    );
    assert!(
        b.db.connection
            .execute("DELETE FROM release_fence WHERE id=?1", [fence.to_string()])
            .is_err()
    );
}
#[test]
fn missing_cipher_key_corruption_and_generation_never_output() {
    for failure in 0..4 {
        let (mut b, _d, ctx, a, c) = setup();
        let i = b.request(&ctx, c, id(1), id(8), 500).unwrap();
        let p = b.db.plan(&ctx, c).unwrap();
        let (pid, k, o, kind, _) = p.copies.last().unwrap();
        let bind = binding(&ctx, *pid, *k, *o, *kind);
        match failure {
            0 => {
                let bytes = b.store.read(*pid).unwrap();
                b.store.anchor.erase(&pid.to_string(), &bytes).unwrap();
            }
            1 => {
                let bytes = b.vault.anchor.read(&k.to_string()).unwrap();
                b.vault.anchor.erase(&k.to_string(), &bytes).unwrap();
            }
            2 => {
                b.revoke_key(&a, &bind).unwrap();
                let name = format!("revoked-{k}");
                let bytes = b.vault.anchor.read(&name).unwrap();
                b.vault.anchor.erase(&name, &bytes).unwrap();
            }
            _ => {
                let mut wrong = bind;
                wrong.seal_generation += 1;
                assert!(
                    b.db.local_key(&ctx, &wrong, &b.vault, "memory.read", false)
                        .is_err()
                );
                b.db.connection
                    .execute("UPDATE local_key_gate SET state='unknown'", [])
                    .unwrap();
            }
        }
        assert!(
            b.release(&ctx, i, id(8), Sink::Synthetic { limit: 4096 }, 0)
                .is_err()
        );
        assert!(!b.events.contains(&"sink-accepted"));
    }
}
#[test]
fn singleton_handle_refuses_second_connection_and_releases_on_exit() {
    let (b, d, _ctx, _a, _c) = setup();
    let a = winfs::Anchor::open(&d.path().join("cipher")).unwrap();
    assert!(a.broker_lock().is_err());
    drop(b);
    assert!(a.broker_lock().is_ok());
}
#[test]
fn broker_child() {
    let Ok(root) = std::env::var("YIXU_W3_CHILD") else {
        return;
    };
    let path = PathBuf::from(root);
    let anchor = winfs::Anchor::open(&path.join("cipher")).unwrap();
    if std::env::var("YIXU_W3_LOCK_ONLY").is_ok() {
        assert!(anchor.broker_lock().is_err());
        return;
    }
    let db = AuthDatabase::open_profile(&path.join("content.sqlite"), TestClock::new(), 8).unwrap();
    let ctx = db
        .identify(TrustedIdentity { subject: id(4) }, id(3))
        .unwrap();
    let mut b = Broker::start(
        db,
        Store::new(path.join("cipher")).unwrap(),
        LocalVault::new(path.join("keys")).unwrap(),
    )
    .unwrap();
    let i = b.request(&ctx, id(70), id(25), id(8), 500).unwrap();
    if std::env::var("YIXU_W3_COMMANDS").is_ok() {
        use std::io::BufRead;
        let a = BootstrapAuthority { realm: ctx.realm };
        for line in std::io::stdin().lock().lines() {
            match line.unwrap().as_str() {
                "R" => {
                    let _ = b.release(&ctx, i, id(8), Sink::InheritedPipe, 0);
                }
                "V" => revoke(&mut b, &a, &ctx),
                "Q" => break,
                _ => panic!("invalid synthetic command"),
            }
        }
        eprintln!("W3-EVENTS:{:?}", b.events);
    } else {
        let stop = std::env::var("YIXU_W3_STOP")
            .ok()
            .map(|v| v.parse().unwrap())
            .unwrap_or(0);
        b.release(&ctx, i, id(8), Sink::InheritedPipe, stop)
            .unwrap();
    }
}
#[test]
fn dedicated_child_real_dpapi_pipe_and_cross_process_singleton() {
    use std::process::Command;
    let (b, d, _ctx, _a, _c) = setup();
    let exe = std::env::current_exe().unwrap();
    let child = "auth::persistence::broker::tests::broker_child";
    let out = Command::new(&exe)
        .args(["--exact", child, "--nocapture"])
        .env("YIXU_W3_CHILD", d.path())
        .env("YIXU_W3_LOCK_ONLY", "1")
        .output()
        .unwrap();
    assert!(out.status.success());
    drop(b);
    let out = Command::new(&exe)
        .args(["--exact", child, "--nocapture"])
        .env("YIXU_W3_CHILD", d.path())
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(out.stdout.windows(CANARY.len()).any(|w| w == CANARY));
    let db =
        AuthDatabase::open_profile(&d.path().join("content.sqlite"), TestClock::new(), 8).unwrap();
    let n: i64 = db
        .connection
        .query_row(
            "SELECT count(*) FROM release_intent WHERE state='Accepted'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn real_process_kill_armed_and_output_window_are_unknown() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
    };
    for stop in [3, 4] {
        let (b, d, ctx, _a, _c) = setup();
        drop(b);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "auth::persistence::broker::tests::broker_child",
                "--nocapture",
            ])
            .env("YIXU_W3_CHILD", d.path())
            .env("YIXU_W3_STOP", stop.to_string())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stderr.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert!(line.starts_with("W3-"));
        child.kill().unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert_eq!(
            output.stdout.windows(CANARY.len()).any(|w| w == CANARY),
            stop == 4
        );
        let mut b = Broker::start(
            AuthDatabase::open_profile(&d.path().join("content.sqlite"), TestClock::new(), 8)
                .unwrap(),
            Store::new(d.path().join("cipher")).unwrap(),
            LocalVault::new(d.path().join("keys")).unwrap(),
        )
        .unwrap();
        let id: String =
            b.db.connection
                .query_row("SELECT id FROM release_intent", [], |r| r.get(0))
                .unwrap();
        let id = id.parse().unwrap();
        assert_eq!(state(&b, id), "Unknown");
        assert!(
            b.release(
                &ctx,
                id,
                id_from_destination(),
                Sink::Synthetic { limit: 4096 },
                0
            )
            .is_err()
        );
    }
}
fn id_from_destination() -> Id {
    id(8)
}
#[test]
fn real_process_command_queue_serializes_revoke_and_release() {
    use std::process::{Command, Stdio};
    for first in [true, false] {
        let (b, d, _ctx, _a, _c) = setup();
        drop(b);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "auth::persistence::broker::tests::broker_child",
                "--nocapture",
            ])
            .env("YIXU_W3_CHILD", d.path())
            .env("YIXU_W3_COMMANDS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(if first { b"V\nR\nQ\n" } else { b"R\nV\nR\nQ\n" })
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success());
        assert_eq!(
            out.stdout
                .windows(CANARY.len())
                .filter(|w| *w == CANARY)
                .count(),
            usize::from(!first)
        );
        let events = String::from_utf8(out.stderr).unwrap();
        if !first {
            assert!(events.find("sink-accepted").unwrap() < events.find("revoke-commit").unwrap());
        } else {
            assert!(!events.contains("sink-accepted"));
        }
    }
}

#[test]
fn revoke_during_real_unwrap_second_connection_is_denied() {
    let (mut b, d, ctx, a, c) = setup();
    let i = b.request(&ctx, c, id(1), id(8), 500).unwrap();
    let p = d.path().join("content.sqlite");
    let realm = a.realm;
    let scope = ctx.scope;
    b.vault.after_unprotect = Some(Box::new(move || {
        let mut db = AuthDatabase::open_profile(&p, TestClock::new(), 8).unwrap();
        db.administer(
            &BootstrapAuthority { realm },
            AdminChange::MemoryGrant {
                scope,
                principal: id(2),
                capability: "memory.read",
                enabled: false,
                expires_at_ms: 1000,
            },
        )
        .unwrap();
    }));
    assert!(
        b.release(&ctx, i, id(8), Sink::Synthetic { limit: 4096 }, 0)
            .is_err()
    );
    assert!(!b.events.contains(&"sink-accepted"));
}

#[test]
fn actual_receipt_commit_failure_after_output_blocks_actor() {
    let (mut b, _d, ctx, a, c) = setup();
    let i = b.request(&ctx, c, id(1), id(8), 500).unwrap();
    // Real SQLite trigger error, not a mocked receipt result. Test-only corruption injection.
    b.db.connection.execute_batch("CREATE TRIGGER inject_receipt_failure BEFORE UPDATE ON release_intent WHEN NEW.state='Accepted' BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(
        b.release(&ctx, i, id(8), Sink::Synthetic { limit: 4096 }, 0)
            .is_err()
    );
    assert_eq!(state(&b, i), "Armed");
    assert!(b.events.contains(&"sink-accepted"));
    assert!(
        b.administer(
            &a,
            AdminChange::MemoryGrant {
                scope: ctx.scope,
                principal: ctx.principal,
                capability: "memory.read",
                enabled: false,
                expires_at_ms: 1000
            }
        )
        .is_err()
    );
}

#[test]
fn expiry_failed_and_no_terminal_reactivation() {
    use std::sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    };
    struct MovingClock(Arc<AtomicI64>);
    impl Clock for MovingClock {
        fn utc_millis(&self) -> Result<i64> {
            Ok(self.0.load(Ordering::SeqCst))
        }
        fn monotonic_ticks(&self) -> u64 {
            0
        }
    }
    let (b, d, ctx, _a, c) = setup();
    drop(b);
    let now = Arc::new(AtomicI64::new(100));
    let mut b = Broker::start(
        AuthDatabase::open_profile(
            &d.path().join("content.sqlite"),
            MovingClock(now.clone()),
            8,
        )
        .unwrap(),
        Store::new(d.path().join("cipher")).unwrap(),
        LocalVault::new(d.path().join("keys")).unwrap(),
    )
    .unwrap();
    let i = b.request(&ctx, c, id(1), id(8), 500).unwrap();
    b.release(&ctx, i, id(8), Sink::Synthetic { limit: 0 }, 0)
        .unwrap();
    let j = b.request(&ctx, c, id(2), id(8), 500).unwrap();
    now.store(500, Ordering::SeqCst);
    assert!(
        b.release(&ctx, j, id(8), Sink::Synthetic { limit: 4096 }, 0)
            .is_err()
    );
    let states: Vec<String> =
        b.db.connection
            .prepare("SELECT state FROM release_intent ORDER BY request_id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
    assert_eq!(states, ["Failed", "Expired"]);
    assert!(
        b.db.connection
            .execute("UPDATE release_intent SET state='Pending',attempt=0", [])
            .is_err()
    );
}
