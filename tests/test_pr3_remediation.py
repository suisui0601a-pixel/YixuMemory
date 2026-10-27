"""A1/A2/A3 review contracts. No real MCP SDK, filesystem race or broker executes."""
from __future__ import annotations

import copy
import json
from datetime import datetime
from pathlib import Path
from typing import Any, cast

import pytest
from jsonschema import Draft202012Validator, FormatChecker

from checks.deployment_contracts import filesystem_violations, plan_violations
from checks.mcp_wire_contracts import LEGACY, MODERN, fallback_decision, wire_decision

ROOT = Path(__file__).resolve().parents[1] / "contracts/deployment"
NOW = datetime.fromisoformat("2026-10-28T00:00:00+00:00")


def read(name: str) -> dict[str, Any]:
    return cast(dict[str, Any], json.loads((ROOT / name).read_text("utf-8")))


def validator(name: str) -> Any:
    return Draft202012Validator(read(name + ".schema.json"), format_checker=FormatChecker())


@pytest.mark.parametrize("entry", read("mcp-wire-examples.json")["requests"], ids=lambda e: e["name"])
def test_two_era_wire_vectors(entry: dict[str, Any]) -> None:
    assert validator("mcp-wire").is_valid(entry["case"]) == entry["schema_valid"]
    assert wire_decision(entry["case"]) == entry["expected"]
    if "client-must-abort" in entry["name"]:
        assert entry["expected"]["selected_version"] != entry["case"]["request"]["params"]["protocolVersion"]


@pytest.mark.parametrize("entry", read("mcp-wire-examples.json")["results"],
                         ids=lambda e: e["era"] + ":" + e["method"])
def test_two_era_response_contract(entry: dict[str, Any]) -> None:
    result = entry["result"]
    if entry["era"] == "modern":
        assert result["resultType"] == "complete"
    else:
        assert "resultType" not in result
    if entry["method"] == "server/discover":
        assert result["supportedVersions"] == [MODERN, LEGACY]
        assert result["capabilities"] == {"tools": {}}
    if entry["method"] == "initialize":
        assert result["protocolVersion"] == LEGACY


@pytest.mark.parametrize("transport", ["stdio", "http"])
@pytest.mark.parametrize("code", [-32020, -32021, -32022])
def test_modern_errors_never_select_legacy(transport: str, code: int) -> None:
    assert fallback_decision(transport, "dual-era", None, 400, code,
                             "non-modern-error") == "stay-modern"


@pytest.mark.parametrize("transport,status,code,kind,cached,mode,expected", [
    ("stdio", 200, -32601, "non-modern-error", None, "dual-era", "fresh-legacy-handshake"),
    ("stdio", 200, -32602, "non-modern-error", None, "dual-era", "fresh-legacy-handshake"),
    ("stdio", 0, None, "timeout", None, "dual-era", "fresh-legacy-handshake"),
    ("http", 400, None, "empty", None, "dual-era", "fresh-legacy-handshake"),
    ("http", 400, None, "non-modern-error", None, "dual-era", "fresh-legacy-handshake"),
    ("http", 401, None, "empty", None, "dual-era", "authenticate-or-fail"),
    ("http", 403, None, "empty", None, "dual-era", "authenticate-or-fail"),
    ("http", 404, -32601, "non-modern-error", None, "dual-era", "stay-modern"),
    ("http", 404, None, "empty", None, "dual-era", "fail-no-downgrade"),
    ("http", 400, None, "empty", "modern", "dual-era", "fail-no-downgrade"),
    ("stdio", 0, None, "timeout", "modern", "dual-era", "fail-no-downgrade"),
    ("stdio", 200, -32601, "non-modern-error", None, "modern-only", "fail-no-downgrade"),
])
def test_transport_detection_boundaries(transport: str, status: int, code: int | None,
                                        kind: str, cached: str | None, mode: str,
                                        expected: str) -> None:
    assert fallback_decision(transport, mode, cached, status, code, kind) == expected


