//! Incremental regression for Evidence novelty relative to the latest resolved decision.
use super::*;

fn resolved_with_extra() -> (Fixture, MemoryReceipt, ConflictReceipt) {
    let mut f = Fixture::new();
    let (left, right) = f.pair();
    let open = f
        .conflict(
            ConflictTarget::Create {
                left: left.revision,
                right: right.revision,
            },
            42,
        )
        .unwrap();
    f.source(scope(), id(2), 64, 65, 66, 67);
    let mut c = revise(43, 14, left, Validity::Unknown);
    c.write.evidence = vec![id(21), id(25), id(65)];
    let resolution = f.write(c).unwrap();
    let resolved = f
        .conflict(
            ConflictTarget::Resolve {
                conflict: open.conflict,
                expected_version: open.version,
                resolution_revision: resolution.revision,
            },
            44,
        )
        .unwrap();
    (f, resolution, resolved)
}

fn reopen_target(resolved: &ConflictReceipt, candidate: MemoryReceipt) -> ConflictTarget {
    ConflictTarget::Reopen {
        conflict: resolved.conflict,
        expected_version: resolved.version,
        new_revision: candidate.revision,
    }
}

fn durable_state(f: &Fixture) -> Vec<i64> {
    let mut state = f.snapshot();
    state.extend([f.count("conflict_audit"), f.count("metadata_read_audit")]);
    state.push(
        f.db.connection
            .query_row(
                "SELECT graph_version FROM space WHERE tenant_id=?1 AND id=?2",
                params![scope().tenant.to_string(), scope().space.to_string()],
                |r| r.get(0),
            )
            .unwrap(),
    );
    state
}

#[test]
fn reopen_regression_rejects_resolution_evidence_reuse() {
    let (mut f, resolution, resolved) = resolved_with_extra();
    let mut c = revise(45, 15, resolution, Validity::Unknown);
    c.write.evidence = vec![id(21), id(25), id(65)];
    let candidate = f.write(c).unwrap();
    assert!(candidate.revision_number > resolution.revision_number);
    assert!(candidate.version > resolution.version);
    assert!(candidate.knowledge_seq > resolved.knowledge_seq);
    let old_history = f.history(resolved.conflict, None).unwrap();
    assert_eq!(old_history.decisions.len(), 2);
    let before = durable_state(&f);
    let result = f.conflict(reopen_target(&resolved, candidate), 46);
    // Check atomic non-mutation first, so the old false-pass is visible in counts too.
    assert_eq!(
        durable_state(&f),
        before,
        "rejected Reopen must be atomic; result={result:?}"
    );
    assert_eq!(result, Err(Error::InvalidInput));
    let after = f.history(resolved.conflict, None).unwrap();
    assert_eq!(after, old_history);
}

fn candidate(
    f: &mut Fixture,
    previous: MemoryReceipt,
    key: u8,
    payload: u8,
    evidence: &[u8],
) -> MemoryReceipt {
    let mut c = revise(key, payload, previous, Validity::Unknown);
    c.write.evidence = evidence.iter().map(|n| id(*n)).collect();
    f.write(c).unwrap()
}

#[test]
fn reopen_regression_evidence_reordering_is_not_novelty() {
    let (mut f, resolution, resolved) = resolved_with_extra();
    let next = candidate(&mut f, resolution, 45, 15, &[65, 25, 21]);
    let before = durable_state(&f);
    assert_eq!(
        f.conflict(reopen_target(&resolved, next), 46),
        Err(Error::InvalidInput)
    );
    assert_eq!(durable_state(&f), before);
    let history = f.history(resolved.conflict, None).unwrap();
    assert_eq!(history.decisions.len(), 2);
    assert_eq!(history.decisions[1].status, ConflictStatus::Resolved);
}

