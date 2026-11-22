//! Shared private R4/R5 transaction gates. No credential or production capability issuer.
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

/// Caller obtains exact object generations and retirement facts in its own transaction.
pub(super) fn live_graph_binding(
    revision: [u8; 16],
    nodes: impl IntoIterator<Item = ([u8; 16], i64, bool)>,
) -> Option<[u8; 32]> {
    let mut hash = Sha256::new();
    hash.update(b"YXIP/r5-read-graph/v1\0");
    hash.update(revision);
    for (id, generation, retired) in nodes {
        if retired {
            return None;
        }
        hash.update(id);
        hash.update(generation.to_be_bytes());
    }
    Some(hash.finalize().into())
}

pub(super) fn closing(db: &Connection) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM security_event WHERE state<>'Applied')",
        [],
        |row| row.get(0),
    )
}

pub(super) fn authorized_snapshot(
    db: &Connection,
    scope: &[u8],
    principal: &[u8],
    target: &[u8],
    epoch: i64,
    now: i64,
) -> rusqlite::Result<Option<[i64; 7]>> {
    db.query_row(
        "SELECT epoch, expiry, revision, source, key_gen, copy_gen, deletion_gen
         FROM authority WHERE scope=?1 AND principal=?2 AND target=?3 AND epoch=?4
         AND expiry>?5 AND grant_ok=1 AND principal_ok=1 AND space_ok=1 AND source_ok=1
         AND key_ok=1 AND copy_ok=1 AND deletion_gen=0",
        params![scope, principal, target, epoch, now],
        |row| {
            Ok([
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ])
        },
    )
    .optional()
}

pub(super) fn make_effective(db: &Connection, event: i64) -> rusqlite::Result<usize> {
    db.execute(
        "UPDATE security_event SET state='EffectiveFence'
         WHERE id=?1 AND state='Closing'
         AND NOT EXISTS(SELECT 1 FROM security_event WHERE id<?1 AND state<>'Applied')",
        [event],
    )
}
