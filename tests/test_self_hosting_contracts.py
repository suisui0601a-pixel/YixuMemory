"""Design fixtures and negative contract checks, not live deployment acceptance."""
from __future__ import annotations

import copy
from collections.abc import Hashable, Mapping
import json
from datetime import datetime
from pathlib import Path
from typing import Any, cast

import pytest
from jsonschema import Draft202012Validator, FormatChecker
from openapi_spec_validator import validate

from checks.deployment_contracts import memory_call_violations, plan_violations

ROOT = Path(__file__).resolve().parents[1]
CONTRACT = ROOT / "contracts/deployment"
BASE = ROOT / "contracts/openapi/openapi.json"
NOW = datetime.fromisoformat("2026-10-28T00:00:00+00:00")


def load(path: Path) -> dict[str, Any]:
    return cast(dict[str, Any], json.loads(path.read_text("utf-8")))


def read(name: str) -> dict[str, Any]:
    return load(CONTRACT / name)


def validator(name: str) -> Any:
    return Draft202012Validator(read(name + ".schema.json"), format_checker=FormatChecker())


def catalog() -> dict[str, dict[str, Any]]:
    return {t["name"]: t for t in read("tools.json")["tools"]}


def merged_api() -> dict[str, Any]:
    api = load(BASE)
    ext = read("agent-extension.openapi.json")
    api["paths"].update(ext["paths"])
    for kind in ("schemas", "parameters", "securitySchemes"):
        overlap = set(api["components"][kind]) & set(ext["components"][kind])
        assert not overlap, overlap
        api["components"][kind].update(ext["components"][kind])
    return api


@pytest.mark.parametrize("path", sorted(CONTRACT.glob("*.schema.json")), ids=lambda p: p.name)
def test_self_hosted_schemas_are_valid(path: Path) -> None:
    Draft202012Validator.check_schema(load(path))


@pytest.mark.parametrize("profile", ["desktop", "container", "server", "postgres"])
def test_profile_defaults_are_safe_and_no_model(profile: str) -> None:
    value = read(f"profiles/{profile}.json")
    validator("config").validate(value)
    assert value["model"] == {"mode": "none", "external_calls": False}
    assert not any(value["licensing"][key] for key in (
        "registration_required", "activation_required", "periodic_online_check",
        "memory_upload_required",
    ))
    assert value["licensing"]["device_limit"] is None
    assert value["network"]["authentication_required"]


@pytest.mark.parametrize("mutation", [
    "native-public", "publish-public", "public-db", "anonymous", "http-proxy",
    "destructive-reinstall", "network-share", "activation", "device-limit",
    "plaintext-token", "wrong-audience", "no-backup", "container-no-volume",
    "pg-no-secret", "pg-no-tls",
])
def test_unsafe_profile_mutations_fail(mutation: str) -> None:
    profile = "container" if mutation == "container-no-volume" else (
        "postgres" if mutation.startswith("pg-") else "desktop"
    )
    value = read(f"profiles/{profile}.json")
    if mutation == "native-public":
        value["network"]["application_bind"] = "0.0.0.0"
    elif mutation == "publish-public":
        value["network"]["host_publish_bind"] = "0.0.0.0"
    elif mutation == "public-db":
        value["database"]["publish_host_port"] = True
    elif mutation == "anonymous":
        value["network"]["authentication_required"] = False
    elif mutation == "http-proxy":
        value["network"].update(remote_enabled=True, proxy_endpoint="http://example.invalid")
    elif mutation == "destructive-reinstall":
        value["storage"]["preserve_existing_data"] = False
    elif mutation == "network-share":
        value["storage"]["filesystem"] = "nfs"
    elif mutation == "activation":
        value["licensing"]["activation_required"] = True
    elif mutation == "device-limit":
        value["licensing"]["device_limit"] = 1
    elif mutation == "plaintext-token":
        value["secrets"]["token"] = "synthetic-do-not-store"
    elif mutation == "wrong-audience":
        value["secrets"]["client_audience"] = "yixumemory-control"
    elif mutation == "no-backup":
        value["backup"]["required_before_upgrade"] = False
    elif mutation == "container-no-volume":
        value["storage"]["volume_ref"] = None
    elif mutation == "pg-no-secret":
        value["database"]["credential_ref"] = None
    elif mutation == "pg-no-tls":
        value["database"]["tls_mode"] = "local-socket"
    assert not validator("config").is_valid(value)


@pytest.mark.parametrize("name", ["plan", "approval", "client-reader", "audit"])
def test_deployment_examples(name: str) -> None:
    target = {"client-reader": "client-config", "audit": "audit-record"}.get(name, name)
    validator(target).validate(read(f"examples/{name}.json"))


