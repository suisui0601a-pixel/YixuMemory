//! Windows-only experiment: isolated VirtualAlloc pages, checked VirtualLock, wipe before unlock.
//! Does not prevent dumps, privileged inspection, cipher internal copies, hibernation or VM snapshots.
#![allow(unsafe_code)]
use super::{Error, Result};
use std::{ffi::c_void, ptr::NonNull};
use zeroize::Zeroize;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(address: *mut c_void, size: usize, kind: u32, protection: u32) -> *mut c_void;
    fn VirtualFree(address: *mut c_void, size: usize, kind: u32) -> i32;
    fn VirtualLock(address: *mut c_void, size: usize) -> i32;
    fn VirtualUnlock(address: *mut c_void, size: usize) -> i32;
}
pub(super) struct LockedBuffer {
    ptr: NonNull<u8>,
    length: usize,
}
// SAFETY: allocation is exclusively owned, no aliases escape; OS pages are not thread-affine.
// Not Sync: sharing requires the enclosing release gate's Mutex.
unsafe impl Send for LockedBuffer {}
impl LockedBuffer {
    pub(super) fn new(length: usize, simulate_lock_failure: bool) -> Result<Self> {
        if length == 0 || length > 4096 {
            return Err(Error::Integrity);
        }
        // SAFETY: OS allocates a fresh private committed RW region, never executable.
        let ptr = NonNull::new(
            unsafe { VirtualAlloc(std::ptr::null_mut(), length, 0x3000, 0x04) }.cast::<u8>(),
        )
        .ok_or(Error::Unavailable)?;
        // No secret is copied before lock succeeds. The simulation follows the same error path.
        if simulate_lock_failure || unsafe { VirtualLock(ptr.as_ptr().cast(), length) } == 0 {
            // SAFETY: owns this exact VirtualAlloc base; it still contains only zeroes.
            unsafe {
                VirtualFree(ptr.as_ptr().cast(), 0, 0x8000);
            }
            return Err(Error::Unavailable);
        }
        Ok(Self { ptr, length })
    }
    pub(super) fn bytes(&self) -> &[u8] {
        // SAFETY: allocation is live for self, fully initialized, bounded by allocation.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), self.length) }
    }
    pub(super) fn bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: exclusive &mut self prevents aliases; no raw pointer escapes this module.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.length) }
    }
    pub(super) fn wipe(&mut self) {
        self.bytes_mut().zeroize();
    }
}
impl Drop for LockedBuffer {
    fn drop(&mut self) {
        self.wipe();
        // SAFETY: same owned allocation; zeroed before unlock/release. No retries on foreign memory.
        unsafe {
            VirtualUnlock(self.ptr.as_ptr().cast(), self.length);
            VirtualFree(self.ptr.as_ptr().cast(), 0, 0x8000);
        }
    }
}
