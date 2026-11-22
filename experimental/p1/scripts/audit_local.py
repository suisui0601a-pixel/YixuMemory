"""Local source boundary, license inventory, and live OSV lockfile audit. No secrets read."""

from __future__ import annotations

import hashlib
import json
import os
import re
import subprocess
import sys
import tomllib
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = Path(os.environ.get("YIXU_VERIFICATION_OUT", str(ROOT / "verification")))


def main() -> int:
    OUT.mkdir(exist_ok=True)
    manifest = tomllib.loads((ROOT / "Cargo.toml").read_text())
    assert manifest["workspace"]["package"]["publish"] is False
    graph: dict[str, list[str]] = {}
    for crate in sorted((ROOT / "crates").iterdir()):
        cfg = tomllib.loads((crate / "Cargo.toml").read_text())
        deps = sorted(cfg.get("dependencies", {}))
        graph[cfg["package"]["name"]] = deps
        if crate.name == "domain":
            assert not deps, "Domain must have no external dependencies"
        if crate.name in {"ports", "application"}:
            assert set(deps) <= {"yixu-domain", "yixu-ports"}
        if crate.name == "test-support":
            assert cfg["package"]["publish"] is False, "Test support must remain unpublished"
        else:
            assert cfg["package"]["publish"]["workspace"] is True
        assert not any("plaintext" in key for key in cfg.get("features", {}))
    (OUT / "dependency-direction.json").write_text(json.dumps(graph, indent=2) + "\n")

    files = []
    forbidden = [
        r"gh[pousr]_" + r"[A-Za-z0-9]{30,}",
        r"github_pat_" + r"[A-Za-z0-9_]{40,}",
        r"sk-" + r"[A-Za-z0-9_-]{24,}",
        r"-----BEGIN " + r"(?:RSA |EC |OPENSSH )?PRIVATE KEY-----",
    ]
    for path in sorted(ROOT.rglob("*")):
        relative = path.relative_to(ROOT)
        if not path.is_file() or set(relative.parts) & {
            ".git",
            "target",
            "verification",
            "__pycache__",
            ".ruff_cache",
            ".mypy_cache",
            ".pytest_cache",
        }:
            continue
        assert path.suffix.lower() not in {".sqlite", ".db", ".pem", ".key", ".log"}
        content = path.read_bytes()
        text = content.decode("utf-8")
        assert not any(re.search(pattern, text) for pattern in forbidden), (
            f"Credential pattern: {relative}"
        )
        files.append(
            {
                "path": relative.as_posix(),
                "sha256": hashlib.sha256(content).hexdigest(),
                "bytes": len(content),
            }
        )
    (OUT / "source-manifest.json").write_text(json.dumps(files, indent=2) + "\n")
    if "--manifest-only" in sys.argv:
        print(f"Source manifest: {len(files)} files; static boundary and credential-pattern checks passed")
        return 0

    # Cargo supplies the locked dependency resolution and upstream declared license expressions.
    metadata = json.loads(
        subprocess.check_output(
            ["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT, text=True
        )
    )
    inventory = [
        {"name": p["name"], "version": p["version"], "license": p["license"], "source": p["source"]}
        for p in metadata["packages"]
        if p["source"]
    ]
    (OUT / "dependency-licenses.json").write_text(json.dumps(inventory, indent=2) + "\n")
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    packages = [p for p in lock["package"] if p.get("source", "").startswith("registry+")]
    query = {
        "queries": [
            {"package": {"ecosystem": "crates.io", "name": p["name"]}, "version": p["version"]}
            for p in packages
        ]
    }
    request = urllib.request.Request(
        "https://api.osv.dev/v1/querybatch",
        data=json.dumps(query).encode(),
        headers={"Content-Type": "application/json"},
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        result = json.load(response)
    assert len(result["results"]) == len(packages), "Incomplete audit response"
    findings = [
        {"name": package["name"], "version": package["version"], "advisories": observed["vulns"]}
        for package, observed in zip(packages, result["results"], strict=True)
        if observed.get("vulns")
    ]
    report = {
        "provider": "https://api.osv.dev/v1/querybatch",
        "packages": len(packages),
        "findings": findings,
        "scope": "Exact Rust lockfile registry versions only; not proof of native compiler/SQLite safety or legal compatibility.",
    }
    (OUT / "dependency-audit.json").write_text(json.dumps(report, indent=2) + "\n")
    print(
        f"Source files: {len(files)}; boundary checks passed; registry packages audited: {len(packages)}; advisories: {len(findings)}"
    )
    return int(bool(findings))


if __name__ == "__main__":
    raise SystemExit(main())
