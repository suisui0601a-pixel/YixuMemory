"""P0-SC pure review models. No database, crypto, storage, auth or deletion executes.

Graph and observations model future trusted server results, never client proofs.
Passing these predicates is not runtime security evidence.
"""
from __future__ import annotations

from typing import Any, Mapping, Sequence

OPERATIONS = {"memory_graph": "memory", "evidence_graph": "evidence", "source_graph": "event"}
STORES = {"payload", "key_backend", "index", "cache", "export", "staging", "backup", "jobs", "metadata_mappings", "temporary"}
SURFACES = {"search", "hydrate", "history", "cache", "index", "export", "context", "count"}

NODE_KINDS = frozenset({"event", "evidence", "revision", "memory"})
# Content derivation includes captured/model output content, not mutable aggregates.
# Source capture and aggregate membership retain their separate typed tables.
RELATION_TYPES = {
    "evidence_events": (frozenset({"evidence"}), frozenset({"event"})),
    "revision_evidence": (frozenset({"revision"}), frozenset({"evidence"})),
    "revision_memory": (frozenset({"revision"}), frozenset({"memory"})),
    "derived_from": (frozenset({"event", "evidence", "revision"}),
                     frozenset({"event", "evidence", "revision"})),
}
PAYLOAD_OWNER_KINDS = frozenset({"event", "evidence", "revision", "memory"})
GRAPH_FIELDS = frozenset({"tenant_id", "space_id", "nodes", "snapshot_complete",
                          "evidence_events", "revision_evidence", "revision_memory",
                          "derived_from", "payload_owners", "memory_versions"})


def _graph_violation(graph: Mapping[str, Any]) -> str | None:
    """Validate the entire projection before traversal; never repair/drop an edge."""
    if set(graph) - GRAPH_FIELDS:
        return "invalid_graph"
    if GRAPH_FIELDS - set(graph) or graph["snapshot_complete"] is not True:
        return "incomplete_graph"
    if any(not isinstance(graph[k], str) or not graph[k] for k in ("tenant_id", "space_id")):
        return "invalid_graph"
    nodes = graph["nodes"]
    if (not isinstance(nodes, Mapping)
            or any(not isinstance(k, str) or not k or not isinstance(v, str) or v not in NODE_KINDS
                   for k, v in nodes.items())):
        return "invalid_graph"

    def endpoint(ref: Any, kinds: frozenset[str]) -> str | None:
        if not isinstance(ref, str) or not ref:
            return "invalid_graph"
        if ref not in nodes:
            return "incomplete_graph"
        return None if nodes[ref] in kinds else "invalid_graph"

    for relation, (dependent_kinds, source_kinds) in RELATION_TYPES.items():
        table = graph[relation]
        if not isinstance(table, Mapping):
            return "invalid_graph"
        for dependent, sources in table.items():
            error = endpoint(dependent, dependent_kinds)
            if error:
                return error
            if relation in {"evidence_events", "revision_memory"}:
                sources = [sources]
            elif not isinstance(sources, list):
                return "invalid_graph"
            if not sources and relation == "revision_evidence":
                return "incomplete_graph"
            for source in sources:
                error = endpoint(source, source_kinds)
                if error:
                    return error
            if len(set(sources)) != len(sources):
                return "invalid_graph"

    owners, versions = graph["payload_owners"], graph["memory_versions"]
    if not isinstance(owners, Mapping) or not isinstance(versions, Mapping):
        return "invalid_graph"
    for payload, refs in owners.items():
        if not isinstance(payload, str) or not payload or not isinstance(refs, list):
            return "invalid_graph"
        if not refs:
            return "incomplete_graph"
        for owner in refs:
            error = endpoint(owner, PAYLOAD_OWNER_KINDS)
            if error:
                return error
        if len(set(refs)) != len(refs):
            return "invalid_graph"
    for memory, version in versions.items():
        error = endpoint(memory, frozenset({"memory"}))
        if error:
            return error
        if type(version) is not int or version < 1:
            return "invalid_graph"
    # A declared complete projection cannot omit mandatory FK/version rows.
    for table, kind in (("evidence_events", "evidence"), ("revision_evidence", "revision"),
                        ("revision_memory", "revision"), ("memory_versions", "memory")):
        if set(graph[table]) != {ref for ref, value in nodes.items() if value == kind}:
            return "incomplete_graph"
    return None


