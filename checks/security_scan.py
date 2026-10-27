"""Supplemental denylist scan. Never prints matched secret material."""
from __future__ import annotations

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
PATTERNS = [
    re.compile(r"gh[pousr]_" + r"[A-Za-z0-9]{30,}"),
    re.compile(r"github_pat_" + r"[A-Za-z0-9_]{40,}"),
    re.compile(r"sk-" + r"[A-Za-z0-9_-]{24,}"),
    re.compile(r"-----BEGIN " + r"(?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
]
FORBIDDEN = {".db", ".sqlite", ".sqlite3", ".pem", ".key", ".p12", ".pfx", ".log"}


def inspect(path: str, content: str) -> list[str]:
    p = Path(path)
    errors: list[str] = []
    if p.suffix.lower() in FORBIDDEN or p.name == ".env" or p.name.startswith(".env."):
        errors.append("forbidden runtime/secret file")
    if any(part.lower() in {"data", "backups", "chats", "memories", "exports"} for part in p.parts):
        errors.append("private runtime directory")
    if any(pattern.search(content) for pattern in PATTERNS):
        errors.append("credential pattern")
    return errors


def main() -> int:
    result = subprocess.run(
        ["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
        cwd=ROOT, check=True, capture_output=True,
    )
    files = sorted(set(result.stdout.decode("utf-8").strip("\0").split("\0")))
    findings: list[str] = []
    for name in files:
        if not name:
            continue
        path = ROOT / name
        if not path.is_file():
            continue
        content = path.read_bytes().decode("utf-8", errors="replace")
        findings.extend(f"{name}: {error}" for error in inspect(name, content))
    for finding in findings:
        print(finding)
    print(f"Supplemental scan: {len(files)} paths, {len(findings)} findings")
    return int(bool(findings))


if __name__ == "__main__":
    raise SystemExit(main())
