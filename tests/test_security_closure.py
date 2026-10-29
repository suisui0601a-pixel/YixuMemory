"""R1/R14 design predicates and shapes only, never real crypto/erasure acceptance."""
from __future__ import annotations

import copy
import json
from pathlib import Path
from typing import Any, cast

import pytest
from jsonschema import Draft202012Validator, FormatChecker

from checks.security_closure import (
    STORES, SURFACES, deletion_completion, deletion_plan, effective_classification,
    orphan_cleanup_status, production_write_violations, read_allowed, replacement_allowed,
)

ROOT = Path(__file__).resolve().parents[1]


def load(name: str) -> dict[str, Any]:
    return cast(dict[str, Any], json.loads((ROOT / name).read_text("utf-8")))


def examples() -> dict[str, Any]:
    return load("contracts/security/examples.json")


def validator(name: str) -> Any:
    file = ("contracts/security/deletion-intent.schema.json" if name == "deletion-intent"
            else f"contracts/jsonschema/{name}.schema.json")
    value = load(file)
    Draft202012Validator.check_schema(value)
    return Draft202012Validator(value, format_checker=FormatChecker())


@pytest.mark.parametrize("name", ["deletion-intent", "deletion-request", "payload"])
def test_security_shapes_positive_and_required(name: str) -> None:
    value = examples()[name.replace("-", "_")]
    validator(name).validate(value)
    for field in validator(name).schema["required"]:
        broken = copy.deepcopy(value)
        del broken[field]
        assert not validator(name).is_valid(broken), field


@pytest.mark.parametrize("field,value", [
    ("effective_scope", "tenant"), ("requested_scope", "tenant"), ("coverage", "pending"),
    ("receipt_set_ref", None), ("generation", 0), ("target_type", "memory"),
    ("operation", "source_all_copies"),
])
def test_completed_scope_cannot_be_misrepresented(field: str, value: Any) -> None:
    record = examples()["deletion_request"]
    record[field] = value
    assert not validator("deletion-request").is_valid(record)


@pytest.mark.parametrize("field,value", [
    ("encrypted", False), ("effective_classification", "synthetic"), ("key_ref", None),
    ("encrypted_ref", None), ("plaintext", "synthetic-canary-never-persist"),
    ("wrapped_dek", "synthetic-key-material-not-allowed"),
])
def test_payload_metadata_never_permits_a_plaintext_escape(field: str, value: Any) -> None:
    payload = examples()["payload"]
    payload[field] = value
    assert not validator("payload").is_valid(payload)


def intent(operation: str, target: str) -> dict[str, Any]:
    value = cast(dict[str, Any], examples()["deletion_intent"])
    value.update(operation=operation, target_id=target,
                 target_type={"memory_graph": "memory", "evidence_graph": "evidence",
                              "source_graph": "event"}[operation])
    return value


def plan(operation: str = "source_graph", target: str = "event") -> dict[str, Any]:
    graph = examples()["shared_graph"]
    return deletion_plan(intent(operation, target), graph, set(graph["nodes"]),
                         expected_version=1 if operation == "memory_graph" else None)


@pytest.mark.parametrize("operation,target", [("source_graph", "event"), ("evidence_graph", "e")])
def test_shared_source_invalidates_both_memories(operation: str, target: str) -> None:
    result = plan(operation, target)
    assert result["decision"] == "admit_graph"
    assert result["invalid_revisions"] == {"ra", "rb"}
    assert result["affected_memories"] == {"ma", "mb"}
    assert result["erase_payloads"] == {"source", "pa", "pb"}


def test_memory_only_preserves_independently_owned_shared_evidence() -> None:
    result = plan("memory_graph", "ma")
    assert result["fenced"] == {"ma", "ra"}
    assert result["erase_payloads"] == {"pa"}
    assert result["retained_payloads"] == {"source", "pb"}
    assert read_allowed("rb", "mb", {"e", "event"}, result["fenced"], "hydrate")


@pytest.mark.parametrize("operation,target_type", [("source_all_copies", "canonical_source"),
                                                  ("subject_all_copies", "subject")])