@pytest.mark.parametrize("version,mode,fallback", [(MODERN, "modern-only", False),
                                                    (MODERN, "dual-era", True),
                                                    (LEGACY, "legacy-only", False)])
def test_client_config_explicit_era(version: str, mode: str, fallback: bool) -> None:
    value = read("examples/client-reader.json")
    value.update(mcp_protocol=version,
                 mcp_compatibility={"mode": mode, "legacy_fallback_allowed": fallback})
    validator("client-config").validate(value)
    value["mcp_protocol"] = "1900-01-01"
    assert not validator("client-config").is_valid(value)


def approval_observation() -> dict[str, Any]:
    return {"host_id": "synthetic-host", "config_digest": "b" * 64,
            "preflight_digest": "c" * 64, "artifact_digest": "d" * 64,
            "canonical_plan_hash": "a" * 64, "unknown_files": False,
            "paths_verified": True, "audit_available": True, "broker_proof_verified": True,
            "elevation_policy_allows": False}


@pytest.mark.parametrize("case,expected", [
    ("extra-action", "approval_required"), ("duplicate-action", "approval_required"),
    ("extra-scope", "permission_denied"), ("admin-scope", "permission_denied"),
    ("duplicate-scope", "permission_denied"), ("duplicate-plan-scope", "security_blocked"),
    ("unused-elevation", "permission_denied"), ("required-elevation-no-policy", "security_blocked"),
    ("proof-in-caller-json", "permission_denied"), ("broad-approval-kind", "approval_required"),
])
def test_single_plan_approval_rejects_supersets(case: str, expected: str) -> None:
    plan, approval, observation = read("examples/plan.json"), read("examples/approval.json"), approval_observation()
    if case == "extra-action":
        approval["action_ids"].append("not-in-this-plan")
    elif case == "duplicate-action":
        approval["action_ids"].append(approval["action_ids"][0])
    elif case == "extra-scope":
        approval["scopes"].append("config.write")
    elif case == "admin-scope":
        approval["scopes"].append("installation.admin")
    elif case == "duplicate-scope":
        approval["scopes"].append(approval["scopes"][0])
    elif case == "duplicate-plan-scope":
        plan["actions"][0]["required_scopes"].append("filesystem.create")
    elif case == "unused-elevation":
        approval["allow_elevation"] = True
    elif case == "required-elevation-no-policy":
        plan["actions"][0]["requires_elevation"] = True
        approval["allow_elevation"] = True
    elif case == "proof-in-caller-json":
        approval["broker_proof_verified"] = True
        observation["broker_proof_verified"] = False
        assert not validator("approval").is_valid(approval)
    elif case == "broad-approval-kind":
        approval["approval_kind"] = "all-admin-actions"
    assert expected in plan_violations(plan, approval, observation, NOW, read("action-policy.json")["rules"])


def test_single_plan_exact_union_and_approved_elevation() -> None:
    plan, approval, observation = read("examples/plan.json"), read("examples/approval.json"), approval_observation()
    action = copy.deepcopy(plan["actions"][0])
    action.update(id="write-config", kind="write_config", required_scopes=["config.write"],
                  requires_elevation=True)
    plan["actions"].append(action)
    approval.update(action_ids=["write-config", "init-dirs"],
                    scopes=["config.write", "filesystem.create"], allow_elevation=True)
    observation["elevation_policy_allows"] = True
    assert not plan_violations(plan, approval, observation, NOW, read("action-policy.json")["rules"])


def fs_observation() -> dict[str, Any]:
    return {"anchor_verified": True, "owner_acl_verified": True, "protected_ancestors": True,
            "primitives_available": True, "all_components_no_reparse": True, "types_verified": True,
            "handle_relative_only": True, "ancestry_verified": True, "link_count_verified": True, "path_string_reopened": False,
            "live_anchor_identity": "demo-anchor-1", "live_volume_identity": "demo-volume-1",
            "target_exists": False, "cleanup_receipt_owned": True, "cleanup_identity_matches": True,
            "unknown_descendants": False, "sentinel_bytes": "unchanged-synthetic-sentinel"}


