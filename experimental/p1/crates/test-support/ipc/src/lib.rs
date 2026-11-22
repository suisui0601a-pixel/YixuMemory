//! Public synthetic fixtures for test builds. These keys and channels carry no authority.
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

#[derive(Clone, Copy)]
pub enum Role {
    Client,
    Server,
}

fn signer(role: Role, transcript: &[u8]) -> Hmac<Sha256> {
    let byte = match role {
        Role::Client => 41,
        Role::Server => 73,
    };
    let mut mac = Hmac::<Sha256>::new_from_slice(&[byte; 32]).expect("valid fixture key");
    mac.update(transcript);
    mac
}

pub fn prove(role: Role, transcript: &[u8]) -> [u8; 32] {
    signer(role, transcript).finalize().into_bytes().into()
}

pub fn verify(role: Role, transcript: &[u8], proof: &[u8]) -> bool {
    signer(role, transcript).verify_slice(proof).is_ok()
}

/// Deterministic transport with a bounded fragment size and explicit EOF.
pub struct Fragments<'a> {
    pub remaining: &'a [u8],
    pub width: std::num::NonZeroUsize,
}

impl<'a> Iterator for Fragments<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining.is_empty() {
            return None;
        }
        let (part, rest) = self
            .remaining
            .split_at(self.width.get().min(self.remaining.len()));
        self.remaining = rest;
        Some(part)
    }
}
