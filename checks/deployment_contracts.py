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
    """Check semantic relationships after JSON schema validation, without side effects."""
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
    if not observation["broker_proof_verified"]:
        errors.add("permission_denied")
    for field in ("plan_hash", "instance_id", "host_id"):
        if approval[field] != plan[field]:
            errors.add("approval_required")
    if datetime.fromisoformat(approval["valid_until"].replace("Z", "+00:00")) <= now:
        errors.add("approval_required")
    if not set(action_ids) <= set(approval["action_ids"]):
        errors.add("approval_required")
    required = {scope for a in plan["actions"] for scope in a["required_scopes"]}
    if not required <= set(approval["scopes"]):
        errors.add("permission_denied")
    if any(a["requires_elevation"] for a in plan["actions"]) and not approval["allow_elevation"]:
        errors.add("permission_denied")
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