def observation() -> dict[str, Any]:
    return {
        "host_id": "synthetic-host", "config_digest": "b" * 64,
        "preflight_digest": "c" * 64, "artifact_digest": "d" * 64,
        "canonical_plan_hash": "a" * 64, "unknown_files": False,
        "paths_verified": True, "audit_available": True, "broker_proof_verified": True,
    }


def test_plan_semantic_reference_positive() -> None:
    assert not plan_violations(read("examples/plan.json"), read("examples/approval.json"),
                               observation(), NOW, read("action-policy.json")["rules"])


@pytest.mark.parametrize("case,expected", [
    ("host-drift", "state_conflict"), ("artifact-drift", "state_conflict"),
    ("facts-drift", "state_conflict"), ("config-drift", "state_conflict"),
    ("hash-drift", "state_conflict"), ("unknown-files", "state_conflict"),
    ("path-escape", "security_blocked"), ("no-audit", "security_blocked"),
    ("no-approval", "approval_required"), ("expired", "approval_required"),
    ("expired-plan", "approval_required"), ("forged-proof", "permission_denied"),
    ("other-instance", "approval_required"), ("other-plan", "approval_required"),
    ("missing-action", "approval_required"), ("missing-scope", "permission_denied"),
    ("elevation", "permission_denied"), ("upgrade-no-backup", "compatibility_blocked"),
    ("restore-no-watermark", "compatibility_blocked"),
    ("omitted-required-scope", "security_blocked"), ("duplicate-action", "state_conflict"),
])
def test_plan_semantic_reference_rejects(case: str, expected: str) -> None:
    plan, approval, facts = read("examples/plan.json"), read("examples/approval.json"), observation()
    maybe_approval: dict[str, Any] | None = approval
    if case.endswith("drift"):
        field = {"host-drift": "host_id", "artifact-drift": "artifact_digest",
                 "facts-drift": "preflight_digest", "config-drift": "config_digest",
                 "hash-drift": "canonical_plan_hash"}[case]
        facts[field] = "different"
    elif case == "unknown-files":
        facts["unknown_files"] = True
    elif case == "path-escape":
        facts["paths_verified"] = False
    elif case == "no-audit":
        facts["audit_available"] = False
    elif case == "no-approval":
        maybe_approval = None
    elif case == "expired":
        approval["valid_until"] = "2026-10-27T00:00:00Z"
    elif case == "expired-plan":
        plan["valid_until"] = "2026-10-27T00:00:00Z"
    elif case == "forged-proof":
        facts["broker_proof_verified"] = False
    elif case == "other-instance":
        approval["instance_id"] = "00000000-0000-4000-8000-000000000099"
    elif case == "other-plan":
        approval["plan_hash"] = "f" * 64
    elif case == "missing-action":
        approval["action_ids"] = []
    elif case == "missing-scope":
        approval["scopes"] = []
    elif case == "elevation":
        plan["actions"][0]["requires_elevation"] = True
    elif case == "upgrade-no-backup":
        plan["upgrade"]["required"] = True
    elif case == "restore-no-watermark":
        plan["actions"][0].update(kind="restore_backup", required_scopes=["backup.restore"])
        plan["upgrade"].update(backup_verified=True, restore_drill_verified=True,
                               migration_compatible=True, deletion_watermark_verified=False)
    elif case == "omitted-required-scope":
        plan["actions"][0]["required_scopes"] = ["config.write"]
    elif case == "duplicate-action":
        plan["actions"].append(copy.deepcopy(plan["actions"][0]))
    assert expected in plan_violations(plan, maybe_approval, facts, NOW,
                                       read("action-policy.json")["rules"])


@pytest.mark.parametrize("entry", read("exit-codes.json")["entries"], ids=lambda e: e["code"])
def test_cli_exit_codes_and_envelopes(entry: dict[str, Any]) -> None:
    ok = entry["exit_code"] == 0
    value = {"contract_version": "1.0", "request_id": "00000000-0000-4000-8000-000000000001",
             "command": "doctor", "status": "ok" if ok else "error",
             "exit_code": entry["exit_code"], "error": None if ok else {
                 "code": entry["code"], "retryable": entry["retryable"], "message_id": "synthetic"
             }, "result_ref": None, "diagnostic_ref": None}
    validator("result").validate(value)
    if ok:
        value["status"] = "error"
    else:
        value["error"]["code"] = "ok"
    assert not validator("result").is_valid(value)


@pytest.mark.parametrize("mutation", ["control-audience", "admin-scope", "token", "remote-http",
                                      "url-secret", "multi-space"])
