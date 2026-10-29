//! No production SQLite write entry point is supplied.
//! The lab is opt-in and cannot be injected into NoContentRuntime.
//! Ordinary callers cannot use the old root write API, even with all features:
//! ```compile_fail
//! use yixu_sqlite_foundation::MetadataLab;
//! ```
#[cfg(feature = "metadata-lab")]
mod schema;
#[cfg(feature = "metadata-lab")]
mod storage;
/// Synthetic metadata experiments only; NEVER an authenticated ingress.
#[cfg(feature = "metadata-lab")]
pub mod experimental {
    pub use crate::storage::{
        APPLICATION_ID, MIGRATION, MetadataLab, MetadataTx, SCHEMA_VERSION, migration_checksum,
    };
}
