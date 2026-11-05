//! Isolated synthetic Win32 experiment. Never a production output capability.
#![allow(unsafe_code)]
use super::*;
use std::{
    ffi::c_void,
    fs::File,
    io::Read,
    mem::ManuallyDrop,
    os::windows::io::{AsRawHandle, FromRawHandle},
    sync::atomic::{AtomicBool, Ordering},
};
type H = *mut c_void;
#[repr(C)]
struct Overlapped {
    internal: usize,
    high: usize,
    offset: u64,
    event: H,
}
#[repr(C)]
struct Attributes {
    length: u32,
    descriptor: H,
    inherit: i32,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateNamedPipeW(
        name: *const u16,
        access: u32,
        mode: u32,
        max: u32,
        out_size: u32,
        in_size: u32,
        timeout: u32,
        sa: *const Attributes,
    ) -> H;
    fn ConnectNamedPipe(h: H, op: *mut Overlapped) -> i32;
    fn CreateEventW(sa: H, manual: i32, initial: i32, name: *const u16) -> H;
    fn CloseHandle(h: H) -> i32;
    fn WriteFile(h: H, body: *const u8, size: u32, written: *mut u32, op: *mut Overlapped) -> i32;
    fn GetOverlappedResult(h: H, op: *mut Overlapped, n: *mut u32, wait: i32) -> i32;
    fn CancelIoEx(h: H, op: *mut Overlapped) -> i32;
    fn WaitForSingleObject(h: H, ms: u32) -> u32;
    fn GetLastError() -> u32;
    fn GetCurrentProcess() -> H;
    fn LocalFree(p: H) -> H;
    fn GetHandleInformation(h: H, flags: *mut u32) -> i32;
    fn DuplicateHandle(
        source_process: H,
        source: H,
        target_process: H,
        target: *mut H,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
}
#[link(name = "advapi32")]
unsafe extern "system" {
    fn OpenProcessToken(process: H, access: u32, token: *mut H) -> i32;
    fn GetTokenInformation(token: H, class: u32, data: H, size: u32, needed: *mut u32) -> i32;
    fn ConvertSidToStringSidW(sid: H, out: *mut *mut u16) -> i32;
    fn ConvertStringSecurityDescriptorToSecurityDescriptorW(
        sddl: *const u16,
        revision: u32,
        out: *mut H,
        size: *mut u32,
    ) -> i32;
}
fn failure() -> std::io::Error {
    std::io::Error::last_os_error()
}
struct Handle(H);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Local(H);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn descriptor() -> std::io::Result<Local> {
    // SAFETY: all native buffers are correctly aligned/sized, and Local allocations get one owner.
    unsafe {
        let mut h = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), 8, &mut h) == 0 {
            return Err(failure());
        }
        let token = Handle(h);
        let mut needed = 0;
        GetTokenInformation(token.0, 1, std::ptr::null_mut(), 0, &mut needed);
        if needed == 0 || needed > 4096 {
            return Err(std::io::Error::other("token size"));
        }
        let mut data = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        if GetTokenInformation(token.0, 1, data.as_mut_ptr().cast(), needed, &mut needed) == 0 {
            return Err(failure());
        }
        let sid = *(data.as_ptr().cast::<H>());
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(failure());
        }
        let _text = Local(text.cast());
        let mut len = 0;
        while len < 256 && *text.add(len) != 0 {
            len += 1;
        }
        if len == 256 {
            return Err(std::io::Error::other("SID length"));
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
        let sddl: Vec<u16> = format!("D:P(A;;GA;;;{sid})")
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut sd = std::ptr::null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut sd,
            std::ptr::null_mut(),
        ) == 0
        {
            return Err(failure());
        }
        Ok(Local(sd))
    }
}
static BUSY: AtomicBool = AtomicBool::new(false);
static POISONED: AtomicBool = AtomicBool::new(false);
struct Resources {
    pipe: Handle,
    event: Handle,
    op: Box<Overlapped>,
    body: LockedBuffer,
    reader: Option<File>,
}
struct Session {
    resources: ManuallyDrop<Resources>,
    identity: Identity,
    submitted: bool,
    terminal: bool,
    pending_at_submit: bool,
    events: Vec<String>,
}
impl Session {
    fn new(identity: Identity, length: usize) -> std::io::Result<Self> {
        if length == 0 || length > 4096 || identity.attempt != 1 {
            return Err(std::io::Error::other("budget/attempt"));
        }
        if BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(std::io::Error::other("occupied/unsettled"));
        }
        let result = Self::build(identity, length);
        if result.is_err() && !POISONED.load(Ordering::SeqCst) {
            BUSY.store(false, Ordering::SeqCst);
        }
        result
    }
    fn build(identity: Identity, length: usize) -> std::io::Result<Self> {
        let path = format!(
            "\\\\.\\pipe\\yixu-w4a-{}",
            fresh().map_err(|_| std::io::Error::other("rng"))?
        );
        let name: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let sd = descriptor()?;
        let sa = Attributes {
            length: std::mem::size_of::<Attributes>() as u32,
            descriptor: sd.0,
            inherit: 0,
        };
        // SAFETY: owned descriptor/name valid throughout creation; local only, first instance,
        // noninheritable, byte pipe, one instance, OVERLAPPED. No client-selected path.
        let h = unsafe { CreateNamedPipeW(name.as_ptr(), 0x4008_0002, 8, 1, 1, 1, 0, &sa) };
        if h as isize == -1 {
            return Err(failure());
        }
        let pipe = Handle(h);
        let event = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(failure());
        }
        let event = Handle(event);
        let mut op = Box::new(Overlapped {
            internal: 0,
            high: 0,
            offset: 0,
            event: event.0,
        });
        // A2 same-identity synthetic reader, not production authentication. CreateFile connects
        // before ConnectNamedPipe, yielding ERROR_PIPE_CONNECTED with no pending connect buffer.
        let reader = std::fs::OpenOptions::new().read(true).open(&path)?;
        let connected = unsafe { ConnectNamedPipe(pipe.0, &mut *op) };
        if connected == 0 {
            let error = unsafe { GetLastError() };
            if error == 997 {
                // Unexpected pending connect still owns this OVERLAPPED. Do not free it on error.
                unsafe {
                    CancelIoEx(pipe.0, &mut *op);
                }
                let waited = unsafe { WaitForSingleObject(event.0, 2000) };
                let mut n = 0;
                let _ = unsafe { GetOverlappedResult(pipe.0, &mut *op, &mut n, 0) };
                if waited != 0 || op.internal == 259 {
                    POISONED.store(true, Ordering::SeqCst);
                    std::mem::forget(op);
                    std::mem::forget(pipe);
                    std::mem::forget(event);
                    std::mem::forget(reader);
                }
                return Err(std::io::Error::other(
                    "unexpected pending connect; no content admission",
                ));
            }
            if error != 535 {
                return Err(std::io::Error::from_raw_os_error(error as i32));
            }
        }
        let mut body =
            LockedBuffer::new(length, false).map_err(|_| std::io::Error::other("lock"))?;
        body.bytes_mut().fill(b'X');
        Ok(Self {
            resources: ManuallyDrop::new(Resources {
                pipe,
                event,
                op,
                body,
                reader: Some(reader),
            }),
            identity,
            submitted: false,
            terminal: false,
            pending_at_submit: false,
            events: vec!["connected current-user DACL local-only".into()],
        })
    }
    fn submit(&mut self) -> std::io::Result<()> {
        if self.submitted {
            return Err(std::io::Error::other("single use"));
        }
        self.submitted = true;
        let r = &mut *self.resources;
        // SAFETY: allocation, Box<OVERLAPPED>, event and HANDLE remain live and immutable until
        // actual completion. Moving Session never moves these backing allocations.
        let ok = unsafe {
            WriteFile(
                r.pipe.0,
                r.body.bytes().as_ptr(),
                r.body.bytes().len() as u32,
                std::ptr::null_mut(),
                &mut *r.op,
            )
        };
        let status = if ok != 0 {
            0
        } else {
            unsafe { GetLastError() }
        };
        self.events.push(format!("WriteFile status={status}"));
        self.pending_at_submit = status == 997;
        if status != 0 && status != 997 {
            return Err(std::io::Error::from_raw_os_error(status as i32));
        }
        Ok(())
    }
    fn cancel(&mut self, identity: Identity) -> std::io::Result<u32> {
        if identity != self.identity || !self.submitted || self.terminal {
            return Err(std::io::Error::other("wrong operation"));
        }
        let r = &mut *self.resources;
        let ok = unsafe { CancelIoEx(r.pipe.0, &mut *r.op) };
        let status = if ok != 0 {
            0
        } else {
            unsafe { GetLastError() }
        };
        self.events
            .push(format!("CancelIoEx status={status} (not terminal)"));
        Ok(status)
    }
    fn poll(&mut self, identity: Identity, ms: u32) -> std::io::Result<Option<TerminalEvidence>> {
        if identity != self.identity || !self.submitted || self.terminal {
            return Err(std::io::Error::other("wrong/replayed operation"));
        }
        let r = &mut *self.resources;
        let wait = unsafe { WaitForSingleObject(r.event.0, ms) };
        if wait == 258 {
            self.events.push("wait timeout (not terminal)".into());
            return Ok(None);
        }
        if wait != 0 {
            return Err(failure());
        }
        let mut n = 0;
        let ok = unsafe { GetOverlappedResult(r.pipe.0, &mut *r.op, &mut n, 0) };
        let status = if ok != 0 {
            0
        } else {
            unsafe { GetLastError() }
        };
        if status == 996 || r.op.internal == 259 {
            return Ok(None);
        }
        // Event is private and signaled by this operation; raw Internal is no longer PENDING.
        // Error counts are NOT treated as zero. Record an uncertainty range instead.
        let count = if ok != 0 {
            Count::Exact(n as usize)
        } else {
            Count::Range(0, r.body.bytes().len())
        };
        self.terminal = true;
        self.events
            .push(format!("completion status={status} count={count:?}"));
        Ok(Some(TerminalEvidence {
            identity: self.identity,
            count,
            status,
        }))
    }
    fn read(&mut self, n: usize) -> std::io::Result<usize> {
        let mut data = vec![0; n];
        self.resources
            .reader
            .as_mut()
            .ok_or_else(|| std::io::Error::other("reader closed"))?
            .read(&mut data)
    }
    fn close_reader(&mut self) {
        self.resources.reader.take();
        self.events.push("peer closed (not terminal)".into());
    }
    fn duplicate_reader_for_attack_test(&self) -> std::io::Result<File> {
        let reader = self
            .resources
            .reader
            .as_ref()
            .ok_or_else(|| std::io::Error::other("no reader"))?;
        let mut target = std::ptr::null_mut();
        // SAFETY: synthetic owned reader; duplicate ownership transferred once into File.
        let ok = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                reader.as_raw_handle(),
                GetCurrentProcess(),
                &mut target,
                0,
                0,
                2,
            )
        };
        if ok == 0 {
            return Err(failure());
        }
        Ok(unsafe { File::from_raw_handle(target) })
    }
    fn noninheritable(&self) -> bool {
        let mut handles = vec![self.resources.pipe.0, self.resources.event.0];
        if let Some(reader) = self.resources.reader.as_ref() {
            handles.push(reader.as_raw_handle());
        }
        handles.into_iter().all(|handle| {
            let mut flags = 0;
            unsafe { GetHandleInformation(handle, &mut flags) != 0 && flags & 1 == 0 }
        })
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.submitted && !self.terminal {
            let _ = self.cancel(self.identity);
            let _ = self.poll(self.identity, 2000);
        }
        if !self.submitted || self.terminal {
            // SAFETY: either no I/O ever referenced buffers, or matching native completion
            // proved it cannot reference them again. Reader/server/event close before admission.
            unsafe {
                ManuallyDrop::drop(&mut self.resources);
            }
            BUSY.store(false, Ordering::SeqCst);
        }
        // Otherwise intentionally retain the entire ManuallyDrop allocation AND global budget.
        // No fallback, no free/wipe of a pending buffer, no new connection. Stuck is fail-closed.
    }
}
#[cfg(test)]
mod tests;
