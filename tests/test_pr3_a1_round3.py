"""Official response schemas and conservative fallback; no transport is executed."""
from __future__ import annotations

import copy
import hashlib
import json
from typing import Any

import pytest
from jsonschema import Draft202012Validator, ValidationError

from checks.mcp_responses import RESULTS, official_validator, read_schema, response_validator
from checks.mcp_wire_contracts import MODERN, fallback_decision, parse_probe_response

EXAMPLES = read_schema("mcp-wire-examples.json")["results"]
CACHEABLE = [e for e in EXAMPLES if e["era"] == "modern"
             and e["method"] in {"server/discover", "tools/list"}]


@pytest.mark.parametrize("entry", CACHEABLE, ids=lambda e: e["method"])
@pytest.mark.parametrize("field", ["resultType", "ttlMs", "cacheScope", "method-required"])
def test_missing_required_fails_official_and_profile(entry: dict[str, Any], field: str) -> None:
    broken = copy.deepcopy(entry)
    if field == "method-required":
        field = "capabilities" if entry["method"] == "server/discover" else "tools"
    del broken["result"][field]
    official = official_validator("modern", RESULTS["modern"][entry["method"]])
    with pytest.raises(ValidationError):
        official.validate(broken["result"])
    with pytest.raises(ValidationError):
        response_validator().validate(broken)
    official.validate(entry["result"])
    response_validator().validate(entry)


@pytest.mark.parametrize("entry", CACHEABLE, ids=lambda e: e["method"])
@pytest.mark.parametrize("field,value", [
    ("ttlMs", -1), ("ttlMs", "0"), ("ttlMs", True), ("ttlMs", 0.5),
    ("cacheScope", "shared"), ("cacheScope", None), ("cacheScope", 1),
    ("resultType", 0), ("resultType", None),
])
def test_official_rejects_invalid_cache_fields(entry: dict[str, Any], field: str, value: Any) -> None:
    broken = copy.deepcopy(entry)
    broken["result"][field] = value
    assert not official_validator("modern", RESULTS["modern"][entry["method"]]).is_valid(broken["result"])
    assert not response_validator().is_valid(broken)


@pytest.mark.parametrize("field,value", [("supportedVersions", [1]), ("supportedVersions", None),
                                         ("capabilities", []), ("tools", {})])
def test_official_method_specific_types(field: str, value: Any) -> None:
    entry = copy.deepcopy(CACHEABLE[1 if field == "tools" else 0])
    entry["result"][field] = value
    assert not official_validator("modern", RESULTS["modern"][entry["method"]]).is_valid(entry["result"])
    assert not response_validator().is_valid(entry)


@pytest.mark.parametrize("entry", CACHEABLE, ids=lambda e: e["method"])
@pytest.mark.parametrize("field,value", [("cacheScope", "public"), ("ttlMs", 100),
                                         ("resultType", "input_required")])
def test_product_profile_is_stricter_than_official(entry: dict[str, Any], field: str, value: Any) -> None:
    entry = copy.deepcopy(entry)
    entry["result"][field] = value
    # Official allows these values; our outgoing complete/zero/private profile does not.
    official_validator("modern", RESULTS["modern"][entry["method"]]).validate(entry["result"])
    assert not response_validator().is_valid(entry)


@pytest.mark.parametrize("entry", [e for e in EXAMPLES if e["era"] == "legacy"],
                         ids=lambda e: e["method"])
@pytest.mark.parametrize("field,value", [("resultType", "complete"), ("ttlMs", 0),
                                         ("cacheScope", "private")])
def test_legacy_profile_does_not_mix_modern_fields(entry: dict[str, Any], field: str, value: Any) -> None:
    response_validator().validate(entry)
    broken = copy.deepcopy(entry)
    broken["result"][field] = value
    assert not response_validator().is_valid(broken)


@pytest.mark.parametrize("method", ["tools/call", "ping"])
def test_other_declared_modern_results_are_not_cacheable(method: str) -> None:
    entry = next(e for e in EXAMPLES if e["era"] == "modern" and e["method"] == method)
    assert not {"ttlMs", "cacheScope"} & entry["result"].keys()
    response_validator().validate(entry)
    broken = copy.deepcopy(entry)
    del broken["result"]["resultType"]
    assert not response_validator().is_valid(broken)


