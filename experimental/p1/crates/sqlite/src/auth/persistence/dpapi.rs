//! Real Windows current-user DPAPI; no machine scope, prompts, export or fallback.
#![allow(unsafe_code)]
use super::*;
use std::ffi::c_void;
use zeroize::Zeroize;
#[repr(C)]
struct Blob {
    len: u32,
    data: *mut u8,
}
#[link(name = "crypt32")]
unsafe extern "system" {
    fn CryptProtectData(
        input: *const Blob,
        description: *const u16,
        entropy: *const Blob,
        reserved: *mut c_void,
        prompt: *mut c_void,
        flags: u32,
        out: *mut Blob,
    ) -> i32;
    fn CryptUnprotectData(
        input: *const Blob,
        description: *mut *mut u16,
        entropy: *const Blob,
        reserved: *mut c_void,
        prompt: *mut c_void,
        flags: u32,
        out: *mut Blob,
    ) -> i32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn LocalFree(p: *mut c_void) -> *mut c_void;
    fn LocalSize(p: *mut c_void) -> usize;
}
thread_local! {static LAST_WIPE:std::cell::Cell<bool>=const{std::cell::Cell::new(false)};}
struct Output(Blob);
impl Output {
    fn bytes(&self) -> &[u8] {
        // SAFETY: checked owned Windows allocation remains live for self.
        unsafe { std::slice::from_raw_parts(self.0.data, self.0.len as usize) }
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        if !self.0.data.is_null() {
            // SAFETY: LocalSize validates the Windows local allocation (zero for invalid handle).
            // Never construct a slice using untrusted cbData, including on API failure.
            let capacity = unsafe { LocalSize(self.0.data.cast()) };
            if capacity > 0 && capacity <= isize::MAX as usize {
                // SAFETY: OS-validated allocation size; owned DPAPI local block is still live.
                unsafe {
                    let bytes = std::slice::from_raw_parts_mut(self.0.data, capacity);
                    bytes.zeroize();
                    LAST_WIPE.set(bytes.iter().all(|v| *v == 0));
                    LocalFree(self.0.data.cast());
                }
            }
            // If no allocation can be validated, do not dereference/free an unknown pointer.
        }
    }
}
fn call(input: &[u8], entropy: &[u8], protect: bool) -> Result<Output> {
    if input.is_empty() || input.len() > 8192 {
        return Err(Error::Constraint);
    }
    let i = Blob {
        len: input.len() as u32,
        data: input.as_ptr().cast_mut(),
    };
    let e = Blob {
        len: entropy.len() as u32,
        data: entropy.as_ptr().cast_mut(),
    };
    let mut o = Output(Blob {
        len: 0,
        data: std::ptr::null_mut(),
    });
    // SAFETY: bounded borrowed blobs live throughout synchronous call, null prompts/description;
    // output is initialized and RAII wiped/freed even on validation failure. Flag1 = UI_FORBIDDEN.
    let ok = unsafe {
        if protect {
            CryptProtectData(
                &i,
                std::ptr::null(),
                &e,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut o.0,
            )
        } else {
            CryptUnprotectData(
                &i,
                std::ptr::null_mut(),
                &e,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                &mut o.0,
            )
        }
    };
    let capacity = if o.0.data.is_null() {
        0
    } else {
        unsafe { LocalSize(o.0.data.cast()) }
    };
    if ok == 0
        || o.0.data.is_null()
        || o.0.len == 0
        || o.0.len > 8192
        || o.0.len as usize > capacity
    {
        return Err(Error::CapabilityUnavailable);
    }
    Ok(o)
}
fn protect(b: &PayloadBinding, key: &LockedBuffer) -> Result<Vec<u8>> {
    let aad = b.aad(32);
    let mut input =
        LockedBuffer::new(32 + aad.len(), false).map_err(|_| Error::CapabilityUnavailable)?;
    input.bytes_mut()[..32].copy_from_slice(key.bytes());
    input.bytes_mut()[32..].copy_from_slice(&aad);
    let out = call(input.bytes(), &aad, true)?;
    // SAFETY: successful checked OS result; contains only wrapped ciphertext.
    let mut bytes = b"YIXUK001".to_vec();
    bytes.extend(unsafe { std::slice::from_raw_parts(out.0.data, out.0.len as usize) });
    Ok(bytes)
}
fn unprotect(b: &PayloadBinding, bytes: &[u8]) -> Result<LockedBuffer> {
    if bytes.len() < 9 || &bytes[..8] != b"YIXUK001" {
        return Err(Error::Constraint);
    }
    let aad = b.aad(32);
    let out = call(&bytes[8..], &aad, false)?;
    if out.0.len as usize != 32 + aad.len() {
        return Err(Error::Constraint);
    }
    // SAFETY: checked OS allocation, copied only after locked target allocated, wiped by Output Drop.
    let plain = unsafe { std::slice::from_raw_parts(out.0.data, out.0.len as usize) };
    if plain[32..] != aad {
        return Err(Error::Constraint);
    }
    let mut key = LockedBuffer::new(32, false).map_err(|_| Error::CapabilityUnavailable)?;
    key.bytes_mut().copy_from_slice(&plain[..32]);
    Ok(key)
}
pub(super) struct LocalVault {
    pub(super) anchor: winfs::Anchor,
    pub(super) after_unprotect: Option<Box<dyn Fn() + Send + Sync>>,
}
impl LocalVault {
    pub(super) fn new(root: PathBuf) -> Result<Self> {
        Ok(Self {
            anchor: winfs::Anchor::open(&root).map_err(|_| Error::CapabilityUnavailable)?,
            after_unprotect: None,
        })
    }
    fn not_revoked(&self, b: &PayloadBinding) -> Result<()> {
        match self
            .anchor
            .read(&format!("revoked-{}", Id::from_bytes(b.key)))
        {
            Err(e) if e.raw_os_error() == Some(2) => Ok(()),
            _ => Err(Error::CapabilityUnavailable),
        }
    }
    pub(super) fn revoke_marker(&self, b: &PayloadBinding) -> Result<()> {
        let name = format!("revoked-{}", Id::from_bytes(b.key));
        if self.anchor.read(&name).is_ok() {
            return Ok(());
        }
        self.anchor
            .publish(
                &format!("pending-revoke-{}", fresh()?),
                &name,
                &b.aad(32),
                0,
                |_| {},
            )
            .map_err(|_| Error::CapabilityUnavailable)
    }
    pub(super) fn create(&self, b: &PayloadBinding) -> Result<(LockedBuffer, Vec<u8>)> {
        self.not_revoked(b)?;
        let mut k = LockedBuffer::new(32, false).map_err(|_| Error::CapabilityUnavailable)?;
        getrandom::fill(k.bytes_mut()).map_err(|_| Error::CapabilityUnavailable)?;
        let bytes = protect(b, &k)?;
        let name = Id::from_bytes(b.key).to_string();
        self.anchor
            .publish(&format!("pending-{}", fresh()?), &name, &bytes, 0, |_| {})
            .map_err(|_| Error::CapabilityUnavailable)?;
        let actual = self
            .anchor
            .read(&name)
            .map_err(|_| Error::CapabilityUnavailable)?;
        if actual != bytes {
            return Err(Error::Constraint);
        }
        Ok((k, hash(&bytes)))
    }
    pub(super) fn load(&self, b: &PayloadBinding) -> Result<(LockedBuffer, Vec<u8>)> {
        self.not_revoked(b)?;
        let bytes = self
            .anchor
            .read(&Id::from_bytes(b.key).to_string())
            .map_err(|_| Error::CapabilityUnavailable)?;
        let k = unprotect(b, &bytes)?;
        if let Some(h) = &self.after_unprotect {
            h();
        }
        self.not_revoked(b)?;
        Ok((k, hash(&bytes)))
    }
}
#[cfg(test)]
mod tests;