def test_two_independent_space_copies_cannot_be_tenant_completed(operation: str, target_type: str) -> None:
    graph = examples()["shared_graph"]
    second = copy.deepcopy(graph)
    second["space_id"] = "00000000-0000-4000-8000-000000000099"
    request = examples()["deletion_intent"]
    request.update(operation=operation, requested_scope="tenant", target_type=target_type, space_id=None)
    validator("deletion-intent").validate(request)
    for local_graph in [graph, second]:
        result = deletion_plan(request, local_graph, set(local_graph["nodes"]))
        assert result == {"decision": "unsupported_erasure_scope", "fenced": set(), "erase_payloads": set()}
    assert second == graph | {"space_id": second["space_id"]}


@pytest.mark.parametrize("case,reason", [("no-mb-permission", "denied"),
                                       ("unknown-graph", "incomplete_graph"),
                                       ("missing-reference", "incomplete_graph"),
                                       ("unknown-owner", "incomplete_graph"),
                                       ("other-space", "scope_mismatch"),
                                       ("tenant-scope", "unsupported_erasure_scope"),
                                       ("wrong-target-kind", "invalid_target")])
def test_admission_fails_before_any_partial_fence(case: str, reason: str) -> None:
    graph = examples()["shared_graph"]
    request = intent("source_graph", "event")
    grants = set(graph["nodes"])
    if case == "no-mb-permission":
        grants.remove("mb")
    elif case == "unknown-graph":
        graph["snapshot_complete"] = False
    elif case == "missing-reference":
        graph["revision_evidence"]["rb"] = ["unregistered-evidence"]
    elif case == "unknown-owner":
        graph["payload_owners"]["source"] = []
    elif case == "other-space":
        request["space_id"] = "another-space"
    elif case == "tenant-scope":
        request["requested_scope"] = "tenant"
    else:
        request["target_type"] = "memory"
    before = copy.deepcopy(graph)
    result = deletion_plan(request, graph, grants)
    assert result == {"decision": reason, "fenced": set(), "erase_payloads": set()}
    assert graph == before  # Pure model only; does not prove a real DB rollback.


def test_explicit_derived_edges_and_ambiguous_shared_memory_content() -> None:
    graph = examples()["shared_graph"]
    graph["derived_from"] = {"rb": ["ra"]}
    result = deletion_plan(intent("memory_graph", "ma"), graph, set(graph["nodes"]), expected_version=1)
    assert result["invalid_revisions"] == {"ra", "rb"}
    graph["derived_from"] = {}
    graph["payload_owners"]["pa"] = ["ra", "rb"]
    result = deletion_plan(intent("memory_graph", "ma"), graph, set(graph["nodes"]), expected_version=1)
    assert result["decision"] == "ambiguous_payload_ownership" and not result["fenced"]


def completion() -> dict[str, Any]:
    return {"requested_scope": "space", "effective_scope": "space", "operation": "source_graph",
            "closure_complete": True, "fence_active": True, "writers_drained": True,
            "generation": 2, "verified_generation": 2, "receipts": dict.fromkeys(STORES, True)}


@pytest.mark.parametrize("store", sorted(STORES))
def test_partial_erasure_is_blocked_and_all_read_surfaces_stay_fenced(store: str) -> None:
    observed = completion()
    assert deletion_completion(observed) == "completed"
    observed["receipts"][store] = False
    assert deletion_completion(observed) == "blocked"
    fences = plan()["fenced"]
    for surface in SURFACES:
        for revision, memory in [("ra", "ma"), ("rb", "mb")]:
            assert not read_allowed(revision, memory, {"e", "event"}, fences, surface)
    del observed["receipts"][store]
    assert deletion_completion(observed) == "blocked"


@pytest.mark.parametrize("field,value", [("writers_drained", False), ("closure_complete", False),
                                         ("fence_active", False), ("verified_generation", 1),
                                         ("generation", True), ("effective_scope", "tenant")])
def test_completion_needs_current_full_generation(field: str, value: Any) -> None:
    observed = completion()
    observed[field] = value
    assert deletion_completion(observed) == "blocked"


