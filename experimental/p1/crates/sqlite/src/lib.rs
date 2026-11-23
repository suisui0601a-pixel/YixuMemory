//! No production SQLite write entry point is supplied.
//! Synthetic fixture support lives in the unpublished yixu-sqlite-test-support crate.
//! Ordinary callers cannot import that API from this crate, including under all features:
//! ```compile_fail
//! use yixu_sqlite_foundation::experimental::MetadataLab;
//! ```

// No feature, including --all-features, can activate identity or bootstrap issuance.
#[cfg(test)]
mod auth;

// Private metadata-only P12 package/Application. No production content adapter.
#[cfg(test)]
mod profile12;

// One private copy shared by the historical experiment and P12 Application.
#[cfg(test)]
#[path = "../../ipc/src/tests/transaction_gates.rs"]
mod transaction_gates;
