use super::*;
use std::cell::Cell;
use std::rc::Rc;
use wire::{FrameReader, HEADER, MAX_FRAME, MAX_PAYLOAD, VERSION};
use yixu_ipc_test_support::{Fragments, Role, prove, verify};

const REALM: Id = [1; 16];
const INC: Id = [2; 16];
const PRINCIPAL: Id = [3; 16];
const SCOPE: Scope = Scope {
    realm: REALM,
    tenant: [4; 16],
    space: [5; 16],
};

struct FixtureAuthority {
    epoch: Rc<Cell<Option<u64>>>,
    nonce: Cell<u8>,
}

impl ClientAuthentication for FixtureAuthority {
    fn verify_client(&self, t: &Transcript, proof: &[u8]) -> Result<(), Error> {
        verify(Role::Client, &t.canonical_bytes(), proof)
            .then_some(())
            .ok_or(Error::Denied)
    }
}
impl ServerAuthentication for FixtureAuthority {
    fn prove_server(&self, t: &Transcript) -> Result<[u8; 32], Error> {
        Ok(prove(Role::Server, &t.canonical_bytes()))
    }
    fn verify_server(&self, t: &Transcript, proof: &[u8]) -> Result<(), Error> {
        verify(Role::Server, &t.canonical_bytes(), proof)
            .then_some(())
            .ok_or(Error::Denied)
    }
}
impl Authority for FixtureAuthority {
    fn fresh_nonce(&self) -> Result<[u8; 32], Error> {
        let n = self.nonce.get().checked_add(1).ok_or(Error::Exhausted)?;
        self.nonce.set(n);
        Ok([n; 32])
    }
    fn current_epoch(&self, scope: Scope, principal: Id) -> Option<u64> {
        (scope == SCOPE && principal == PRINCIPAL)
            .then(|| self.epoch.get())
            .flatten()
    }
}

fn fixture(inc: Id) -> (Core, Rc<Cell<Option<u64>>>) {
    let epoch = Rc::new(Cell::new(Some(7)));
    (
        Core::new(
            REALM,
            inc,
            Box::new(FixtureAuthority {
                epoch: epoch.clone(),
                nonce: Cell::new(100),
            }),
        ),
        epoch,
    )
}

fn frame(kind: Kind, sid: Id, rid: Id, seq: u64, body: &[u8]) -> Frame {
    Frame::new(kind, sid, rid, seq, body).unwrap()
}

fn hello(scope: Scope, principal: Id, version: u16, epoch: u64) -> Frame {
    let mut body = Vec::new();
    body.extend(version.to_be_bytes());
    body.extend(scope.encode());
    body.extend(principal);
    body.extend(epoch.to_be_bytes());
    body.extend([99; 32]);
    frame(Kind::Hello, ZERO, ZERO, 0, &body)
}

fn initial_hello() -> Frame {
    hello(SCOPE, PRINCIPAL, VERSION, 7)
}

fn send(core: &mut Core, connection: Connection, f: &Frame, now: u64) -> Result<Outcome, Error> {
    core.receive(connection, &f.encode(), now)
}

fn challenge(core: &mut Core, c: Connection, h: &Frame) -> Frame {
    let Outcome::Reply(reply) = send(core, c, h, 1).unwrap() else {
        panic!("expected challenge")
    };
    assert_eq!(core.state(c), State::Authenticating);
    reply
}

fn proof(core: &Core, h: &Frame, reply: &Frame, channel: [u8; 32]) -> Frame {
    let t = Transcript::verify_challenge(h, reply, core.incarnation, channel, &*core.authority)
        .unwrap();
    frame(
        Kind::Authenticate,
        reply.session,
        ZERO,
        1,
        &prove(Role::Client, &t.canonical_bytes()),
    )
}

fn establish(core: &mut Core, channel: u8) -> (Connection, Id) {
    let c = core.connect([channel; 32], 1).unwrap();
    assert_eq!(core.state(c), State::Connected);
    let h = initial_hello();
    let reply = challenge(core, c, &h);
    let a = proof(core, &h, &reply, [channel; 32]);
    assert_eq!(
        send(core, c, &a, 1),
        Ok(Outcome::Reply(frame(
            Kind::Ready,
            reply.session,
            ZERO,
            1,
            &[]
        )))
    );
    assert_eq!(core.state(c), State::Established);
    (c, reply.session)
}

