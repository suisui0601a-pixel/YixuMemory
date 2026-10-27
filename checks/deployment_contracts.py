"""Pure P0 contract-review oracle. No installer, token verifier, or memory engine.

Observations stand for future trusted OS/broker results. Passing this function
cannot authenticate an approval or prove a filesystem/network is safe.
"""
from __future__ import annotations

from datetime import datetime
from typing import Any, Mapping


def plan_violations(
    plan: Mapping[str, Any],
    approval: Mapping[str, Any] | None,
    observation: Mapping[str, Any],
    now: datetime,
    action_policy: Mapping[str, list[str]],
) -> set[str]:
    """Synthetic single-plan relationships only; caller-provided booleans are NOT proof.

    Production must get authenticated broker/OS observations across a protected IPC
    boundary, not deserialize these flags from agent-controlled JSON. This module
    has no production authorization entry point and performs no cryptography.
    """
    errors: set[str] = set()
    for field in ("host_id", "config_digest", "preflight_digest", "artifact_digest"):
        if plan[field] != observation[field]:
            errors.add("state_conflict")
    if plan["plan_hash"] != observation["canonical_plan_hash"]:
        errors.add("state_conflict")
    if datetime.fromisoformat(plan["valid_until"].replace("Z", "+00:00")) <= now:
        errors.add("approval_required")
    if observation["unknown_files"]:
        errors.add("state_conflict")
    if not observation["paths_verified"]:
        errors.add("security_blocked")
    if not observation["audit_available"]:
        errors.add("security_blocked")
    action_ids = [a["id"] for a in plan["actions"]]
    if len(set(action_ids)) != len(action_ids):
        errors.add("state_conflict")
    for action in plan["actions"]:
        if set(action["required_scopes"]) != set(action_policy[action["kind"]]):
            errors.add("security_blocked")
    restore = any(a["kind"] == "restore_backup" for a in plan["actions"])
    migration = any(a["kind"] == "migrate_schema" for a in plan["actions"])
    if plan["upgrade"]["required"] or restore or migration:
        if not all(plan["upgrade"][key] for key in (
            "backup_verified", "restore_drill_verified", "migration_compatible",
            "deletion_watermark_verified",
        )):
            errors.add("compatibility_blocked")
    if approval is None:
        return errors | {"approval_required"}
    if approval.get("approval_kind") != "single-plan":
        errors.add("approval_required")
    if not observation["broker_proof_verified"]:
        errors.add("permission_denied")
    for field in ("plan_hash", "instance_id", "host_id"):
        if approval[field] != plan[field]:
            errors.add("approval_required")
    if datetime.fromisoformat(approval["valid_until"].replace("Z", "+00:00")) <= now:
        errors.add("approval_required")
    approved_ids = approval["action_ids"]
    if len(approved_ids) != len(set(approved_ids)) or set(action_ids) != set(approved_ids):
        errors.add("approval_required")
    required = {scope for a in plan["actions"] for scope in a["required_scopes"]}
    approved_scopes = approval["scopes"]
    if len(approved_scopes) != len(set(approved_scopes)) or required != set(approved_scopes):
        errors.add("permission_denied")
    for action in plan["actions"]:
        scopes = action["required_scopes"]
        if len(scopes) != len(set(scopes)):
            errors.add("security_blocked")
    elevation_required = any(a["requires_elevation"] for a in plan["actions"])
    if approval["allow_elevation"] != elevation_required:
        errors.add("permission_denied")
    if elevation_required and not observation.get("elevation_policy_allows", False):
        errors.add("security_blocked")
    return errors


def memory_call_violations(
    arguments: Mapping[str, Any],
    binding: Mapping[str, Any],
    required_capabilities: list[str],
) -> set[str]:
    """Synthetic policy oracle. Binding represents authenticated server state, not client JSON."""
    if binding["audience"] != "yixumemory-memory":
        return {"unauthenticated"}
    if arguments["tenant_id"] != binding["tenant_id"]:
        return {"not_visible"}
    if arguments["space_id"] not in binding["space_ids"]:
        return {"not_visible"}
    if not set(required_capabilities) <= set(binding["scopes"]):
        return {"denied"}
    return set()


def filesystem_violations(intent: Mapping[str, Any], observation: Mapping[str, Any]) -> set[str]:
    """Model only: booleans/IDs simulate trusted syscalls and cannot prove race safety."""
    errors: set[str] = set()
    for flag in ("anchor_verified", "owner_acl_verified", "protected_ancestors",
                 "primitives_available", "all_components_no_reparse", "types_verified",
                 "handle_relative_only", "ancestry_verified", "link_count_verified"):
        if not observation.get(flag, False):
            errors.add("security_blocked")
    if observation["path_string_reopened"]:
        errors.add("security_blocked")
    for expected, live in (("anchor_identity", "live_anchor_identity"),
                           ("volume_identity", "live_volume_identity")):
        if intent[expected] != observation[live]:
            errors.add("state_conflict")
    if intent["operation"] == "publish_new_file" and observation["target_exists"]:
        errors.add("state_conflict")
    if intent["operation"] == "cleanup_owned":
        if (not observation["cleanup_receipt_owned"] or not observation["cleanup_identity_matches"]
                or observation["unknown_descendants"]):
            errors.add("security_blocked")
    reserved = {"CON", "PRN", "AUX", "NUL", *(f"COM{i}" for i in range(1, 10)),
                *(f"LPT{i}" for i in range(1, 10))}
    for name in intent["components"]:
        if name.upper().split(".")[0] in reserved:
            errors.add("security_blocked")
    return errors
