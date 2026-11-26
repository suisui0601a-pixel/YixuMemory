//! Test-only NTFS handle layer; no production export. All unsafe code is localized here.
#![allow(unsafe_code)]
use std::{
    ffi::c_void,
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    os::windows::io::{AsRawHandle, FromRawHandle},
    path::{Component, Path, Prefix},
};
type H = *mut c_void;
#[repr(C)]
struct Unicode {
    length: u16,
    maximum: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct Attributes {
    length: u32,
    root: H,
    name: *mut Unicode,
    flags: u32,
    sd: H,
    qos: H,
}
#[repr(C)]
struct Status {
    status: usize,
    information: usize,
}
#[repr(C)]
#[derive(Default, Debug, PartialEq, Eq)]
struct Info {
    attrs: u32,
    creation: [u32; 2],
    access: [u32; 2],
    write: [u32; 2],
    volume: u32,
    size_hi: u32,
    size_lo: u32,
    links: u32,
    index_hi: u32,
    index_lo: u32,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtCreateFile(
        out: *mut H,
        access: u32,
        attrs: *mut Attributes,
        status: *mut Status,
        allocation: *mut i64,
        file_attrs: u32,
        share: u32,
        disposition: u32,
        options: u32,
        ea: H,
        ea_length: u32,
    ) -> i32;
    fn NtSetInformationFile(file: H, status: *mut Status, info: H, size: u32, class: u32) -> i32;
    fn NtQueryDirectoryFile(
        file: H,
        event: H,
        apc: H,
        context: H,
        status: *mut Status,
        buffer: H,
        size: u32,
        class: u32,
        single: u8,
        name: H,
        restart: u8,
    ) -> i32;
    fn RtlNtStatusToDosError(status: i32) -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        sa: H,
        disposition: u32,
        flags: u32,
        template: H,
    ) -> H;
    fn GetFileInformationByHandle(file: H, info: *mut Info) -> i32;
    fn GetVolumeInformationByHandleW(
        file: H,
        name: *mut u16,
        n: u32,
        serial: *mut u32,
        max: *mut u32,
        flags: *mut u32,
        fs: *mut u16,
        fsn: u32,
    ) -> i32;
    fn SetFileInformationByHandle(file: H, class: u32, info: H, size: u32) -> i32;
    fn GetFileType(file: H) -> u32;
}
pub(super) fn inherited_stdout_is_pipe() -> bool {
    // SAFETY: borrowed standard handle, inspected only, never closed or retained here.
    unsafe { GetFileType(std::io::stdout().as_raw_handle()) == 3 }
}
fn denied() -> io::Error {
    io::Error::other("unsupported NTFS object or identity")
}
fn info(f: &File) -> io::Result<Info> {
    let mut i = Info::default();
    // SAFETY: live owned File handle and correctly sized initialized output structure.
    if unsafe { GetFileInformationByHandle(f.as_raw_handle(), &mut i) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(i)
}
fn ident(i: &Info) -> (u32, u32, u32) {
    (i.volume, i.index_hi, i.index_lo)
}
fn component(s: &str) -> io::Result<()> {
    if s.is_empty()
        || s.len() > 100
        || s == "."
        || s == ".."
        || s.contains(['\\', '/', ':', '\0'])
        || s.ends_with(['.', ' '])
    {
        Err(denied())
    } else {
        Ok(())
    }
}
fn relative(
    root: &File,
    name: &str,
    directory: bool,
    create: bool,
    modify: bool,
) -> io::Result<File> {
    component(name)?;
    let mut w: Vec<u16> = name.encode_utf16().collect();
    let mut u = Unicode {
        length: (w.len() * 2) as u16,
        maximum: (w.len() * 2) as u16,
        buffer: w.as_mut_ptr(),
    };
    let mut a = Attributes {
        length: std::mem::size_of::<Attributes>() as u32,
        root: root.as_raw_handle(),
        name: &mut u,
        flags: 0x40,
        sd: std::ptr::null_mut(),
        qos: std::ptr::null_mut(),
    };
    let mut st = Status {
        status: 0,
        information: 0,
    };
    let mut h = std::ptr::null_mut();
    let access = 0x100000 | 0x80 | 1 | if modify { 0x10000 | 2 } else { 0 };
    // SAFETY: synchronous native call, single bounded component, root File retained, no pointers escape.
    let status = unsafe {
        NtCreateFile(
            &mut h,
            access,
            &mut a,
            &mut st,
            std::ptr::null_mut(),
            0x80,
            if modify { 0 } else { 1 },
            if create { 2 } else { 1 },
            0x200000 | 0x20 | if directory { 1 } else { 0x40 },
            std::ptr::null_mut(),
            0,
        )
    };
    if status < 0 {
        return Err(io::Error::from_raw_os_error(
            unsafe { RtlNtStatusToDosError(status) } as i32,
        ));
    }
    // SAFETY: NtCreateFile transferred a unique valid handle on success.
    Ok(unsafe { File::from_raw_handle(h) })
}
fn directory_names(bytes: &[u8]) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    let mut offset = 0;
    loop {
        let header = bytes.get(offset..offset + 12).ok_or_else(denied)?;
        let next = u32::from_le_bytes(header[..4].try_into().unwrap()) as usize;
        let len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
        if len == 0 || !len.is_multiple_of(2) {
            return Err(denied());
        }
        let raw = bytes
            .get(offset + 12..offset + 12 + len)
            .ok_or_else(denied)?;
        let wide: Vec<u16> = raw
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        let name = String::from_utf16(&wide).map_err(|_| denied())?;
        if name != "." && name != ".." {
            component(&name)?;
            names.push(name);
        }
        if next == 0 {
            break;
        }
        if next < 12 + len || !next.is_multiple_of(4) || next > bytes.len() - offset {
            return Err(denied());
        }
        offset += next;
    }
    if names.len() > 4096 {
        return Err(denied());
    }
    Ok(names)
}
pub(super) struct Anchor {
    pins: Vec<File>,
    id: (u32, u32, u32),
}

