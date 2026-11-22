# M3 ADR-003 — synthetic encrypted content experiment

Status: local test-only experiment; no production encryption backend is installed.
Challenge ADR candidates 03/05/06/07; ordinary reversible library selection under approved direction.

## Selection and format

Use pinned RustCrypto chacha20poly1305 0.10.1, zeroize 1.9.1 and getrandom 0.4.3 as dev-dependencies
of the empty normal m3-lab crate. RustCrypto instead of libsodium is an explicit experiment choice:
the mature detached in-place API avoids another native build/ABI in the constrained Windows lab.
It is not a decision to replace a production provider; libsodium interoperability and release
selection remain gates. The published crate describes constant-time multiplication assumptions
and MIT/Apache-2.0 licensing: https://docs.rs/chacha20poly1305/0.10.1/chacha20poly1305/ .
No upstream implementation code was copied. Cargo.lock pins transitive packages and checksums;
dependency advisory results are evidence at query time, not an assertion of zero vulnerabilities.

Each stage generates random server payload/key IDs and a 256-bit DEK with OS randomness. Each
key/seal generation permits only one encryption attempt, even if it fails. A random 192-bit nonce
and detached 128-bit tag accompany ciphertext. The fixture uses one generation; production rotation
and key wrapping are not implemented. New independent stages have different keys and nonces.

AAD experiment v1: ASCII `YIXU-M3-AAD`, zero separator, version 1, algorithm 1, three fixed 16-byte
realm/tenant/Space IDs, 1-byte owner kind, three fixed 16-byte owner/payload/key IDs, big-endian u64
seal generation and vault incarnation, 32-byte trusted protection digest, big-endian u32 length.
It is 163 bytes. Event/Evidence/Revision owner kinds are explicit; Evidence can be shared without
rebinding it to every Revision. This is not yet a released cross-language persistence contract.
No request classification is accepted by the lab: its protected fixture digest is internal. The
existing production API still rejects synthetic/personal/sensitive equally. Real R14 policy join
and signed/transaction-bound protection-context issuance remain unimplemented.

## Memory and files

Body range: 1..4096 bytes; executable accepts only fixed synthetic fixtures, no external body CLI.
Windows test wrapper uses fresh VirtualAlloc pages and checked VirtualLock BEFORE copying a body
or generating a key; Drop zeroizes, then unlocks and frees that exact allocation. It exposes no raw
pointer, Clone, Debug or serialization. The tiny unsafe OS adapter is the sole local exception to
workspace unsafe prohibition, isolated in cfg(test); SAFETY comments document ownership and Send.
https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-virtuallock

In-place AEAD transforms the locked body. Only ciphertext is copied into ordinary Vec/file. Read
loads bounded ciphertext, verifies manifest hash/length, authenticates before releasing the private
locked output. Corrupt tag/nonce/body/AAD/key/generation is rejected. Lock/random failure closes the
operation. Cipher internal key copies/stack/registers, static synthetic canary in the test binary,
dump/hibernation/VM snapshot remain limitations; this is NOT a complete SecureMemory guarantee.

CipherStore owns anonymous temporary file handles created on C: NTFS during tests, syncs ciphertext,
and retains each handle without reopening a string path. It rejects duplicate IDs and revoked names;
receiver fence and tombstone checks are in the same exclusive test call. File close is not physical
erasure. This proves ciphertext-only file I/O and bounded corruption checks, NOT durable named-file
publication, directory fsync, hostile-path safety or restart recovery. Temporary files created by
tests are normally released by their owning handles; no preexisting files/caches are cleaned.

Tstage->T1 uses a private in-memory active set to model the association commit. Injected T1 failure
leaves no active metadata and revokes the allocated fake key; an inaccessible ciphertext orphan may
remain until test shutdown. The original SQLite admission trigger stays untouched. The accepted
SQLite Foundation/M1/M2 tests continue separately; this experiment does not fake an admitted payload.

## Cost, alternatives and gates

- Checked locks cost working-set resources and may reject large/concurrent buffers. No limit raising
  or privileged fallback is automatic. Linux mlock/dump policy is a later native adapter.
- Anonymous handle storage avoids claiming unverified directory durability. Durable Linux openat2/
  no-replace/fsync publication and full SQLite sidecar VFS coverage remain separate milestones.
- Volatile FakeKeyVault cannot restore keys after process exit and cannot prove anti-rollback. It
  is suitable for tests only; protected claims require real independent Witness/KeyVault acceptance.
- AEAD tests, tamper cases, no-replace, revocation/release race, fault injection and dependency scan
  are required now; physical erasure, production key destruction and power-loss tests are not passed.