def test_handle_contract_positive_is_only_synthetic() -> None:
    intent = read("examples/filesystem-operation.json")
    validator("filesystem-operation").validate(intent)
    binding = read("examples/plan.json")["path_bindings"][0]
    for field in ("anchor_ref", "anchor_identity", "volume_identity", "owner_ref", "acl_digest"):
        assert binding[field] == intent[field]
    assert not filesystem_violations(intent, fs_observation())
    assert read("filesystem-policy.json")["runtime_verified"] is False


@pytest.mark.parametrize("case,field,value,expected", [
    ("parent-after-plan", "live_anchor_identity", "replaced", "state_conflict"),
    ("component-during-create", "all_components_no_reparse", False, "security_blocked"),
    ("symlink-before-publish", "all_components_no_reparse", False, "security_blocked"),
    ("junction-before-publish", "all_components_no_reparse", False, "security_blocked"),
    ("rename-out-of-root", "ancestry_verified", False, "security_blocked"),
    ("cross-volume", "live_volume_identity", "another-volume", "state_conflict"),
    ("existing-bytes", "target_exists", True, "state_conflict"),
    ("cleanup-replaced-object", "cleanup_identity_matches", False, "security_blocked"),
    ("cleanup-unknown-descendants", "unknown_descendants", True, "security_blocked"),
    ("unsupported-no-replace", "primitives_available", False, "security_blocked"),
    ("unverified-acl", "owner_acl_verified", False, "security_blocked"),
    ("mutable-ancestors", "protected_ancestors", False, "security_blocked"),
    ("wrong-type", "types_verified", False, "security_blocked"),
    ("unexpected-hardlink", "link_count_verified", False, "security_blocked"),
    ("path-string-fallback", "path_string_reopened", True, "security_blocked"),
])
def test_race_review_contract_rejects_observations(case: str, field: str, value: Any,
                                                  expected: str) -> None:
    intent, observation = read("examples/filesystem-operation.json"), fs_observation()
    if case.startswith("cleanup"):
        intent["operation"] = "cleanup_owned"
    observation[field] = value
    snapshot = copy.deepcopy(observation)
    assert expected in filesystem_violations(intent, observation)
    # Proves only oracle purity, not preservation by an actual OS operation.
    assert observation == snapshot and observation["sentinel_bytes"] == "unchanged-synthetic-sentinel"


@pytest.mark.parametrize("name", ["..", "/tmp", "a/b", "a\\b", "x:stream", "tail.", " padded ", "CON"])
def test_handle_component_restrictions(name: str) -> None:
    intent = read("examples/filesystem-operation.json")
    intent["components"] = [name]
    if name == "CON":
        assert filesystem_violations(intent, fs_observation()) == {"security_blocked"}
    else:
        assert not validator("filesystem-operation").is_valid(intent)


def test_design_matrix_catalog_and_openapi_agree() -> None:
    matrix, catalog = read("mcp-protocols.json"), read("tools.json")
    assert matrix["runtime_verified"] is False
    assert matrix["first_release_target"] == MODERN and matrix["compatibility_target"] == LEGACY
    assert catalog["mcp_protocols"] == [MODERN, LEGACY]
    assert "server/discover" in catalog["protocol_methods"]["modern"]
    assert "initialize" not in catalog["protocol_methods"]["modern"]
    assert all(t["name"] != "server/discover" for t in catalog["tools"])
    ext = read("agent-extension.openapi.json")
    expected = {k: v for k, v in read("capabilities.schema.json").items() if k not in ("$schema", "$id")}
    assert ext["components"]["schemas"]["Capabilities"] == expected
    assert ext["x-mcp-wire-contract"]["runtime_implemented"] is False
