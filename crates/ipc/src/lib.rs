//! Control-plane framing and scheduling. The public host has no authentication backend.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

mod auth;
pub mod wire;
use auth::{Authority, Deny, Hello};
pub use auth::{ClientAuthentication, Scope, ServerAuthentication, Transcript};
use sha2::{Digest, Sha256};
use wire::{Frame, Id, Kind, ZERO, field};

pub const MAX_CONNECTIONS: usize = 8;
pub const MAX_PENDING_PER_CONNECTION: usize = 4;
pub const MAX_QUEUED: usize = 16;
pub const MAX_REQUEST_IDS: usize = 32;
const MAX_FENCES: usize = 16;
const HANDSHAKE_TTL: u64 = 30;
const SESSION_TTL: u64 = 300;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Truncated,
    TrailingBytes,
    FrameTooLarge,
    BadEncoding,
    Version,
    UnknownFlags,
    UnknownMessage,
    CapabilityUnavailable,
    Denied,
    Expired,
    State,
    Binding,
    Sequence,
    Conflict,
    Replay,
    Backpressure,
    Exhausted,
    Clock,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Disconnected,
    Connected,
    Negotiating,
    Authenticating,
    Established,
    Closing,
    Closed,
    Rejected,
    Expired,
    Quarantined,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Connection {
    incarnation: Id,
    ordinal: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RequestState {
    Queued,
    Dispatched,
    Dropped,
    CancelRequested,
    Unknown,
    /// Reserved for an independently settled metadata result, never native output.
    SettledMetadata,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Outcome {
    Reply(Frame),
    Queued,
    Duplicate(RequestState),
    Dropped,
    CancelRequested,
    Closing,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    Probe,
    Inspect(Id),
}

/// A control-plane scheduling descriptor. It conveys no business authority.
#[derive(Debug, Eq, PartialEq)]
pub struct Scheduled {
    connection: Connection,
    request_id: Id,
    scope: Scope,
    principal: Id,
    epoch: u64,
    request: Request,
}

impl Scheduled {
    pub fn request(&self) -> Request {
        self.request
    }
    pub fn request_id(&self) -> Id {
        self.request_id
    }
    pub fn scope(&self) -> Scope {
        self.scope
    }
    pub fn principal(&self) -> Id {
        self.principal
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

struct AuthenticatedSession {
    scope: Scope,
    principal: Id,
    epoch: u64,
}

struct Entry {
    digest: [u8; 32],
    request: Request,
    state: RequestState,
}

struct Slot {
    id: Id,
    channel: [u8; 32],
    state: State,
    deadline: u64,
    next_seq: u64,
    challenge: Option<Transcript>,
    auth: Option<AuthenticatedSession>,
    requests: BTreeMap<Id, Entry>,
    queue: VecDeque<Id>,
}

impl Slot {
    fn accepting(&self) -> bool {
        matches!(
            self.state,
            State::Connected | State::Negotiating | State::Authenticating | State::Established
        )
    }

    fn invalidate(&mut self, state: State, lost: bool) {
        self.state = state;
        self.challenge = None;
        self.auth = None;
        for id in self.queue.drain(..) {
            self.requests.get_mut(&id).expect("queued id exists").state = RequestState::Dropped;
        }
        for entry in self.requests.values_mut() {
            if matches!(
                entry.state,
                RequestState::Dispatched | RequestState::CancelRequested
            ) {
                entry.state = if lost {
                    RequestState::Unknown
                } else {
                    RequestState::CancelRequested
                };
            }
        }
    }
}

/// Single-owner scheduler. The authenticated assembly is intentionally absent from the public API.
pub struct Core {
    realm: Id,
    incarnation: Id,
    authority: Box<dyn Authority>,
    last_tick: u64,
    ordinal: u64,
    sessions: BTreeMap<Connection, Slot>,
    fences: BTreeSet<Scope>,
    realm_blocked: bool,
    active: Option<(Connection, Id)>,
    cursor: Option<Connection>,
}

impl Default for Core {
    fn default() -> Self {
        Self::new(ZERO, ZERO, Box::new(Deny))
    }
}

impl Core {
    fn new(realm: Id, incarnation: Id, authority: Box<dyn Authority>) -> Self {
        Self {
            realm,
            incarnation,
            authority,
            last_tick: 0,
            ordinal: 0,
            sessions: BTreeMap::new(),
            fences: BTreeSet::new(),
            realm_blocked: false,
            active: None,
            cursor: None,
        }
    }

    fn advance(&mut self, now: u64) -> Result<(), Error> {
        if now < self.last_tick {
            return Err(Error::Clock);
        }
        self.last_tick = now;
        for slot in self.sessions.values_mut() {
            if now >= slot.deadline && slot.accepting() {
                slot.invalidate(State::Expired, true);
            }
        }
        Ok(())
    }

    /// Channel binding comes from the trusted transport, never from a frame.
    pub fn connect(&mut self, channel: [u8; 32], now: u64) -> Result<Connection, Error> {
        if self.realm == ZERO || self.incarnation == ZERO {
            return Err(Error::CapabilityUnavailable);
        }
        self.advance(now)?;
        if self.realm_blocked {
            return Err(Error::Denied);
        }
        if channel == [0; 32] || self.sessions.values().any(|slot| slot.channel == channel) {
            return Err(Error::Binding);
        }
        if self.sessions.len() >= MAX_CONNECTIONS {
            return Err(Error::Backpressure);
        }
        let ordinal = self.ordinal.checked_add(1).ok_or(Error::Exhausted)?;
        // Keep incarnation and connection identity in the namespace of the ordinal.
        let mut hash = Sha256::new();
        hash.update(b"YXIP/session/v1\0");
        hash.update(self.incarnation);
        hash.update(channel);
        hash.update(ordinal.to_be_bytes());
        let id = field::<16>(&hash.finalize(), 0);
        self.ordinal = ordinal;
        if id == ZERO || self.sessions.values().any(|slot| slot.id == id) {
            return Err(Error::Binding);
        }
        let connection = Connection {
            incarnation: self.incarnation,
            ordinal,
        };
        let deadline = now.checked_add(HANDSHAKE_TTL).ok_or(Error::Clock)?;
        self.sessions.insert(
            connection,
            Slot {
                id,
                channel,
                deadline,
                state: State::Connected,
                next_seq: 0,
                challenge: None,
                auth: None,
                requests: BTreeMap::new(),
                queue: VecDeque::new(),
            },
        );
        Ok(connection)
    }

    pub fn state(&self, connection: Connection) -> State {
        self.sessions
            .get(&connection)
            .map_or(State::Disconnected, |s| s.state)
    }

    pub fn request_state(&self, connection: Connection, request: Id) -> Option<RequestState> {
        self.sessions
            .get(&connection)?
            .requests
            .get(&request)
            .map(|e| e.state)
    }

    pub fn receive(
        &mut self,
        connection: Connection,
        bytes: &[u8],
        now: u64,
    ) -> Result<Outcome, Error> {
        self.advance(now)?;
        let mut slot = self.sessions.remove(&connection).ok_or(Error::Binding)?;
        let result = Frame::decode(bytes).and_then(|frame| self.handle(&mut slot, frame, now));
        if let Err(error) = result {
            let terminal = match error {
                Error::Backpressure | Error::State => None,
                Error::Expired => Some(State::Expired),
                Error::Conflict | Error::Replay | Error::Sequence | Error::Binding => {
                    Some(State::Quarantined)
                }
                _ => Some(State::Rejected),
            };
            if let Some(state) = terminal.filter(|_| slot.accepting()) {
                slot.invalidate(state, true);
            }
        }
        self.sessions.insert(connection, slot);
        result
    }

    fn handle(&self, slot: &mut Slot, frame: Frame, now: u64) -> Result<Outcome, Error> {
        match slot.state {
            State::Connected => self.negotiate(slot, &frame),
            State::Authenticating => self.authenticate(slot, &frame, now),
            State::Established => self.admit(slot, &frame),
            State::Expired => Err(Error::Expired),
            _ => Err(Error::State),
        }
    }

    fn negotiate(&self, slot: &mut Slot, frame: &Frame) -> Result<Outcome, Error> {
        slot.state = State::Negotiating;
        let hello = Hello::parse(frame)?;
        if self.realm_blocked
            || hello.scope.realm != self.realm
            || self.fences.contains(&hello.scope)
        {
            return Err(Error::Denied);
        }
        let nonce = self.authority.fresh_nonce()?;
        if nonce == [0; 32] || nonce == hello.nonce {
            return Err(Error::Binding);
        }
        let transcript = Transcript::new(hello, slot.id, self.incarnation, nonce, slot.channel);
        let proof = self.authority.prove_server(&transcript)?;
        let mut payload = Vec::with_capacity(80);
        payload.extend(self.incarnation);
        payload.extend(nonce);
        payload.extend(proof);
        let reply = Frame::new(Kind::Challenge, slot.id, ZERO, 0, &payload)?;
        slot.challenge = Some(transcript);
        slot.state = State::Authenticating;
        Ok(Outcome::Reply(reply))
    }

    fn authenticate(&self, slot: &mut Slot, frame: &Frame, now: u64) -> Result<Outcome, Error> {
        let transcript = slot.challenge.take().ok_or(Error::State)?;
        if frame.session != slot.id {
            return Err(Error::Binding);
        }
        if frame.kind != Kind::Authenticate
            || frame.request != ZERO
            || frame.seq != 1
            || frame.payload().len() != 32
        {
            return Err(Error::BadEncoding);
        }
        let hello = transcript.hello;
        self.authority.verify_client(&transcript, frame.payload())?;
        if !self.current(hello.scope, hello.principal, hello.epoch) {
            return Err(Error::Denied);
        }
        slot.deadline = now.checked_add(SESSION_TTL).ok_or(Error::Clock)?;
        slot.auth = Some(AuthenticatedSession {
            scope: hello.scope,
            principal: hello.principal,
            epoch: hello.epoch,
        });
        slot.next_seq = 2;
        slot.state = State::Established;
        Ok(Outcome::Reply(Frame::new(
            Kind::Ready,
            slot.id,
            ZERO,
            1,
            &[],
        )?))
    }

    fn current(&self, scope: Scope, principal: Id, epoch: u64) -> bool {
        !self.realm_blocked
            && scope.realm == self.realm
            && !self.fences.contains(&scope)
            && self.authority.current_epoch(scope, principal) == Some(epoch)
    }

    fn admit(&self, slot: &mut Slot, frame: &Frame) -> Result<Outcome, Error> {
        if frame.session != slot.id {
            return Err(Error::Binding);
        }
        if frame.seq != slot.next_seq {
            return Err(Error::Sequence);
        }
        let next = slot.next_seq.checked_add(1).ok_or(Error::Exhausted)?;
        let auth = slot.auth.as_ref().ok_or(Error::State)?;
        if !self.current(auth.scope, auth.principal, auth.epoch) {
            return Err(Error::Denied);
        }
        let outcome = match frame.kind {
            Kind::Request => self.enqueue(slot, frame)?,
            Kind::Cancel if frame.request != ZERO && frame.payload().is_empty() => {
                Self::cancel(slot, frame.request)?
            }
            Kind::Close if frame.request == ZERO && frame.payload().is_empty() => {
                slot.invalidate(State::Closing, false);
                Outcome::Closing
            }
            Kind::Hello | Kind::Challenge | Kind::Authenticate | Kind::Ready => {
                return Err(Error::Replay);
            }
            _ => return Err(Error::BadEncoding),
        };
        slot.next_seq = next;
        Ok(outcome)
    }

    fn enqueue(&self, slot: &mut Slot, frame: &Frame) -> Result<Outcome, Error> {
        let auth = slot.auth.as_ref().ok_or(Error::State)?;
        if frame.request == ZERO || frame.payload().len() < 49 {
            return Err(Error::BadEncoding);
        }
        let scope = Scope::decode(frame.payload());
        if scope != auth.scope {
            return Err(Error::Binding);
        }
        let request = match (frame.payload()[48], frame.payload().len()) {
            (0, 49) => Request::Probe,
            (1, 65) if field::<16>(frame.payload(), 49) != ZERO => {
                Request::Inspect(field(frame.payload(), 49))
            }
            _ => return Err(Error::BadEncoding),
        };
        let mut digest = Sha256::new();
        digest.update(b"YXIP/request/v1\0");
        digest.update(auth.principal);
        digest.update(auth.epoch.to_be_bytes());
        digest.update(frame.payload());
        let digest = digest.finalize().into();
        if let Some(entry) = slot.requests.get(&frame.request) {
            return if entry.digest == digest {
                Ok(Outcome::Duplicate(entry.state))
            } else {
                Err(Error::Conflict)
            };
        }
        if slot.requests.len() == MAX_REQUEST_IDS {
            return Err(Error::Exhausted);
        }
        let local_pending = slot
            .requests
            .values()
            .filter(|e| {
                matches!(
                    e.state,
                    RequestState::Queued
                        | RequestState::Dispatched
                        | RequestState::CancelRequested
                        | RequestState::Unknown
                )
            })
            .count();
        if local_pending >= MAX_PENDING_PER_CONNECTION
            || self.sessions.values().map(|s| s.queue.len()).sum::<usize>() + slot.queue.len()
                >= MAX_QUEUED
        {
            return Err(Error::Backpressure);
        }
        slot.requests.insert(
            frame.request,
            Entry {
                digest,
                request,
                state: RequestState::Queued,
            },
        );
        slot.queue.push_back(frame.request);
        Ok(Outcome::Queued)
    }

    fn cancel(slot: &mut Slot, id: Id) -> Result<Outcome, Error> {
        let entry = slot.requests.get_mut(&id).ok_or(Error::State)?;
        Ok(match entry.state {
            RequestState::Queued => {
                slot.queue.retain(|queued| *queued != id);
                entry.state = RequestState::Dropped;
                Outcome::Dropped
            }
            RequestState::Dispatched | RequestState::CancelRequested => {
                entry.state = RequestState::CancelRequested;
                Outcome::CancelRequested
            }
            state => Outcome::Duplicate(state),
        })
    }

    /// Host security events bypass the request queue. No frame can issue or clear a fence.
    pub fn deletion_fence(&mut self, scope: Scope) -> Result<(), Error> {
        if scope.realm != self.realm || self.realm == ZERO {
            return Err(Error::Binding);
        }
        if !scope.valid() {
            self.realm_blocked = true;
            self.invalidate_authority();
            return Err(Error::BadEncoding);
        }
        if self.fences.len() == MAX_FENCES && !self.fences.contains(&scope) {
            self.realm_blocked = true;
        } else {
            self.fences.insert(scope);
        }
        self.invalidate_authority();
        Ok(())
    }

    /// Apply current epoch/revocation changes ahead of another dispatch.
    pub fn authorization_changed(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        self.invalidate_authority();
        Ok(())
    }

    fn invalidate_authority(&mut self) {
        let authority = &self.authority;
        for slot in self.sessions.values_mut() {
            let identity = slot
                .auth
                .as_ref()
                .map(|a| (a.scope, a.principal, a.epoch))
                .or_else(|| {
                    slot.challenge
                        .as_ref()
                        .map(|t| (t.hello.scope, t.hello.principal, t.hello.epoch))
                });
            let invalid = self.realm_blocked
                || identity.is_some_and(|(scope, principal, epoch)| {
                    self.fences.contains(&scope)
                        || authority.current_epoch(scope, principal) != Some(epoch)
                });
            if invalid && slot.accepting() {
                slot.invalidate(State::Closing, false);
            }
        }
    }

    pub fn dispatch(&mut self, now: u64) -> Result<Option<Scheduled>, Error> {
        self.authorization_changed(now)?;
        if self.active.is_some() {
            return Ok(None);
        }
        let eligible = |slot: &Slot| slot.state == State::Established && !slot.queue.is_empty();
        let chosen = self
            .sessions
            .iter()
            .find(|(key, slot)| self.cursor.is_none_or(|cursor| **key > cursor) && eligible(slot))
            .or_else(|| self.sessions.iter().find(|(_, slot)| eligible(slot)))
            .map(|(key, _)| *key);
        let Some(connection) = chosen else {
            return Ok(None);
        };
        let slot = self.sessions.get_mut(&connection).ok_or(Error::State)?;
        let auth = slot.auth.as_ref().ok_or(Error::State)?;
        let request_id = slot.queue.pop_front().ok_or(Error::State)?;
        let entry = slot.requests.get_mut(&request_id).ok_or(Error::State)?;
        entry.state = RequestState::Dispatched;
        self.active = Some((connection, request_id));
        self.cursor = Some(connection);
        Ok(Some(Scheduled {
            connection,
            request_id,
            scope: auth.scope,
            principal: auth.principal,
            epoch: auth.epoch,
            request: entry.request,
        }))
    }

    /// Loss of a transport is not evidence that dispatched work stopped.
    pub fn disconnect(&mut self, connection: Connection) -> Result<(), Error> {
        let slot = self.sessions.get_mut(&connection).ok_or(Error::Binding)?;
        slot.invalidate(State::Closed, true);
        Ok(())
    }

    pub fn retire(&mut self, connection: Connection) -> Result<(), Error> {
        if self.active.is_some_and(|(owner, _)| owner == connection) {
            return Err(Error::State);
        }
        match self.state(connection) {
            State::Closed => {
                self.sessions.remove(&connection);
                Ok(())
            }
            _ => Err(Error::State),
        }
    }
}

#[cfg(test)]
mod tests;