@pytest.mark.parametrize("era", ["modern", "legacy"])
def test_pinned_official_definitions_and_reference_closure(era: str) -> None:
    source = read_schema(f"upstream/mcp-{era}-results.schema.json")
    Draft202012Validator.check_schema(source)
    defs = source["$defs"]
    provenance = read_schema("upstream/provenance.json")["sources"][era]
    canonical = json.dumps(defs, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    assert hashlib.sha256(canonical).hexdigest() == provenance["definitions_sha256"]
    assert len(defs) == provenance["definition_count"]

    def walk(value: Any) -> None:
        if isinstance(value, dict):
            if "$ref" in value:
                assert value["$ref"].startswith("#/$defs/")
                assert value["$ref"].split("/")[-1] in defs
            for child in value.values():
                walk(child)
        elif isinstance(value, list):
            for child in value:
                walk(child)
    walk(defs)
    if era == "modern":
        for name in ["DiscoverResult", "ListToolsResult"]:
            assert {"ttlMs", "cacheScope", "resultType"} <= set(defs[name]["required"])
            assert defs[name]["properties"]["ttlMs"]["minimum"] == 0


def test_catalog_cache_policy_and_http_auth_isolation() -> None:
    policy = read_schema("mcp-cache-policy.json")
    assert set(policy["modern_cacheable_methods"]) == {"server/discover", "tools/list"}
    assert set(policy["modern_noncacheable_methods"]) == {"tools/call", "ping"}
    assert policy["outgoing_ttl_ms"] == 0 and policy["outgoing_cache_scope"] == "private"
    assert policy["http_headers"]["Cache-Control"] == "private, no-store"
    assert policy["http_headers"]["Vary"] == "Authorization"
    assert policy["cache_reuse_across_auth_contexts"] is False
    assert policy["runtime_verified"] is False


def probe_body(case: dict[str, Any]) -> str | None:
    kind = case["body_kind"]
    if kind == "empty":
        return None
    if kind == "malformed":
        return "{"
    if kind == "array":
        return "[]"
    if kind == "duplicate-id":
        return '{"jsonrpc":"2.0","id":"probe-1","id":"probe-1","error":{"code":-32601,"message":"x"}}'
    if kind == "nan":
        return '{"jsonrpc":"2.0","id":"probe-1","error":{"code":NaN,"message":"x"}}'
    value: dict[str, Any] = {"jsonrpc": "2.0", "id": "probe-1"}
    if kind in {"modern-result", "modern-result-public", "missing-ttl", "missing-scope",
                "missing-result-type", "negative-ttl", "bad-scope"}:
        value["result"] = copy.deepcopy(CACHEABLE[0]["result"])
        for label, field in [("missing-ttl", "ttlMs"), ("missing-scope", "cacheScope"),
                             ("missing-result-type", "resultType")]:
            if kind == label:
                del value["result"][field]
        if kind == "negative-ttl":
            value["result"]["ttlMs"] = -1
        if kind == "bad-scope":
            value["result"]["cacheScope"] = "shared"
        if kind == "modern-result-public":
            value["result"].update(ttlMs=100, cacheScope="public")
    elif kind == "legacy-result":
        value["result"] = {"protocolVersion": "2025-11-25", "capabilities": {},
                           "serverInfo": {"name": "synthetic", "version": "0"}}
    else:
        value["error"] = {"code": case["code"] if case["code"] is not None else -32601,
                          "message": "synthetic error"}
        if case["code"] == -32021:
            value["error"]["data"] = {"requiredCapabilities": {"tools": {}}}
        if case["code"] == -32022:
            value["error"]["data"] = {"supported": [MODERN], "requested": "future"}
        if kind == "missing-error-data":
            del value["error"]["data"]
        if kind == "wrong-id":
            value["id"] = "different"
        if kind == "null-id":
            value["id"] = None
        if kind == "boolean-id":
            value["id"] = True
        if kind == "both":
            value["result"] = {}
        if kind == "bad-error":
            value["error"] = []
        if kind == "missing-error-message":
            value["error"].pop("message")
        if kind == "non-integer-code":
            value["error"]["code"] = "-32601"
    return json.dumps(value)


@pytest.mark.parametrize("case", read_schema("mcp-fallback-cases.json")["cases"], ids=lambda c: c["name"])
def test_structured_fallback_cases(case: dict[str, Any]) -> None:
    parsed = parse_probe_response(case["transport"], case["status"], probe_body(case),
                                  content_type=case.get("content_type", "application/json"),
                                  transport_error=case.get("transport_error"))
    result = fallback_decision(case["transport"], "dual-era", case["cached"], case["status"],
                               parsed.error_code, "non-modern-error", parsed=parsed,
                               phase=case["phase"], probe_method=case["method"])
    assert result == case["expected"]
    # Every negative vector is replayed against both protected cached eras too.
    for cached in ["modern", "legacy"]:
        decision = fallback_decision(case["transport"], "dual-era", cached, case["status"],
                                     parsed.error_code, "non-modern-error", parsed=parsed,
                                     phase=case["phase"], probe_method=case["method"])
        assert decision != "fresh-legacy-handshake"


def test_reported_internal_error_and_caller_labels_are_not_evidence() -> None:
    assert fallback_decision("stdio", "dual-era", None, 200, -32603,
                             "non-modern-error") == "fail-no-downgrade"
    assert fallback_decision("stdio", "dual-era", None, 200, None,
                             "discover-result") == "fail-no-downgrade"
    parsed = parse_probe_response("stdio", 200, json.dumps({"jsonrpc": "2.0", "id": "probe-1",
                                  "error": {"code": -32601, "message": "synthetic"}}))
    for transport, status, code in [("http", 200, -32601), ("stdio", 400, -32601),
                                    ("stdio", 200, -32602)]:
        assert fallback_decision(transport, "dual-era", "legacy", status, code, "non-modern-error",
                                 parsed=parsed, phase="initial-probe",
                                 probe_method="server/discover") == "fail-no-downgrade"


@pytest.mark.parametrize("entry", EXAMPLES, ids=lambda e: e["era"] + ":" + e["method"])
def test_response_era_cannot_be_relabelled(entry: dict[str, Any]) -> None:
    broken = copy.deepcopy(entry)
    broken["era"] = "legacy" if entry["era"] == "modern" else "modern"
    assert not response_validator().is_valid(broken)


@pytest.mark.parametrize("transport,status", [("stdio", 200), ("http", 400)])
@pytest.mark.parametrize("code,kind", [(-32603, "ordinary-failure"), (-32700, "ordinary-failure"),
                                      (-32600, "ordinary-failure"), (-32601, "ambiguous-rejection"),
                                      (-32602, "ambiguous-rejection")])
def test_error_classification_is_not_a_caller_label(transport: str, status: int, code: int, kind: str) -> None:
    body = json.dumps({"jsonrpc": "2.0", "id": "probe-1", "error": {"code": code, "message": "synthetic"}})
    parsed = parse_probe_response(transport, status, body)
    assert parsed.kind == kind and parsed.error_code == code
    for label in ["non-modern-error", "discover-result", "timeout", "empty"]:
        assert fallback_decision(transport, "dual-era", None, status, code, label,
                                 parsed=parsed, phase="initial-probe",
                                 probe_method="server/discover") == "fail-no-downgrade"


@pytest.mark.parametrize("phase,method", [("unspecified", None), ("business", "server/discover"),
                                         ("initial-probe", "ping"), ("initial-probe", "tools/call")])
def test_fallback_requires_explicit_initial_discover(phase: str, method: str | None) -> None:
    body = json.dumps({"jsonrpc": "2.0", "id": "probe-1", "error": {"code": -32601, "message": "synthetic"}})
    parsed = parse_probe_response("stdio", 200, body)
    assert fallback_decision("stdio", "dual-era", "legacy", 200, -32601, "non-modern-error",
                             parsed=parsed, phase=phase, probe_method=method) == "fail-no-downgrade"


def test_explicit_legacy_is_a_configuration_choice_not_inferred_fallback() -> None:
    body = json.dumps({"jsonrpc": "2.0", "id": "probe-1", "error": {"code": -32601, "message": "synthetic"}})
    parsed = parse_probe_response("stdio", 200, body)
    assert fallback_decision("stdio", "legacy-only", None, 200, -32601, "non-modern-error",
                             parsed=parsed, phase="initial-probe",
                             probe_method="server/discover") == "legacy-explicit"
    assert fallback_decision("stdio", "legacy-only", "modern", 200, -32601, "non-modern-error",
                             parsed=parsed, phase="initial-probe",
                             probe_method="server/discover") == "fail-no-downgrade"


@pytest.mark.parametrize("entry", [e for e in EXAMPLES if e["era"] == "modern"],
                         ids=lambda e: e["method"])
@pytest.mark.parametrize("field,value", [("protocolVersion", "2025-11-25"),
                                         ("serverInfo", {"name": "legacy", "version": "0"})])
def test_modern_profile_rejects_legacy_handshake_fields(entry: dict[str, Any], field: str, value: Any) -> None:
    broken = copy.deepcopy(entry)
    broken["result"][field] = value
    assert not response_validator().is_valid(broken)


def test_discover_requires_supported_versions() -> None:
    entry = copy.deepcopy(CACHEABLE[0])
    del entry["result"]["supportedVersions"]
    assert not official_validator("modern", "DiscoverResult").is_valid(entry["result"])
    assert not response_validator().is_valid(entry)


@pytest.mark.parametrize("method", ["tools/call", "ping"])
@pytest.mark.parametrize("field,value", [("ttlMs", 0), ("cacheScope", "private")])
def test_noncacheable_profile_rejects_cache_decoration(method: str, field: str, value: Any) -> None:
    entry = copy.deepcopy(next(e for e in EXAMPLES if e["era"] == "modern" and e["method"] == method))
    entry["result"][field] = value
    assert not response_validator().is_valid(entry)


def test_machine_contract_references_match() -> None:
    policy = read_schema("mcp-cache-policy.json")
    catalog = read_schema("tools.json")
    matrix = read_schema("mcp-protocols.json")
    ext = read_schema("agent-extension.openapi.json")["x-mcp-wire-contract"]
    assert set(catalog["protocol_methods"]["modern"]) == set(RESULTS["modern"])
    assert set(policy["modern_cacheable_methods"]) | set(policy["modern_noncacheable_methods"]) == set(RESULTS["modern"])
    for obj in [catalog, matrix, ext]:
        assert obj["response_schema"] == "mcp-response.schema.json"
        assert obj["cache_policy"] == "mcp-cache-policy.json"
    assert matrix["fallback_policy"]["automatic_legacy_downgrade"] is False
    assert matrix["fallback_policy"]["strategy"] == ext["fallback_strategy"] == "fail-closed-explicit-legacy"
    for entry in CACHEABLE:
        assert entry["result"]["ttlMs"] == policy["outgoing_ttl_ms"]
        assert entry["result"]["cacheScope"] == policy["outgoing_cache_scope"]