def test_client_config_never_grants_control(mutation: str) -> None:
    value = read("examples/client-reader.json")
    if mutation == "control-audience":
        value["audience"] = "yixumemory-control"
    elif mutation == "admin-scope":
        value["scopes"].append("installation.admin")
    elif mutation == "token":
        value["token"] = "synthetic-secret"
    elif mutation in ("remote-http", "url-secret"):
        value.update(transport="mcp-http", connection_ref=None,
                     endpoint="http://example.invalid/mcp" if mutation == "remote-http"
                     else "https://example.invalid/mcp?access_token=synthetic")
    else:
        value["space_ids"].append("00000000-0000-4000-8000-000000000099")
    assert not validator("client-config").is_valid(value)


def test_additive_openapi_and_catalog_are_consistent() -> None:
    api = merged_api()
    validate(cast(Mapping[Hashable, Any], api))
    original = load(BASE)
    assert api["components"]["securitySchemes"] == original["components"]["securitySchemes"]
    for name in ("Memory", "Event", "Revision", "Evidence"):
        assert api["components"]["schemas"][name] == original["components"]["schemas"][name]
    operations = {op["operationId"]: op for item in api["paths"].values()
                  for method, op in item.items() if method in ("get", "post", "patch", "delete")}
    tools = catalog()
    assert len(tools) == 13
    for tool in tools.values():
        assert tool["operation_id"] in operations
        Draft202012Validator.check_schema(tool["inputSchema"])
        assert tool["outputSchema"] == {
            k: v for k, v in read("tool-result.schema.json").items() if k not in ("$schema", "$id")
        }
        assert not any(scope.startswith(("installation.", "keys.", "service."))
                       for scope in tool["required_capabilities"])
        assert tool["annotations"]["readOnlyHint"] != tool["side_effect"]
    path = "/v1/tenants/{tenant_id}/spaces/{space_id}/search"
    extended_search = copy.deepcopy(api["paths"][path])
    assert extended_search["post"]["parameters"].pop()["name"] == "X-Min-Commit-Token"
    assert extended_search == original["paths"][path]


def test_synthetic_loop_shapes_references_and_watermarks() -> None:
    fixture = read("examples/synthetic-loop.json")
    assert fixture["synthetic_only"] is True and fixture["runtime_executed"] is False
    tools, api = catalog(), merged_api()
    operations = {op["operationId"]: op for item in api["paths"].values()
                  for m, op in item.items() if m in ("get", "post", "patch", "delete")}
    schemas = api["components"]["schemas"]
    root = {"$schema": "https://json-schema.org/draft/2020-12/schema", "components": api["components"]}
    for step in fixture["steps"]:
        tool = tools[step["tool"]]
        Draft202012Validator(tool["inputSchema"], format_checker=FormatChecker()).validate(
            step["arguments"])
        validator("tool-result").validate(step["receipt"])
        assert set(tool["required_capabilities"]) <= set(fixture["scopes"])
        assert step["arguments"]["tenant_id"] == fixture["tenant_id"]
        assert step["arguments"]["space_id"] == fixture["space_id"]
        responses = operations[tool["operation_id"]]["responses"]
        success = next(v for k, v in responses.items() if k.startswith("2"))
        resource = success["content"]["application/json"]["schema"]
        Draft202012Validator(root | resource, format_checker=FormatChecker()).validate(step["expected"])
    Draft202012Validator(schemas["Revision"], format_checker=FormatChecker()).validate(fixture["revision"])
    steps = fixture["steps"]
    payload, locator, revision_payload, ingest, memory, search, context = [s["expected"] for s in steps[1:]]
    assert ingest["event"]["payload_id"] == payload["id"]
    assert ingest["evidence"]["event_id"] == ingest["event"]["id"]
    assert ingest["evidence"]["source_locator"] == locator["id"]
    assert fixture["revision"]["evidence_ids"] == [ingest["evidence"]["id"]]
    assert fixture["revision"]["content_payload_id"] == revision_payload["id"]
    assert revision_payload["id"] != payload["id"]  # Independent erasure/key ownership.
    assert memory["current_revision_id"] == fixture["revision"]["id"]
    assert search["items"] == [memory]
    assert context["items"][0]["memory_id"] == memory["id"]
    assert context["items"][0]["revision_id"] == memory["current_revision_id"]
    assert context["items"][0]["evidence_ids"] == fixture["revision"]["evidence_ids"]
    assert context["items"][0]["text"] == steps[1]["arguments"]["text"]
    assert context["knowledge_token"] == search["index_watermark"] == steps[5]["receipt"]["commit_token"]
    assert context["items"][0]["trust"] == "untrusted_data"
    assert not {"memory.erase", "installation.admin"} & set(fixture["scopes"])
    assert fixture["negative_cases"][0]["space_id"] != fixture["space_id"]
    assert fixture["negative_cases"][1]["expected_code"] == "denied"
    assert fixture["negative_cases"][2]["tool"] not in tools


