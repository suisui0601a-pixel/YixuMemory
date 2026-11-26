//! Concrete W1 S3 adapter, compiled only inside the private Windows test host.
//! Low-level I/O is not a publish permit or a NativeTerminalEvidence issuer.
use super::winfs::{Anchor, VerifiedMarkerHandle};
use std::{io, path::Path};

pub(crate) struct MarkerStore {
    marker: Anchor,
    payload: Anchor,
    backup: Anchor,
}
pub(crate) struct MarkerPin(VerifiedMarkerHandle);
impl MarkerPin {
    pub(crate) fn bytes(&self) -> &[u8] {
        self.0.bytes()
    }
    pub(crate) fn identity(&self) -> [u8; 12] {
        self.0.identity()
    }
}
impl MarkerStore {
    pub(crate) fn open(marker: &Path, payload: &Path, backup: &Path) -> io::Result<Self> {
        Ok(Self {
            marker: Anchor::open(marker)?,
            payload: Anchor::open(payload)?,
            backup: Anchor::open(backup)?,
        })
    }
    pub(crate) fn binding(&self) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        for a in [&self.marker, &self.payload, &self.backup] {
            for v in a.directory_identity()? {
                bytes.extend(v.to_be_bytes());
            }
        }
        Ok(bytes)
    }
    pub(crate) fn observe(&self, name: &str) -> io::Result<Option<MarkerPin>> {
        self.marker.observe_marker(name).map(|p| p.map(MarkerPin))
    }
    pub(crate) fn verify(&self, pin: &mut MarkerPin) -> io::Result<()> {
        pin.0.verify(&self.marker)
    }
    pub(crate) fn names(&self) -> io::Result<Vec<String>> {
        self.marker.names()
    }
    pub(crate) fn publish_once(
        &self,
        temp: &str,
        name: &str,
        bytes: &[u8],
    ) -> io::Result<MarkerPin> {
        // Existing W1 publication/flags/no-replace are unchanged. Any failure is
        // propagated; no retry, fallback, replacement, erase or auto-repair.
        self.marker.publish(temp, name, bytes, 0, |_| {})?;
        self.observe(name)?
            .ok_or_else(|| io::Error::other("published marker unavailable"))
    }
}

#[cfg(test)]
mod tests {
    use crate::auth::marker_io::MarkerStore;
    fn fixture() -> (tempfile::TempDir, MarkerStore) {
        let dir = tempfile::tempdir().unwrap();
        for name in ["markers", "payload", "backups"] {
            std::fs::create_dir(dir.path().join(name)).unwrap();
        }
        let store = MarkerStore::open(
            &dir.path().join("markers"),
            &dir.path().join("payload"),
            &dir.path().join("backups"),
        )
        .unwrap();
        (dir, store)
    }
    #[test]
    fn native_same_handle_pin_binds_bytes_identity_and_denies_write_delete() {
        let (dir, store) = fixture();
        let mut pin = store
            .publish_once("pending-native", "revoked-native", b"SYNTHETIC-MARKER")
            .unwrap();
        let id = pin.identity();
        assert_eq!(pin.bytes(), b"SYNTHETIC-MARKER");
        assert_eq!(store.binding().unwrap().len(), 36);
        assert_eq!(store.names().unwrap(), vec!["revoked-native"]);
        let path = dir.path().join("markers/revoked-native");
        let write = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap_err();
        let delete = std::fs::remove_file(&path).unwrap_err();
        println!(
            "S3_NATIVE write_error={:?} delete_error={:?} identity={id:?}",
            write.raw_os_error(),
            delete.raw_os_error()
        );
        assert!(matches!(write.raw_os_error(), Some(5 | 32)));
        assert!(matches!(delete.raw_os_error(), Some(5 | 32)));
        store.verify(&mut pin).unwrap();
        assert_eq!(id, pin.identity());
        assert_eq!(
            store.observe("revoked-native").unwrap().unwrap().identity(),
            id
        );
        drop(pin);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn native_absence_and_invalid_path_are_distinct_and_no_replace_preserves_fileid() {
        let (_dir, store) = fixture();
        assert!(store.observe("revoked-absent").unwrap().is_none());
        assert!(store.observe("../escape").is_err());
        assert!(store.observe("revoked-file:ads").is_err());
        let pin = store
            .publish_once("pending-first", "revoked-same", b"FIRST")
            .unwrap();
        let id = pin.identity();
        drop(pin);
        let error = match store.publish_once("pending-second", "revoked-same", b"SECOND") {
            Err(e) => e,
            Ok(_) => panic!("no-replace unexpectedly overwrote marker"),
        };
        let after = store.observe("revoked-same").unwrap().unwrap();
        println!(
            "S3_NATIVE collision_error={:?} original_identity_preserved={}",
            error.raw_os_error(),
            id == after.identity()
        );
        assert_eq!(after.bytes(), b"FIRST");
        assert_eq!(after.identity(), id);
    }
}
