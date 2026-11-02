//! Unpublished synthetic experiment. The normal rlib has no APIs or backends.
//! No Cargo feature can enable the cfg(test) assembly for an external dependency.
//! ```compile_fail
//! use yixu_m3_lab::experiment::FakeKeyVault;
//! ```
#[cfg(all(test, target_os = "windows"))]
mod experiment;

#[cfg(all(test, target_os = "linux"))]
mod linux_store;
