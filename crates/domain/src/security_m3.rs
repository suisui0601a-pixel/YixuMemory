//! Pure protocol model. Observations are NOT authenticated security evidence.
//! No I/O, plaintext, keys, permissions or production capability issuer lives here.
use std::collections::{BTreeMap, BTreeSet};

pub type Identity = [u8; 16];
pub type Digest = [u8; 32];
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Profile {
    Local,
    Protected,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct SecurityScope {
    pub realm: Identity,
    pub tenant: Identity,
    pub space: Identity,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum OwnerKind {
    Event,
    Evidence,
    Revision,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct PayloadBinding {
    pub scope: SecurityScope,
    pub owner_kind: OwnerKind,
    pub owner: Identity,
    pub payload: Identity,
    pub key: Identity,
    pub seal_generation: u64,
    pub vault_incarnation: u64,
    pub protection: Digest,
}
impl PayloadBinding {
    /// Experimental canonical wire format, not yet a published storage version.
    /// Classification is a trusted protection digest, never a plaintext exemption.
    pub fn aad(&self, length: u32) -> Vec<u8> {
        let mut out = b"YIXU-M3-AAD\0\x01\x01".to_vec();
        for id in [self.scope.realm, self.scope.tenant, self.scope.space] {
            out.extend(id);
        }
        out.push(match self.owner_kind {
            OwnerKind::Event => 1,
            OwnerKind::Evidence => 2,
            OwnerKind::Revision => 3,
        });
        for id in [self.owner, self.payload, self.key] {
            out.extend(id);
        }
        out.extend(self.seal_generation.to_be_bytes());
        out.extend(self.vault_incarnation.to_be_bytes());
        out.extend(self.protection);
        out.extend(length.to_be_bytes());
        out
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    Denied,
    Unavailable,
    Unknown,
    Conflict,
    Integrity,
    Unsupported,
}
pub type ProtocolResult<T> = Result<T, ProtocolError>;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SecurityIntent {
    pub id: Identity,
    pub scope: SecurityScope,
    pub request_digest: Digest,
    pub authorization_epoch: u64,
    pub deletion_generation: u64,
    pub closure_version: u64,
    /// Explicit immutable closure, supplied by a trusted transactional graph reader.
    /// The model validates structure; it does NOT discover a complete database graph.
    pub targets: BTreeSet<PayloadBinding>,
    pub copies: BTreeSet<Identity>,
}
impl SecurityIntent {
    pub fn validate(&self) -> ProtocolResult<()> {
        if self.targets.is_empty()
            || self.copies.is_empty()
            || self.authorization_epoch == 0
            || self.deletion_generation == 0
            || self.closure_version == 0
            || self.targets.iter().any(|t| {
                t.scope != self.scope || t.seal_generation == 0 || t.vault_incarnation == 0
            })
        {
            return Err(ProtocolError::Integrity);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntentState {
    OutcomeUnknown,
    Guarded,
    DbFenced,
    Receipted,
    Finalized,
}
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ReceiptTarget {
    pub binding: PayloadBinding,
    pub copy: Identity,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErasureObservation {
    pub intent: Identity,
    pub request_digest: Digest,
    pub target: ReceiptTarget,
    pub deletion_generation: u64,
    pub closure_version: u64,
    pub worker_fence: u64,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IntentRecord {
    pub intent: SecurityIntent,
    pub state: IntentState,
    pub worker_fence: u64,
    pub receipts: BTreeSet<ReceiptTarget>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GuardModel {
    pub profile: Profile,
    realm: Identity,
    incarnation: u64,
    security_seq: u64,
    available: bool,
    records: BTreeMap<Identity, IntentRecord>,
}
impl GuardModel {
    pub fn new(profile: Profile, realm: Identity, incarnation: u64) -> ProtocolResult<Self> {
        if incarnation == 0 {
            return Err(ProtocolError::Integrity);
        }
        Ok(Self {
            profile,
            realm,
            incarnation,
            security_seq: 0,
            available: true,
            records: BTreeMap::new(),
        })
    }
    pub fn records(&self) -> &BTreeMap<Identity, IntentRecord> {
        &self.records
    }
    pub fn security_seq(&self) -> u64 {
        self.security_seq
    }
    pub fn incarnation(&self) -> u64 {
        self.incarnation
    }
    pub fn set_available(&mut self, available: bool) {
        self.available = available;
    }
    /// Must precede sending PREPARE. Unknown outcomes permanently deny until reconciled.
    /// Caller must hold authorization/DB/release serialization; this is a model, not that lock.
    pub fn begin(&mut self, intent: SecurityIntent) -> ProtocolResult<()> {
        self.require_available()?;
        intent.validate()?;
        if intent.scope.realm != self.realm {
            return Err(ProtocolError::Denied);
        }
        if let Some(old) = self.records.get(&intent.id) {
            return if old.intent == intent {
                Ok(())
            } else {
                Err(ProtocolError::Conflict)
            };
        }
        self.security_seq = self
            .security_seq
            .checked_add(1)
            .ok_or(ProtocolError::Integrity)?;
        self.records.insert(
            intent.id,
            IntentRecord {
                intent,
                state: IntentState::OutcomeUnknown,
                worker_fence: 0,
                receipts: BTreeSet::new(),
            },
        );
        Ok(())
    }
    pub fn prepared(&mut self, id: Identity) -> ProtocolResult<()> {
        self.advance(id, IntentState::OutcomeUnknown, IntentState::Guarded)
    }
    pub fn tdel(&mut self, id: Identity) -> ProtocolResult<()> {
        self.advance(id, IntentState::Guarded, IntentState::DbFenced)
    }
    fn advance(&mut self, id: Identity, from: IntentState, to: IntentState) -> ProtocolResult<()> {
        self.require_available()?;
        let record = self.records.get_mut(&id).ok_or(ProtocolError::Integrity)?;
        if record.state == to {
            return Ok(());
        }
        if record.state != from {
            return Err(ProtocolError::Conflict);
        }
        record.state = to;
        Ok(())
    }
    pub fn claim(&mut self, id: Identity) -> ProtocolResult<u64> {
        self.require_available()?;
        let record = self.records.get_mut(&id).ok_or(ProtocolError::Integrity)?;
        if record.state != IntentState::DbFenced {
            return Err(ProtocolError::Conflict);
        }
        let next = record
            .worker_fence
            .checked_add(1)
            .ok_or(ProtocolError::Integrity)?;
        record.worker_fence = next;
        // Previous-generation observations cannot satisfy the new claimant's completion.
        record.receipts.clear();
        Ok(next)
    }
    pub fn observe(&mut self, receipt: ErasureObservation) -> ProtocolResult<()> {
        self.require_available()?;
        let record = self
            .records
            .get_mut(&receipt.intent)
            .ok_or(ProtocolError::Integrity)?;
        let i = &record.intent;
        if record.state != IntentState::DbFenced
            || record.worker_fence == 0
            || receipt.worker_fence != record.worker_fence
            || receipt.request_digest != i.request_digest
            || receipt.deletion_generation != i.deletion_generation
            || receipt.closure_version != i.closure_version
            || !i.targets.contains(&receipt.target.binding)
            || !i.copies.contains(&receipt.target.copy)
        {
            return Err(ProtocolError::Integrity);
        }
        record.receipts.insert(receipt.target);
        if record.receipts.len()
            == i.targets
                .len()
                .checked_mul(i.copies.len())
                .ok_or(ProtocolError::Integrity)?
        {
            record.state = IntentState::Receipted;
        }
        Ok(())
    }
    pub fn finalize(&mut self, id: Identity) -> ProtocolResult<()> {
        self.advance(id, IntentState::Receipted, IntentState::Finalized)
    }
    /// Model admission only; cannot mint a ReleasePermit or open content.
    pub fn check_release(
        &self,
        binding: &PayloadBinding,
        incarnation: u64,
        sequence: u64,
    ) -> ProtocolResult<()> {
        self.require_available()?;
        if binding.scope.realm != self.realm
            || incarnation != self.incarnation
            || sequence != self.security_seq
        {
            return Err(ProtocolError::Denied);
        }
        for record in self.records.values() {
            if record.intent.scope == binding.scope
                && (record.state != IntentState::Finalized
                    || record
                        .intent
                        .targets
                        .iter()
                        .any(|t| t.payload == binding.payload || t.key == binding.key))
            {
                return Err(ProtocolError::Denied);
            }
        }
        Ok(())
    }
    fn require_available(&self) -> ProtocolResult<()> {
        if self.available {
            Ok(())
        } else {
            Err(ProtocolError::Unavailable)
        }
    }
    /// Simulates loading a CURRENT authenticated external ledger after a request barrier.
    /// Real adapters must authenticate/obtain freshness and fence old senders first.
    /// Local has no accepted backup restore path. No automatic profile downgrade exists.
    pub fn reconcile(&mut self, current: &Self, new_incarnation: u64) -> ProtocolResult<()> {
        self.available = false;
        if self.profile != Profile::Protected || current.profile != self.profile {
            return Err(ProtocolError::Unsupported);
        }
        if !current.available
            || current.realm != self.realm
            || current.security_seq < self.security_seq
            || new_incarnation <= self.incarnation
            || new_incarnation <= current.incarnation
        {
            return Err(ProtocolError::Integrity);
        }
        for (id, local) in &self.records {
            let remote = current.records.get(id).ok_or(ProtocolError::Unknown)?;
            if local.intent != remote.intent
                || rank(remote.state) < rank(local.state)
                || remote.worker_fence < local.worker_fence
            {
                return Err(ProtocolError::Integrity);
            }
        }
        *self = current.clone();
        self.incarnation = new_incarnation;
        Ok(())
    }
}
fn rank(state: IntentState) -> u8 {
    match state {
        IntentState::OutcomeUnknown => 0,
        IntentState::Guarded => 1,
        IntentState::DbFenced => 2,
        IntentState::Receipted => 3,
        IntentState::Finalized => 4,
    }
}

#[cfg(test)]
mod tests;
