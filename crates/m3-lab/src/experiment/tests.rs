use super::*;
#[test]
fn xchacha_matches_published_draft_appendix_a1_vector() {
    // Numeric test data, not implementation code, from IETF draft-irtf-cfrg-xchacha-03 A.1.
    // Attribution/terms and draft status are recorded in docs/M3_THIRD_PARTY.md.
    fn hex(s: &str) -> Vec<u8> {
        s.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    }
    let key: Vec<u8> = (0x80..=0x9f).collect();
    let nonce: Vec<u8> = (0x40..=0x57).collect();
    let aad = hex("50515253c0c1c2c3c4c5c6c7");
    let mut body = hex(
        "4c616469657320616e642047656e746c656d656e206f662074686520636c617373206f66202739393a204966204920636f756c64206f6666657220796f75206f6e6c79206f6e652074697020666f7220746865206675747572652c2073756e73637265656e20776f756c642062652069742e",
    );
    let cipher = XChaCha20Poly1305::new_from_slice(&key).unwrap();
    let tag = cipher
        .encrypt_in_place_detached(XNonce::from_slice(&nonce), &aad, &mut body)
        .unwrap();
    assert_eq!(
        body,
        hex(
            "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b4522f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff921f9664c97637da9768812f615c68b13b52e"
        )
    );
    assert_eq!(tag.as_slice(), hex("c0875924c1c7987947deafd8780acf49"));
}
#[test]
fn witness_version_and_remote_unknown_intent_are_not_cached_permission() {
    for change in 0..2 {
        let mut lab = Lab::new(Profile::Protected);
        let identity = lab.identity();
        let b = lab.stage_canary(&identity, Fault::None).unwrap();
        let permit = lab.permit(&identity, b).unwrap();
        if change == 0 {
            lab.witness.model = GuardModel::new(Profile::Protected, [1; 16], 2).unwrap();
        } else {
            let i = lab.begin_delete(&identity, b, Fault::None).unwrap();
            lab.model = GuardModel::new(Profile::Protected, [1; 16], 1).unwrap();
            assert!(lab.witness.model.records().contains_key(&i.id));
        }
        assert!(lab.release(permit).is_err());
        assert!(lab.stage_canary(&identity, Fault::None).is_err());
        assert_eq!(lab.released, 0);
    }
}
#[test]
fn completion_replay_is_idempotent_and_changed_input_does_not_claim() {
    let mut lab = Lab::new(Profile::Protected);
    let identity = lab.identity();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    let i = lab.begin_delete(&identity, b, Fault::None).unwrap();
    let before = lab.model.clone();
    let mut wrong = i.clone();
    wrong.closure_version += 1;
    assert_eq!(lab.finish_delete(&wrong, Fault::None), Err(Error::Conflict));
    assert_eq!(lab.model, before);
    lab.finish_delete(&i, Fault::None).unwrap();
    let before = lab.model.clone();
    lab.finish_delete(&i, Fault::None).unwrap();
    assert_eq!(lab.model, before);
}
#[test]
fn real_aead_ciphertext_file_roundtrip_in_both_profiles_without_llm() {
    for profile in [Profile::Local, Profile::Protected] {
        let mut lab = Lab::new(profile);
        let identity = lab.identity();
        let b = lab.stage_canary(&identity, Fault::None).unwrap();
        let encrypted = lab.store.read(&b).unwrap();
        assert!(!encrypted.bytes.windows(CANARY.len()).any(|w| w == CANARY));
        let permit = lab.permit(&identity, b).unwrap();
        assert_eq!(
            lab.release(permit).unwrap(),
            <[u8; 32]>::from(Sha256::digest(CANARY))
        );
        assert_eq!(lab.t1_commits, 1);
        let intent = lab.begin_delete(&identity, b, Fault::None).unwrap();
        lab.finish_delete(&intent, Fault::None).unwrap();
        assert!(lab.permit(&identity, b).is_err());
        assert!(lab.vault.open(&encrypted).is_err());
        assert_eq!(
            lab.model.records()[&intent.id].state,
            IntentState::Finalized
        );
    }
}
#[test]
fn allocation_random_encrypt_publish_and_t1_faults_never_activate_content() {
    for fault in [
        Fault::Random,
        Fault::Lock,
        Fault::Encrypt,
        Fault::Publish,
        Fault::T1,
    ] {
        let mut lab = Lab::new(Profile::Protected);
        let identity = lab.identity();
        assert!(lab.stage_canary(&identity, fault).is_err());
        assert!(lab.active.is_empty());
        assert!(lab.vault.keys.is_empty());
        assert_eq!(lab.t1_commits, 0);
        assert_eq!(lab.released, 0);
        for stored in lab.store.entries.values_mut() {
            stored.file.seek(SeekFrom::Start(0)).unwrap();
            let mut data = Vec::new();
            stored.file.read_to_end(&mut data).unwrap();
            assert!(!data.windows(CANARY.len()).any(|w| w == CANARY));
        }
    }
}
#[test]
fn unavailable_key_backend_fails_before_payload_publication() {
    let mut lab = Lab::new(Profile::Local);
    let identity = lab.identity();
    lab.vault.available = false;
    assert!(lab.stage_canary(&identity, Fault::None).is_err());
    assert!(lab.store.entries.is_empty());
}
#[test]
fn protected_unavailable_witness_never_falls_back_to_local() {
    let mut lab = Lab::new(Profile::Protected);
    let identity = lab.identity();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    let permit = lab.permit(&identity, b).unwrap();
    lab.witness.available = false;
    assert_eq!(lab.release(permit), Err(Error::Unavailable));
    assert!(lab.stage_canary(&identity, Fault::None).is_err());
    assert_eq!(lab.model.profile, Profile::Protected);
    assert_eq!(lab.released, 0);
}
#[test]
fn local_synthetic_roundtrip_does_not_call_external_witness() {
    let mut lab = Lab::new(Profile::Local);
    lab.witness.available = false;
    let identity = lab.identity();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    let permit = lab.permit(&identity, b).unwrap();
    assert!(lab.release(permit).is_ok());
}
#[test]
fn revoke_after_permit_before_release_denies_old_identity() {
    let mut lab = Lab::new(Profile::Protected);
    let identity = lab.identity();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    let permit = lab.permit(&identity, b).unwrap();
    lab.grant = false;
    lab.epoch += 1;
    assert_eq!(lab.release(permit), Err(Error::Denied));
    assert!(lab.stage_canary(&identity, Fault::None).is_err());
    assert_eq!(lab.released, 0);
}
#[test]
fn deletion_after_permit_before_release_denies_plaintext() {
    let mut lab = Lab::new(Profile::Protected);
    let identity = lab.identity();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    let permit = lab.permit(&identity, b).unwrap();
    lab.begin_delete(&identity, b, Fault::None).unwrap();
    assert_eq!(lab.release(permit), Err(Error::Denied));
    assert_eq!(lab.released, 0);
}
#[test]
fn prepare_ack_loss_and_tdel_failure_remain_denied() {
    for fault in [Fault::PrepareAck, Fault::Tdel] {
        let mut lab = Lab::new(Profile::Protected);
        let identity = lab.identity();
        let b = lab.stage_canary(&identity, Fault::None).unwrap();
        assert!(lab.begin_delete(&identity, b, fault).is_err());
        assert!(lab.permit(&identity, b).is_err());
        assert_eq!(lab.model.records().len(), 1);
        let expected = if fault == Fault::PrepareAck {
            IntentState::OutcomeUnknown
        } else {
            IntentState::Guarded
        };
        assert_eq!(lab.model.records().values().next().unwrap().state, expected);
    }
}
#[test]
fn partial_erasure_retry_finishes_without_reactivating_key() {
    let mut lab = Lab::new(Profile::Protected);
    let identity = lab.identity();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    let i = lab.begin_delete(&identity, b, Fault::None).unwrap();
    assert!(lab.finish_delete(&i, Fault::Erase).is_err());
    assert!(lab.vault.revoked.contains(&b.key));
    assert!(lab.permit(&identity, b).is_err());
    lab.finish_delete(&i, Fault::None).unwrap();
    assert_eq!(lab.model.records()[&i.id].state, IntentState::Finalized);
    assert!(lab.vault.allocate(b, Fault::None).is_err());
}
#[test]
fn finalize_ack_loss_reconciles_without_an_unfenced_window() {
    let mut lab = Lab::new(Profile::Protected);
    let identity = lab.identity();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    let i = lab.begin_delete(&identity, b, Fault::None).unwrap();
    assert_eq!(
        lab.finish_delete(&i, Fault::FinalizeAck),
        Err(Error::Unknown)
    );
    assert!(lab.permit(&identity, b).is_err());
    lab.model.reconcile(&lab.witness.model, 2).unwrap();
    assert_eq!(lab.model.records()[&i.id].state, IntentState::Finalized);
    assert!(lab.permit(&identity, b).is_err());
}
#[test]
fn aad_key_nonce_tag_ciphertext_and_length_tamper_fail() {
    let mut lab = Lab::new(Profile::Local);
    let identity = lab.identity();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    for kind in 0..8 {
        let mut sealed = lab.store.read(&b).unwrap();
        match kind {
            0 => sealed.bytes[0] ^= 1,
            1 => sealed.bytes[24] ^= 1,
            2 => sealed.bytes[40] ^= 1,
            3 => sealed.binding.scope.space = [99; 16],
            4 => sealed.binding.protection = [99; 32],
            5 => sealed.binding.seal_generation += 1,
            6 => sealed.binding.key = [99; 16],
            _ => sealed.length += 1,
        }
        assert!(lab.vault.open(&sealed).is_err());
    }
    assert_eq!(lab.released, 0);
}
#[test]
fn actual_ciphertext_file_corruption_and_truncation_are_rejected() {
    for truncate in [false, true] {
        let mut lab = Lab::new(Profile::Local);
        let identity = lab.identity();
        let b = lab.stage_canary(&identity, Fault::None).unwrap();
        let file = &mut lab.store.entries.get_mut(&b.payload).unwrap().file;
        if truncate {
            file.set_len(2).unwrap();
        } else {
            file.seek(SeekFrom::Start(0)).unwrap();
            file.write_all(&[0; 24]).unwrap();
        }
        assert!(lab.store.read(&b).is_err());
    }
}
#[test]
fn no_replace_and_stale_worker_are_enforced_at_receiving_store() {
    let mut lab = Lab::new(Profile::Local);
    let identity = lab.identity();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    let sealed = lab.store.read(&b).unwrap();
    assert_eq!(lab.store.publish(sealed, Fault::None), Err(Error::Conflict));
    lab.store.erase(&b, 2).unwrap();
    assert_eq!(lab.store.erase(&b, 1), Err(Error::Denied));
    lab.vault.revoke(&b, 2).unwrap();
    assert_eq!(lab.vault.revoke(&b, 1), Err(Error::Denied));
}
#[test]
fn a_seal_generation_cannot_be_encrypted_twice_even_after_failure() {
    let mut vault = FakeKeyVault::new();
    let b = fixture_binding();
    vault.allocate(b, Fault::None).unwrap();
    let mut plain = LockedBuffer::new(32, false).unwrap();
    assert!(vault.seal(&b, &mut plain, Fault::Encrypt).is_err());
    assert!(vault.seal(&b, &mut plain, Fault::None).is_err());
}
#[test]
fn secure_memory_lock_failure_bounds_and_explicit_wipe() {
    assert!(LockedBuffer::new(16, true).is_err());
    assert!(LockedBuffer::new(0, false).is_err());
    assert!(LockedBuffer::new(4097, false).is_err());
    let mut buffer = LockedBuffer::new(4096, false).unwrap();
    buffer.bytes_mut().fill(0xa5);
    buffer.wipe();
    assert!(buffer.bytes().iter().all(|b| *b == 0));
}
#[test]
fn separate_stages_use_independent_random_keys_and_nonces() {
    let mut lab = Lab::new(Profile::Local);
    let identity = lab.identity();
    let a = lab.stage_canary(&identity, Fault::None).unwrap();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    assert_ne!(a.key, b.key);
    assert_ne!(a.payload, b.payload);
    assert_ne!(
        lab.store.read(&a).unwrap().bytes,
        lab.store.read(&b).unwrap().bytes
    );
}
#[test]
fn retained_old_ciphertext_cannot_open_after_lab_key_revocation() {
    let mut lab = Lab::new(Profile::Local);
    let identity = lab.identity();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    let old = lab.store.read(&b).unwrap();
    let i = lab.begin_delete(&identity, b, Fault::None).unwrap();
    lab.finish_delete(&i, Fault::None).unwrap();
    assert!(lab.vault.open(&old).is_err());
    // This says nothing about a snapshot of the entire fake vault; that adversary is unsupported.
}
#[test]
fn mutex_release_order_is_linearized_against_delete_in_two_threads() {
    use std::sync::{Arc, Barrier, Mutex};
    let mut lab = Lab::new(Profile::Protected);
    let identity = lab.identity();
    let b = lab.stage_canary(&identity, Fault::None).unwrap();
    let lab = Arc::new(Mutex::new(lab));
    let start = Arc::new(Barrier::new(2));
    let a = Arc::clone(&lab);
    let barrier = Arc::clone(&start);
    let reader = std::thread::spawn(move || {
        barrier.wait();
        let mut l = a.lock().unwrap();
        let identity = l.identity();
        l.permit(&identity, b).and_then(|p| l.release(p)).is_ok()
    });
    start.wait();
    let released_at_guard = {
        let mut l = lab.lock().unwrap();
        let identity = l.identity();
        l.begin_delete(&identity, b, Fault::None).unwrap();
        l.released
    };
    let success = reader.join().unwrap();
    let l = lab.lock().unwrap();
    assert_eq!(l.released, released_at_guard);
    assert_eq!(usize::from(success), released_at_guard);
}
#[test]
fn production_no_content_runtime_is_not_upgraded_by_lab_dependencies() {
    let runtime = yixu_application::NoContentRuntime::default();
    assert!(!runtime.content_available());
    let id = yixu_domain::Id::from_bytes([1; 16]);
    for classification in [
        yixu_domain::Classification::Synthetic,
        yixu_domain::Classification::Personal,
        yixu_domain::Classification::Sensitive,
    ] {
        assert_eq!(
            runtime.stage(
                yixu_domain::Scope {
                    tenant: id,
                    space: id
                },
                classification,
                CANARY
            ),
            Err(yixu_domain::Error::CapabilityUnavailable)
        );
    }
}
