"""Offline validation against pinned official definitions, plus an outgoing profile."""
from __future__ import annotations

import json
from functools import lru_cache
from pathlib import Path
from typing import Any, cast

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = Path(__file__).resolve().parents[1] / "contracts/deployment"
RESULTS = {
    "modern": {"server/discover": "DiscoverResult", "tools/list": "ListToolsResult",
               "tools/call": "CallToolResult", "ping": "EmptyResult"},
    "legacy": {"initialize": "InitializeResult", "tools/list": "ListToolsResult",
               "tools/call": "CallToolResult", "ping": "EmptyResult"},
}


def read_schema(name: str) -> dict[str, Any]:
    return cast(dict[str, Any], json.loads((ROOT / name).read_text("utf-8")))


@lru_cache
def official_validator(era: str, definition: str) -> Any:
    source = read_schema(f"upstream/mcp-{era}-results.schema.json")
    return Draft202012Validator({"$schema": source["$schema"], "$defs": source["$defs"],
                                "$ref": f"#/$defs/{definition}"})


@lru_cache
def response_validator() -> Any:
    registry: Registry[Any] = Registry()
    for era in RESULTS:
        source = read_schema(f"upstream/mcp-{era}-results.schema.json")
        registry = registry.with_resource(source["$id"], Resource.from_contents(source))
    return Draft202012Validator(read_schema("mcp-response.schema.json"), registry=registry)