def test_replacement_does_not_revalidate_old_revision() -> None:
    fences = plan()["fenced"]
    assert not read_allowed("ra", "ma", {"new-evidence"}, fences, "history")
    new = {"id": "ra2", "parent_revision_id": "ra", "number": 2, "evidence_ids": ["new-evidence"],
           "derived_from": [], "content_payload_id": "independent-new-payload"}
    observed = {"previous_number": 1, "all_deletions_completed": True, "authorized": True,
                "cas_matches": True, "fresh_evidence_valid": True}
    assert replacement_allowed("ra", new, fences, observed)
    after_explicit_replacement = fences - {"ma"}
    assert read_allowed("ra2", "ma", {"new-evidence"}, after_explicit_replacement, "hydrate")
    assert not read_allowed("ra", "ma", {"e"}, after_explicit_replacement, "history")
    for field, value in [("id", "ra"), ("evidence_ids", ["e"]), ("derived_from", ["ra"]),
                          ("parent_revision_id", "unrelated"), ("number", 1)]:
        assert not replacement_allowed("ra", new | {field: value}, fences, observed)
    assert not replacement_allowed("ra", new, fences, observed | {"all_deletions_completed": False})
    assert not replacement_allowed("ra", new, fences, observed | {"cas_matches": False})


@pytest.mark.parametrize("floor,claim,lineage,previous,expected", [
    ("personal", "personal", ["personal"], None, "personal"),
    ("personal", "synthetic", [], None, "sensitive"),
    ("personal", "personal", ["sensitive"], None, "sensitive"),
    ("sensitive", "personal", ["personal"], None, "sensitive"),
    (None, "personal", ["personal"], None, "sensitive"),
    ("personal", "personal", [None], None, "sensitive"),
    ("personal", "personal", ["personal"], "sensitive", "sensitive"),
    ("personal", None, [], None, "sensitive"),
])
def test_server_classification_join(floor: str | None, claim: str | None,
                                   lineage: list[str | None], previous: str | None, expected: str) -> None:
    assert effective_classification(floor, claim, lineage, previous) == expected


def protected_write() -> dict[str, Any]:
    return {"secure_backend": True, "key_available": True, "allocation_recoverable": True, "seal_ok": True, "aad_verified": True,
            "ciphertext_only": True, "effective_classification": "sensitive",
            "required_classification": "sensitive", "policy_epoch": 2,
            "current_policy_epoch": 2, "reauthorized": True, "lineage_rechecked": True,
            "cleanup_registered": True, "plaintext_surfaces": [], "test_plaintext_adapter": False}


@pytest.mark.parametrize("field,value,reason", [
    ("secure_backend", False, "key_backend_unavailable"),
    ("key_available", False, "key_backend_unavailable"),
    ("allocation_recoverable", False, "key_backend_unavailable"),
    ("seal_ok", False, "encryption_failed"), ("aad_verified", False, "encryption_failed"),
    ("ciphertext_only", False, "encryption_failed"),
    ("policy_epoch", 1, "protection_context_invalid"),
    ("policy_epoch", None, "protection_context_invalid"),
    ("lineage_rechecked", False, "protection_context_invalid"),
    ("reauthorized", False, "protection_context_invalid"),
    ("cleanup_registered", False, "protection_context_invalid"),
    ("effective_classification", "synthetic", "protection_context_invalid"),
    ("effective_classification", "personal", "protection_context_invalid"),
    ("test_plaintext_adapter", True, "plaintext_forbidden"),
    ("plaintext_surfaces", ["WAL"], "plaintext_forbidden"),
])
def test_production_write_fail_closed(field: str, value: Any, reason: str) -> None:
    observed = protected_write()
    assert not production_write_violations(observed)
    observed[field] = value
    # synthetic/NoModel labels do not alter the mandatory backend/seal gate.
    observed.update(declared_classification="synthetic", model_mode="none", backend_mode="offline-local")
    assert reason in production_write_violations(observed)


