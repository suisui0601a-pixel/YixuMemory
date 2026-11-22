use super::*;
use std::sync::Mutex;
pub(super) static SERIAL: Mutex<()> = Mutex::new(());
fn identity() -> Identity {
    Identity {
        operation: fresh().unwrap(),
        connection: fresh().unwrap(),
        incarnation: fresh().unwrap(),
        attempt: 1,
    }
}
#[test]
fn real_pipe_saturation_timeout_cancel_and_terminal() {
    let _g = SERIAL.lock().unwrap();
    let id = identity();
    let mut s = Session::new(id, 4096).unwrap();
    assert!(s.noninheritable());
    s.submit().unwrap();
    println!("native submit pending={}", s.pending_at_submit);
    assert!(
        s.pending_at_submit,
        "4096 bytes and one-byte quota must demonstrate actual kernel pending, not a fake status"
    );
    assert!(s.poll(id, 20).unwrap().is_none());
    assert!(Session::new(identity(), 4096).is_err());
    let cancel = s.cancel(id).unwrap();
    assert!(cancel == 0 || cancel == 1168);
    let e = s
        .poll(id, 2000)
        .unwrap()
        .expect("native cancellation terminal required for A2");
    assert!(e.status == 995 || e.status == 0);
    println!("events={:?}", s.events);
    let mut op = Operation::new(id, 4096, 0).unwrap();
    op.submit().unwrap();
    op.cancel().unwrap();
    op.timeout().unwrap();
    let outcome = op.settle(e).unwrap();
    assert!(matches!(outcome, Outcome::Unknown | Outcome::Accepted));
}
#[test]
fn normal_completion_and_cancel_not_found_are_distinct() {
    let _g = SERIAL.lock().unwrap();
    let id = identity();
    let mut s = Session::new(id, 4096).unwrap();
    s.submit().unwrap();
    let mut n = 0;
    while n < 4096 {
        n += s.read(4096 - n).unwrap();
    }
    assert_eq!(n, 4096);
    let cancel = s.cancel(id).unwrap();
    assert!(cancel == 0 || cancel == 1168);
    let e = s.poll(id, 2000).unwrap().unwrap();
    assert_eq!(e.count, Count::Exact(4096));
    println!("events={:?}", s.events);
    assert!(s.submit().is_err());
    assert!(s.poll(id, 0).is_err());
}
#[test]
fn partial_read_disconnect_and_wrong_completion_identity() {
    let _g = SERIAL.lock().unwrap();
    let id = identity();
    let mut s = Session::new(id, 4096).unwrap();
    s.submit().unwrap();
    assert_eq!(s.read(17).unwrap(), 17);
    assert!(s.poll(identity(), 0).is_err());
    s.close_reader();
    let e = s
        .poll(id, 2000)
        .unwrap()
        .expect("peer-close must still be harvested");
    println!("events={:?}", s.events);
    assert!(matches!(
        e.count,
        Count::Range(0, 4096) | Count::Exact(4096)
    ));
}
#[test]
fn finite_repeated_requests_and_cancellation_completion_race() {
    let _g = SERIAL.lock().unwrap();
    for n in 0..12 {
        let id = identity();
        let mut s = Session::new(id, 4096).unwrap();
        s.submit().unwrap();
        if n % 2 == 0 {
            let _ = s.read(23).unwrap();
        }
        s.cancel(id).unwrap();
        let e = s.poll(id, 2000).unwrap().unwrap();
        assert!(e.status == 0 || e.status == 995);
        println!("round={n}, events={:?}", s.events);
        assert!(Session::new(identity(), 1).is_err());
        drop(s); // closes BOTH owned endpoints before next connection budget is released
    }
}

#[test]
fn duplicated_reader_keeps_output_pending_close_is_not_evidence() {
    let _g = SERIAL.lock().unwrap();
    let id = identity();
    let mut s = Session::new(id, 4096).unwrap();
    let duplicate = s.duplicate_reader_for_attack_test().unwrap();
    s.submit().unwrap();
    s.close_reader();
    assert!(s.poll(id, 20).unwrap().is_none());
    assert!(Session::new(identity(), 1).is_err());
    drop(duplicate);
    let evidence = s.poll(id, 2000).unwrap().unwrap();
    assert_eq!(evidence.status, 109);
    assert_eq!(evidence.count, Count::Range(0, 4096));
    println!("duplicate-handle events={:?}", s.events);
}

