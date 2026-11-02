//! Linux reference source; Windows cannot validate this module. Test binary ONLY.
//! Authority is an already-held private directory descriptor supplied by trusted assembly.
//! Same-uid compromise/root, mount-admin and hostile filesystems are outside this experiment.
use rustix::fs::{
    AtFlags, Mode, OFlags, RenameFlags, ResolveFlags, openat2, renameat_with, unlinkat,
};
use std::{
    fs::File,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
};
struct Store {
    root: File,
    device: u64,
}
fn err() -> io::Error {
    io::Error::other("unsupported or untrusted storage state")
}
fn resolve() -> ResolveFlags {
    ResolveFlags::BENEATH
        | ResolveFlags::NO_SYMLINKS
        | ResolveFlags::NO_MAGICLINKS
        | ResolveFlags::NO_XDEV
}
fn envelope(b: &[u8]) -> io::Result<()> {
    if b.len() < 215 || &b[..8] != b"YIXUB001" {
        return Err(err());
    }
    let n = u32::from_be_bytes(b[8..12].try_into().map_err(|_| err())?) as usize;
    if n == 0 || n > 4096 || b.len() != 12 + 163 + 40 + n {
        return Err(err());
    }
    Ok(())
}
impl Store {
    fn from_anchor(root: File) -> io::Result<Self> {
        let m = root.metadata()?;
        if !m.is_dir()
            || m.uid() != rustix::process::geteuid().as_raw()
            || m.mode() & 0o777 != 0o700
        {
            return Err(err());
        }
        root.sync_all()?; // Unsupported directory fsync fails closed, never downgraded.
        Ok(Self {
            device: m.dev(),
            root,
        })
    }
    fn name(id: [u8; 16]) -> String {
        id.iter().map(|n| format!("{n:02x}")).collect()
    }
    fn check(&self, f: &File) -> io::Result<()> {
        let m = f.metadata()?;
        if !m.is_file()
            || m.dev() != self.device
            || m.uid() != rustix::process::geteuid().as_raw()
            || m.mode() & 0o777 != 0o600
            || m.nlink() != 1
            || m.len() > 8192
        {
            return Err(err());
        }
        Ok(())
    }
    fn read(&self, id: [u8; 16]) -> io::Result<Vec<u8>> {
        let fd = openat2(
            &self.root,
            Self::name(id),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
            resolve(),
        )?;
        let mut f = File::from(fd);
        self.check(&f)?;
        let mut b = Vec::new();
        Read::by_ref(&mut f).take(8193).read_to_end(&mut b)?;
        envelope(&b)?;
        Ok(b)
    }
    fn publish(&self, id: [u8; 16], b: &[u8]) -> io::Result<()> {
        envelope(b)?;
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|_| err())?;
        let temp = format!("pending-{}", Self::name(nonce));
        let fd = openat2(
            &self.root,
            &temp,
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
            resolve(),
        )?;
        let mut f = File::from(fd);
        self.check(&f)?;
        // Failure deliberately retains the allocated pending ciphertext for inventory/reconciliation.
        f.write_all(b)?;
        f.sync_all()?;
        renameat_with(
            &self.root,
            &temp,
            &self.root,
            Self::name(id),
            RenameFlags::NOREPLACE,
        )?;
        self.root.sync_all()?; // An error is UNKNOWN, not a failed publication; inspect by hash before retry.
        Ok(())
    }
    fn inventory_names(&self) -> io::Result<Vec<String>> {
        let mut names = Vec::new();
        for e in rustix::fs::Dir::read_from(&self.root)? {
            let e = e?;
            let name = e.file_name().to_str().map_err(|_| err())?;
            if name != "." && name != ".." {
                names.push(name.to_owned());
            }
        }
        Ok(names) // Reconcile against DB bindings; unknown names are NEVER cleanup authority.
    }
    fn erase_known(&self, id: [u8; 16], expected: &[u8]) -> io::Result<()> {
        // Only a trusted coordinator after durable fencing may call; never accept arbitrary paths.
        if self.read(id)? != expected {
            return Err(err());
        }
        unlinkat(&self.root, Self::name(id), AtFlags::empty())?;
        self.root.sync_all()
    }
}
#[test]
fn private_anchor_publish_restart_noreplace_and_erase() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let s = Store::from_anchor(File::open(d.path()).unwrap()).unwrap();
    let mut b = b"YIXUB001".to_vec();
    b.extend(1u32.to_be_bytes());
    b.extend([0; 204]);
    s.publish([1; 16], &b).unwrap();
    assert!(s.inventory_names().unwrap().contains(&Store::name([1; 16])));
    assert!(s.publish([1; 16], &b).is_err());
    let reopened = Store::from_anchor(File::open(d.path()).unwrap()).unwrap();
    assert_eq!(reopened.read([1; 16]).unwrap(), b);
    reopened.erase_known([1; 16], &b).unwrap();
    assert!(reopened.read([1; 16]).is_err());
}
#[test]
fn symlink_and_wrong_anchor_rejected() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let s = Store::from_anchor(File::open(d.path()).unwrap()).unwrap();
    symlink("/etc/passwd", d.path().join(Store::name([1; 16]))).unwrap();
    assert!(s.read([1; 16]).is_err());
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(Store::from_anchor(File::open(d.path()).unwrap()).is_err());
}