fn request(sid: Id, id: u8, seq: u64, scope: Scope, target: Option<Id>) -> Frame {
    let mut body = scope.encode().to_vec();
    if let Some(target) = target {
        body.push(1);
        body.extend(target);
    } else {
        body.push(0);
    }
    frame(Kind::Request, sid, [id; 16], seq, &body)
}

// Metadata simulator only. No native terminal evidence type exists in this crate.
fn settle_metadata(core: &mut Core, dispatched: Scheduled) {
    let key = (dispatched.connection, dispatched.request_id);
    assert_eq!(core.active, Some(key));
    let entry = core
        .sessions
        .get_mut(&key.0)
        .unwrap()
        .requests
        .get_mut(&key.1)
        .unwrap();
    assert_eq!(entry.state, RequestState::Dispatched);
    entry.state = RequestState::SettledMetadata;
    core.active = None;
}

#[test]
fn canonical_wire_and_bounded_fragmentation() {
    let f = frame(
        Kind::Request,
        [2; 16],
        [3; 16],
        0x0102030405060708,
        &[8; MAX_PAYLOAD],
    );
    let bytes = f.encode();
    assert_eq!(bytes.len(), MAX_FRAME);
    assert_eq!(&bytes[..8], b"YXIP\x00\x01\x05\x00");
    assert_eq!(&bytes[40..52], &[1, 2, 3, 4, 5, 6, 7, 8, 0, 0, 2, 0]);
    assert_eq!(Frame::decode(&bytes), Ok(f.clone()));
    for width in [1, 7, HEADER, MAX_FRAME] {
        let mut reader = FrameReader::default();
        let parts = Fragments {
            remaining: &bytes,
            width: width.try_into().unwrap(),
        };
        let mut output = Vec::new();
        for chunk in parts {
            let (used, next) = reader.feed(chunk).unwrap();
            assert_eq!(used, chunk.len());
            output.extend(next);
        }
        assert_eq!(reader.finish(), Ok(()));
        assert_eq!(output, std::slice::from_ref(&f));
    }
    let small = initial_hello().encode();
    let two = [small.as_slice(), small.as_slice()].concat();
    let mut reader = FrameReader::default();
    let (used, first) = reader.feed(&two).unwrap();
    assert_eq!(used, small.len());
    assert_eq!(first, Some(initial_hello()));
    assert_eq!(
        reader.feed(&two[used..]).unwrap(),
        (small.len(), Some(initial_hello()))
    );
}

#[test]
fn rejects_truncation_trailing_invalid_and_overflow_lengths() {
    let bytes = initial_hello().encode();
    for n in 0..bytes.len() {
        assert_eq!(Frame::decode(&bytes[..n]), Err(Error::Truncated));
    }
    for (offset, value, error) in [
        (0, 0, Error::BadEncoding),
        (5, 2, Error::Version),
        (6, 255, Error::UnknownMessage),
        (7, 128, Error::UnknownFlags),
    ] {
        let mut bad = bytes.clone();
        bad[offset] = value;
        assert_eq!(Frame::decode(&bad), Err(error));
    }
    for len in [513u32, u32::MAX] {
        let mut bad = bytes.clone();
        bad[48..52].copy_from_slice(&len.to_be_bytes());
        assert_eq!(Frame::decode(&bad), Err(Error::FrameTooLarge));
        let mut reader = FrameReader::default();
        assert_eq!(reader.feed(&bad), Err(Error::FrameTooLarge));
        assert_eq!(reader.feed(&bytes), Err(Error::State));
    }
    let mut reader = FrameReader::default();
    assert_eq!(reader.feed(&[0; MAX_FRAME + 1]), Err(Error::FrameTooLarge));
    let mut reader = FrameReader::default();
    reader.feed(&bytes[..bytes.len() - 1]).unwrap();
    assert_eq!(reader.finish(), Err(Error::Truncated));
    let mut trailing = bytes;
    trailing.push(0);
    assert_eq!(Frame::decode(&trailing), Err(Error::TrailingBytes));
    assert_eq!(
        Frame::new(Kind::Request, ZERO, ZERO, 0, &[0; MAX_PAYLOAD + 1]),
        Err(Error::FrameTooLarge)
    );
}

#[test]
fn default_assembly_is_closed() {
    let mut core = Core::default();
    assert_eq!(core.connect([1; 32], 1), Err(Error::CapabilityUnavailable));
    assert_eq!(core.dispatch(1), Ok(None));
}

