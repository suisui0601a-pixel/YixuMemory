"""R1 normalized-graph type regressions only; no real FK or erasure executes."""
from __future__ import annotations

import copy
import json
from pathlib import Path
from typing import Any, cast

import pytest

from checks.security_closure import (
    GRAPH_FIELDS, NODE_KINDS, PAYLOAD_OWNER_KINDS, RELATION_TYPES, deletion_plan,
)

ROOT = Path(__file__).resolve().parents[1]


def graph_sample() -> dict[str, Any]:
    data = json.loads((ROOT / "contracts/security/examples.json").read_text("utf-8"))
    return cast(dict[str, Any], data["shared_graph"])


def source_intent(graph: dict[str, Any]) -> dict[str, Any]:
    return {"operation": "source_graph", "requested_scope": "space", "target_type": "event",
            "target_id": "event", "tenant_id": graph["tenant_id"], "space_id": graph["space_id"]}


@pytest.mark.parametrize("shared_payload", [True, False], ids=["exact-fixture", "independent-payloads"])
def test_evidence_event_wrong_kind_rejected_before_fencing(shared_payload: bool) -> None:
    graph = graph_sample()
    if not shared_payload:
        graph["payload_owners"]["source"] = ["event"]
        graph["payload_owners"]["evidence-body"] = ["e"]
    graph["evidence_events"]["e"] = "ma"  # Exists, but is a Memory, not an Event.
    original = copy.deepcopy(graph)
    result = deletion_plan(source_intent(graph), graph, set(graph["nodes"]))
    assert result["decision"] != "admit_graph"
    assert result["fenced"] == set() and result["erase_payloads"] == set()
    assert graph == original


def rejected_without_changes(graph: dict[str, Any], *, operation: str = "source_graph") -> None:
    original = copy.deepcopy(graph)
    request = source_intent(graph)
    if operation == "memory_graph":
        request.update(operation=operation, target_type="memory", target_id="ma")
    result = deletion_plan(request, graph, set(graph["nodes"]),
                           expected_version=1 if operation == "memory_graph" else None)
    assert result["decision"] != "admit_graph"
    assert result == {"decision": result["decision"], "fenced": set(), "erase_payloads": set()}
    assert graph == original


# Every wrong known kind is exercised, rather than only a specific ID spelling.
IDS = {"event": "event", "evidence": "e", "revision": "rb", "memory": "mb"}
EXPECTED_RELATION_TYPES = {
    "evidence_events": ({"evidence"}, {"event"}),
    "revision_evidence": ({"revision"}, {"evidence"}),
    "revision_memory": ({"revision"}, {"memory"}),
    "derived_from": ({"event", "evidence", "revision"}, {"event", "evidence", "revision"}),
}
WRONG_ENDPOINTS = [
    (relation, side, kind)
    for relation, (left, right) in EXPECTED_RELATION_TYPES.items()
    for side, allowed in [("dependent", left), ("source", right)]
    for kind in sorted(NODE_KINDS - allowed)
]


@pytest.mark.parametrize("relation,side,kind", WRONG_ENDPOINTS)
@pytest.mark.parametrize("operation", ["source_graph", "memory_graph"])
def test_every_relation_rejects_wrong_endpoint_kind(relation: str, side: str, kind: str,
                                                    operation: str) -> None:
    graph = graph_sample()
    if relation == "derived_from":
        graph[relation] = {"rb": ["ra"]}
    key = "e" if relation == "evidence_events" else "rb"
    if side == "dependent":
        value = graph[relation].pop(key)
        graph[relation][IDS[kind]] = value
    else:
        graph[relation][key] = IDS[kind] if relation in {"evidence_events", "revision_memory"} else [IDS[kind]]
    rejected_without_changes(graph, operation=operation)


@pytest.mark.parametrize("relation", ["evidence_events", "revision_evidence", "revision_memory",
                                       "derived_from", "payload_owners", "memory_versions"])
