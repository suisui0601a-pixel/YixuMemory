# M3-W1: Windows x64 NTFS synthetic handle-layer hardening

Base 9b7c46175b826b1714835a6ec20b4bf351643a08; local branch only. No production content entry, real user data, KeyVault, Witness or erasure acceptance. Profile 6 schema and prior M1/M2 semantics unchanged.

## Threat boundary

Only local Windows x64 NTFS is admitted. Anchor creation starts at an explicit drive root and opens each normal component relative to the retained preceding directory HANDLE. UNC/device/relative paths, dots, ADS, separators in child names and unknown prefixes are refused. NtCreateFile uses OPEN_REPARSE_POINT and checks attributes on the opened object, never canonicalize-then-open. Every directory remains pinned with FILE_SHARE_READ only, denying write/delete handles, so ordinary namespace rename/reparse conversion is blocked; pre-existing incompatible handles make admission fail closed. Regular files require same VolumeSerialNumber, non-directory/non-reparse type, one link and bounded size. Reads deny write/delete sharing and verify the handle before/after reading. IDs have no caller-controlled pathname components.

This does not resist an attacker fully controlling the same OS principal/process/handles, administrator/kernel/driver compromise, raw disk access, handle injection or hostile storage firmware. Owner/ACL provisioning, process isolation and a production broker remain future gates; this code does not supply them. A filesystem writer can cause refusal or change bytes between operations; actual AEAD/AAD and persisted digest validation remain mandatory. Identity checks do not replace authorization or content integrity.

## Native ABI and publication

Unsafe FFI is confined to a test-only module. repr(C) structures cover UNICODE_STRING, OBJECT_ATTRIBUTES, IO_STATUS_BLOCK and BY_HANDLE_FILE_INFORMATION. Pointer size other than 8 fails closed before use. Existing crate-wide unsafe deny remains; a documented local exception applies only to native declarations and bounded owned buffers. File owns and closes every transferred native handle. FILE_RENAME_INFORMATION buffer is u64-aligned, ReplaceIfExists FALSE, offsets 8/16/20, at least sizeof(header)+UTF16 bytes.

1. Create independent random pending file with FILE_CREATE, relative to held anchor, exclusive sharing, DELETE+read/write access.
2. Store validates exact Envelope bounds before entry. Write ciphertext only; sync_all, bounded readback and exact byte comparison, identity/volume/type/link checks.
3. NtSetInformationFile class FileRenameInformation renames the source HANDLE with NULL RootDirectory and a simple destination name in the SAME source directory. No path string traversal and no overwrite flag. Retained ancestor handles prevent moving that directory.
4. sync_all on file after rename, same-handle identity check, then reopen relative to anchor and compare identity. A sharing conflict/mismatch after rename is UNKNOWN (private application currently exposes generic ResourceUnavailable), not proof of absence; next authorized retry reconciles by exact envelope/AAD/tag/hash. No blind overwrite.

Initial Win32 SetFileInformationByHandle call with a RootDirectory returned error87; native relative-root rename conflicted with strong directory share constraints (error32). Same-directory native rename is explicitly documented and succeeded without relaxing the share deny. Short names initially exposed minimum native buffer length error24; fixed before acceptance. Diagnostic raw errors never contain content/key material.

Official references: [NtCreateFile](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntcreatefile), [FILE_RENAME_INFORMATION](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information), [GetFileInformationByHandle](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-getfileinformationbyhandle), [FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers).

## SQLite and cleanup

All native I/O remains outside SQLite writer transactions. Durable one-use claim, Tseal exact four receipts, T1 graph/CAS/knowledge_seq/Outbox/copy_manifest remain unchanged. Published ciphertext + failed T1 stays reconcilable and unreadable; a claimed seal with only pending bytes is UNKNOWN and cannot reseal that key/generation. Pending files are retained, discoverable by prefix, not automatically deleted based only on a guessed name. The existing orphan API inventories final IDs; full pending registry recovery/cleanup remains unimplemented.

Known intent cleanup still durably fences first and refuses committed objects. It opens the candidate with exclusive DELETE access, checks type/link/volume and exact validated ciphertext, then SetDisposition on THAT handle. It does not check then remove_file by pathname. Missing names are handled by existing compensation logic; malformed/unknown copies remain refused. This is not secure erasure or key revocation. Fixture teardown drops anchors before its owned TempDir.

## Evidence and limitations

Two deterministic real-filesystem schedules reproduce the prior primitives: metadata check, replacement with real symlink, old path open reads alternate bytes; and create_new final name, partial write visible to another reader. These are instrumented reproductions of the exact old sequences, not a claim that an unmodified old binary was deterministically paused. New code rejects reparse opens and exposes no final name before rename.

Native tests use only fresh isolated TempDirs: real file symlink (not skipped), real junction, ADS/traversal/wrong type, hard links, identity/volume from opened NTFS handles, held-handle write/rename refusals, repeated parent rename attempts, bounded target/symlink swap race, concurrent same-name publication, no-replace retry and exact-object deletion. No actual other-volume substitution is tested (D: exFAT is excluded); real serial capture/check is exercised but not generalized to all NTFS devices. Root/parent replacement tests require denial while pins live, not successful mutation of system directories.

Process tests use actual child termination at file-written-before-flush, file-flushed-before-rename, and rename-before-post-flush/SQLite binding. Earlier M3-B process termination points Tstage/publication/Tseal/T1/reply remain. Pending cases refuse automatic retries and retain evidence; published case reconciles and commits one graph. No physical power loss or real disk-full/fsync hardware failure is simulated by these results.

File sync_all exercises FlushFileBuffers but **directory naming power-loss durability is NOT proved**. No Linux fsync(dirfd) equivalence is assumed, no administrative volume flush is performed, and no directory flush success is represented as a guaranteed fence. A crash-tested process-level receipt does not prove physical durability. Sharing-constrained namespace operations, antivirus/filter-driver behavior, ACL provisioning and all future production adapters need separate support/acceptance.

Linux/macOS remain NOT TESTED; exFAT unaccepted. Public platform matrix, PR #5, LICENSE, public main and tags are unchanged. Local-only test code and snapshots must not be pushed to the public MIT repository.
