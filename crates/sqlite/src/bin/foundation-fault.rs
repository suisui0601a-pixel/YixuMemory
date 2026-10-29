//! Test-only crash child. Only opaque, synthetic metadata can be written.
use std::{
    io::{self, Write},
    path::PathBuf,
    time::Duration,
};
use yixu_domain::{Id, Result};
use yixu_ports::{MetadataTransaction, UnitOfWork};
use yixu_sqlite_foundation::MetadataLab;
fn id(n: u8) -> Id {
    let mut b = [0; 16];
    b[6] = 0x40;
    b[8] = 0x80;
    b[15] = n;
    Id::from_bytes(b)
}
fn run() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(args.next().expect("synthetic database path required"));
    let commit = args.next().is_some_and(|s| s == "commit");
    let mut lab = MetadataLab::open(&path, Duration::from_secs(1))?;
    let mut tx = lab.begin(id(1))?;
    tx.append_system_event(id(3), id(99), id(2))?;
    if commit {
        tx.commit()?;
        println!("COMMITTED");
    } else {
        println!("UNCOMMITTED");
    }
    io::stdout().flush().expect("flush test barrier");
    loop {
        std::thread::park();
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
