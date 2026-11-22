# ADR-M3-017: Profile 12 schema package (S1 only)

2026-11-23. Accepted for the owner's S1 implementation authorization. Experimental,
metadata-only SQLite schema reference; no Application/content/restore admission.
Parent public baseline: d9109f4f73362bfdee419843bb2fe92e5478235e.

## Decision

Freeze Profiles 9, 10 and both historical Profile 11 definitions. Install a new
Profile 12 into an empty database only, using unchanged Profile 10 base SQL plus
the independent complete 0012 extension. Never install 0011 then ALTER its tables.
The content-family application_id remains 1498952246; user_version is 12.

deletion_member has an immediate composite FK to deletion_closure and a deferred
event FK to security_receipt. The receipt's existing deferred Applied-phase FK
prevents a member transaction committing without that event reaching Applied.
An immutable closure_seal is written after closure rows while Closing; later
closure inserts are refused. Fence requires a nonempty sealed closure. Applied
has its own guard checking both EXCEPT directions, scope and committed operations.
It does not rely on a count or digest equality, or on trigger execution order.
Ordinary Write events without a deletion_intent retain their existing behavior.

Package hashing uses a versioned domain and u64 big-endian length-delimited
components: base SQL, extension SQL, exact generated guard SQL, profile, family ID,
content format, format tag and definition encoding version. Actual attestation
compares the entire trusted rebuilt sqlite_schema including generated triggers
and implicit indexes, plus configuration, application/profile IDs, integrity and
the unchanged two-pair object FK diagnostic whitelist. The stored format row
cannot establish authenticity by itself.

## Scope and isolation

The new private profile12 module is cfg(test), consistent with the existing
experimental assembly. It is not a production Repository, authentication issuer,
deletion coordinator, KeyVault or migration service. The normal crate exports no
new API and no feature enables this module. SQL tests construct content-free
synthetic metadata graphs through real constraints; their placeholder publication
hashes and key rows are NOT encryption, file or key-revocation evidence.

Schema install rejects nonempty/previously-versioned databases; attestation rejects
old profiles and unknown format claims. These connection-level tests do not prove
an old Engine open leaves original DB/WAL/SHM bytes unchanged. The future S4
content-profile file, pre-SQLite gate and snapshot inspection are NOT implemented.

S2 must still calculate/authorize the full dependency graph, do its own final set
validation and apply mutations in one transaction. S3 must implement Marker and
Owner health changes. No new Profile is registered in the old content Engine.
Profile 12 does not mean R12 successful recovery. Native output, services,
Protected Witness, strong deletion and production content remain unavailable.

## Alternatives and validation

Reusing Profile 11 would give one version multiple new meanings. A composite FK
alone would miss omitted members. Deferred checking alone would not express set
equality. Automatic migration would require a separate ledger/key/recovery proof.
These alternatives are rejected for S1.

Tests use real temporary SQLite databases for fresh creation, immediate/deferred
FK behavior, closure sealing, Applied guards, rollback, schema drift, hash binding,
configuration and legacy byte goldens. Failed COMMIT is explicitly tested to leave
a transaction requiring ROLLBACK. Future release needs Application and platform
evidence; schema tests are not a production or independent safety certification.
