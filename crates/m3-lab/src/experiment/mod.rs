//! Private test assembly. Keys/Witness/metadata are volatile simulations; AEAD and file I/O are real.
mod memory;
mod tests;
use chacha20poly1305::{
    Tag, XChaCha20Poly1305, XNonce,
    aead::{AeadInPlace, KeyInit},
};
use memory::LockedBuffer;
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
};
use yixu_domain::security_m3::*;
type Error = ProtocolError;
type Result<T> = ProtocolResult<T>;
const CANARY: &[u8] = b"YIXU M3 SYNTHETIC ONLY: blue marble remembers 42";
const KEY_COPY: Identity = [240; 16];
const BLOB_COPY: Identity = [241; 16];
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum Fault {
    #[default]
    None,
    Random,
    Lock,
    Encrypt,
    Publish,
    T1,
    PrepareAck,
    Tdel,
    Erase,
    FinalizeAck,
}
fn random(out: &mut [u8], fault: Fault) -> Result<()> {
    if fault == Fault::Random {
        return Err(Error::Unavailable);
    }
    getrandom::fill(out).map_err(|_| Error::Unavailable)
}
struct FakeKeyVault {
    keys: BTreeMap<Identity, (PayloadBinding, LockedBuffer, bool)>,
    revoked: BTreeSet<Identity>,
    highest_fence: BTreeMap<Identity, u64>,
    available: bool,
}
impl FakeKeyVault {
    fn new() -> Self {
        Self {
            keys: BTreeMap::new(),
            revoked: BTreeSet::new(),
            highest_fence: BTreeMap::new(),
            available: true,
        }
    }
    fn allocate(&mut self, binding: PayloadBinding, fault: Fault) -> Result<()> {
        if !self.available {
            return Err(Error::Unavailable);
        }
        if self.revoked.contains(&binding.key) || self.keys.contains_key(&binding.key) {
            return Err(Error::Conflict);
        }
        let mut key = LockedBuffer::new(32, fault == Fault::Lock)?;
        random(key.bytes_mut(), fault)?;
        self.keys.insert(binding.key, (binding, key, false));
        Ok(())
    }
    fn seal(
        &mut self,
        binding: &PayloadBinding,
        plain: &mut LockedBuffer,
        fault: Fault,
    ) -> Result<Sealed> {
        if !self.available {
            return Err(Error::Unavailable);
        }
        let (actual, key, used) = self.keys.get_mut(&binding.key).ok_or(Error::Denied)?;
        if actual != binding || *used || self.revoked.contains(&binding.key) {
            return Err(Error::Denied);
        }
        *used = true; // Never retry this key/generation, even on uncertain encryption outcome.
        let mut nonce = [0; 24];
        random(&mut nonce, fault)?;
        if fault == Fault::Encrypt {
            return Err(Error::Unavailable);
        }
        let length = plain.bytes().len() as u32;
        let cipher =
            XChaCha20Poly1305::new_from_slice(key.bytes()).map_err(|_| Error::Integrity)?;
        let tag = cipher
            .encrypt_in_place_detached(
                XNonce::from_slice(&nonce),
                &binding.aad(length),
                plain.bytes_mut(),
            )
            .map_err(|_| Error::Integrity)?;
        let mut bytes = Vec::with_capacity(24 + 16 + length as usize);
        bytes.extend(nonce);
        bytes.extend(tag);
        bytes.extend(plain.bytes());
        Ok(Sealed {
            binding: *binding,
            length,
            bytes,
        })
    }
    fn open(&self, sealed: &Sealed) -> Result<LockedBuffer> {
        if !self.available {
            return Err(Error::Unavailable);
        }
        let (actual, key, _) = self.keys.get(&sealed.binding.key).ok_or(Error::Denied)?;
        if actual != &sealed.binding || self.revoked.contains(&sealed.binding.key) {
            return Err(Error::Denied);
        }
        if sealed.length == 0
            || sealed.length > 4096
            || sealed.bytes.len() != 40 + sealed.length as usize
        {
            return Err(Error::Integrity);
        }
        let mut plain = LockedBuffer::new(sealed.length as usize, false)?;
        plain.bytes_mut().copy_from_slice(&sealed.bytes[40..]);
        let cipher =
            XChaCha20Poly1305::new_from_slice(key.bytes()).map_err(|_| Error::Integrity)?;
        cipher
            .decrypt_in_place_detached(
                XNonce::from_slice(&sealed.bytes[..24]),
                &sealed.binding.aad(sealed.length),
                plain.bytes_mut(),
                Tag::from_slice(&sealed.bytes[24..40]),
            )
            .map_err(|_| Error::Integrity)?;
        Ok(plain)
    }
    fn revoke(&mut self, binding: &PayloadBinding, fence: u64) -> Result<()> {
        if !self.available {
            return Err(Error::Unavailable);
        }
        let old = self.highest_fence.get(&binding.key).copied().unwrap_or(0);
        if fence == 0 || fence < old {
            return Err(Error::Denied);
        }
        if let Some((actual, _, _)) = self.keys.get(&binding.key)
            && actual != binding
        {
            return Err(Error::Integrity);
        }
        self.highest_fence.insert(binding.key, fence);
        self.revoked.insert(binding.key);
        self.keys.remove(&binding.key);
        Ok(())
    }
}
struct Sealed {
    binding: PayloadBinding,
    length: u32,
    bytes: Vec<u8>,
}
struct Stored {
    binding: PayloadBinding,
    length: u32,
    hash: Digest,
    file: File,
}
struct CipherStore {
    entries: BTreeMap<Identity, Stored>,
    tombstones: BTreeSet<Identity>,
    fences: BTreeMap<Identity, u64>,
}
impl CipherStore {
    fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            tombstones: BTreeSet::new(),
            fences: BTreeMap::new(),
        }
    }
    fn publish(&mut self, sealed: Sealed, fault: Fault) -> Result<()> {
        if self.entries.contains_key(&sealed.binding.payload)
            || self.tombstones.contains(&sealed.binding.payload)
        {
            return Err(Error::Conflict);
        }
        if fault == Fault::Publish {
            return Err(Error::Unavailable);
        }
        // Anonymous, fresh OS file; retain handle for entire lifetime, never reopen a string path.
        // Test runner pins TEMP/TMP to a C: NTFS test directory. This is NOT durable directory publication.
        let mut file = tempfile::tempfile().map_err(|_| Error::Unavailable)?;
        file.write_all(&sealed.bytes)
            .map_err(|_| Error::Unavailable)?;
        file.sync_all().map_err(|_| Error::Unavailable)?;
        let hash = Sha256::digest(&sealed.bytes).into();
        self.entries.insert(
            sealed.binding.payload,
            Stored {
                binding: sealed.binding,
                length: sealed.length,
                hash,
                file,
            },
        );
        Ok(())
    }
    fn read(&mut self, binding: &PayloadBinding) -> Result<Sealed> {
        if self.tombstones.contains(&binding.payload) {
            return Err(Error::Denied);
        }
        let stored = self
            .entries
            .get_mut(&binding.payload)
            .ok_or(Error::Denied)?;
        if &stored.binding != binding {
            return Err(Error::Denied);
        }
        let size = stored
            .file
            .metadata()
            .map_err(|_| Error::Unavailable)?
            .len();
        if size != u64::from(stored.length) + 40 || size > 4136 {
            return Err(Error::Integrity);
        }
        stored
            .file
            .seek(SeekFrom::Start(0))
            .map_err(|_| Error::Unavailable)?;
        let mut bytes = vec![0; size as usize];
        stored
            .file
            .read_exact(&mut bytes)
            .map_err(|_| Error::Unavailable)?;
        if <[u8; 32]>::from(Sha256::digest(&bytes)) != stored.hash {
            return Err(Error::Integrity);
        }
        Ok(Sealed {
            binding: *binding,
            length: stored.length,
            bytes,
        })
    }
    fn erase(&mut self, binding: &PayloadBinding, fence: u64) -> Result<()> {
        if fence == 0 || fence < self.fences.get(&binding.payload).copied().unwrap_or(0) {
            return Err(Error::Denied);
        }
        if let Some(stored) = self.entries.get(&binding.payload)
            && stored.binding != *binding
        {
            return Err(Error::Integrity);
        }
        self.fences.insert(binding.payload, fence);
        self.tombstones.insert(binding.payload);
        // Handle close/unlink is operational removal only, NOT physical erasure.
        self.entries.remove(&binding.payload);
        Ok(())
    }
}
struct FakeWitness {
    model: GuardModel,
    available: bool,
}
impl FakeWitness {
    fn require_matching_ledger(&self, local: &GuardModel) -> Result<()> {
        if !self.available {
            return Err(Error::Unavailable);
        }
        if self.model.profile != local.profile
            || self.model.realm() != local.realm()
            || self.model.incarnation() != local.incarnation()
            || self.model.security_seq() != local.security_seq()
            || self.model.records().len() != local.records().len()
            || local.records().iter().any(|(id, r)| {
                self.model
                    .records()
                    .get(id)
                    .is_none_or(|w| w.intent != r.intent)
            })
        {
            return Err(Error::Integrity);
        }
        Ok(())
    }
    fn project_completion(&mut self, local: &GuardModel, id: Identity) -> Result<()> {
        self.require_matching_ledger(local)?;
        let completed = local.records().get(&id).ok_or(Error::Integrity)?;
        if completed.state != IntentState::Receipted {
            return Err(Error::Conflict);
        }
        // Start from remote, not local: other intents' phases, fences and receipts are preserved.
        let mut next = self.model.clone();
        let remote = next.records().get(&id).ok_or(Error::Integrity)?;
        if remote.worker_fence > completed.worker_fence {
            return Err(Error::Integrity);
        }
        if remote.state == IntentState::Finalized || remote.state == IntentState::Receipted {
            if remote.worker_fence != completed.worker_fence
                || remote.receipts != completed.receipts
            {
                return Err(Error::Integrity);
            }
            next.finalize(id)?;
        } else {
            if remote.state == IntentState::Guarded {
                next.tdel(id)?;
            }
            while next.records()[&id].worker_fence < completed.worker_fence {
                next.claim(id)?;
            }
            for receipt in &completed.receipts {
                if next.records()[&id].receipts.contains(receipt) {
                    continue;
                }
                next.observe(observation(
                    &completed.intent,
                    receipt.binding,
                    receipt.copy,
                    completed.worker_fence,
                ))?;
            }
            next.finalize(id)?;
        }
        self.model = next;
        Ok(())
    }
    fn preflight_completion(&self, local: &GuardModel, id: Identity) -> Result<()> {
        self.require_matching_ledger(local)?;
        let l = local.records().get(&id).ok_or(Error::Integrity)?;
        let r = self.model.records().get(&id).ok_or(Error::Integrity)?;
        let compatible = match l.state {
            IntentState::DbFenced => {
                matches!(r.state, IntentState::Guarded | IntentState::DbFenced)
            }
            IntentState::Receipted => matches!(
                r.state,
                IntentState::Guarded
                    | IntentState::DbFenced
                    | IntentState::Receipted
                    | IntentState::Finalized
            ),
            IntentState::Finalized => r == l,
            _ => false,
        };
        if !compatible
            || r.worker_fence > l.worker_fence
            || (r.worker_fence == l.worker_fence && !r.receipts.is_subset(&l.receipts))
        {
            return Err(Error::Integrity);
        }
        Ok(())
    }
    fn prepare(&mut self, intent: SecurityIntent, fault: Fault) -> Result<()> {
        if !self.available {
            return Err(Error::Unavailable);
        }
        self.model.begin(intent.clone())?;
        self.model.prepared(intent.id)?;
        if fault == Fault::PrepareAck {
            return Err(Error::Unknown);
        }
        Ok(())
    }
}
// Not exported. No fields from a request can construct an authenticated subject or permit.
struct LabIdentity {
    epoch: u64,
}
struct LabPermit {
    binding: PayloadBinding,
    epoch: u64,
    incarnation: u64,
    sequence: u64,
}
struct Lab {
    model: GuardModel,
    witness: FakeWitness,
    vault: FakeKeyVault,
    store: CipherStore,
    active: BTreeSet<PayloadBinding>,
    grant: bool,
    epoch: u64,
    released: usize,
    t1_commits: u64,
}
impl Lab {
    fn new(profile: Profile) -> Self {
        let model = GuardModel::new(profile, [1; 16], 1).unwrap();
        Self {
            witness: FakeWitness {
                model: model.clone(),
                available: true,
            },
            model,
            vault: FakeKeyVault::new(),
            store: CipherStore::new(),
            active: BTreeSet::new(),
            grant: true,
            epoch: 1,
            released: 0,
            t1_commits: 0,
        }
    }
    fn identity(&self) -> LabIdentity {
        LabIdentity { epoch: self.epoch }
    }
    fn authorize(&self, identity: &LabIdentity) -> Result<()> {
        if !self.grant || identity.epoch != self.epoch {
            return Err(Error::Denied);
        }
        if self.model.profile == Profile::Protected && !self.witness.available {
            return Err(Error::Unavailable);
        }
        Ok(())
    }
    fn witness_gate(&self, binding: &PayloadBinding) -> Result<()> {
        if self.model.profile == Profile::Protected {
            if !self.witness.available {
                return Err(Error::Unavailable);
            }
            self.witness.model.check_release(
                binding,
                self.model.incarnation(),
                self.model.security_seq(),
            )?;
        }
        Ok(())
    }
    fn stage_canary(&mut self, identity: &LabIdentity, fault: Fault) -> Result<PayloadBinding> {
        self.authorize(identity)?;
        let mut binding = fixture_binding();
        random(&mut binding.payload, fault)?;
        random(&mut binding.key, fault)?;
        self.witness_gate(&binding)?;
        self.model.check_release(
            &binding,
            self.model.incarnation(),
            self.model.security_seq(),
        )?;
        self.vault.allocate(binding, fault)?;
        let outcome = (|| {
            let mut plain = LockedBuffer::new(CANARY.len(), fault == Fault::Lock)?;
            plain.bytes_mut().copy_from_slice(CANARY);
            let sealed = self.vault.seal(&binding, &mut plain, fault)?;
            self.store.publish(sealed, fault)?;
            if fault == Fault::T1 {
                return Err(Error::Unavailable);
            }
            self.active.insert(binding);
            self.t1_commits += 1;
            Ok(binding)
        })();
        if outcome.is_err() {
            self.vault.revoke(&binding, 1)?;
        } // ciphertext orphan may remain; never active
        outcome
    }
    fn permit(&self, identity: &LabIdentity, binding: PayloadBinding) -> Result<LabPermit> {
        self.authorize(identity)?;
        self.witness_gate(&binding)?;
        if !self.active.contains(&binding) {
            return Err(Error::Denied);
        }
        self.model.check_release(
            &binding,
            self.model.incarnation(),
            self.model.security_seq(),
        )?;
        Ok(LabPermit {
            binding,
            epoch: self.epoch,
            incarnation: self.model.incarnation(),
            sequence: self.model.security_seq(),
        })
    }
    fn release(&mut self, permit: LabPermit) -> Result<Digest> {
        // Whole call runs under caller's single lab Mutex; no async/public byte sink exists.
        self.authorize(&LabIdentity {
            epoch: permit.epoch,
        })?;
        self.model
            .check_release(&permit.binding, permit.incarnation, permit.sequence)?;
        self.witness_gate(&permit.binding)?;
        if !self.active.contains(&permit.binding) {
            return Err(Error::Denied);
        }
        let sealed = self.store.read(&permit.binding)?;
        let plain = self.vault.open(&sealed)?;
        // Synthetic sink only hashes, never emits body/log/Vec. This is NOT a network release proof.
        let digest = Sha256::digest(plain.bytes()).into();
        self.released += 1;
        Ok(digest)
    }
    fn begin_delete(
        &mut self,
        identity: &LabIdentity,
        binding: PayloadBinding,
        fault: Fault,
    ) -> Result<SecurityIntent> {
        self.authorize(identity)?;
        if !self.active.contains(&binding) {
            return Err(Error::Denied);
        }
        let mut id = [0; 16];
        random(&mut id, Fault::None)?;
        let intent = SecurityIntent {
            id,
            scope: binding.scope,
            request_digest: Sha256::digest(binding.aad(CANARY.len() as u32)).into(),
            authorization_epoch: self.epoch,
            deletion_generation: self.model.security_seq() + 1,
            closure_version: 1,
            targets: BTreeSet::from([binding]),
            copies: BTreeSet::from([KEY_COPY, BLOB_COPY]),
        };
        self.model.begin(intent.clone())?;
        // Local is an in-memory local journal simulation, no external dependency.
        if self.model.profile == Profile::Protected {
            self.witness.prepare(intent.clone(), fault)?;
        }
        self.model.prepared(intent.id)?;
        if fault == Fault::Tdel {
            return Err(Error::Unavailable);
        }
        self.model.tdel(intent.id)?;
        self.active.remove(&binding);
        Ok(intent)
    }
    fn finish_delete(&mut self, intent: &SecurityIntent, fault: Fault) -> Result<()> {
        if self.model.profile == Profile::Protected {
            self.witness.preflight_completion(&self.model, intent.id)?;
        }
        let record = self
            .model
            .records()
            .get(&intent.id)
            .ok_or(Error::Integrity)?;
        if record.intent != *intent {
            return Err(Error::Conflict);
        }
        if record.state == IntentState::Finalized {
            if self.model.profile == Profile::Protected
                && (!self.witness.available
                    || self.witness.model.records().get(&intent.id) != Some(record))
            {
                return Err(Error::Unknown);
            }
            return Ok(());
        }
        if self.model.profile == Profile::Protected && !self.witness.available {
            return Err(Error::Unavailable);
        }
        if record.state == IntentState::DbFenced {
            let fence = self.model.claim(intent.id)?;
            for b in &intent.targets {
                self.vault.revoke(b, fence)?;
                self.model
                    .observe(observation(intent, *b, KEY_COPY, fence))?;
                if fault == Fault::Erase {
                    return Err(Error::Unavailable);
                }
                self.store.erase(b, fence)?;
                self.model
                    .observe(observation(intent, *b, BLOB_COPY, fence))?;
            }
        } else if record.state != IntentState::Receipted {
            return Err(Error::Conflict);
        }
        if self.model.profile == Profile::Protected {
            self.witness.project_completion(&self.model, intent.id)?;
            if fault == Fault::FinalizeAck {
                return Err(Error::Unknown);
            }
        }
        self.model.finalize(intent.id)
    }
}
fn observation(i: &SecurityIntent, b: PayloadBinding, c: Identity, f: u64) -> ErasureObservation {
    ErasureObservation {
        intent: i.id,
        request_digest: i.request_digest,
        target: ReceiptTarget {
            binding: b,
            copy: c,
        },
        deletion_generation: i.deletion_generation,
        closure_version: i.closure_version,
        worker_fence: f,
    }
}
fn fixture_binding() -> PayloadBinding {
    PayloadBinding {
        scope: SecurityScope {
            realm: [1; 16],
            tenant: [2; 16],
            space: [3; 16],
        },
        owner_kind: OwnerKind::Evidence,
        owner: [4; 16],
        payload: [5; 16],
        key: [6; 16],
        seal_generation: 1,
        vault_incarnation: 1,
        protection: [7; 32],
    }
}
