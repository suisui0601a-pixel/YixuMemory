use super::*;
fn binding() -> PayloadBinding {
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
fn intent(id: u8) -> SecurityIntent {
    SecurityIntent {
        id: [id; 16],
        scope: binding().scope,
        request_digest: [id; 32],
        authorization_epoch: 1,
        deletion_generation: u64::from(id),
        closure_version: 1,
        targets: BTreeSet::from([binding()]),
        copies: BTreeSet::from([[9; 16], [10; 16]]),
    }
}
fn model() -> GuardModel {
    GuardModel::new(Profile::Protected, [1; 16], 1).unwrap()
}
fn receipt(i: &SecurityIntent, fence: u64, copy: Identity) -> ErasureObservation {
    ErasureObservation {
        intent: i.id,
        request_digest: i.request_digest,
        target: ReceiptTarget {
            binding: binding(),
            copy,
        },
        deletion_generation: i.deletion_generation,
        closure_version: i.closure_version,
        worker_fence: fence,
    }
}
fn finish(m: &mut GuardModel, i: &SecurityIntent) {
    m.prepared(i.id).unwrap();
    m.tdel(i.id).unwrap();
    let f = m.claim(i.id).unwrap();
    for c in &i.copies {
        m.observe(receipt(i, f, *c)).unwrap();
    }
    m.finalize(i.id).unwrap();
}
#[test]
fn tguard_and_every_later_phase_deny_release() {
    let mut m = model();
    let i = intent(11);
    m.begin(i.clone()).unwrap();
    assert!(m.check_release(&binding(), 1, m.security_seq()).is_err());
    m.prepared(i.id).unwrap();
    assert!(m.check_release(&binding(), 1, m.security_seq()).is_err());
    m.tdel(i.id).unwrap();
    assert!(m.check_release(&binding(), 1, m.security_seq()).is_err());
    let f = m.claim(i.id).unwrap();
    for c in &i.copies {
        m.observe(receipt(&i, f, *c)).unwrap();
        assert!(m.check_release(&binding(), 1, m.security_seq()).is_err());
    }
    m.finalize(i.id).unwrap();
    assert!(m.check_release(&binding(), 1, m.security_seq()).is_err());
}
#[test]
fn ack_loss_and_db_failure_never_abort_intent() {
    let mut m = model();
    m.begin(intent(11)).unwrap();
    let before = m.clone();
    assert_eq!(m.tdel([11; 16]), Err(ProtocolError::Conflict));
    assert_eq!(m, before);
    assert!(m.check_release(&binding(), 1, 1).is_err());
}
#[test]
fn idempotency_is_exact_and_does_not_consume_sequence() {
    let mut m = model();
    let i = intent(11);
    m.begin(i.clone()).unwrap();
    m.begin(i.clone()).unwrap();
    assert_eq!(m.security_seq(), 1);
    let before = m.clone();
    let mut changed = i;
    changed.closure_version += 1;
    assert_eq!(m.begin(changed), Err(ProtocolError::Conflict));
    assert_eq!(m, before);
}
#[test]
fn cross_space_and_empty_closures_fail_without_state_changes() {
    for kind in 0..4 {
        let mut m = model();
        let before = m.clone();
        let mut i = intent(11);
        match kind {
            0 => i.scope.space = [77; 16],
            1 => i.targets.clear(),
            2 => i.copies.clear(),
            _ => i.authorization_epoch = 0,
        }
        assert!(m.begin(i).is_err());
        assert_eq!(m, before);
    }
}
#[test]
fn stale_worker_and_every_receipt_identity_mismatch_rejected() {
    for kind in 0..8 {
        let mut m = model();
        let i = intent(11);
        m.begin(i.clone()).unwrap();
        m.prepared(i.id).unwrap();
        m.tdel(i.id).unwrap();
        m.claim(i.id).unwrap();
        let f = m.claim(i.id).unwrap();
        let mut r = receipt(&i, f, [9; 16]);
        match kind {
            0 => r.worker_fence = 1,
            1 => r.target.binding.scope.space = [8; 16],
            2 => r.target.binding.seal_generation += 1,
            3 => r.deletion_generation += 1,
            4 => r.closure_version += 1,
            5 => r.request_digest = [0; 32],
            6 => r.target.copy = [0; 16],
            _ => r.target.binding.key = [0; 16],
        }
        let before = m.clone();
        assert_eq!(m.observe(r), Err(ProtocolError::Integrity));
        assert_eq!(m, before);
    }
}
#[test]
fn missing_or_duplicate_receipt_cannot_finalize() {
    let mut m = model();
    let i = intent(11);
    m.begin(i.clone()).unwrap();
    m.prepared(i.id).unwrap();
    m.tdel(i.id).unwrap();
    let f = m.claim(i.id).unwrap();
    m.observe(receipt(&i, f, [9; 16])).unwrap();
    m.observe(receipt(&i, f, [9; 16])).unwrap();
    assert_eq!(m.finalize(i.id), Err(ProtocolError::Conflict));
    assert_eq!(m.records()[&i.id].receipts.len(), 1);
}
#[test]
fn overlapping_intent_completion_does_not_unfence_other_scope_work() {
    let mut m = model();
    let a = intent(11);
    let b = intent(12);
    m.begin(a.clone()).unwrap();
    m.begin(b.clone()).unwrap();
    finish(&mut m, &a);
    let mut survivor = binding();
    survivor.payload = [99; 16];
    survivor.key = [98; 16];
    assert!(m.check_release(&survivor, 1, 2).is_err());
    finish(&mut m, &b);
    assert_eq!(m.check_release(&survivor, 1, 2), Ok(()));
    assert!(m.check_release(&binding(), 1, 2).is_err());
}
#[test]
fn deleted_payload_and_key_ids_cannot_be_reactivated_with_generation_changes() {
    let mut m = model();
    let i = intent(11);
    m.begin(i.clone()).unwrap();
    finish(&mut m, &i);
    let mut b = binding();
    b.seal_generation += 1;
    b.key = [100; 16];
    assert!(m.check_release(&b, 1, 1).is_err());
    b = binding();
    b.payload = [100; 16];
    assert!(m.check_release(&b, 1, 1).is_err());
}
#[test]
fn restore_old_db_replays_witness_tombstones_and_invalidates_old_incarnation() {
    let mut db = model();
    let mut witness = model();
    let i = intent(11);
    witness.begin(i.clone()).unwrap();
    finish(&mut witness, &i);
    db.reconcile(&witness, 2).unwrap();
    assert_eq!(db.records(), witness.records());
    assert!(db.check_release(&binding(), 2, 1).is_err());
    let mut survivor = binding();
    survivor.payload = [8; 16];
    survivor.key = [8; 16];
    assert!(db.check_release(&survivor, 1, 1).is_err());
    assert_eq!(db.check_release(&survivor, 2, 1), Ok(()));
}
#[test]
fn rollback_unknown_and_unavailable_witness_keep_recovery_blocked() {
    let old = model();
    let mut m = model();
    m.begin(intent(11)).unwrap();
    assert!(m.reconcile(&old, 2).is_err());
    assert_eq!(
        m.check_release(&binding(), 1, 1),
        Err(ProtocolError::Unavailable)
    );
    let mut unavailable = model();
    unavailable.set_available(false);
    assert!(model().reconcile(&unavailable, 2).is_err());
}
#[test]
fn terminal_state_cannot_regress_at_equal_sequence() {
    let mut m = model();
    let i = intent(11);
    m.begin(i.clone()).unwrap();
    let stale = m.clone();
    finish(&mut m, &i);
    assert_eq!(m.reconcile(&stale, 2), Err(ProtocolError::Integrity));
}
#[test]
fn local_restore_is_explicitly_unsupported_not_protected_downgrade() {
    let mut local = GuardModel::new(Profile::Local, [1; 16], 1).unwrap();
    assert_eq!(
        local.reconcile(&model(), 2),
        Err(ProtocolError::Unsupported)
    );
    assert_eq!(local.profile, Profile::Local);
}
#[test]
fn aad_is_unambiguous_and_binds_all_identity_and_version_fields() {
    let b = binding();
    let original = b.aad(13);
    assert_eq!(original.len(), 163);
    for kind in 0..11 {
        let mut x = b;
        match kind {
            0 => x.scope.realm = [9; 16],
            1 => x.scope.tenant = [9; 16],
            2 => x.scope.space = [9; 16],
            3 => x.owner_kind = OwnerKind::Revision,
            4 => x.owner = [9; 16],
            5 => x.payload = [9; 16],
            6 => x.key = [9; 16],
            7 => x.seal_generation += 1,
            8 => x.vault_incarnation += 1,
            9 => x.protection = [9; 32],
            _ => {
                assert_ne!(original, x.aad(14));
                continue;
            }
        }
        assert_ne!(original, x.aad(13));
    }
}
#[test]
fn bounded_transition_enumeration_never_admits_after_begin() {
    // 5^5 schedules; illegal transitions must leave the model unchanged.
    for mut code in 0..3125 {
        let mut m = model();
        let i = intent(11);
        m.begin(i.clone()).unwrap();
        for _ in 0..5 {
            let before = m.clone();
            let result = match code % 5 {
                0 => m.prepared(i.id),
                1 => m.tdel(i.id),
                2 => m.claim(i.id).map(|_| ()),
                3 => m.observe(receipt(&i, 1, [9; 16])),
                _ => m.finalize(i.id),
            };
            code /= 5;
            if result.is_err() {
                assert_eq!(m, before);
            }
            assert!(m.check_release(&binding(), 1, m.security_seq()).is_err());
        }
    }
}