#[test]
fn actual_reader_thread_races_cancel_without_replay() {
    let _g = SERIAL.lock().unwrap();
    for round in 0..16 {
        let id = identity();
        let mut s = Session::new(id, 4096).unwrap();
        s.submit().unwrap();
        let mut reader = s.resources.reader.take().unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let other = barrier.clone();
        let thread = std::thread::spawn(move || {
            let mut bytes = [0u8; 4096];
            other.wait();
            let mut total = 0;
            while total < bytes.len() {
                match reader.read(&mut bytes[total..]) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => total += n,
                }
            }
            total
        });
        barrier.wait();
        if round % 2 == 0 {
            std::thread::yield_now();
        }
        s.cancel(id).unwrap();
        let e = s.poll(id, 2000).unwrap().unwrap();
        let status = e.status;
        assert!(status == 0 || status == 995);
        drop(s);
        let observed = thread.join().unwrap();
        println!("concurrent round={round}, terminal={status}, received={observed}");
    }
}

#[test]
fn separate_control_queue_closing_waits_for_real_native_terminal() {
    let _g = SERIAL.lock().unwrap();
    let id = identity();
    let mut s = Session::new(id, 4096).unwrap();
    s.submit().unwrap();
    let (tx, rx) = std::sync::mpsc::sync_channel(16);
    let sender = std::thread::spawn(move || {
        for n in 0..16 {
            tx.try_send(n).unwrap();
        }
        assert!(tx.try_send(17).is_err());
    });
    sender.join().unwrap();
    let mut op = Operation::new(id, 4096, 0).unwrap();
    op.submit().unwrap();
    assert_eq!(op.pending_state(), PendingState::Unsettled);
    assert_eq!(rx.try_recv().unwrap(), 0);
    op.cancel().unwrap();
    s.cancel(id).unwrap();
    assert_eq!(op.pending_state(), PendingState::Unsettled);
    let e = s.poll(id, 2000).unwrap().unwrap();
    op.settle(e).unwrap();
    assert_eq!(op.pending_state(), PendingState::NoPending);
    println!(
        "control Closing -> {:?} -> EffectiveFence eligible",
        s.events
    );
}

#[test]
fn native_child_probe() {
    let Ok(root) = std::env::var("YIXU_W4A_CHILD_ROOT") else {
        return;
    };
    let anchor = winfs::Anchor::open(std::path::Path::new(&root)).unwrap();
    if std::env::var("YIXU_W4A_LOCK_PROBE").is_ok() {
        assert!(anchor.broker_lock().is_err());
        return;
    }
    let _lock = anchor.broker_lock().unwrap();
    let mut s = Session::new(identity(), 4096).unwrap();
    s.submit().unwrap();
    assert!(s.pending_at_submit);
    let read = s.read(17).unwrap();
    assert_eq!(read, 17);
    eprintln!("W4A-PENDING partial-read={read}");
    loop {
        std::thread::park();
    }
}

#[test]
fn process_kill_does_not_supply_terminal_or_recovery_admission() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
    };
    struct OwnChild(std::process::Child);
    impl Drop for OwnChild {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    let _g = SERIAL.lock().unwrap();
    let f = super::super::super::tests::Fixture::profile(9);
    let root = f._dir.path().join("cipher");
    let exe = std::env::current_exe().unwrap();
    let child = Command::new(&exe)
        .args([
            "--exact",
            "auth::persistence::output::native::tests::native_child_probe",
            "--nocapture",
        ])
        .env("YIXU_W4A_CHILD_ROOT", &root)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child = OwnChild(child);
    let mut msg = String::new();
    BufReader::new(child.0.stderr.take().unwrap())
        .read_line(&mut msg)
        .unwrap();
    assert!(msg.starts_with("W4A-PENDING"));
    let probe = Command::new(&exe)
        .args([
            "--exact",
            "auth::persistence::output::native::tests::native_child_probe",
            "--nocapture",
        ])
        .env("YIXU_W4A_CHILD_ROOT", &root)
        .env("YIXU_W4A_LOCK_PROBE", "1")
        .output()
        .unwrap();
    assert!(probe.status.success());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let state: String =
        f.db.connection
            .query_row("SELECT state FROM output_runtime", [], |r| r.get(0))
            .unwrap();
    assert_eq!(state, "RecoveryBlocked");
    assert!(winfs::Anchor::open(&root).unwrap().broker_lock().is_ok());
    // Lock release / killed PID is explicitly NOT converted into TerminalEvidence or Ready.
    println!(
        "actual child killed while IO_PENDING after 17 bytes read; lock now acquirable; recovery remains {state}"
    );
}
