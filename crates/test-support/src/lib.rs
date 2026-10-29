//! Unpublished helpers for deterministic synthetic SQLite contract tests.
//! This crate is not a production storage adapter and is not a dependency of the application.

#[cfg(feature = "synthetic-fixtures")]
mod schema;
#[cfg(feature = "synthetic-fixtures")]
mod storage;

#[cfg(feature = "synthetic-fixtures")]
pub use storage::{
    APPLICATION_ID, MIGRATION, MIGRATION_RECORD, MetadataLab, MetadataTx, SCHEMA_VERSION,
    migration_checksum,
};

#[cfg(feature = "synthetic-fixtures")]
pub fn validate_schema_definitions(
    actual: &rusqlite::Connection,
    foundation: &str,
    extension: &str,
) -> yixu_domain::Result<()> {
    schema::validate_definitions(actual, foundation, extension)
}
