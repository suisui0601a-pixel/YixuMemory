//! A2-R1 owned-child experiments only. No recovery issuer or production API.
#![allow(unsafe_code)]
use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    process::{Child, ChildStderr, ChildStdin, Command, Stdio},
};
#[repr(C)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    id: u32,
    owner: u32,
    base: i32,
    delta: i32,
    flags: u32,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> H;
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> H;
    fn Thread32First(snapshot: H, entry: *mut ThreadEntry) -> i32;
    fn Thread32Next(snapshot: H, entry: *mut ThreadEntry) -> i32;
    fn OpenThread(access: u32, inherit: i32, id: u32) -> H;
    fn SuspendThread(thread: H) -> u32;
    fn ResumeThread(thread: H) -> u32;
    fn GetProcessIdOfThread(thread: H) -> u32;
    fn PeekNamedPipe(
        pipe: H,
        buffer: H,
        size: u32,
        read: *mut u32,
        available: *mut u32,
        left: *mut u32,
    ) -> i32;
}
const CHILD_TEST: &str =
    "auth::persistence::output::native::recovery_experiments::owned_child_probe";
fn identity() -> Identity {
    Identity {
        operation: fresh().unwrap(),
        connection: fresh().unwrap(),
        incarnation: fresh().unwrap(),
        attempt: 1,
    }
}
struct OwnedChild {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStderr>,
}
impl OwnedChild {
    fn spawn(root: &std::path::Path, mode: &str) -> Self {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", CHILD_TEST, "--nocapture"])
            .env("YIXU_R1_ROOT", root)
            .env("YIXU_R1_MODE", mode)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stderr.take().unwrap());
        Self {
            child,
            input,
            output,
        }
    }
    fn line(&mut self) -> String {
        let mut s = String::new();
        self.output.read_line(&mut s).unwrap();
        assert!(s.starts_with("R1 "), "unexpected owned child response");
        s.trim().to_string()
    }
    fn command(&mut self, cmd: &str) -> String {
        writeln!(self.input, "{cmd}").unwrap();
        self.input.flush().unwrap();
        self.line()
    }
    fn open(&self, access: u32) -> Handle {
        // SAFETY: only the PID obtained from this still-owned Child is ever targeted.
        let h = unsafe { OpenProcess(access, 0, self.child.id()) };
        assert!(!h.is_null(), "OpenProcess own child: {}", unsafe {
            GetLastError()
        });
        println!(
            "OpenProcess owned pid={} access={access:#x} success",
            self.child.id()
        );
        Handle(h)
    }
    fn duplicate(&self, process: &Handle, remote: usize) -> std::io::Result<Handle> {
        let mut target = std::ptr::null_mut();
        // SAFETY: remote handle number came over the private pipe from this owned child.
        let ok = unsafe {
            DuplicateHandle(
                process.0,
                remote as H,
                GetCurrentProcess(),
                &mut target,
                0,
                0,
                2,
            )
        };
        if ok == 0 {
            let e = failure();
            println!("DuplicateHandle owned child error={:?}", e.raw_os_error());
            return Err(e);
        }
        println!("DuplicateHandle owned child -> parent success");
        Ok(Handle(target))
    }
    fn kill_wait(&mut self) {
        self.child.kill().unwrap();
        let exit = self.child.wait().unwrap();
        let signal = unsafe { WaitForSingleObject(self.child.as_raw_handle(), 0) };
        assert_eq!(signal, 0);
        println!(
            "owned child exited status={exit}; process handle wait={signal}; no per-operation outcome receipt"
        );
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
struct Paused {
    threads: Vec<Handle>,
}
impl Paused {
    fn new(child: &OwnedChild) -> Self {
        // Enumerate metadata, then open/suspend ONLY threads owned by our spawned child PID.
        let h = unsafe { CreateToolhelp32Snapshot(4, 0) };
        assert!(h as isize != -1);
        let snapshot = Handle(h);
        let mut entry = ThreadEntry {
            size: std::mem::size_of::<ThreadEntry>() as u32,
            usage: 0,
            id: 0,
            owner: 0,
            base: 0,
            delta: 0,
            flags: 0,
        };
        let mut paused = Self {
            threads: Vec::new(),
        };
        let mut more = unsafe { Thread32First(snapshot.0, &mut entry) };
        while more != 0 {
            if entry.owner == child.child.id() {
                let thread = unsafe { OpenThread(0x802, 0, entry.id) };
                assert!(!thread.is_null());
                let thread = Handle(thread);
                // Snapshot TIDs can be reused; verify the stable handle before side effects.
                let owner = unsafe { GetProcessIdOfThread(thread.0) };
                assert_eq!(owner, child.child.id());
                let previous = unsafe { SuspendThread(thread.0) };
                assert_ne!(previous, u32::MAX);
                paused.threads.push(thread);
                println!(
                    "SuspendThread owned child pid={owner} thread={} previous={previous}",
                    entry.id
                );
            }
            more = unsafe { Thread32Next(snapshot.0, &mut entry) };
        }
        assert!(!paused.threads.is_empty());
        paused
    }
}
impl Drop for Paused {
    fn drop(&mut self) {
        for thread in self.threads.iter().rev() {
            let n = unsafe { ResumeThread(thread.0) };
            println!("ResumeThread owned child previous={n}");
        }
    }
}
fn peek(h: &Handle) -> (bool, u32, u32) {
    let mut available = 0;
    let ok = unsafe {
        PeekNamedPipe(
            h.0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut available,
            std::ptr::null_mut(),
        )
    };
    let status = if ok != 0 {
        0
    } else {
        unsafe { GetLastError() }
    };
    println!(
        "PeekNamedPipe success={} available={available} status={status}",
        ok != 0
    );
    (ok != 0, available, status)
}
fn handles(line: &str) -> (usize, usize, usize) {
    let values: Vec<_> = line.split_whitespace().collect();
    assert_eq!(values[1], "HANDLES");
    (
        values[2].parse().unwrap(),
        values[3].parse().unwrap(),
        values[4].parse().unwrap(),
    )
}
fn startup_refused(root: &std::path::Path) {
    let mut probe = OwnedChild::spawn(root, "lock");
    let response = probe.line();
    assert_eq!(response, "R1 STARTUP_REFUSED 32");
    println!("managed replacement {response}");
    assert!(probe.child.wait().unwrap().success());
}
#[test]
fn owned_child_probe() {
    let Ok(root) = std::env::var("YIXU_R1_ROOT") else {
        return;
    };
    let mode = std::env::var("YIXU_R1_MODE").unwrap();
    let anchor = winfs::Anchor::open(std::path::Path::new(&root)).unwrap();
    if mode == "lock" {
        let error = anchor
            .broker_lock()
            .expect_err("old instance must retain exclusive lock");
        eprintln!("R1 STARTUP_REFUSED {}", error.raw_os_error().unwrap_or(0));
        return;
    }
    let _lock = anchor.broker_lock().unwrap();
    let id = identity();
    let mut s = Session::new(id, 4096).unwrap();
    assert!(s.noninheritable());
    s.submit().unwrap();
    assert!(s.pending_at_submit);
    let reader = s.resources.reader.as_ref().unwrap().as_raw_handle() as usize;
    eprintln!(
        "R1 HANDLES {} {} {}",
        s.resources.pipe.0 as usize, reader, s.resources.event.0 as usize
    );
    if mode == "observed" {
        let mut n = 0;
        while n < 4096 {
            n += s.read(4096 - n).unwrap();
        }
        let evidence = s.poll(id, 2000).unwrap().unwrap();
        assert_eq!(evidence.count, Count::Exact(4096));
        // Actual Native evidence is only in this process's memory. No DB claim is emitted.
        eprintln!("R1 OBSERVED_NORMAL_NOT_PERSISTED");
    }
    for line in std::io::stdin().lock().lines() {
        match line.unwrap().as_str() {
            "close-reader" => {
                s.close_reader();
                eprintln!("R1 READER_CLOSED");
            }
            "poll" => {
                assert!(s.poll(id, 20).unwrap().is_none());
                eprintln!("R1 IO_STILL_PENDING");
            }
            "finish" => {
                s.cancel(id).unwrap();
                let e = s.poll(id, 2000).unwrap().unwrap();
                eprintln!("R1 TERMINAL {} {:?}", e.status, e.count);
            }
            "quit" => return,
            _ => panic!("unrecognized synthetic control"),
        }
    }
}
#[test]
fn noninherit_does_not_block_same_user_cross_process_duplication() {
    let _g = super::tests::SERIAL.lock().unwrap();
    let f = super::super::super::tests::Fixture::profile(9);
    let root = f._dir.path().join("cipher");
    let mut child = OwnedChild::spawn(&root, "pending");
    let (server, reader, event) = handles(&child.line());
    let limited = child.open(0x1000);
    assert_eq!(
        child
            .duplicate(&limited, reader)
            .err()
            .unwrap()
            .raw_os_error(),
        Some(5)
    );
    let process = child.open(0x40);
    let duplicate = child.duplicate(&process, reader).unwrap();
    let server_dup = child.duplicate(&process, server).unwrap();
    let event_dup = child.duplicate(&process, event).unwrap();
    let mut flags = 0;
    assert_ne!(unsafe { GetHandleInformation(duplicate.0, &mut flags) }, 0);
    assert_eq!(flags & 1, 0);
    assert_eq!(child.command("close-reader"), "R1 READER_CLOSED");
    assert_eq!(child.command("poll"), "R1 IO_STILL_PENDING");
    assert!(peek(&duplicate).0);
    startup_refused(&root);
    // A raw synthetic Session in a different process can bypass the process-local BUSY budget;
    // it is not an authorized content channel. The managed startup above remains denied.
    let id = identity();
    let mut raw = Session::new(id, 4096).unwrap();
    raw.submit().unwrap();
    assert!(raw.pending_at_submit);
    raw.cancel(id).unwrap();
    assert!(raw.poll(id, 2000).unwrap().is_some());
    drop(raw);
    drop(server_dup);
    drop(event_dup);
    drop(duplicate);
    let terminal = child.command("finish");
    assert!(terminal.starts_with("R1 TERMINAL"));
    println!("child {terminal}");
    writeln!(child.input, "quit").unwrap();
    child.input.flush().unwrap();
    assert!(child.child.wait().unwrap().success());
}
#[test]
fn actual_suspend_resume_does_not_release_instance_or_authorize_takeover() {
    let _g = super::tests::SERIAL.lock().unwrap();
    let f = super::super::super::tests::Fixture::profile(9);
    let root = f._dir.path().join("cipher");
    let mut child = OwnedChild::spawn(&root, "pending");
    handles(&child.line());
    {
        let _paused = Paused::new(&child);
        startup_refused(&root);
        println!("all enumerated owned-child threads suspended; managed replacement refused");
    }
    assert_eq!(child.command("poll"), "R1 IO_STILL_PENDING");
    assert!(child.command("finish").starts_with("R1 TERMINAL"));
    writeln!(child.input, "quit").unwrap();
    child.input.flush().unwrap();
    assert!(child.child.wait().unwrap().success());
}
#[test]
fn kill_with_foreign_reader_and_server_handles_preserves_object_not_evidence() {
    let _g = super::tests::SERIAL.lock().unwrap();
    for keep_server in [false, true] {
        let f = super::super::super::tests::Fixture::profile(9);
        let root = f._dir.path().join("cipher");
        let mut child = OwnedChild::spawn(&root, "pending");
        let (server, reader, event) = handles(&child.line());
        let process = child.open(0x40);
        let reader_dup = child.duplicate(&process, reader).unwrap();
        let server_dup = if keep_server {
            Some(child.duplicate(&process, server).unwrap())
        } else {
            None
        };
        let event_dup = child.duplicate(&process, event).unwrap();
        assert_eq!(child.command("close-reader"), "R1 READER_CLOSED");
        assert_eq!(child.command("poll"), "R1 IO_STILL_PENDING");
        child.kill_wait();
        let state = peek(&reader_dup);
        println!("after process exit keep_server={keep_server} pipe observation={state:?}");
        if keep_server {
            assert!(
                state.0,
                "foreign server handle should keep pipe object open"
            );
        }
        let event_state = unsafe { WaitForSingleObject(event_dup.0, 0) };
        println!("foreign event wait={event_state}; not a transferable completion receipt");
        assert!(winfs::Anchor::open(&root).unwrap().broker_lock().is_ok());
        let blocked: String =
            f.db.connection
                .query_row("SELECT state FROM output_runtime", [], |r| r.get(0))
                .unwrap();
        assert_eq!(blocked, "RecoveryBlocked");
        let old_inc: String =
            f.db.connection
                .query_row("SELECT incarnation FROM output_runtime", [], |r| r.get(0))
                .unwrap();
        let reopened =
            AuthDatabase::open_profile(&f.path, super::super::super::tests::TestClock::new(), 9)
                .unwrap();
        let new_state: (String, String) = reopened
            .connection
            .query_row("SELECT incarnation,state FROM output_runtime", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_ne!(old_inc, new_state.0);
        assert_eq!(new_state.1, "RecoveryBlocked");
        println!(
            "new Profile9 opening minted a different incarnation, still RecoveryBlocked; no Ready issuer"
        );
        drop(server_dup);
        drop(reader_dup);
        drop(event_dup);
    }
}
#[test]
fn observed_native_completion_lost_before_persistence_stays_blocked() {
    let _g = super::tests::SERIAL.lock().unwrap();
    let f = super::super::super::tests::Fixture::profile(9);
    let root = f._dir.path().join("cipher");
    let mut child = OwnedChild::spawn(&root, "observed");
    handles(&child.line());
    assert_eq!(child.line(), "R1 OBSERVED_NORMAL_NOT_PERSISTED");
    child.kill_wait();
    let n: i64 =
        f.db.connection
            .query_row("SELECT count(*) FROM terminal_evidence", [], |r| r.get(0))
            .unwrap();
    assert_eq!(n, 0);
    let state: String =
        f.db.connection
            .query_row("SELECT state FROM output_runtime", [], |r| r.get(0))
            .unwrap();
    assert_eq!(state, "RecoveryBlocked");
    println!(
        "native observed in child, child killed before DB handoff, no persisted terminal receipt, state={state}"
    );
}