def test_dangling_relationships_fail_closed(relation: str) -> None:
    graph = graph_sample()
    if relation == "evidence_events":
        graph[relation]["e"] = "absent-event"
    elif relation in {"revision_evidence", "derived_from"}:
        graph[relation]["rb"] = ["absent-source"]
    elif relation == "revision_memory":
        graph[relation]["rb"] = "absent-memory"
    elif relation == "payload_owners":
        graph[relation]["source"] = ["absent-owner"]
    else:
        graph[relation]["absent-memory"] = 1
    rejected_without_changes(graph)


@pytest.mark.parametrize("kind", ["event", "evidence", "revision"])
def test_versions_must_belong_to_memories_even_for_source_deletion(kind: str) -> None:
    graph = graph_sample()
    graph["memory_versions"][IDS[kind]] = graph["memory_versions"].pop("mb")
    rejected_without_changes(graph)


@pytest.mark.parametrize("version", [0, -1, True, False, "1", 1.0, None])
def test_non_target_memory_version_is_a_positive_integer(version: Any) -> None:
    graph = graph_sample()
    graph["memory_versions"]["mb"] = version
    rejected_without_changes(graph)


@pytest.mark.parametrize("case", ["unknown-relation", "unknown-node-kind", "wrong-payload-owner",
                                  "duplicate-owner", "owner-string", "evidence-list-string",
                                  "empty-evidence-list", "missing-evidence-row", "missing-revision-row",
                                  "missing-memory-version", "missing-table", "malformed-table"])
def test_projection_rejects_unknown_inconsistent_or_malformed_edges(case: str) -> None:
    graph = graph_sample()
    if case == "unknown-relation":
        graph["revision_events"] = {"rb": ["event"]}
    elif case == "unknown-node-kind":
        graph["nodes"]["unused-grant"] = "grant"
    elif case == "wrong-payload-owner":
        graph["nodes"]["not-an-owner"] = "payload"
        graph["payload_owners"]["source"] = ["not-an-owner"]
    elif case == "duplicate-owner":
        graph["payload_owners"]["source"] = ["event", "event"]
    elif case == "owner-string":
        graph["payload_owners"]["source"] = "event"
    elif case == "evidence-list-string":
        graph["revision_evidence"]["rb"] = "e"
    elif case == "empty-evidence-list":
        graph["revision_evidence"]["rb"] = []
    elif case == "missing-evidence-row":
        del graph["evidence_events"]["e"]
    elif case == "missing-revision-row":
        del graph["revision_memory"]["rb"]
    elif case == "missing-memory-version":
        del graph["memory_versions"]["mb"]
    elif case == "missing-table":
        del graph["derived_from"]
    else:
        graph["derived_from"] = []
    rejected_without_changes(graph)


def disconnected_component() -> dict[str, Any]:
    graph = graph_sample()
    graph["nodes"].update(event2="event", e2="evidence")
    graph["evidence_events"]["e2"] = "event2"
    graph["revision_evidence"]["rb"] = ["e2"]
    graph["payload_owners"]["source2"] = ["event2", "e2"]
    return graph


def test_invalid_edge_outside_the_current_closure_still_rejects_entire_graph() -> None:
    graph = disconnected_component()
    valid = deletion_plan(source_intent(graph), graph, set(graph["nodes"]))
    assert valid["fenced"] == {"event", "e", "ra", "ma"}
    graph["evidence_events"]["e2"] = "mb"  # Not traversed by this deletion.
    rejected_without_changes(graph)


@pytest.mark.parametrize("source", ["event", "e", "ra"])
def test_each_declared_derived_source_kind_preserves_complete_closure(source: str) -> None:
    graph = disconnected_component()
    graph["derived_from"] = {"rb": [source]}
    original = copy.deepcopy(graph)
    result = deletion_plan(source_intent(graph), graph, set(graph["nodes"]))
    assert result["decision"] == "admit_graph"
    assert result["invalid_revisions"] == {"ra", "rb"}
    assert result["affected_memories"] == {"ma", "mb"}
    assert result["erase_payloads"] == {"source", "pa", "pb"}
    assert result["retained_payloads"] == {"source2"}
    assert graph == original