@pytest.mark.parametrize("case", ["no-evidence", "trusted-context", "bad-valid-time", "source-free"])
def test_context_and_source_negative_shapes(case: str) -> None:
    bundle = read("examples/synthetic-loop.json")["steps"][-1]["expected"]
    if case == "no-evidence":
        bundle["items"][0]["evidence_ids"] = []
    elif case == "trusted-context":
        bundle["items"][0]["trust"] = "system_instruction"
    elif case == "bad-valid-time":
        bundle["items"][0]["valid_from"] = "not-a-time"
    else:
        bundle["items"][0]["source_kinds"] = []
    assert not validator("context-bundle").is_valid(bundle)


@pytest.mark.parametrize("command", ["doctor", "plan", "init", "up", "status", "connect", "verify"])
def test_all_cli_commands_have_structured_output(command: str) -> None:
    validator("result").validate({
        "contract_version": "1.0", "request_id": "00000000-0000-4000-8000-000000000001",
        "command": command, "status": "ok", "exit_code": 0, "error": None,
        "result_ref": "synthetic-receipt", "diagnostic_ref": None,
    })


@pytest.mark.parametrize("case", ["shell-action", "token-in-audit", "token-in-envelope"])
def test_no_shell_or_secret_fields(case: str) -> None:
    if case == "shell-action":
        value = read("examples/plan.json")
        value["actions"][0]["shell_command"] = "synthetic-never-execute"
        assert not validator("plan").is_valid(value)
    elif case == "token-in-audit":
        value = read("examples/audit.json")
        value["token"] = "synthetic-never-store"
        assert not validator("audit-record").is_valid(value)
    else:
        value = {"contract_version": "1.0", "request_id": "00000000-0000-4000-8000-000000000001",
                 "command": "status", "status": "ok", "exit_code": 0, "error": None,
                 "result_ref": None, "diagnostic_ref": None, "token": "synthetic-never-print"}
        assert not validator("result").is_valid(value)


@pytest.mark.parametrize("case,expected", [
    ("same-target", set()), ("foreign-tenant", {"not_visible"}),
    ("foreign-space", {"not_visible"}), ("control-credential", {"unauthenticated"}),
    ("erase", {"denied"}), ("context-without-evidence", {"denied"}),
])
def test_memory_authorization_reference_is_independent(case: str, expected: set[str]) -> None:
    binding = read("examples/client-reader.json")
    args = {"tenant_id": binding["tenant_id"], "space_id": binding["space_ids"][0]}
    tool = catalog()["memories_search"]
    if case == "foreign-tenant":
        args["tenant_id"] = "00000000-0000-4000-8000-000000000099"
    elif case == "foreign-space":
        args["space_id"] = "00000000-0000-4000-8000-000000000099"
    elif case == "control-credential":
        binding["audience"] = "yixumemory-control"
    elif case == "erase":
        tool = catalog()["memories_erase"]
    elif case == "context-without-evidence":
        binding["scopes"].append("memory.context")
        tool = catalog()["memories_context"]
    assert memory_call_violations(args, binding, tool["required_capabilities"]) == expected


def test_action_policy_and_error_precedence_are_complete() -> None:
    policy = read("action-policy.json")["rules"]
    kinds = read("plan.schema.json")["properties"]["actions"]["items"]["properties"]["kind"]["enum"]
    assert set(policy) == set(kinds)
    entries = read("exit-codes.json")
    precedence = entries["error_precedence"]
    assert len(precedence) == len(set(precedence))
    assert set(precedence) == {e["exit_code"] for e in entries["entries"] if e["exit_code"]}
    assert precedence.index(9) < precedence.index(8)


def test_valid_remote_proxy_requires_https_and_preserves_loopback() -> None:
    config = read("profiles/server.json")
    config["network"].update(remote_enabled=True, proxy_endpoint="https://memory.example.invalid")
    validator("config").validate(config)
    assert config["network"]["application_bind"] == "127.0.0.1"


def test_backup_verification_does_not_require_a_previous_restore_drill() -> None:
    plan = read("examples/plan.json")
    plan["actions"][0].update(kind="verify_backup", required_scopes=["backup.verify"])
    approval = read("examples/approval.json")
    approval["scopes"] = ["backup.verify"]
    assert not plan_violations(plan, approval, observation(), NOW, read("action-policy.json")["rules"])
