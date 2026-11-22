// Independent R1 tests. Private fixtures may inject entropy; requests cannot.
use super::*;
use std::{collections::VecDeque, sync::atomic::AtomicUsize};
struct ScriptedIds {
    values: VecDeque<Result<Id>>,
    calls: Arc<AtomicUsize>,
}
impl ResourceIds for ScriptedIds {
    fn next(&mut self) -> Result<Id> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.values
            .pop_front()
            .unwrap_or(Err(Error::ResourceUnavailable))
    }
}
fn uuid(n: u8) -> Id {
    let mut b = [n; 16];
    b[6] = (b[6] & 15) | 64;
    b[8] = (b[8] & 63) | 128;
    Id::from_bytes(b)
}
fn scripted(db: &mut AuthDatabase<TestClock>, values: Vec<Result<Id>>) -> Arc<AtomicUsize> {
    let calls = Arc::new(AtomicUsize::new(0));
    db.ids = Box::new(ScriptedIds {
        values: values.into(),
        calls: calls.clone(),
    });
    calls
}
fn two_principals() -> (Fixture, AuthContext, AuthContext, Scope) {
    let mut f = Fixture::new(true);
    // Protected synthetic bootstrap fixture only, not a normal Principal-management API.
    f.db.connection
        .execute(
            "INSERT INTO principal VALUES(?1,?2,'user','active')",
            params![id(1).to_string(), id(12).to_string()],
        )
        .unwrap();
    f.db.connection
        .execute(
            "INSERT INTO identity_binding VALUES(?1,?2,?3,1)",
            params![id(14).to_string(), id(1).to_string(), id(12).to_string()],
        )
        .unwrap();
    let other = Scope {
        tenant: id(1),
        space: id(13),
    };
    f.db.administer(
        &f.authority,
        AdminChange::CreateSpace {
            scope: other,
            owner: id(12),
        },
    )
    .unwrap();
    f.db.administer(
        &f.authority,
        AdminChange::Grant {
            scope: other,
            principal: id(12),
            enabled: true,
            expires_at_ms: 1000,
        },
    )
    .unwrap();
    let a = f.context();
    let b =
        f.db.identify(TrustedIdentity { subject: id(14) }, other.space)
            .unwrap();
    (f, a, b, other)
}
#[test]
fn normal_creation_succeeds_independently_of_other_space_occupancy() {
    for occupied in [false, true] {
        let (mut f, a, b, other) = two_principals();
        let foreign = if occupied {
            Some(f.db.apply(Some(&a), command(20)).unwrap())
        } else {
            None
        };
        let mut cmd = command(30);
        cmd.scope = other;
        let mut app = MetadataApplication::assemble(f.open());
        let own = app.execute(Some(&b), cmd).unwrap();
        assert_eq!(own.version, 1);
        assert!(!own.replayed);
        if let Some(foreign) = foreign {
            assert_ne!(own.marker, foreign.marker);
            assert_ne!(own.event, foreign.event);
        }
        let count: i64 =
            f.db.connection
                .query_row(
                    "SELECT count(*) FROM metadata_marker WHERE tenant_id=?1 AND space_id=?2",
                    params![other.tenant.to_string(), other.space.to_string()],
                    |r| r.get(0),
                )
                .unwrap();
        assert_eq!(count, 1);
    }
}
#[test]
fn foreign_and_absent_update_targets_have_identical_denial() {
    let (mut f, a, b, other) = two_principals();
    let foreign = f.db.apply(Some(&a), command(20)).unwrap();
    let calls = scripted(&mut f.db, vec![Err(Error::ResourceUnavailable)]);
    for expected in [0, 1, 99, u64::MAX] {
        let mut cmd = update_command(30, foreign.marker, expected);
        cmd.scope = other;
        let occupied = f.db.apply(Some(&b), cmd);
        cmd.target = MarkerTarget::Existing(uuid(71));
        let absent = f.db.apply(Some(&b), cmd);
        assert_eq!(occupied, absent);
        assert_eq!(
            occupied,
            Err(if expected == u64::MAX {
                Error::InvalidInput
            } else {
                Error::AuthorizationDenied
            })
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.seq(), 1);
}
#[test]
fn forced_foreign_marker_collision_is_silently_reissued() {
    let (mut f, a, b, other) = two_principals();
    let foreign = f.db.apply(Some(&a), command(20)).unwrap();
    let calls = scripted(
        &mut f.db,
        vec![Ok(foreign.marker), Ok(uuid(60)), Ok(uuid(61))],
    );
    let mut cmd = command(30);
    cmd.scope = other;
    let result = f.db.apply(Some(&b), cmd).unwrap();
    assert_eq!((result.marker, result.event), (uuid(60), uuid(61)));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(f.seq(), 2);
}
#[test]
fn forced_foreign_event_collision_is_silently_reissued() {
    let (mut f, a, b, other) = two_principals();
    let foreign = f.db.apply(Some(&a), command(20)).unwrap();
    let calls = scripted(
        &mut f.db,
        vec![Ok(uuid(60)), Ok(foreign.event), Ok(uuid(61))],
    );
    let mut cmd = command(30);
    cmd.scope = other;
    let result = f.db.apply(Some(&b), cmd).unwrap();
    assert_eq!((result.marker, result.event), (uuid(60), uuid(61)));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(f.seq(), 2);
}
#[test]
fn exhausted_ids_and_entropy_failure_share_generic_error_without_partial_rows() {
    let (mut f, a, b, other) = two_principals();
    let foreign = f.db.apply(Some(&a), command(20)).unwrap();
    for own_space in [false, true] {
        for kind in [ResourceKind::Marker, ResourceKind::Event] {
            let values = match kind {
                ResourceKind::Marker => vec![Ok(foreign.marker); 8],
                ResourceKind::Event => {
                    let mut v = vec![Ok(uuid(60))];
                    v.extend(vec![Ok(foreign.event); 8]);
                    v
                }
            };
            let expected_calls = values.len();
            let calls = scripted(&mut f.db, values);
            let mut cmd = command(30);
            cmd.scope = if own_space { scope() } else { other };
            let ctx = if own_space { &a } else { &b };
            let error = f.db.apply(Some(ctx), cmd);
            assert_eq!(error, Err(Error::ResourceUnavailable));
            assert_eq!(format!("{}", error.unwrap_err()), "ResourceUnavailable");
            assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
            assert_eq!(f.seq(), 1);
            assert_eq!(f.count("event"), 1);
            assert_eq!(f.count("metadata_marker"), 1);
            assert_eq!(f.count("metadata_retry"), 1);
        }
    }
    scripted(&mut f.db, vec![Err(Error::Database)]);
    let mut cmd = command(30);
    cmd.scope = other;
    assert_eq!(f.db.apply(Some(&b), cmd), Err(Error::ResourceUnavailable));
    assert_eq!(f.seq(), 1);
}
#[test]
fn response_loss_replay_returns_committed_ids_without_using_generator() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    let calls = scripted(&mut f.db, vec![Ok(uuid(60)), Ok(uuid(61))]);
    let committed = f.db.apply(Some(&ctx), command(20)).unwrap();
    let replay = f.db.apply(Some(&ctx), command(20)).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(replay.replayed);
    assert_eq!(
        (committed.marker, committed.event),
        (replay.marker, replay.event)
    );
    let mut reopened = f.open();
    let reopened_calls = scripted(&mut reopened, vec![Err(Error::ResourceUnavailable)]);
    let after_loss = reopened.apply(Some(&ctx), command(20)).unwrap();
    assert!(after_loss.replayed);
    assert_eq!(after_loss.marker, committed.marker);
    assert_eq!(after_loss.event, committed.event);
    assert_eq!(reopened_calls.load(Ordering::SeqCst), 0);
    assert_eq!(f.seq(), 1);
    assert_eq!(f.count("event"), 1);
}
#[test]
fn another_principal_reusing_same_key_receives_only_own_resources() {
    let (mut f, a, b, other) = two_principals();
    let own_a = f.db.apply(Some(&a), command(20)).unwrap();
    let mut cmd_b = command(20);
    cmd_b.scope = other;
    let own_b = f.db.apply(Some(&b), cmd_b).unwrap();
    assert_ne!(own_a.marker, own_b.marker);
    assert_ne!(own_a.event, own_b.event);
    assert!(!own_b.replayed);
    assert_eq!(f.db.apply(Some(&b), cmd_b).unwrap().marker, own_b.marker);
    assert_eq!(
        f.db.apply(Some(&a), command(20)).unwrap().marker,
        own_a.marker
    );
    assert_eq!(f.count("metadata_retry"), 2);
}
#[test]
fn os_source_produces_canonical_uuid_v4() {
    let mut source = OsResourceIds;
    let mut sample = std::collections::HashSet::new();
    for _ in 0..32 {
        let id = source.next().unwrap();
        let text = id.to_string();
        assert_eq!(text.as_bytes()[14], b'4');
        assert!(b"89ab".contains(&text.as_bytes()[19]));
        assert_eq!(text.parse::<Id>().unwrap(), id);
        assert!(sample.insert(id));
    }
    // Smoke test of API/format/OS calls, NOT proof of entropy quality or collision impossibility.
}
#[test]
fn policy_rejection_does_not_allocate_resource_ids() {
    let mut f = Fixture::new(false);
    let ctx = f.context();
    let calls = scripted(&mut f.db, vec![Err(Error::Database)]);
    assert_eq!(
        f.db.apply(Some(&ctx), command(20)),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    f.assert_empty();
}
#[test]
fn concurrent_creates_with_different_keys_get_distinct_server_resources() {
    let f = Fixture::new(true);
    let ctx = f.context();
    let barrier = Arc::new(Barrier::new(2));
    let mut handles = Vec::new();
    for n in [20, 30] {
        let mut db = f.open();
        let ctx = ctx.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            db.apply(Some(&ctx), command(n)).unwrap()
        }));
    }
    let a = handles.remove(0).join().unwrap();
    let b = handles.remove(0).join().unwrap();
    assert_ne!(a.marker, b.marker);
    assert_ne!(a.event, b.event);
    assert_eq!(f.seq(), 2);
}
#[test]
fn old_auth_profile_2_is_rejected_without_schema_or_history_mutation() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("auth-v2.sqlite");
    let c = Connection::open(&path).unwrap();
    c.execute_batch(MIGRATION_RECORD).unwrap();
    c.execute_batch(MIGRATION).unwrap();
    c.execute_batch(AUTH_DDL).unwrap();
    let old_hash: String = Sha256::digest(format!("{MIGRATION}\n{AUTH_DDL}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    c.execute(
        "INSERT INTO migration_record VALUES(2,?1,'applied')",
        [&old_hash],
    )
    .unwrap();
    c.pragma_update(None, "user_version", 2).unwrap();
    c.pragma_update(None, "application_id", APPLICATION_ID)
        .unwrap();
    assert!(matches!(
        AuthDatabase::open(&path, TestClock::new()),
        Err(Error::IncompatibleSchema)
    ));
    let checksum: String = c
        .query_row("SELECT checksum FROM migration_record", [], |r| r.get(0))
        .unwrap();
    assert_eq!(old_hash, checksum);
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM pragma_table_info('metadata_retry') WHERE name='marker_id'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}
#[test]
fn rolled_back_create_can_retry_once_with_new_ids_and_same_key() {
    let mut f = Fixture::new(true);
    let ctx = f.context();
    let calls = scripted(
        &mut f.db,
        vec![Ok(uuid(60)), Ok(uuid(61)), Ok(uuid(62)), Ok(uuid(63))],
    );
    f.db.connection.execute_batch("CREATE TEMP TRIGGER fail_create BEFORE INSERT ON metadata_retry BEGIN SELECT RAISE(ABORT,'synthetic fault'); END;").unwrap();
    assert_eq!(f.db.apply(Some(&ctx), command(20)), Err(Error::Constraint));
    f.assert_empty();
    f.db.connection
        .execute_batch("DROP TRIGGER fail_create")
        .unwrap();
    let result = f.db.apply(Some(&ctx), command(20)).unwrap();
    assert_eq!((result.marker, result.event), (uuid(62), uuid(63)));
    assert_eq!(f.seq(), 1);
    assert_eq!(f.count("event"), 1);
    assert!(f.db.apply(Some(&ctx), command(20)).unwrap().replayed);
    assert_eq!(calls.load(Ordering::SeqCst), 4);
}
#[test]
fn same_random_values_in_different_tenants_preserve_tenant_namespace() {
    let mut f = Fixture::new(true);
    let a = f.context();
    let own_a = f.db.apply(Some(&a), command(20)).unwrap();
    let other = Scope {
        tenant: id(5),
        space: id(7),
    };
    f.db.administer(
        &f.authority,
        AdminChange::Grant {
            scope: other,
            principal: id(6),
            enabled: true,
            expires_at_ms: 1000,
        },
    )
    .unwrap();
    let b =
        f.db.identify(TrustedIdentity { subject: id(8) }, other.space)
            .unwrap();
    let calls = scripted(&mut f.db, vec![Ok(own_a.marker), Ok(own_a.event)]);
    let mut cmd = command(30);
    cmd.scope = other;
    let own_b = f.db.apply(Some(&b), cmd).unwrap();
    assert_eq!((own_a.marker, own_a.event), (own_b.marker, own_b.event));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(f.count("metadata_marker"), 2);
    assert_eq!(
        f.db.apply(Some(&b), command(40)),
        Err(Error::AuthorizationDenied)
    );
}
