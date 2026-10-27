"""Pure wire-review oracle, not an MCP server, SDK, auth layer or transport."""
from __future__ import annotations

import base64
import binascii
from typing import Any, Mapping

MODERN = "2026-07-28"
LEGACY = "2025-11-25"
PREFIX = "io.modelcontextprotocol/"
MODERN_ERRORS = {-32020, -32021, -32022}


def _decision(code: int = 0, status: int = 200, outcome: str = "accept") -> dict[str, Any]:
    return {"code": code, "http_status": status, "outcome": outcome}


def _header_value(value: str) -> str:
    if value.startswith("=?base64?") and value.endswith("?="):
        return base64.b64decode(value[9:-2], validate=True).decode("utf-8")
    if value != value.strip() or any(ord(c) < 32 or ord(c) > 126 for c in value):
        raise ValueError("unsafe header")
    return value


def wire_decision(case: Mapping[str, Any]) -> dict[str, Any]:
    """Subset of normalized wire fixtures; no business call is ever executed."""
    request, state = case["request"] or {}, case["state"]
    method, params = request.get("method", ""), request.get("params", {})
    meta = params.get("_meta", {})
    http = case["transport"] == "http"
    headers: dict[str, str] = {}
    for header in case["headers"]:
        name = header["name"].lower()
        if name in headers:
            return _decision(-32020, 400, "reject_duplicate_header")
        headers[name] = header["value"]
    if http and case["http_method"] != "POST":
        if case["era"] == "modern":
            return _decision(0, 405, "method_not_allowed")
        if headers.get("mcp-protocol-version") != LEGACY:
            return _decision(-32602, 400, "legacy_version_required")
        if state["legacy_session_id"] and headers.get("mcp-session-id") != state["legacy_session_id"]:
            return _decision(0, 404, "invalid_legacy_session")
        if not state["legacy_initialized"]:
            return _decision(-32600, 400, "legacy_not_initialized")
        return _decision(outcome="legacy_session_transport")
    if http and case["http_method"] == "POST":
        if headers.get("content-type", "").split(";")[0].strip().lower() != "application/json":
            return _decision(0, 415, "unsupported_media_type")
        accepted = {v.strip().split(";")[0] for v in headers.get("accept", "").split(",")}
        if not {"application/json", "text/event-stream"} <= accepted:
            return _decision(0, 406, "invalid_accept")
    if case["era"] == "modern":
        if http and case["http_method"] != "POST":
            return _decision(0, 405, "method_not_allowed")
        if PREFIX + "protocolVersion" not in meta or PREFIX + "clientCapabilities" not in meta:
            return _decision(-32602, 400, "invalid_meta")
        version = meta[PREFIX + "protocolVersion"]
        if not isinstance(meta[PREFIX + "clientCapabilities"], dict):
            return _decision(-32602, 400, "invalid_meta")
        if http:
            expected = {"mcp-protocol-version": version, "mcp-method": method}
            if method == "tools/call":
                expected["mcp-name"] = params.get("name")
            try:
                if any(k not in headers or _header_value(headers[k]) != v for k, v in expected.items()):
                    return _decision(-32020, 400, "header_mismatch")
            except (ValueError, UnicodeError, binascii.Error):
                return _decision(-32020, 400, "header_mismatch")
        if version == LEGACY:
            return _decision(-32602, 400, "legacy_version_requires_handshake")
        if version != MODERN:
            result = _decision(-32022, 400, "unsupported_modern_version")
            result["error_data"] = {"supported": [MODERN, LEGACY], "requested": version}
            return result
        if method == "server/discover" and set(params) != {"_meta"}:
            return _decision(-32602, 400, "invalid_discover_params")
        if method == "tools/call" and (not isinstance(params.get("name"), str)
                                      or not isinstance(params.get("arguments"), dict)):
            return _decision(-32602, 400, "invalid_tool_params")
        if method not in {"server/discover", "tools/list", "tools/call", "ping"}:
            return _decision(-32601, 404 if http else 200, "unknown_modern_method")
        # Session/Last-Event-ID headers are ignored, never read as authentication or state.
        return _decision(outcome="stateless_accept")
    if PREFIX + "protocolVersion" in meta or headers.get("mcp-protocol-version") == MODERN:
        return _decision(-32020 if http else -32602, 400, "cross_era_rejected")
    if method == "initialize":
        if not all(k in params for k in ("protocolVersion", "capabilities", "clientInfo")):
            return _decision(-32602, 400, "invalid_initialize")
        result = _decision(outcome="legacy_selected" if params["protocolVersion"] == LEGACY
                           else "legacy_offer_client_must_check")
        result["selected_version"] = LEGACY
        return result
    if http:
        if headers.get("mcp-protocol-version") != LEGACY:
            return _decision(-32602, 400, "legacy_version_required")
        if state["legacy_session_id"] and headers.get("mcp-session-id") != state["legacy_session_id"]:
            return _decision(0, 404, "invalid_legacy_session")
    if method == "notifications/initialized" and state["legacy_protocol"] == LEGACY:
        return _decision(status=202 if http else 200, outcome="legacy_ready_notification")
    if not state["legacy_initialized"] or state["legacy_protocol"] != LEGACY:
        return _decision(-32600, 400, "legacy_not_initialized")
    if http and case["http_method"] in {"GET", "DELETE"}:
        return _decision(outcome="legacy_session_transport")
    if method not in {"tools/list", "tools/call", "ping"}:
        return _decision(-32601, 200, "unknown_legacy_method")
    return _decision(outcome="legacy_accept")


def fallback_decision(
    transport: str, client_mode: str, cached_era: str | None, status: int,
    error_code: int | None, response_kind: str,
) -> str:
    """Only an initial probe may pick legacy; never replay a mutating business call."""
    if response_kind == "discover-result" or error_code in MODERN_ERRORS:
        return "stay-modern"
    if transport == "http" and status == 404 and error_code == -32601:
        return "stay-modern"
    if transport == "http" and status in {401, 403, 429}:
        return "authenticate-or-fail"
    if cached_era == "modern" or client_mode == "modern-only":
        return "fail-no-downgrade"
    if client_mode != "dual-era":
        return "legacy-explicit"
    if transport == "stdio" and response_kind in {"non-modern-error", "timeout"}:
        return "fresh-legacy-handshake"
    if transport == "http" and status == 400 and response_kind in {"non-modern-error", "empty"}:
        return "fresh-legacy-handshake"
    return "fail-no-downgrade"
