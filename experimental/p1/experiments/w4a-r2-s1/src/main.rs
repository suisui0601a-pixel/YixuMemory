#![cfg(windows)]

use std::ffi::c_void;
use std::io::{Error, Result};
use std::mem::{MaybeUninit, size_of};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Security::Authorization::*;
use windows_sys::Win32::Security::*;
use windows_sys::Win32::Storage::FileSystem::*;
use windows_sys::Win32::System::Pipes::*;
use windows_sys::Win32::System::Services::*;
use windows_sys::Win32::System::Threading::*;
use windows_sys::Win32::UI::Shell::GetUserProfileDirectoryW;

fn check(ok: i32) -> Result<()> {
    if ok == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

fn require(condition: bool, description: &'static str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::other(description))
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

// SAFETY invariant: h is the newly returned owned HANDLE of a documented Win32 API.
unsafe fn owned(h: HANDLE) -> Result<OwnedHandle> {
    if h.is_null() || h == INVALID_HANDLE_VALUE {
        return Err(Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(h) })
}

struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: this owner is created only for successful LocalFree-compatible results.
        unsafe { LocalFree(self.0) };
    }
}

// Callers choose the exact Win32 structure for the information class.
unsafe fn fixed_token<T>(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<T> {
    let mut value = MaybeUninit::<T>::uninit();
    let mut len = 0;
    check(unsafe {
        GetTokenInformation(
            token,
            class,
            value.as_mut_ptr().cast(),
            size_of::<T>() as u32,
            &mut len,
        )
    })?;
    require(
        len as usize == size_of::<T>(),
        "unexpected token record length",
    )?;
    Ok(unsafe { value.assume_init() })
}

fn token_preflight(token: HANDLE) -> Result<()> {
    // SAFETY: these classes return TOKEN_ELEVATION and DWORD respectively.
    let elevation: TOKEN_ELEVATION = unsafe { fixed_token(token, TokenElevation)? };
    require(
        elevation.TokenIsElevated == 0,
        "run this probe without elevation",
    )?;
    let session: u32 = unsafe { fixed_token(token, TokenSessionId)? };
    let elevation_type: TOKEN_ELEVATION_TYPE = unsafe { fixed_token(token, TokenElevationType)? };
    let kind: TOKEN_TYPE = unsafe { fixed_token(token, TokenType)? };
    let mut len = 0;
    // SAFETY: sizing call, no output buffer.
    let status = unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len) };
    require(
        status == 0 && Error::last_os_error().raw_os_error() == Some(122),
        "TokenUser sizing failed",
    )?;
    require(
        (size_of::<TOKEN_USER>() as u32..=4096).contains(&len),
        "unexpected TokenUser size",
    )?;
    let mut bytes = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
    // SAFETY: aligned backing storage of at least len bytes, valid TOKEN_QUERY handle.
    check(unsafe {
        GetTokenInformation(token, TokenUser, bytes.as_mut_ptr().cast(), len, &mut len)
    })?;
    // SAFETY: successful TokenUser call returned the structure and its SID in bytes.
    let sid = unsafe { (*bytes.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let valid = unsafe { IsValidSid(sid) } != 0;
    require(valid, "invalid TokenUser SID")?;
    let system = unsafe { IsWellKnownSid(sid, WinLocalSystemSid) } != 0;
    require(
        !system && session != 0 && kind == TokenPrimary,
        "expected a non-elevated interactive primary token",
    )?;
    let mut chars = 0;
    // SAFETY: sizing call, records only the result, never the profile path.
    let status = unsafe { GetUserProfileDirectoryW(token, null_mut(), &mut chars) };
    let profile = status == 0 && Error::last_os_error().raw_os_error() == Some(122) && chars > 0;
    println!(
        "token_user_valid={valid} local_system={system} elevated=false elevation_type={elevation_type} session={session} primary=true profile_path_resolvable={profile}"
    );
    Ok(())
}

struct ServiceHandle(SC_HANDLE);
impl Drop for ServiceHandle {
    fn drop(&mut self) {
        // SAFETY: owned SCM/service handle.
        unsafe { CloseServiceHandle(self.0) };
    }
}

fn service_name_preflight() -> Result<()> {
    let scm = unsafe { OpenSCManagerW(null(), null(), SC_MANAGER_CONNECT) };
    if scm.is_null() {
        return Err(Error::last_os_error());
    }
    let scm = ServiceHandle(scm);
    let name = wide("YixuMemoryBroker");
    let service = unsafe { OpenServiceW(scm.0, name.as_ptr(), SERVICE_QUERY_STATUS) };
    if !service.is_null() {
        let _service = ServiceHandle(service);
        return Err(Error::other(
            "service name occupied, no experiment against it",
        ));
    }
    let error = Error::last_os_error();
    println!(
        "service_query_status_win32={}",
        error.raw_os_error().unwrap_or_default()
    );
    require(
        error.raw_os_error() == Some(ERROR_SERVICE_DOES_NOT_EXIST as i32),
        "cannot establish service-name absence",
    )
}

fn inspect_dacl(handle: HANDLE, label: &str) -> Result<()> {
    let mut acl = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: queried handle has READ_CONTROL. All returned fields borrow the allocated SD.
    let status = unsafe {
        GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut acl,
            null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(Error::from_raw_os_error(status as i32));
    }
    let _descriptor = LocalAllocation(descriptor);
    require(!acl.is_null(), "missing DACL")?;
    println!("{label}_dacl_ace_count={}", unsafe { (*acl).AceCount });
    Ok(())
}

fn owned_handle_probe(token: HANDLE) -> Result<()> {
    // SAFETY: all targets are this probe's process, current thread and private unnamed event.
    let pid = unsafe { GetCurrentProcessId() };
    let process = unsafe { owned(OpenProcess(READ_CONTROL | PROCESS_DUP_HANDLE, 0, pid))? };
    let query = unsafe { owned(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid))? };
    let thread = unsafe { owned(OpenThread(READ_CONTROL, 0, GetCurrentThreadId()))? };
    let event = unsafe { owned(CreateEventW(null(), 1, 0, null()))? };
    inspect_dacl(process.as_raw_handle(), "process")?;
    inspect_dacl(thread.as_raw_handle(), "thread")?;
    inspect_dacl(token, "token")?;
    let mut flags = 0;
    check(unsafe { GetHandleInformation(event.as_raw_handle(), &mut flags) })?;
    require(flags & HANDLE_FLAG_INHERIT == 0, "event was inheritable")?;
    for (label, source) in [("query_only", query), ("dup_right", process)] {
        let mut duplicate = null_mut();
        let status = unsafe {
            DuplicateHandle(
                source.as_raw_handle(),
                event.as_raw_handle(),
                GetCurrentProcess(),
                &mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        };
        if status == 0 {
            let error = Error::last_os_error();
            println!(
                "duplicate_{label}_win32={}",
                error.raw_os_error().unwrap_or_default()
            );
            require(
                label == "query_only" && error.raw_os_error() == Some(5),
                "unexpected DuplicateHandle denial",
            )?;
        } else {
            let _duplicate = unsafe { owned(duplicate)? };
            println!("duplicate_{label}_win32=0 noninheritable_original=true");
            require(
                label == "dup_right",
                "query-only handle duplicated the event",
            )?;
        }
    }
    Ok(())
}

fn pipe_preflight() -> Result<()> {
    let name = wide(&format!(r"\\.\pipe\yixu-s1-preflight-{}", unsafe {
        GetCurrentProcessId()
    }));
    let sddl = wide("D:P"); // Empty protected DACL: no external client can connect.
    let mut descriptor = null_mut();
    check(unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        )
    })?;
    let _descriptor = LocalAllocation(descriptor);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let create = || unsafe {
        owned(CreateNamedPipeW(
            name.as_ptr(),
            PIPE_ACCESS_OUTBOUND | FILE_FLAG_FIRST_PIPE_INSTANCE | FILE_FLAG_OVERLAPPED,
            PIPE_REJECT_REMOTE_CLIENTS,
            1,
            512,
            0,
            0,
            &attributes,
        ))
    };
    // Only our first instance. If an outsider pre-created the name, fail without connecting.
    let _server = create()?;
    let collision = match create() {
        Ok(_) => return Err(Error::other("duplicate first instance unexpectedly opened")),
        Err(error) => error.raw_os_error(),
    };
    require(
        collision == Some(5),
        "unexpected first-instance collision status",
    )?;
    let access = unsafe {
        owned(CreateFileW(
            name.as_ptr(),
            GENERIC_READ,
            0,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        ))
    };
    let denied = match access {
        Ok(_) => return Err(Error::other("empty DACL accepted client")),
        Err(error) => error.raw_os_error(),
    };
    require(denied == Some(5), "unexpected pipe DACL status")?;
    println!("pipe_first_instance_collision_win32=5 pipe_empty_dacl_client_win32=5");
    Ok(())
}

fn main() -> Result<()> {
    require(
        std::env::args_os().len() == 1,
        "no argument or external targets accepted",
    )?;
    let mut token = null_mut();
    // SAFETY: process pseudo-handle is borrowed; the returned token handle is owned.
    check(unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | READ_CONTROL, &mut token)
    })?;
    let token = unsafe { owned(token)? };
    token_preflight(token.as_raw_handle())?;
    service_name_preflight()?;
    owned_handle_probe(token.as_raw_handle())?;
    pipe_preflight()?;
    println!("S1_PRECHECK_OK service_identity_and_ipc_auth=NOT_RUN");
    Ok(())
}
