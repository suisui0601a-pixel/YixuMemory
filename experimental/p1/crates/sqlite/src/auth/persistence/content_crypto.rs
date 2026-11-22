//! Private ciphertext format support for the Windows persistence experiments.
use super::{Error, LockedBuffer, PayloadBinding, Result};
use chacha20poly1305::{
    Tag, XChaCha20Poly1305, XNonce,
    aead::{AeadInPlace, KeyInit},
};

const MAX_BODY: usize = 4096;
const MAGIC: &[u8; 8] = b"YIXUB001";
const NONCE_BYTES: usize = 24;
const TAG_BYTES: usize = 16;
const PREFIX_BYTES: usize = 12;

pub(super) fn encoded_length(envelope: &[u8]) -> Result<usize> {
    let len = envelope.get(8..PREFIX_BYTES).ok_or(Error::Constraint)?;
    let len = u32::from_be_bytes(len.try_into().map_err(|_| Error::Constraint)?) as usize;
    if !(1..=MAX_BODY).contains(&len) {
        return Err(Error::Constraint);
    }
    Ok(len)
}

pub(super) fn seal(binding: &PayloadBinding, body: &[u8], key: &LockedBuffer) -> Result<Vec<u8>> {
    if !(1..=MAX_BODY).contains(&body.len()) {
        return Err(Error::Constraint);
    }
    let len = body.len() as u32;
    let mut buffer =
        LockedBuffer::new(body.len(), false).map_err(|_| Error::CapabilityUnavailable)?;
    buffer.bytes_mut().copy_from_slice(body);
    let cipher = XChaCha20Poly1305::new_from_slice(key.bytes()).map_err(|_| Error::Constraint)?;
    let mut nonce = [0; NONCE_BYTES];
    getrandom::fill(&mut nonce).map_err(|_| Error::ResourceUnavailable)?;
    let aad = binding.aad(len);
    let tag = cipher
        .encrypt_in_place_detached(XNonce::from_slice(&nonce), &aad, buffer.bytes_mut())
        .map_err(|_| Error::Constraint)?;
    let mut envelope =
        Vec::with_capacity(PREFIX_BYTES + aad.len() + NONCE_BYTES + TAG_BYTES + body.len());
    envelope.extend(MAGIC);
    envelope.extend(len.to_be_bytes());
    envelope.extend(aad);
    envelope.extend(nonce);
    envelope.extend(tag);
    envelope.extend(buffer.bytes());
    Ok(envelope)
}

pub(super) fn open(
    binding: &PayloadBinding,
    envelope: &[u8],
    key: &LockedBuffer,
) -> Result<LockedBuffer> {
    let len = encoded_length(envelope)?;
    let aad = binding.aad(len as u32);
    let nonce_start = PREFIX_BYTES + aad.len();
    let tag_start = nonce_start + NONCE_BYTES;
    let body_start = tag_start + TAG_BYTES;
    if envelope.len() != body_start + len
        || &envelope[..8] != MAGIC
        || envelope[PREFIX_BYTES..nonce_start] != aad
    {
        return Err(Error::Constraint);
    }
    let mut buffer = LockedBuffer::new(len, false).map_err(|_| Error::CapabilityUnavailable)?;
    buffer.bytes_mut().copy_from_slice(&envelope[body_start..]);
    XChaCha20Poly1305::new_from_slice(key.bytes())
        .map_err(|_| Error::Constraint)?
        .decrypt_in_place_detached(
            XNonce::from_slice(&envelope[nonce_start..tag_start]),
            &aad,
            buffer.bytes_mut(),
            Tag::from_slice(&envelope[tag_start..body_start]),
        )
        .map_err(|_| Error::Constraint)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use yixu_domain::security_m3::{OwnerKind, SecurityScope};

    // This is a primitive test fixture, not a persisted application permission.
    fn binding() -> PayloadBinding {
        PayloadBinding {
            scope: SecurityScope {
                realm: [1; 16],
                tenant: [2; 16],
                space: [3; 16],
            },
            owner_kind: OwnerKind::Revision,
            owner: [4; 16],
            payload: [5; 16],
            key: [6; 16],
            seal_generation: 2,
            vault_incarnation: 3,
            protection: [7; 32],
        }
    }

    fn random_key() -> LockedBuffer {
        let mut key = LockedBuffer::new(32, false).unwrap();
        getrandom::fill(key.bytes_mut()).unwrap();
        key
    }

    #[test]
    fn utf8_body_round_trips_with_a_fresh_nonce_per_seal() {
        let binding = binding();
        let key = random_key();
        let body = "这是一条多字节记忆。\nUne note dans le journal.".as_bytes();
        let first = seal(&binding, body, &key).unwrap();
        let second = seal(&binding, body, &key).unwrap();
        let nonce = PREFIX_BYTES + binding.aad(body.len() as u32).len();
        assert!(first[nonce..nonce + NONCE_BYTES] != second[nonce..nonce + NONCE_BYTES]);
        assert!(!first.windows(body.len()).any(|candidate| candidate == body));
        for envelope in [&first, &second] {
            assert!(open(&binding, envelope, &key).unwrap().bytes() == body);
        }
    }

    #[test]
    fn malformed_envelopes_and_mismatched_tags_keys_or_bindings_are_rejected() {
        let binding = binding();
        let key = random_key();
        let body = "换一个长度的记忆".as_bytes();
        let envelope = seal(&binding, body, &key).unwrap();
        let nonce = PREFIX_BYTES + binding.aad(body.len() as u32).len();
        for index in [
            0,
            11,
            PREFIX_BYTES,
            nonce,
            nonce + NONCE_BYTES,
            envelope.len() - 1,
        ] {
            let mut corrupt = envelope.clone();
            corrupt[index] ^= 1;
            assert!(open(&binding, &corrupt, &key).is_err());
        }
        for end in [0, 8, 11, nonce, envelope.len() - 1] {
            assert!(open(&binding, &envelope[..end], &key).is_err());
        }
        let mut foreign_realm = binding;
        foreign_realm.scope.realm = [9; 16];
        let mut foreign_tenant = binding;
        foreign_tenant.scope.tenant = [9; 16];
        let mut foreign_space = binding;
        foreign_space.scope.space = [9; 16];
        let mut old_generation = binding;
        old_generation.seal_generation -= 1;
        let mut wrong_key_id = binding;
        wrong_key_id.key = [9; 16];
        let mut wrong_revision = binding;
        wrong_revision.owner = [9; 16];
        let mut wrong_payload = binding;
        wrong_payload.payload = [9; 16];
        let mut wrong_owner_kind = binding;
        wrong_owner_kind.owner_kind = OwnerKind::Evidence;
        let mut wrong_vault_incarnation = binding;
        wrong_vault_incarnation.vault_incarnation += 1;
        let mut wrong_protection = binding;
        wrong_protection.protection = [9; 32];
        for mismatch in [
            foreign_realm,
            foreign_tenant,
            foreign_space,
            old_generation,
            wrong_key_id,
            wrong_revision,
            wrong_payload,
            wrong_owner_kind,
            wrong_vault_incarnation,
            wrong_protection,
        ] {
            assert!(open(&mismatch, &envelope, &key).is_err());
        }
        assert!(open(&binding, &envelope, &random_key()).is_err());
        assert!(seal(&binding, b"", &key).is_err());
        assert!(seal(&binding, &[0; MAX_BODY + 1], &key).is_err());
    }
}