#[test]
fn mutual_auth_checks_both_roles_and_binding() {
    let (mut core, _) = fixture(INC);
    let channel = [11; 32];
    let c = core.connect(channel, 1).unwrap();
    let h = initial_hello();
    let reply = challenge(&mut core, c, &h);
    let mut fake = reply.payload().to_vec();
    fake[79] ^= 1;
    assert!(matches!(
        Transcript::verify_challenge(
            &h,
            &frame(Kind::Challenge, reply.session, ZERO, 0, &fake),
            INC,
            channel,
            &*core.authority
        ),
        Err(Error::Denied)
    ));
    assert!(matches!(
        Transcript::verify_challenge(&h, &reply, [42; 16], channel, &*core.authority),
        Err(Error::Binding)
    ));
    assert!(matches!(
        Transcript::verify_challenge(&h, &reply, INC, [10; 32], &*core.authority),
        Err(Error::Denied)
    ));
    let reflected = frame(
        Kind::Authenticate,
        reply.session,
        ZERO,
        1,
        &reply.payload()[48..],
    );
    assert_eq!(send(&mut core, c, &reflected, 1), Err(Error::Denied));
    assert_eq!(core.state(c), State::Rejected);
    assert_eq!(core.dispatch(1), Ok(None));
    establish(&mut core, 12);
}

#[test]
fn each_transcript_component_changes_both_proofs() {
    let hello = Hello::parse(&initial_hello()).unwrap();
    let sid = [21; 16];
    let nonce = [22; 32];
    let channel = [23; 32];
    let base = Transcript::new(hello, sid, INC, nonce, channel);
    let changed = [
        Transcript::new(
            Hello {
                scope: Scope {
                    realm: [24; 16],
                    ..SCOPE
                },
                ..hello
            },
            sid,
            INC,
            nonce,
            channel,
        ),
        Transcript::new(
            Hello {
                scope: Scope {
                    tenant: [24; 16],
                    ..SCOPE
                },
                ..hello
            },
            sid,
            INC,
            nonce,
            channel,
        ),
        Transcript::new(
            Hello {
                scope: Scope {
                    space: [24; 16],
                    ..SCOPE
                },
                ..hello
            },
            sid,
            INC,
            nonce,
            channel,
        ),
        Transcript::new(
            Hello {
                principal: [24; 16],
                ..hello
            },
            sid,
            INC,
            nonce,
            channel,
        ),
        Transcript::new(Hello { epoch: 8, ..hello }, sid, INC, nonce, channel),
        Transcript::new(
            Hello {
                nonce: [24; 32],
                ..hello
            },
            sid,
            INC,
            nonce,
            channel,
        ),
        Transcript::new(hello, [24; 16], INC, nonce, channel),
        Transcript::new(hello, sid, [24; 16], nonce, channel),
        Transcript::new(hello, sid, INC, [24; 32], channel),
        Transcript::new(hello, sid, INC, nonce, [24; 32]),
    ];
    for role in [Role::Client, Role::Server] {
        let base_proof = prove(role, &base.canonical_bytes());
        for transcript in &changed {
            assert!(!verify(role, &transcript.canonical_bytes(), &base_proof));
        }
    }
}

#[test]
fn incompatible_offer_has_no_downgrade_path() {
    let (mut core, _) = fixture(INC);
    for (index, offered) in [0, 2, u16::MAX].into_iter().enumerate() {
        let c = core.connect([20 + index as u8; 32], 1).unwrap();
        assert_eq!(
            send(&mut core, c, &hello(SCOPE, PRINCIPAL, offered, 7), 1),
            Err(Error::Version)
        );
        assert_eq!(core.state(c), State::Rejected);
        core.disconnect(c).unwrap();
        core.retire(c).unwrap();
    }
}

#[test]
fn replayed_proof_cannot_cross_connections_even_with_sid_rewritten() {
    let (mut core, _) = fixture(INC);
    let h = initial_hello();
    let c1 = core.connect([11; 32], 1).unwrap();
    let r1 = challenge(&mut core, c1, &h);
    let mut a1 = proof(&core, &h, &r1, [11; 32]);
    let c2 = core.connect([12; 32], 1).unwrap();
    let r2 = challenge(&mut core, c2, &h);
    assert_ne!(r1.session, r2.session);
    a1.session = r2.session;
    assert_eq!(send(&mut core, c2, &a1, 1), Err(Error::Denied));
    assert_eq!(core.dispatch(1), Ok(None));
}