#[test]
fn reopen_regression_duplicate_evidence_cannot_create_a_basis_revision() {
    let (mut f, resolution, resolved) = resolved_with_extra();
    let mut c = revise(45, 15, resolution, Validity::Unknown);
    c.write.evidence = vec![id(21), id(25), id(65), id(65)];
    let before = durable_state(&f);
    assert_eq!(f.write(c), Err(Error::InvalidInput));
    assert_eq!(durable_state(&f), before);
    // Conflict commands reference an actual committed Revision, not caller-declared Evidence.
    assert_eq!(
        f.conflict(
            ConflictTarget::Reopen {
                conflict: resolved.conflict,
                expected_version: resolved.version,
                new_revision: id(250),
            },
            46,
        ),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(durable_state(&f), before);
}

#[test]
fn reopen_regression_new_evidence_succeeds_and_replay_does_not_write_again() {
    let (mut f, resolution, resolved) = resolved_with_extra();
    let old = f.history(resolved.conflict, None).unwrap();
    f.source(scope(), id(2), 68, 69, 70, 71);
    let next = candidate(&mut f, resolution, 45, 15, &[21, 25, 65, 69]);
    let target = reopen_target(&resolved, next);
    let seq = f.seq();
    let reopened = f.conflict(target, 46).unwrap();
    assert_eq!(reopened.version, 3);
    assert_eq!(reopened.knowledge_seq, (seq + 1) as u64);
    assert!(!reopened.replayed);
    let before_replay = durable_state(&f);
    let replay = f.conflict(target, 46).unwrap();
    assert_eq!(
        replay,
        ConflictReceipt {
            replayed: true,
            ..reopened.clone()
        }
    );
    assert_eq!(durable_state(&f), before_replay);
    let changed = reopen_target(&resolved, resolution);
    assert_eq!(f.conflict(changed, 46), Err(Error::IdempotencyConflict));
    assert_eq!(durable_state(&f), before_replay);
    let now = f.history(resolved.conflict, None).unwrap();
    assert_eq!(now.decisions.len(), 3);
    assert_eq!(now.decisions[2].basis_revision, Some(next.revision));
    assert_eq!(now.decisions[2].status, ConflictStatus::Open);
    assert_eq!(
        f.history(resolved.conflict, Some(old.snapshot.clone()))
            .unwrap(),
        old
    );
    // Even an already committed idempotency receipt cannot authorize a revoked principal.
    let stale_context = f.context();
    f.grant("memory.revise", false);
    let before = durable_state(&f);
    assert_eq!(
        f.db.write_conflict(
            Some(&stale_context),
            ConflictWrite {
                scope: scope(),
                target,
                idempotency: id(46),
            },
        ),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(durable_state(&f), before);
}

#[test]
fn reopen_regression_foreign_evidence_and_revoked_request_are_denied() {
    let (mut f, resolution, resolved) = resolved_with_extra();
    let other = Scope {
        tenant: scope().tenant,
        space: id(80),
    };
    f.db.administer(
        &f.authority,
        AdminChange::CreateSpace {
            scope: other,
            owner: id(2),
        },
    )
    .unwrap();
    f.source(other, id(2), 82, 83, 84, 85);
    let mut foreign = revise(45, 15, resolution, Validity::Unknown);
    foreign.write.evidence = vec![id(21), id(25), id(65), id(83)];
    let before = durable_state(&f);
    assert_eq!(f.write(foreign), Err(Error::AuthorizationDenied));
    assert_eq!(durable_state(&f), before);
    f.source(scope(), id(2), 68, 69, 70, 71);
    let next = candidate(&mut f, resolution, 45, 15, &[21, 25, 65, 69]);
    let stale_context = f.context();
    f.grant("memory.revise", false);
    let before = durable_state(&f);
    assert_eq!(
        f.db.write_conflict(
            Some(&stale_context),
            ConflictWrite {
                scope: scope(),
                target: reopen_target(&resolved, next),
                idempotency: id(46),
            },
        ),
        Err(Error::AuthorizationDenied)
    );
    assert_eq!(durable_state(&f), before);
}

#[test]
fn reopen_regression_each_cycle_uses_its_latest_resolution_evidence() {
    let (mut f, resolution, resolved) = resolved_with_extra();
    f.source(scope(), id(2), 68, 69, 70, 71);
    let next = candidate(&mut f, resolution, 45, 15, &[21, 25, 65, 69]);
    let reopened = f.conflict(reopen_target(&resolved, next), 46).unwrap();
    let resolved_again = f
        .conflict(
            ConflictTarget::Resolve {
                conflict: resolved.conflict,
                expected_version: reopened.version,
                resolution_revision: next.revision,
            },
            47,
        )
        .unwrap();
    assert_eq!(resolved_again.version, 4);
    let old = f.history(resolved.conflict, None).unwrap();
    let repeat = candidate(&mut f, next, 48, 16, &[21, 25, 65, 69]);
    let before = durable_state(&f);
    assert_eq!(
        f.conflict(reopen_target(&resolved_again, repeat), 49),
        Err(Error::InvalidInput)
    );
    assert_eq!(durable_state(&f), before);
    assert_eq!(
        f.history(resolved.conflict, None).unwrap().decisions,
        old.decisions
    );
    f.source(scope(), id(2), 72, 73, 74, 75);
    let truly_new = candidate(&mut f, repeat, 50, 17, &[21, 25, 65, 69, 73]);
    let reopened_again = f
        .conflict(reopen_target(&resolved_again, truly_new), 51)
        .unwrap();
    assert_eq!(reopened_again.version, 5);
    let latest = f.history(resolved.conflict, None).unwrap();
    assert_eq!(latest.decisions.len(), 5);
    assert_eq!(&latest.decisions[..4], old.decisions.as_slice());
    assert_eq!(latest.decisions[4].basis_revision, Some(truly_new.revision));
}

#[test]
fn reopen_regression_outbox_failure_rolls_back_the_entire_transition() {
    let (mut f, resolution, resolved) = resolved_with_extra();
    f.source(scope(), id(2), 68, 69, 70, 71);
    let next = candidate(&mut f, resolution, 45, 15, &[21, 25, 65, 69]);
    let old = f.history(resolved.conflict, None).unwrap();
    f.db.connection.execute_batch("CREATE TEMP TRIGGER fail_reopen BEFORE INSERT ON main.conflict_outbox BEGIN SELECT RAISE(ABORT,'fault'); END;").unwrap();
    let before = durable_state(&f);
    let target = reopen_target(&resolved, next);
    assert_eq!(f.conflict(target, 46), Err(Error::Constraint));
    assert_eq!(durable_state(&f), before);
    assert_eq!(f.history(resolved.conflict, None).unwrap(), old);
    f.db.connection
        .execute_batch("DROP TRIGGER temp.fail_reopen")
        .unwrap();
    let receipt = f.conflict(target, 46).unwrap();
    assert!(!receipt.replayed);
    assert_eq!(receipt.version, 3);
}

#[test]
fn reopen_regression_resolve_race_preserves_cas_and_state_history() {
    let (mut f, resolution, resolved) = resolved_with_extra();
    f.source(scope(), id(2), 68, 69, 70, 71);
    let next = candidate(&mut f, resolution, 45, 15, &[21, 25, 65, 69]);
    let gate = Arc::new(Barrier::new(3));
    let targets = [
        reopen_target(&resolved, next),
        ConflictTarget::Resolve {
            conflict: resolved.conflict,
            expected_version: resolved.version,
            resolution_revision: next.revision,
        },
    ];
    let workers: Vec<_> = targets
        .into_iter()
        .enumerate()
        .map(|(n, target)| {
            let mut db = f.open();
            let ctx = f.context();
            let gate = gate.clone();
            thread::spawn(move || {
                gate.wait();
                db.write_conflict(
                    Some(&ctx),
                    ConflictWrite {
                        scope: scope(),
                        target,
                        idempotency: id(46 + n as u8),
                    },
                )
            })
        })
        .collect();
    let before_seq = f.seq();
    gate.wait();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results[0].as_ref().unwrap().version, 3);
    assert!(matches!(
        results[1],
        Err(Error::CasConflict | Error::StateConflict)
    ));
    assert_eq!(f.seq(), before_seq + 1);
    assert_eq!(f.count("conflict_decision"), 3);
    assert_eq!(f.count("conflict_audit"), 3);
    assert_eq!(f.count("conflict_outbox"), 3);
    assert_eq!(f.count("conflict_retry"), 3);
    let history = f.history(resolved.conflict, None).unwrap();
    assert_eq!(history.decisions.len(), 3);
    assert_eq!(history.decisions[2].status, ConflictStatus::Open);
}

#[test]
fn reopen_regression_does_not_invent_a_source_event_after_resolution_requirement() {
    let mut f = Fixture::new();
    let (left, right) = f.pair();
    let open = f
        .conflict(
            ConflictTarget::Create {
                left: left.revision,
                right: right.revision,
            },
            42,
        )
        .unwrap();
    f.source(scope(), id(2), 64, 65, 66, 67);
    f.source(scope(), id(2), 68, 69, 70, 71);
    let resolution = candidate(&mut f, left, 43, 14, &[21, 25, 65]);
    let resolved = f
        .conflict(
            ConflictTarget::Resolve {
                conflict: open.conflict,
                expected_version: 1,
                resolution_revision: resolution.revision,
            },
            44,
        )
        .unwrap();
    let source_seq: i64 =
        f.db.connection
            .query_row(
                "SELECT knowledge_seq FROM event WHERE tenant_id=?1 AND space_id=?2 AND id=?3",
                params![
                    scope().tenant.to_string(),
                    scope().space.to_string(),
                    id(68).to_string()
                ],
                |r| r.get(0),
            )
            .unwrap();
    assert!((source_seq as u64) < resolved.knowledge_seq);
    let next = candidate(&mut f, resolution, 45, 15, &[21, 25, 65, 69]);
    assert!(next.knowledge_seq > resolved.knowledge_seq);
    assert_eq!(
        f.conflict(reopen_target(&resolved, next), 46)
            .unwrap()
            .version,
        3
    );
}
