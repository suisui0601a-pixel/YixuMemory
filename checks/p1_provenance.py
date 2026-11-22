"""Verify both original Git histories and the immutable import tree, not future source immutability."""
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "docs/integration/20261122/files.json"


def git(*args: str) -> str:
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def main() -> None:
    data = json.loads(MANIFEST.read_text(encoding="utf-8"))
    source = data["source"]
    base = data["public_base"]
    git("merge-base", "--is-ancestor", source, "HEAD")
    git("merge-base", "--is-ancestor", base, "HEAD")
    merges = [line.split() for line in git("rev-list", "--merges", "--parents", "HEAD").splitlines()]
    imports = [row[0] for row in merges if row[1:] == [base, source]]
    assert len(imports) == 1, "Expected one traceable two-parent import"
    expected_tree = git("rev-parse", source + "^{tree}")
    assert git("rev-parse", imports[0] + ":experimental/p1") == expected_tree
    for item in data["files"]:
        content = subprocess.check_output(["git", "show", source + ":" + item["source"]], cwd=ROOT)
        assert hashlib.sha256(content).hexdigest() == item["sha256"]
        if "--current" in sys.argv:
            assert (ROOT / item["destination"]).read_bytes() == content, item["destination"]
    print(f"Verified {len(data['files'])} imported files and both preserved histories")


if __name__ == "__main__":
    main()