#[test]
fn active_session_id_collision_is_rejected_without_replacing_owner() {
    let (mut core, _) = fixture(INC);
    let (old, sid) = establish(&mut core, 11);
    let candidate = core.connect([12; 32], 1).unwrap();
    let candidate_id = core.sessions[&candidate].id;
    core.disconnect(candidate).unwrap();
    core.retire(candidate).unwrap();
    // Force the next generated ID to collide with an occupied ID.
    core.sessions.get_mut(&old).unwrap().id = candidate_id;
    core.ordinal -= 1;
    assert_eq!(core.connect([12; 32], 1), Err(Error::Binding));
    assert_eq!(core.sessions.len(), 1);
    core.sessions.get_mut(&old).unwrap().id = sid;
    assert_eq!(
        send(&mut core, old, &request(sid, 1, 2, SCOPE, None), 1),
        Ok(Outcome::Queued)
    );
}

#[test]
fn stale_scope_epoch_and_principal_fail() {
    let (mut core, epoch) = fixture(INC);
    let c = core.connect([11; 32], 1).unwrap();
    assert_eq!(
        send(
            &mut core,
            c,
            &hello(
                Scope {
                    realm: [44; 16],
                    ..SCOPE
                },
                PRINCIPAL,
                VERSION,
                7
            ),
            1
        ),
        Err(Error::Denied)
    );
    for (index, (scope, principal, claimed_epoch)) in [
        (
            Scope {
                tenant: [44; 16],
                ..SCOPE
            },
            PRINCIPAL,
            7,
        ),
        (
            Scope {
                space: [44; 16],
                ..SCOPE
            },
            PRINCIPAL,
            7,
        ),
        (SCOPE, [44; 16], 7),
        (SCOPE, PRINCIPAL, 6),
    ]
    .into_iter()
    .enumerate()
    {
        let channel = [20 + index as u8; 32];
        let c = core.connect(channel, 1).unwrap();
        let h = hello(scope, principal, VERSION, claimed_epoch);
        let r = challenge(&mut core, c, &h);
        let a = proof(&core, &h, &r, channel);
        assert_eq!(send(&mut core, c, &a, 1), Err(Error::Denied));
    }
    let c = core.connect([31; 32], 1).unwrap();
    let h = initial_hello();
    let r = challenge(&mut core, c, &h);
    epoch.set(Some(8));
    let a = proof(&core, &h, &r, [31; 32]);
    assert_eq!(send(&mut core, c, &a, 1), Err(Error::Denied));
    assert_eq!(core.dispatch(1), Ok(None));
}

#[test]
fn disconnection_and_deadline_invalidate_handshake() {
    let (mut core, _) = fixture(INC);
    let h = initial_hello();
    let c = core.connect([11; 32], 1).unwrap();
    let r = challenge(&mut core, c, &h);
    let a = proof(&core, &h, &r, [11; 32]);
    core.disconnect(c).unwrap();
    assert_eq!(send(&mut core, c, &a, 1), Err(Error::State));
    core.retire(c).unwrap();
    assert_eq!(core.state(c), State::Disconnected);
    let c = core.connect([12; 32], 1).unwrap();
    let r = challenge(&mut core, c, &h);
    let a = proof(&core, &h, &r, [12; 32]);
    assert_eq!(send(&mut core, c, &a, 31), Err(Error::Expired));
    assert_eq!(core.retire(c), Err(Error::State));
    assert_eq!(core.dispatch(30), Err(Error::Clock));
    assert_eq!(core.dispatch(31), Ok(None));
}

#[test]
fn stale_session_and_incarnation_never_restore() {
    let (mut old, _) = fixture(INC);
    let (c1, sid1) = establish(&mut old, 11);
    let captured = request(sid1, 1, 2, SCOPE, None);
    let (mut new, _) = fixture([81; 16]);
    let (c2, sid2) = establish(&mut new, 11);
    assert_ne!(sid1, sid2);
    assert_eq!(send(&mut new, c1, &captured, 1), Err(Error::Binding));
    assert_eq!(send(&mut new, c2, &captured, 1), Err(Error::Binding));
    assert_eq!(new.state(c2), State::Quarantined);
    assert_eq!(new.dispatch(1), Ok(None));
}

