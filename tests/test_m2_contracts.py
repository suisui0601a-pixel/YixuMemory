"""Shape/API tests only; real relational/security assertions run against Rust SQLite."""

from __future__ import annotations
import copy
import json
from pathlib import Path
from typing import Any
import pytest
from jsonschema import Draft202012Validator, FormatChecker  # type: ignore[import-untyped]
from openapi_spec_validator import validate
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1] / "contracts/m2"
EXAMPLES = json.loads((ROOT / "examples.json").read_text(encoding="utf-8"))
SCHEMAS = {
    p.name.split(".")[0]: json.loads(p.read_text(encoding="utf-8"))
    for p in ROOT.glob("*.schema.json")
}
REGISTRY = Registry().with_resources(
    (s["$id"], Resource.from_contents(s)) for s in SCHEMAS.values()
)


def validator(name: str) -> Draft202012Validator:
    return Draft202012Validator(
        SCHEMAS[name], registry=REGISTRY, format_checker=FormatChecker()
    )


@pytest.mark.parametrize(
    "name", ["time", "query", "write", "conflict", "page", "conflict-history"]
)
def test_schema_is_valid(name: str) -> None:
    Draft202012Validator.check_schema(SCHEMAS[name])


@pytest.mark.parametrize("name", ["query", "write", "conflict", "page"])
def test_positive_examples(name: str) -> None:
    validator(name).validate(EXAMPLES[name])


@pytest.mark.parametrize("name", ["unknown_time", "future_time"])
def test_explicit_time_examples(name: str) -> None:
    validator("time").validate(EXAMPLES[name])


@pytest.mark.parametrize(
    "field",
    ["body", "terms", "locator", "admin", "decrypted", "classification", "seal_ok"],
)
def test_no_content_or_privilege_override_in_query(field: str) -> None:
    sample = copy.deepcopy(EXAMPLES["query"])
    sample[field] = "synthetic"
    assert list(validator("query").iter_errors(sample))


@pytest.mark.parametrize(
    "field",
    [
        "profile",
        "projection",
        "tenant_id",
        "space_id",
        "mode",
        "state",
        "retrieval",
        "limit",
    ],
)
def test_required_query_selectors(field: str) -> None:
    sample = copy.deepcopy(EXAMPLES["query"])
    del sample[field]
    assert list(validator("query").iter_errors(sample))


@pytest.mark.parametrize(
    "field,value",
    [
        ("limit", 0),
        ("limit", 101),
        ("limit", True),
        ("state", "active"),
        ("retrieval", "bm25"),
        ("mode", "history"),
    ],
)
def test_invalid_query_shape(field: str, value: Any) -> None:
    sample = copy.deepcopy(EXAMPLES["query"])
    sample[field] = value
    assert list(validator("query").iter_errors(sample))


def test_unknown_has_no_implicit_bounds() -> None:
    assert list(
        validator("time").iter_errors({"kind": "unknown", "from": None, "to": None})
    )


def test_no_client_creation_ids_or_truth_choice() -> None:
    sample = copy.deepcopy(EXAMPLES["write"])
    sample["revision_id"] = sample["content_payload_id"]
    assert list(validator("write").iter_errors(sample))
    sample = copy.deepcopy(EXAMPLES["conflict"])
    sample["winner"] = sample["left_revision_id"]
    assert list(validator("conflict").iter_errors(sample))


def test_openapi_draft_and_embedded_contracts_are_consistent() -> None:
    api = json.loads((ROOT / "openapi.json").read_text(encoding="utf-8"))
    validate(api)
    assert "No network server" in api["info"]["description"]
    assert (
        api["components"]["schemas"]["Query"]["properties"]
        == SCHEMAS["query"]["properties"]
    )
    assert api["security"] == [{"ServerIdentity": []}]
    assert len(api["paths"]) == 4


@pytest.mark.parametrize(
    "field,value",
    [
        ("content_available", True),
        ("body", "synthetic-only"),
        ("key_ref", "fake"),
        ("state", "active"),
    ],
)
def test_response_cannot_advertise_or_expose_content(field: str, value: Any) -> None:
    sample = copy.deepcopy(EXAMPLES["page"])
    sample["rows"][0][field] = value
    assert list(validator("page").iter_errors(sample))