def deletion_plan(intent: Mapping[str, Any], graph: Mapping[str, Any],
                  authorized_ids: set[str], *, expected_version: int | None = None) -> dict[str, Any]:
    """Bounded explicit graph only; no semantic duplicate discovery or side effects."""
    def rejected(reason: str) -> dict[str, Any]:
        return {"decision": reason, "fenced": set(), "erase_payloads": set()}

    operation = intent["operation"]
    if intent["requested_scope"] != "space" or operation not in OPERATIONS:
        return rejected("unsupported_erasure_scope")
    if any(intent[k] != graph.get(k) for k in ("tenant_id", "space_id")):
        return rejected("scope_mismatch")
    target = intent["target_id"]
    if target not in authorized_ids:
        return rejected("denied")
    error = _graph_violation(graph)
    if error:
        return rejected(error)
    nodes = graph["nodes"]
    if intent["target_type"] != OPERATIONS[operation] or nodes.get(target) != intent["target_type"]:
        return rejected("invalid_target")
    evidence, revisions, memories = (graph["evidence_events"], graph["revision_evidence"],
                                     graph["revision_memory"])
    if operation == "memory_graph":
        if expected_version is None:
            return rejected("precondition_required")
        if type(expected_version) is not int or expected_version != graph.get("memory_versions", {}).get(target):
            return rejected("state_conflict")
    erased = {target}
    if operation == "memory_graph":
        erased |= {r for r, m in memories.items() if m == target}
    for _ in range(len(nodes) + 1):
        previous = set(erased)
        erased |= {e for e, event in evidence.items() if event in erased}
        if operation != "memory_graph":
            for owners in graph["payload_owners"].values():
                if set(owners) & erased:
                    erased.update(owners)
        erased |= {r for r, sources in revisions.items() if set(sources) & erased}
        erased |= {r for r, sources in graph["derived_from"].items() if set(sources) & erased}
        if erased == previous:
            break
    if operation == "memory_graph" and any(
            set(owners) & erased and not set(owners) <= erased
            for owners in graph["payload_owners"].values()):
        return rejected("ambiguous_payload_ownership")
    invalid_revisions = erased & revisions.keys()
    affected_memories = {memories[r] for r in invalid_revisions}
    fenced = erased | affected_memories
    if not fenced <= authorized_ids:
        return rejected("denied")
    return {"decision": "admit_graph", "fenced": fenced, "invalid_revisions": invalid_revisions,
            "affected_memories": affected_memories,
            "erase_payloads": {p for p, owners in graph["payload_owners"].items()
                               if owners and set(owners) <= erased},
            "retained_payloads": {p for p, owners in graph["payload_owners"].items()
                                  if not set(owners) <= erased}}


def deletion_completion(observation: Mapping[str, Any]) -> str:
    """Observation comes from the simulated coordinator, not a status request body."""
    if (observation.get("effective_scope") != "space"
            or observation.get("requested_scope") != "space"
            or observation.get("operation") not in OPERATIONS
            or observation.get("closure_complete") is not True
            or observation.get("fence_active") is not True
            or observation.get("writers_drained") is not True
            or observation.get("generation") != observation.get("verified_generation")
            or type(observation.get("generation")) is not int
            or type(observation.get("verified_generation")) is not int
            or observation["generation"] < 1
            or set(observation.get("receipts", {})) != STORES
            or not all(v is True for v in observation.get("receipts", {}).values())):
        return "blocked"
    return "completed"


def read_allowed(revision: str, memory: str, source_ids: set[str], fences: set[str],
                 surface: str) -> bool:
    """Projection of the mandatory current fence gate, independent of as_of/cache age."""
    return surface in SURFACES and not ({revision, memory} | source_ids) & fences


def replacement_allowed(old_id: str, new: Mapping[str, Any], invalid_ids: set[str],
                        observation: Mapping[str, Any]) -> bool:
    return (new["id"] != old_id and new["parent_revision_id"] == old_id
            and new["number"] == observation["previous_number"] + 1
            and bool(new["evidence_ids"]) and not set(new["evidence_ids"]) & invalid_ids
            and not set(new["derived_from"]) & invalid_ids
            and new["content_payload_id"] not in invalid_ids
            and all(observation.get(k) is True for k in
                    ("all_deletions_completed", "authorized", "cas_matches", "fresh_evidence_valid")))


def effective_classification(space_floor: str | None, declared: str | None,
                             lineage: Sequence[str | None], previous: str | None = None) -> str:
    """synthetic is not a trusted security class. Unknown input raises protection."""
    labels = [space_floor, declared, *lineage]
    if previous is not None:
        labels.append(previous)
    if any(value not in {"personal", "sensitive"} for value in labels):
        return "sensitive"
    return "sensitive" if "sensitive" in labels else "personal"


def production_write_violations(observation: Mapping[str, Any]) -> set[str]:
    """Models admission requirements only; never encrypts or authenticates bytes."""
    errors: set[str] = set()
    if not all(observation.get(k) is True for k in ("secure_backend", "key_available", "allocation_recoverable")):
        errors.add("key_backend_unavailable")
    if not all(observation.get(k) is True for k in ("seal_ok", "aad_verified", "ciphertext_only")):
        errors.add("encryption_failed")
    ranks = {"personal": 1, "sensitive": 2}
    actual, required = observation.get("effective_classification"), observation.get("required_classification")
    grade_valid = (isinstance(actual, str) and isinstance(required, str)
                   and actual in ranks and required in ranks and ranks[actual] >= ranks[required])
    if (not grade_valid
            or type(observation.get("policy_epoch")) is not int
            or type(observation.get("current_policy_epoch")) is not int
            or observation.get("policy_epoch", 0) < 1
            or observation.get("policy_epoch") != observation.get("current_policy_epoch")
            or not all(observation.get(k) is True for k in
                       ("reauthorized", "lineage_rechecked", "cleanup_registered"))):
        errors.add("protection_context_invalid")
    if observation.get("plaintext_surfaces") != [] or observation.get("test_plaintext_adapter") is not False:
        errors.add("plaintext_forbidden")
    return errors


def orphan_cleanup_status(observation: Mapping[str, Any]) -> str:
    """Rollback invalidates admission immediately; cleanup is not assumed successful."""
    if all(observation.get(k) is True for k in
           ("unreadable", "key_revocation_verified", "ciphertext_erasure_verified", "generation_matches")):
        return "erased"
    return "blocked"