/// Private S3 observation: bytes and identity originate from this one live File.
/// No raw handle, Clone, or path-based identity accessor is exposed by this Pin.
pub(super) struct VerifiedMarkerHandle {
    file: File,
    bytes: Vec<u8>,
    identity: [u8; 12],
}
impl VerifiedMarkerHandle {
    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub(super) fn identity(&self) -> [u8; 12] {
        self.identity
    }
    pub(super) fn verify(&mut self, anchor: &Anchor) -> io::Result<()> {
        anchor.root()?;
        let before = anchor.check(&self.file)?;
        self.file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut self.file)
            .take(8193)
            .read_to_end(&mut bytes)?;
        let after = anchor.check(&self.file)?;
        let id: Vec<u8> = [after.volume, after.index_hi, after.index_lo]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        if ident(&before) != ident(&after)
            || id != self.identity
            || bytes != self.bytes
            || bytes.len() != after.size_lo as usize
        {
            return Err(denied());
        }
        Ok(())
    }
}
impl Anchor {
    /// Only exact FILE_NOT_FOUND becomes Absent; other I/O failures stay errors.
    /// Uses the existing read-only share policy, so the retained File denies
    /// ordinary write/delete opens for the lifetime of the Pin.
    pub(super) fn observe_marker(&self, name: &str) -> io::Result<Option<VerifiedMarkerHandle>> {
        let mut file = match relative(self.root()?, name, false, false, false) {
            Ok(file) => file,
            Err(e) if e.raw_os_error() == Some(2) => return Ok(None),
            Err(e) => return Err(e),
        };
        let before = self.check(&file)?;
        let mut bytes = Vec::new();
        Read::by_ref(&mut file).take(8193).read_to_end(&mut bytes)?;
        let after = self.check(&file)?;
        if ident(&before) != ident(&after) || bytes.len() != after.size_lo as usize {
            return Err(denied());
        }
        let identity: Vec<u8> = [after.volume, after.index_hi, after.index_lo]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        Ok(Some(VerifiedMarkerHandle {
            file,
            bytes,
            identity: identity.try_into().map_err(|_| denied())?,
        }))
    }
    pub(super) fn directory_identity(&self) -> io::Result<[u32; 3]> {
        let (volume, high, low) = ident(&info(self.root()?)?);
        Ok([volume, high, low])
    }
    /// Enumerate names through the pinned directory HANDLE. Never follows child entries.
    pub(super) fn names(&self) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        let mut restart = 1;
        loop {
            let mut storage = vec![0u64; 8192];
            let mut status = Status {
                status: 0,
                information: 0,
            };
            // SAFETY: live pinned synchronous directory HANDLE and aligned owned 64KiB output.
            let code = unsafe {
                NtQueryDirectoryFile(
                    self.root()?.as_raw_handle(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut status,
                    storage.as_mut_ptr().cast(),
                    65536,
                    12,
                    0,
                    std::ptr::null_mut(),
                    restart,
                )
            };
            if code as u32 == 0x80000006 {
                break;
            } // STATUS_NO_MORE_FILES
            if code < 0 || status.information == 0 || status.information > 65536 {
                return Err(denied());
            }
            restart = 0;
            // SAFETY: checked OS-written byte count bounded by the owned output allocation.
            let bytes = unsafe {
                std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), status.information)
            };
            names.extend(directory_names(bytes)?);
            if names.len() > 4096 {
                return Err(denied());
            }
        }
        names.sort();
        Ok(names)
    }
    /// Exclusive OS handle, not a timeout lease. Retain Anchor and File until process exit.
    pub(super) fn broker_lock(&self) -> io::Result<File> {
        let f = match relative(self.root()?, "broker-lock", false, true, true) {
            Ok(f) => f,
            Err(e) if matches!(e.raw_os_error(), Some(80 | 183)) => {
                relative(self.root()?, "broker-lock", false, false, true)?
            }
            Err(e) => return Err(e),
        };
        self.check(&f)?;
        Ok(f)
    }
    pub(super) fn open(path: &Path) -> io::Result<Self> {
        if std::mem::size_of::<usize>() != 8 {
            return Err(denied());
        }
        let mut components = path.components();
        let drive = match components.next() {
            Some(Component::Prefix(p)) => match p.kind() {
                Prefix::Disk(d) => d,
                _ => return Err(denied()),
            },
            _ => return Err(denied()),
        };
        if components.next() != Some(Component::RootDir) {
            return Err(denied());
        }
        let w: Vec<u16> = format!("\\\\?\\{}:\\", drive as char)
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: NUL-terminated bounded drive root, flags open directory without following reparse.
        let h = unsafe {
            CreateFileW(
                w.as_ptr(),
                0x100081,
                1,
                std::ptr::null_mut(),
                3,
                0x02200000,
                std::ptr::null_mut(),
            )
        };
        if h as isize == -1 {
            return Err(io::Error::last_os_error());
        }
        let mut pins = vec![unsafe { File::from_raw_handle(h) }];
        let volume = info(&pins[0])?.volume;
        for c in components {
            let Component::Normal(n) = c else {
                return Err(denied());
            };
            let s = n.to_str().ok_or_else(denied)?;
            let f = relative(pins.last().unwrap(), s, true, false, false)?;
            let i = info(&f)?;
            if i.attrs & 0x400 != 0 || i.attrs & 0x10 == 0 || i.volume != volume {
                return Err(denied());
            }
            pins.push(f);
        }
        let mut fs = [0u16; 32];
        if unsafe {
            GetVolumeInformationByHandleW(
                pins.last().unwrap().as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                fs.as_mut_ptr(),
                32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if String::from_utf16_lossy(&fs[..fs.iter().position(|v| *v == 0).unwrap_or(32)]) != "NTFS"
        {
            return Err(denied());
        }
        let id = ident(&info(pins.last().unwrap())?);
        Ok(Self { pins, id })
    }
    fn root(&self) -> io::Result<&File> {
        let f = self.pins.last().unwrap();
        let i = info(f)?;
        if ident(&i) != self.id || i.attrs & 0x400 != 0 {
            return Err(denied());
        }
        Ok(f)
    }
    fn check(&self, f: &File) -> io::Result<Info> {
        let i = info(f)?;
        if i.attrs & (0x400 | 0x10) != 0
            || i.volume != self.id.0
            || i.links != 1
            || i.size_hi != 0
            || i.size_lo > 8192
        {
            return Err(denied());
        }
        Ok(i)
    }
    pub(super) fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        let mut f = relative(self.root()?, name, false, false, false)?;
        let before = self.check(&f)?;
        let mut b = Vec::new();
        Read::by_ref(&mut f).take(8193).read_to_end(&mut b)?;
        let after = self.check(&f)?;
        if ident(&before) != ident(&after) || b.len() != after.size_lo as usize {
            return Err(denied());
        }
        Ok(b)
    }
    /// Identity captured through the same anchored, validated object open used by reads.
    pub(super) fn object_identity(&self, name: &str) -> io::Result<[u32; 3]> {
        let f = relative(self.root()?, name, false, false, false)?;
        let (volume, high, low) = ident(&self.check(&f)?);
        Ok([volume, high, low])
    }
    pub(super) fn publish(
        &self,
        temp: &str,
        name: &str,
        bytes: &[u8],
        fault: u8,
        hook: impl Fn(u8),
    ) -> io::Result<()> {
        component(name)?;
        let mut f = relative(self.root()?, temp, false, true, true)?;
        self.check(&f)?;
        if fault == 1 {
            f.write_all(&bytes[..bytes.len() / 2])?;
            hook(1);
            return Err(denied());
        }
        f.write_all(bytes)?;
        hook(1);
        if fault == 2 {
            return Err(denied());
        }
        f.sync_all()?;
        let identity = ident(&self.check(&f)?);
        f.seek(SeekFrom::Start(0))?;
        let mut actual = Vec::new();
        f.read_to_end(&mut actual)?;
        if actual != bytes {
            return Err(denied());
        }
        hook(2);
        let w: Vec<u16> = name.encode_utf16().collect();
        let size = 24 + w.len() * 2;
        let mut buf = vec![0u64; size.div_ceil(8)];
        // SAFETY: aligned zeroed buffer; FILE_RENAME_INFO offsets for this x64-only experiment.
        // ReplaceIfExists FALSE; NULL RootDirectory + simple filename renames within the
        // source handle's current directory (documented FILE_RENAME_INFORMATION contract).
        unsafe {
            let p = buf.as_mut_ptr().cast::<u8>();
            std::ptr::write(p.add(8).cast::<H>(), std::ptr::null_mut());
            std::ptr::write(p.add(16).cast::<u32>(), (w.len() * 2) as u32);
            std::ptr::copy_nonoverlapping(w.as_ptr().cast::<u8>(), p.add(20), w.len() * 2);
            let mut status = Status {
                status: 0,
                information: 0,
            };
            let code =
                NtSetInformationFile(f.as_raw_handle(), &mut status, p.cast(), size as u32, 10);
            if code < 0 {
                return Err(io::Error::from_raw_os_error(
                    RtlNtStatusToDosError(code) as i32
                ));
            }
        }
        hook(3);
        f.sync_all()?;
        if ident(&self.check(&f)?) != identity {
            return Err(denied());
        }
        drop(f);
        let reopened = relative(self.root()?, name, false, false, false)?;
        if ident(&self.check(&reopened)?) != identity {
            return Err(denied());
        }
        Ok(())
    }
    pub(super) fn erase(&self, name: &str, expected: &[u8]) -> io::Result<()> {
        self.erase_object(name, expected, None)
    }
    pub(super) fn erase_verified(
        &self,
        name: &str,
        expected: &[u8],
        expected_identity: &[u8],
    ) -> io::Result<()> {
        self.erase_object(name, expected, Some(expected_identity))
    }
    fn erase_object(
        &self,
        name: &str,
        expected: &[u8],
        expected_identity: Option<&[u8]>,
    ) -> io::Result<()> {
        let mut f = relative(self.root()?, name, false, false, true)?;
        let metadata = self.check(&f)?;
        let (volume, high, low) = ident(&metadata);
        let actual: Vec<u8> = [volume, high, low]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect();
        if expected_identity.is_some_and(|expected| expected != actual) {
            return Err(denied());
        }
        let mut b = Vec::new();
        Read::by_ref(&mut f).take(8193).read_to_end(&mut b)?;
        if b != expected {
            return Err(denied());
        }
        let mut yes = 1u8; // FileDispositionInfo deletes this exact opened object on close, not a reopened name.
        if unsafe {
            SetFileInformationByHandle(f.as_raw_handle(), 4, (&mut yes as *mut u8).cast(), 1)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn directory_records_are_bounded_and_malformed_names_fail_closed() {
        let mut valid = vec![0u8; 14];
        valid[8..12].copy_from_slice(&2u32.to_le_bytes());
        valid[12..14].copy_from_slice(&(b'x' as u16).to_le_bytes());
        assert_eq!(super::directory_names(&valid).unwrap(), vec!["x"]);
        for end in [0, 1, 11, 12, 13] {
            assert!(super::directory_names(&valid[..end]).is_err());
        }
        let mut malformed = valid.clone();
        malformed[8..12].copy_from_slice(&1u32.to_le_bytes());
        assert!(super::directory_names(&malformed).is_err());
        malformed = valid.clone();
        malformed[..4].copy_from_slice(&4u32.to_le_bytes());
        assert!(super::directory_names(&malformed).is_err());
        malformed = valid.clone();
        malformed[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(super::directory_names(&malformed).is_err());
        malformed = valid.clone();
        malformed[12..14].copy_from_slice(&0xd800u16.to_le_bytes());
        assert!(super::directory_names(&malformed).is_err());
    }
    use super::*;
    use std::{
        sync::{Arc, Barrier},
        thread,
    };
    fn setup() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }
    fn bytes() -> Vec<u8> {
        let mut b = b"YIXUB001".to_vec();
        b.extend(1u32.to_be_bytes());
        b.extend([1; 204]);
        b
    }
    #[test]
    fn old_path_check_open_gap_follows_replacement_but_new_handle_rejects() {
        let d = setup();
        let r = d.path().join("root");
        std::fs::create_dir(&r).unwrap();
        let p = r.join("object");
        let outside = d.path().join("outside");
        std::fs::write(&outside, b"attack").unwrap();
        std::fs::write(&p, b"safe").unwrap();
        assert!(std::fs::symlink_metadata(&p).unwrap().is_file());
        std::fs::remove_file(&p).unwrap();
        std::os::windows::fs::symlink_file(&outside, &p).expect("real symlink capability required");
        assert_eq!(std::fs::read(&p).unwrap(), b"attack");
        let a = Anchor::open(&r).unwrap();
        assert!(a.read("object").is_err());
        assert_eq!(std::fs::read(&outside).unwrap(), b"attack");
    }
    #[test]
    fn old_final_path_partial_write_visible_new_publish_keeps_final_absent() {
        let d = setup();
        let root = d.path();
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("old"))
            .unwrap();
        f.write_all(b"partial").unwrap();
        assert_eq!(std::fs::read(root.join("old")).unwrap(), b"partial");
        drop(f);
        let a = Anchor::open(root).unwrap();
        assert!(a.publish("pending", "final", &bytes(), 1, |_| {}).is_err());
        assert!(!root.join("final").exists());
        assert!(root.join("pending").exists());
    }
    #[test]
    fn traversal_ads_and_wrong_types_rejected() {
        let d = setup();
        let a = Anchor::open(d.path()).unwrap();
        for n in [
            "../x", "a:b", "a/b", "a\\b", "..", ".", "x.", "x ", "C:evil",
        ] {
            assert!(a.read(n).is_err());
            assert!(a.publish("temp", n, &bytes(), 0, |_| {}).is_err());
        }
        std::fs::create_dir(d.path().join("dir")).unwrap();
        assert!(a.read("dir").is_err());
    }
    #[test]
    fn hardlink_and_handle_identity_checks() {
        let d = setup();
        let p = d.path().join("x");
        std::fs::write(&p, b"safe").unwrap();
        std::fs::hard_link(&p, d.path().join("alias")).unwrap();
        let a = Anchor::open(d.path()).unwrap();
        assert!(a.read("x").is_err());
        assert!(std::fs::hard_link(&p, d.path().join("another")).is_err());
        drop(a);
        std::fs::remove_file(d.path().join("alias")).unwrap();
        let a = Anchor::open(d.path()).unwrap();
        assert_eq!(a.read("x").unwrap(), b"safe");
        let f = relative(a.root().unwrap(), "x", false, false, false).unwrap();
        assert_eq!(ident(&info(&f).unwrap()).0, a.id.0);
        assert!(std::fs::rename(&p, d.path().join("moved")).is_err());
        assert!(std::fs::write(&p, b"evil").is_err());
        assert_eq!(a.read("x").unwrap(), b"safe");
    }
    #[test]
    fn pinned_parent_and_junction_replacement_refused() {
        let d = setup();
        let parent = d.path().join("parent");
        let root = parent.join("root");
        std::fs::create_dir_all(&root).unwrap();
        let outside = d.path().join("elsewhere");
        std::fs::create_dir(&outside).unwrap();
        {
            let a = Anchor::open(&root).unwrap();
            for _ in 0..32 {
                assert!(std::fs::rename(&parent, d.path().join("moved")).is_err());
                assert!(std::fs::rename(&root, parent.join("moved")).is_err());
            }
            a.publish("pending", "x", &bytes(), 0, |_| {}).unwrap();
        }
        let junction = d.path().join("junction");
        let status = std::process::Command::new("cmd")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(status.status.success(), "junction capability required");
        assert!(Anchor::open(&junction).is_err());
        assert!(Anchor::open(&junction.join("nested")).is_err());
    }
    #[test]
    fn same_name_concurrent_publish_exactly_one_winner() {
        let d = setup();
        let path = d.path().to_path_buf();
        let barrier = Arc::new(Barrier::new(3));
        let mut tasks = Vec::new();
        for i in 0..2 {
            let p = path.clone();
            let b = barrier.clone();
            tasks.push(thread::spawn(move || {
                let a = Anchor::open(&p).unwrap();
                b.wait();
                a.publish(&format!("pending-{i}"), "x", &bytes(), 0, |_| {})
                    .is_ok()
            }));
        }
        barrier.wait();
        assert_eq!(
            tasks
                .into_iter()
                .filter_map(|t| t.join().ok())
                .filter(|x| *x)
                .count(),
            1
        );
        let a = Anchor::open(&path).unwrap();
        assert_eq!(a.read("x").unwrap(), bytes());
        assert!(a.publish("retry", "x", &bytes(), 0, |_| {}).is_err());
    }
    #[test]
    fn delete_uses_verified_handle_never_replacement_name() {
        let d = setup();
        let a = Anchor::open(d.path()).unwrap();
        a.publish("pending", "x", &bytes(), 0, |_| {}).unwrap();
        assert!(a.erase("x", b"wrong").is_err());
        assert_eq!(a.read("x").unwrap(), bytes());
        a.erase("x", &bytes()).unwrap();
        assert!(!d.path().join("x").exists());
    }
    #[test]
    fn symlink_swap_race_has_no_followed_read() {
        let d = setup();
        let root = d.path().join("root");
        std::fs::create_dir(&root).unwrap();
        let outside = d.path().join("outside");
        std::fs::write(&outside, b"attack").unwrap();
        let target = root.join("x");
        std::fs::write(&target, b"safe").unwrap();
        let a = Anchor::open(&root).unwrap();
        let gate = Arc::new(Barrier::new(2));
        let b = gate.clone();
        let p = target.clone();
        let attacker = thread::spawn(move || {
            b.wait();
            for _ in 0..128 {
                if std::fs::remove_file(&p).is_ok() {
                    std::os::windows::fs::symlink_file(&outside, &p).expect("symlink privilege");
                    let _ = std::fs::remove_file(&p);
                    let _ = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&p)
                        .and_then(|mut f| f.write_all(b"safe"));
                }
            }
        });
        gate.wait();
        for _ in 0..256 {
            if let Ok(v) = a.read("x") {
                assert!(v.is_empty() || v == b"safe");
            }
        }
        attacker.join().unwrap();
    }
}