def test_even_personal_production_content_needs_keys_and_plaintext_is_never_staged() -> None:
    observed = protected_write() | {"effective_classification": "personal", "key_available": False}
    assert "key_backend_unavailable" in production_write_violations(observed)
    observed = protected_write()
    for place in ["DB", "WAL", "TEMP", "staging", "index_terms", "log", "audit", "backup"]:
        assert "plaintext_forbidden" in production_write_violations(observed | {"plaintext_surfaces": [place]})


def test_rollback_orphan_cleanup_remains_blocked_without_actual_receipt_observations() -> None:
    observed = {"unreadable": True, "key_revocation_verified": True,
                "ciphertext_erasure_verified": True, "generation_matches": True}
    assert orphan_cleanup_status(observed) == "erased"
    for field in observed:
        assert orphan_cleanup_status(observed | {field: False}) == "blocked"
    assert orphan_cleanup_status({}) == "blocked"


def test_public_api_drift_and_no_client_crypto_flags() -> None:
    base = load("contracts/openapi/openapi.json")
    ext = load("contracts/deployment/agent-extension.openapi.json")
    expected = {k: v for k, v in validator("deletion-request").schema.items() if k not in {"$id", "$schema"}}
    assert base["components"]["schemas"]["Deletion"] == expected
    policy = load("contracts/security/policy.json")
    assert policy["runtime_verified"] is False and policy["all_controlled_copies_required"] is True
    assert set(policy["required_stores"]) == STORES
    assert set(policy["read_surfaces"]) == SURFACES
    assert policy["production_encrypt_all_payloads"] and not policy["public_plaintext_bypass"]
    stage = ext["components"]["schemas"]["PayloadInput"]
    valid: dict[str, Any] = {"purpose": "content", "classification": "synthetic", "text": "synthetic-canary"}
    check = Draft202012Validator(stage)
    check.validate(valid)  # Declaration accepted as data, never an encryption bypass.
    for field in ["effective_classification", "encrypted", "key_backend_safe", "plaintext_adapter", "test_mode"]:
        assert not check.is_valid(valid | {field: True})
    result = ext["components"]["schemas"]["StagedPayload"]
    for field in ["effective_classification", "protection_policy_epoch", "encrypted"]:
        assert field in result["required"] and result["properties"][field]["readOnly"] is True
    assert examples()["runtime_executed"] is False


def test_historical_and_transitive_source_dependencies_are_fenced() -> None:
    graph = examples()["shared_graph"]
    graph["nodes"]["ra-old"] = "revision"
    graph["revision_evidence"]["ra-old"] = ["e"]
    graph["revision_memory"]["ra-old"] = "ma"
    graph["payload_owners"]["pa-old"] = ["ra-old"]
    result = deletion_plan(intent("source_graph", "event"), graph, set(graph["nodes"]))
    assert result["invalid_revisions"] == {"ra", "rb", "ra-old"}
    assert result["erase_payloads"] == {"source", "pa", "pb", "pa-old"}


def test_classification_follows_event_evidence_revision_and_derivation() -> None:
    event = effective_classification("personal", "synthetic", [None])
    evidence = effective_classification("personal", "personal", [event])
    revision = effective_classification("personal", "personal", [evidence], previous="personal")
    projection = effective_classification("personal", "personal", [revision])
    assert [event, evidence, revision, projection] == ["sensitive"] * 4
    # A cached old personal classification is rejected even if its epoch was reused.
    observed = protected_write() | {"effective_classification": "personal", "required_classification": projection}
    assert "protection_context_invalid" in production_write_violations(observed)


def test_clients_cannot_supply_completion_flags_in_intent() -> None:
    valid = examples()["deletion_intent"]
    validator("deletion-intent").validate(valid)
    for key in ["status", "effective_scope", "coverage", "receipt_set_ref", "closure_complete"]:
        assert not validator("deletion-intent").is_valid(valid | {key: True})


def test_generic_memory_intent_cannot_bypass_existing_cas_precondition() -> None:
    graph = examples()["shared_graph"]
    request = intent("memory_graph", "ma")
    for version, reason in [(None, "precondition_required"), (0, "state_conflict"), (True, "state_conflict")]:
        result = deletion_plan(request, graph, set(graph["nodes"]), expected_version=version)
        assert result["decision"] == reason and not result["fenced"]