#[test]
fn duplicates_conflicts_and_wire_replay() {
    let (mut core, _) = fixture(INC);
    let (c, sid) = establish(&mut core, 11);
    assert_eq!(
        send(&mut core, c, &request(sid, 1, 2, SCOPE, None), 1),
        Ok(Outcome::Queued)
    );
    assert_eq!(
        send(&mut core, c, &request(sid, 1, 3, SCOPE, None), 1),
        Ok(Outcome::Duplicate(RequestState::Queued))
    );
    assert_eq!(
        send(&mut core, c, &request(sid, 1, 4, SCOPE, Some([5; 16])), 1),
        Err(Error::Conflict)
    );
    assert_eq!(core.state(c), State::Quarantined);
    assert_eq!(core.request_state(c, [1; 16]), Some(RequestState::Dropped));
    assert_eq!(core.dispatch(1), Ok(None));
    for wrong in [1, 3, u64::MAX] {
        let (c, sid) = establish(&mut core, (wrong as u8).saturating_add(20));
        assert_eq!(
            send(&mut core, c, &request(sid, 2, wrong, SCOPE, None), 1),
            Err(Error::Sequence)
        );
    }
}

#[test]
fn wrong_scope_on_an_established_channel_cannot_dispatch() {
    let (mut core, _) = fixture(INC);
    for (index, scope) in [
        Scope {
            realm: [9; 16],
            ..SCOPE
        },
        Scope {
            tenant: [9; 16],
            ..SCOPE
        },
        Scope {
            space: [9; 16],
            ..SCOPE
        },
    ]
    .into_iter()
    .enumerate()
    {
        let (c, sid) = establish(&mut core, 11 + index as u8);
        assert_eq!(
            send(&mut core, c, &request(sid, 1, 2, scope, None), 1),
            Err(Error::Binding)
        );
        assert_eq!(core.dispatch(1), Ok(None));
    }
}

#[test]
fn queue_connection_limits_backpressure_and_round_robin() {
    let (mut core, _) = fixture(INC);
    let connections: Vec<_> = (1..=MAX_CONNECTIONS)
        .map(|i| establish(&mut core, i as u8))
        .collect();
    assert_eq!(core.connect([40; 32], 1), Err(Error::Backpressure));
    for &(c, sid) in &connections[..4] {
        for i in 0..MAX_PENDING_PER_CONNECTION {
            assert_eq!(
                send(
                    &mut core,
                    c,
                    &request(sid, i as u8 + 1, i as u64 + 2, SCOPE, None),
                    1
                ),
                Ok(Outcome::Queued)
            );
        }
        assert_eq!(
            send(&mut core, c, &request(sid, 5, 6, SCOPE, None), 1),
            Err(Error::Backpressure)
        );
    }
    let (c5, sid5) = connections[4];
    let waiting = request(sid5, 1, 2, SCOPE, None);
    assert_eq!(send(&mut core, c5, &waiting, 1), Err(Error::Backpressure));
    let first = core.dispatch(1).unwrap().unwrap();
    assert_eq!(first.connection, connections[0].0);
    assert_eq!(core.dispatch(1), Ok(None));
    settle_metadata(&mut core, first);
    assert_eq!(send(&mut core, c5, &waiting, 1), Ok(Outcome::Queued));
    for (c, _) in &connections[1..5] {
        let work = core.dispatch(1).unwrap().unwrap();
        assert_eq!(work.connection, *c);
        settle_metadata(&mut core, work);
    }
    let work = core.dispatch(1).unwrap().unwrap();
    assert_eq!(work.connection, connections[0].0);
    assert_eq!(work.request_id(), [2; 16]);
}

#[test]
fn request_history_cannot_evict_an_old_id_to_make_room() {
    let (mut core, _) = fixture(INC);
    let (c, sid) = establish(&mut core, 11);
    for i in 1..=MAX_REQUEST_IDS as u8 {
        assert_eq!(
            send(
                &mut core,
                c,
                &request(sid, i, u64::from(i) + 1, SCOPE, None),
                1
            ),
            Ok(Outcome::Queued)
        );
        let work = core.dispatch(1).unwrap().unwrap();
        settle_metadata(&mut core, work);
    }
    assert_eq!(
        send(&mut core, c, &request(sid, 1, 34, SCOPE, None), 1),
        Ok(Outcome::Duplicate(RequestState::SettledMetadata))
    );
    assert_eq!(
        send(&mut core, c, &request(sid, 33, 35, SCOPE, None), 1),
        Err(Error::Exhausted)
    );
    assert_eq!(core.sessions[&c].requests.len(), MAX_REQUEST_IDS);
    assert_eq!(core.dispatch(1), Ok(None));
}

