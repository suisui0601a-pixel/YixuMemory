use crate::Error;
use crate::wire::{Frame, Id, Kind, VERSION, ZERO, field};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Scope {
    pub realm: Id,
    pub tenant: Id,
    pub space: Id,
}

impl Scope {
    pub(crate) fn valid(self) -> bool {
        ![self.realm, self.tenant, self.space].contains(&ZERO)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Self {
        Self {
            realm: field(bytes, 0),
            tenant: field(bytes, 16),
            space: field(bytes, 32),
        }
    }

    pub fn encode(self) -> [u8; 48] {
        let mut bytes = [0; 48];
        bytes[..16].copy_from_slice(&self.realm);
        bytes[16..32].copy_from_slice(&self.tenant);
        bytes[32..].copy_from_slice(&self.space);
        bytes
    }
}

#[derive(Clone, Copy)]
pub(crate) struct Hello {
    pub scope: Scope,
    pub principal: Id,
    pub epoch: u64,
    pub nonce: [u8; 32],
}

impl Hello {
    pub fn parse(frame: &Frame) -> Result<Self, Error> {
        let bytes = frame.payload();
        if frame.kind != Kind::Hello
            || frame.session != ZERO
            || frame.request != ZERO
            || frame.seq != 0
            || bytes.len() != 106
        {
            return Err(Error::BadEncoding);
        }
        if u16::from_be_bytes(field(bytes, 0)) != VERSION {
            return Err(Error::Version);
        }
        let scope = Scope::decode(&bytes[2..]);
        let principal = field(bytes, 50);
        let epoch = u64::from_be_bytes(field(bytes, 66));
        let nonce = field(bytes, 74);
        if !scope.valid() || principal == ZERO || nonce == [0; 32] {
            return Err(Error::BadEncoding);
        }
        Ok(Self {
            scope,
            principal,
            epoch,
            nonce,
        })
    }
}

/// Canonical signed statement, with an authenticated out-of-band channel binding.
/// A Transcript is descriptive only and cannot mint an authenticated session.
pub struct Transcript {
    pub(crate) hello: Hello,
    session: Id,
    incarnation: Id,
    nonce: [u8; 32],
    channel: [u8; 32],
}

impl Transcript {
    pub(crate) fn new(
        hello: Hello,
        session: Id,
        incarnation: Id,
        nonce: [u8; 32],
        channel: [u8; 32],
    ) -> Self {
        Self {
            hello,
            session,
            incarnation,
            nonce,
            channel,
        }
    }

    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut bytes = b"YXIP/handshake/v1\0".to_vec();
        bytes.extend(VERSION.to_be_bytes()); // offered
        bytes.extend(VERSION.to_be_bytes()); // selected
        bytes.extend(self.hello.scope.encode());
        bytes.extend(self.hello.principal);
        bytes.extend(self.hello.epoch.to_be_bytes());
        bytes.extend(self.hello.nonce);
        bytes.extend(self.nonce);
        bytes.extend(self.session);
        bytes.extend(self.incarnation);
        bytes.extend(self.channel);
        bytes
    }

    pub fn scope(&self) -> Scope {
        self.hello.scope
    }
    pub fn principal(&self) -> Id {
        self.hello.principal
    }
    pub fn epoch(&self) -> u64 {
        self.hello.epoch
    }

    /// Client-side binding check. The expected incarnation and channel come
    /// from an authenticated adapter. This function returns no server-side authority.
    pub fn verify_challenge(
        hello: &Frame,
        challenge: &Frame,
        expected_incarnation: Id,
        channel: [u8; 32],
        verifier: &dyn ServerAuthentication,
    ) -> Result<Self, Error> {
        let hello = Hello::parse(hello)?;
        if challenge.kind != Kind::Challenge
            || challenge.session == ZERO
            || challenge.request != ZERO
            || challenge.seq != 0
            || challenge.payload().len() != 80
        {
            return Err(Error::BadEncoding);
        }
        let incarnation = field(challenge.payload(), 0);
        let nonce = field(challenge.payload(), 16);
        if expected_incarnation == ZERO
            || incarnation != expected_incarnation
            || channel == [0; 32]
            || nonce == [0; 32]
            || nonce == hello.nonce
        {
            return Err(Error::Binding);
        }
        let transcript = Self::new(hello, challenge.session, incarnation, nonce, channel);
        verifier.verify_server(&transcript, &challenge.payload()[48..])?;
        Ok(transcript)
    }
}

/// Future verifier must authenticate the actual peer and verify a client-role
/// proof of the entire transcript, including the channel and claimed identity.
pub trait ClientAuthentication {
    fn verify_client(&self, transcript: &Transcript, proof: &[u8]) -> Result<(), Error>;
}

/// Future server authentication must use an authenticated trust root and a
/// distinct server-role proof domain. A successful proof is not a business Grant.
pub trait ServerAuthentication {
    fn prove_server(&self, transcript: &Transcript) -> Result<[u8; 32], Error>;
    fn verify_server(&self, transcript: &Transcript, proof: &[u8]) -> Result<(), Error>;
}

pub(crate) trait Authority: ClientAuthentication + ServerAuthentication {
    fn fresh_nonce(&self) -> Result<[u8; 32], Error>;
    fn current_epoch(&self, scope: Scope, principal: Id) -> Option<u64>;
}

pub(crate) struct Deny;

impl ClientAuthentication for Deny {
    fn verify_client(&self, _: &Transcript, _: &[u8]) -> Result<(), Error> {
        Err(Error::CapabilityUnavailable)
    }
}

impl ServerAuthentication for Deny {
    fn prove_server(&self, _: &Transcript) -> Result<[u8; 32], Error> {
        Err(Error::CapabilityUnavailable)
    }

    fn verify_server(&self, _: &Transcript, _: &[u8]) -> Result<(), Error> {
        Err(Error::CapabilityUnavailable)
    }
}

impl Authority for Deny {
    fn fresh_nonce(&self) -> Result<[u8; 32], Error> {
        Err(Error::CapabilityUnavailable)
    }
    fn current_epoch(&self, _: Scope, _: Id) -> Option<u64> {
        None
    }
}