@pytest.mark.parametrize("owner", ["event", "e", "ra", "ma"])
def test_all_declared_payload_owner_types_remain_legal(owner: str) -> None:
    graph = graph_sample()
    graph["payload_owners"]["additional-copy"] = [owner]
    result = deletion_plan(source_intent(graph), graph, set(graph["nodes"]))
    assert result["decision"] == "admit_graph"
    assert result["invalid_revisions"] == {"ra", "rb"}


def test_relation_contract_and_oracle_do_not_drift() -> None:
    matrix = json.loads((ROOT / "contracts/security/graph-relations.json").read_text("utf-8"))
    assert set(matrix["node_kinds"]) == NODE_KINDS
    assert set(matrix["graph_fields"]) == GRAPH_FIELDS
    assert set(matrix["payload_owner_kinds"]) == PAYLOAD_OWNER_KINDS
    assert matrix["runtime_verified"] is False
    assert set(matrix["relations"]) == set(RELATION_TYPES) == set(EXPECTED_RELATION_TYPES)
    assert {k: (set(v[0]), set(v[1])) for k, v in RELATION_TYPES.items()} == EXPECTED_RELATION_TYPES
    for relation, (dependent, source) in RELATION_TYPES.items():
        assert set(matrix["relations"][relation]["dependent_kinds"]) == dependent
        assert set(matrix["relations"][relation]["source_kinds"]) == source


@pytest.mark.parametrize("dependent", ["event2", "e2"])
@pytest.mark.parametrize("source", ["event", "e", "ra"])
def test_captured_or_model_output_content_keeps_legal_derivation_types(dependent: str, source: str) -> None:
    graph = disconnected_component()
    graph["payload_owners"]["source2"] = ["event2"]
    graph["payload_owners"]["evidence2-body"] = ["e2"]
    graph["derived_from"] = {dependent: [source]}
    original = copy.deepcopy(graph)
    result = deletion_plan(source_intent(graph), graph, set(graph["nodes"]))
    assert result["decision"] == "admit_graph"
    assert result["invalid_revisions"] == {"ra", "rb"}
    assert result["affected_memories"] == {"ma", "mb"}
    erased = {"source", "pa", "pb", "evidence2-body"}
    if dependent == "event2":
        erased.add("source2")
    assert result["erase_payloads"] == erased
    assert result["retained_payloads"] == ({"source2"} if dependent == "e2" else set())
    assert graph == original


def test_memory_only_follows_explicit_derived_event_capture_without_erasing_shared_source() -> None:
    graph = graph_sample()
    graph["nodes"].update(event3="event", e3="evidence", r3="revision", mc="memory")
    graph["evidence_events"]["e3"] = "event3"
    graph["revision_evidence"]["r3"] = ["e3"]
    graph["revision_memory"]["r3"] = "mc"
    graph["memory_versions"]["mc"] = 1
    graph["payload_owners"].update(source3=["event3", "e3"], pc=["r3"])
    graph["derived_from"] = {"event3": ["ra"]}
    original = copy.deepcopy(graph)
    request = source_intent(graph) | {"operation": "memory_graph", "target_type": "memory", "target_id": "ma"}
    result = deletion_plan(request, graph, set(graph["nodes"]), expected_version=1)
    assert result["decision"] == "admit_graph"
    assert result["invalid_revisions"] == {"ra", "r3"}
    assert result["fenced"] == {"ma", "ra", "event3", "e3", "r3", "mc"}
    assert result["erase_payloads"] == {"pa", "source3", "pc"}
    assert result["retained_payloads"] == {"source", "pb"}
    assert graph == original