#[test]
fn revoke_fence_and_cancel_keep_active_accounted() {
    for fence_first in [false, true] {
        let (mut core, epoch) = fixture(INC);
        let (c, sid) = establish(&mut core, 11);
        for id in [1, 2] {
            assert_eq!(
                send(
                    &mut core,
                    c,
                    &request(sid, id, u64::from(id) + 1, SCOPE, None),
                    1
                ),
                Ok(Outcome::Queued)
            );
        }
        let work = core.dispatch(1).unwrap().unwrap();
        if fence_first {
            core.deletion_fence(SCOPE).unwrap();
        }
        let cancel = frame(Kind::Cancel, sid, [1; 16], 4, &[]);
        assert_eq!(
            send(&mut core, c, &cancel, 1),
            if fence_first {
                Err(Error::State)
            } else {
                Ok(Outcome::CancelRequested)
            }
        );
        epoch.set(None);
        core.authorization_changed(1).unwrap();
        assert_eq!(
            core.request_state(c, [1; 16]),
            Some(RequestState::CancelRequested)
        );
        assert_eq!(core.request_state(c, [2; 16]), Some(RequestState::Dropped));
        assert_eq!(
            send(&mut core, c, &request(sid, 3, 5, SCOPE, None), 1),
            Err(Error::State)
        );
        assert_eq!(core.dispatch(1), Ok(None));
        core.disconnect(c).unwrap();
        assert_eq!(core.request_state(c, [1; 16]), Some(RequestState::Unknown));
        assert_eq!(core.active, Some((work.connection, work.request_id)));
        assert_eq!(core.retire(c), Err(Error::State));
    }
}

#[test]
fn epoch_is_rechecked_on_admission_and_dispatch_without_event() {
    for dispatch_first in [false, true] {
        let (mut core, epoch) = fixture(INC);
        let (c, sid) = establish(&mut core, 11);
        let r = request(sid, 1, 2, SCOPE, None);
        if dispatch_first {
            assert_eq!(send(&mut core, c, &r, 1), Ok(Outcome::Queued));
        }
        epoch.set(Some(8));
        if !dispatch_first {
            assert_eq!(send(&mut core, c, &r, 1), Err(Error::Denied));
        }
        assert_eq!(core.dispatch(1), Ok(None));
    }
}

#[test]
fn malformed_local_fence_blocks_queued_work_and_new_connections() {
    for malformed in [
        Scope {
            tenant: ZERO,
            ..SCOPE
        },
        Scope {
            space: ZERO,
            ..SCOPE
        },
    ] {
        let (mut core, _) = fixture(INC);
        let (c, sid) = establish(&mut core, 11);
        assert_eq!(
            send(&mut core, c, &request(sid, 1, 2, SCOPE, None), 1),
            Ok(Outcome::Queued)
        );
        assert_eq!(core.deletion_fence(malformed), Err(Error::BadEncoding));
        assert_eq!(core.state(c), State::Closing);
        assert_eq!(core.request_state(c, [1; 16]), Some(RequestState::Dropped));
        assert_eq!(core.dispatch(1), Ok(None));
        assert_eq!(core.connect([12; 32], 1), Err(Error::Denied));
    }
}

#[test]
fn close_and_expiry_do_not_settle_dispatched_work() {
    for expired in [false, true] {
        let (mut core, _) = fixture(INC);
        let (c, sid) = establish(&mut core, 11);
        assert_eq!(
            send(&mut core, c, &request(sid, 1, 2, SCOPE, None), 1),
            Ok(Outcome::Queued)
        );
        let work = core.dispatch(1).unwrap().unwrap();
        if expired {
            assert_eq!(core.dispatch(301), Ok(None));
            assert_eq!(core.state(c), State::Expired);
            assert_eq!(core.request_state(c, [1; 16]), Some(RequestState::Unknown));
        } else {
            assert_eq!(
                send(&mut core, c, &frame(Kind::Close, sid, ZERO, 3, &[]), 1),
                Ok(Outcome::Closing)
            );
            assert_eq!(
                send(&mut core, c, &request(sid, 2, 4, SCOPE, None), 1),
                Err(Error::State)
            );
            assert_eq!(
                core.request_state(c, [1; 16]),
                Some(RequestState::CancelRequested)
            );
        }
        assert_eq!(core.active, Some((work.connection, work.request_id)));
        assert_eq!(core.retire(c), Err(Error::State));
    }
}

mod recovery;
mod r4;
