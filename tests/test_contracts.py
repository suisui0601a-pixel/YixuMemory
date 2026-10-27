"""Executable baseline checks. These do not implement or certify an engine."""
from __future__ import annotations

import copy
import json
from pathlib import Path
from typing import Any

import pytest
from jsonschema import Draft202012Validator, FormatChecker
from openapi_spec_validator import validate

from checks.security_scan import inspect

ROOT = Path(__file__).resolve().parents[1]
NAMES = ["memory", "event", "revision", "evidence"]
ID = "00000000-0000-4000-8000-000000000001"
NOW = "2026-10-28T00:00:00.000Z"


def schema(name: str) -> dict[str, Any]:
    return json.loads((ROOT / f"contracts/jsonschema/{name}.schema.json").read_text("utf-8"))  # type: ignore[no-any-return]


def sample(name: str) -> dict[str, Any]:
    base: dict[str, Any] = {"schema_version": "1.0", "id": ID, "tenant_id": ID, "space_id": ID}
    extra: dict[str, dict[str, Any]] = {
        "memory": dict(current_revision_id=ID, version=1, state="active", created_at=NOW),
        "event": dict(kind="raw_event", payload_id=ID, occurred_at=NOW,
                      recorded_at=NOW, knowledge_seq=1, actor_id=ID),
        "revision": dict(memory_id=ID, number=1, parent_revision_id=None,
                         assertion_kind="extracted_fact", content_payload_id=ID,
                         valid_from=NOW, valid_to=None, recorded_at=NOW, knowledge_seq=1, evidence_ids=[ID]),
        "evidence": dict(event_id=ID, source_kind="raw_event", source_locator=ID,
                         payload_id=ID, captured_at=NOW),
    }
    return base | extra[name]


def validator(name: str) -> Any:
    return Draft202012Validator(schema(name), format_checker=FormatChecker())


@pytest.mark.parametrize("name", NAMES)
def test_schema_and_positive_sample(name: str) -> None:
    Draft202012Validator.check_schema(schema(name))
    validator(name).validate(sample(name))


@pytest.mark.parametrize("name", NAMES)
def test_each_required_field_is_enforced(name: str) -> None:
    for field in schema(name)["required"]:
        value = sample(name)
        del value[field]
        assert not validator(name).is_valid(value), field


@pytest.mark.parametrize("name", NAMES)
@pytest.mark.parametrize("mutation", ["unknown", "bad_uuid", "bad_version"])
def test_strict_shape(name: str, mutation: str) -> None:
    value = sample(name)
    if mutation == "unknown":
        value["client_claimed_admin"] = True
    elif mutation == "bad_uuid":
        value["tenant_id"] = "other-tenant"
    else:
        value["schema_version"] = "future"
    assert not validator(name).is_valid(value)


@pytest.mark.parametrize("field,value", [
    ("evidence_ids", []), ("evidence_ids", [ID, ID]),
    ("assertion_kind", "observed_truth"), ("number", 0),
    ("number", 2), ("parent_revision_id", ID),
    ("recorded_at", "2026-10-28T00:00:00+08:00"),
    ("valid_from", "not-a-time"),
])
def test_revision_counterexamples(field: str, value: Any) -> None:
    payload = sample("revision")
    payload[field] = value
    assert not validator("revision").is_valid(payload)


def test_second_revision_shape() -> None:
    payload = sample("revision")
    payload.update(number=2, parent_revision_id=ID)
    validator("revision").validate(payload)


def test_schema_is_not_a_relational_or_auth_checker() -> None:
    payload = sample("revision")
    payload["tenant_id"] = "00000000-0000-4000-8000-000000000002"
    # Shape passes; service must separately reject foreign tenant evidence.
    validator("revision").validate(payload)


def test_openapi_valid_and_resource_shapes_do_not_drift() -> None:
    api = json.loads((ROOT / "contracts/openapi/openapi.json").read_text("utf-8"))
    validate(api)
    for name in NAMES:
        expected = copy.deepcopy(schema(name))
        for key in ("$schema", "$id"):
            expected.pop(key, None)
        assert api["components"]["schemas"][name.title()] == expected


def test_openapi_write_preconditions_and_security() -> None:
    api = json.loads((ROOT / "contracts/openapi/openapi.json").read_text("utf-8"))
    assert api["security"] == [{"bearerAuth": []}]
    for path, item in api["paths"].items():
        for method in ("post", "patch", "delete"):
            if method not in item or path.endswith("/search"):
                continue
            op = item[method]
            refs = [p.get("$ref", "") for p in op["parameters"]]
            assert "#/components/parameters/IdempotencyKey" in refs
            if method in ("patch", "delete"):
                assert "#/components/parameters/IfMatch" in refs
            assert {"409", "422", "401", "404"} <= set(op["responses"])
    ingestion = next(v for k, v in api["paths"].items() if k.endswith("/events"))
    assert ingestion["post"]["responses"]["201"]["content"]["application/json"]["schema"] == {
        "$ref": "#/components/schemas/IngestionResult"
    }


@pytest.mark.parametrize("path,content", [
    ("data/private.txt", "synthetic"), ("user.db", "synthetic"),
    (".env", "synthetic"), ("fixture.txt", "ghp_" + "X" * 36),
    ("fixture.txt", "-----BEGIN " + "PRIVATE KEY-----"),
])
def test_scanner_rejects_synthetic_leaks(path: str, content: str) -> None:
    assert inspect(path, content)


def test_scanner_allows_synthetic_contract() -> None:
    assert not inspect("contracts/example.json", json.dumps(sample("memory")))
