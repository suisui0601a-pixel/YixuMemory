"""Compile-negative release-boundary and opaque-authorization probes."""

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SQLITE = ROOT / "crates/sqlite"
SUPPORT = ROOT / "crates/test-support"
PORTS = ROOT / "crates/ports"
APPLICATION = ROOT / "crates/application"


def check_manifest(
    package: str,
    dependencies: dict[str, tuple[str, str]],
    source: str,
    args: list[str],
    diagnostic: str,
) -> dict[str, str]:
    with tempfile.TemporaryDirectory(prefix="yixu-external-") as temp:
        path = Path(temp)
        (path / "src").mkdir()
        (path / "src/main.rs").write_text(source)
        dependency_lines = "\n".join(
            f'{name} = {{ path = {json.dumps(location)}, {options} }}'
            for name, (location, options) in dependencies.items()
        )
        (path / "Cargo.toml").write_text(
            '[package]\nname="external-probe"\nversion="0.0.0"\nedition="2024"\n'
            f"[dependencies]\n{dependency_lines}\n"
        )
        result = subprocess.run(
            ["cargo", "check", "--offline", *args, "--manifest-path", str(path / "Cargo.toml")],
            text=True,
            capture_output=True,
            check=False,
        )
        assert result.returncode != 0 and diagnostic in result.stderr, (package, result.stderr)
        return {"case": package, "exit": str(result.returncode), "expected_diagnostic": diagnostic}


def normal_reachable(metadata: dict, start_name: str) -> set[str]:
    packages = {package["id"]: package["name"] for package in metadata["packages"]}
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    starts = [package_id for package_id, name in packages.items() if name == start_name]
    assert len(starts) == 1, (start_name, starts)
    reached: set[str] = set()
    pending = starts[:]
    while pending:
        current = pending.pop()
        if current in reached:
            continue
        reached.add(current)
        for dependency in nodes[current]["deps"]:
            kinds = dependency["dep_kinds"]
            if any(kind["kind"] in (None, "normal", "build") for kind in kinds):
                pending.append(dependency["pkg"])
    return {packages[package_id] for package_id in reached}


def main() -> None:
    # TOML path values are quoted below; keep forward slashes and do not pre-quote.
    sqlite = SQLITE.as_posix()
    support = SUPPORT.as_posix()
    ports = PORTS.as_posix()
    application = APPLICATION.as_posix()
    cases = [
        (
            "sqlite_default_has_no_lab_api",
            {"yixu-sqlite-foundation": (sqlite, 'default-features = false')},
            "use yixu_sqlite_foundation::experimental::MetadataLab; fn main() {}",
            [],
            "could not find `experimental`",
        ),
        (
            "sqlite_all_features_has_no_lab_api",
            {
                "yixu-sqlite-foundation": (sqlite, 'default-features = false'),
                "yixu-sqlite-test-support": (support, 'optional = true, default-features = false, features = ["synthetic-fixtures"]'),
            },
            "use yixu_sqlite_foundation::experimental::MetadataLab; fn main() {}",
            ["--all-features"],
            "could not find `experimental`",
        ),
        (
            "support_feature_does_not_unify_into_sqlite",
            {
                "yixu-sqlite-foundation": (sqlite, 'default-features = false'),
                "yixu-sqlite-test-support": (support, 'default-features = false, features = ["synthetic-fixtures"]'),
            },
            "use yixu_sqlite_foundation::experimental::MetadataLab; fn main() {}",
            [],
            "could not find `experimental`",
        ),
        (
            "ordinary_caller_cannot_import_support_transitively",
            {"yixu-sqlite-foundation": (sqlite, 'default-features = false')},
            "use yixu_sqlite_test_support::MetadataLab; fn main() {}",
            [],
            "use of unresolved module or unlinked crate `yixu_sqlite_test_support`",
        ),
        (
            "context_cannot_be_forged",
            {"yixu-ports": (ports, 'default-features = false')},
            "fn main() { let _ = yixu_ports::AuthContext { principal: todo!(), scope: todo!() }; }",
            [],
            "fields `principal` and `scope` of struct `AuthContext` are private",
        ),
        (
            "bootstrap_cannot_be_forged",
            {"yixu-ports": (ports, 'default-features = false')},
            "fn main() { let _ = yixu_ports::BootstrapAuthority { _private: () }; }",
            [],
            "field `_private` of struct `BootstrapAuthority` is private",
        ),
        (
            "ordinary_facade_has_no_bootstrap",
            {
                "yixu-application": (application, 'default-features = false'),
                "yixu-ports": (ports, 'default-features = false'),
            },
            "use yixu_ports::authorized_metadata::AuthorizedMetadata; fn attempt<S:AuthorizedMetadata>(app:&mut yixu_application::metadata::MetadataApplication<S>,ctx:&S::Context){app.bootstrap(ctx);} fn main(){}",
            [],
            "no method named `bootstrap`",
        ),
        (
            "create_marker_cannot_be_chosen",
            {"yixu-ports": (ports, 'default-features = false')},
            "fn main(){let _=yixu_ports::authorized_metadata::MarkerWrite{scope:todo!(),target:yixu_ports::authorized_metadata::MarkerTarget::Create,expected_version:0,idempotency:todo!(),capability:yixu_ports::authorized_metadata::MetadataCapability::Append,marker:todo!()};}",
            [],
            "struct `MarkerWrite` has no field named `marker`",
        ),
        (
            "create_event_cannot_be_chosen",
            {"yixu-ports": (ports, 'default-features = false')},
            "fn main(){let _=yixu_ports::authorized_metadata::MarkerWrite{scope:todo!(),target:yixu_ports::authorized_metadata::MarkerTarget::Create,expected_version:0,idempotency:todo!(),capability:yixu_ports::authorized_metadata::MetadataCapability::Append,event:todo!()};}",
            [],
            "struct `MarkerWrite` has no field named `event`",
        ),
        (
            "request_cannot_inject_id_source",
            {"yixu-ports": (ports, 'default-features = false')},
            "fn main(){let _=yixu_ports::authorized_metadata::MarkerWrite{scope:todo!(),target:yixu_ports::authorized_metadata::MarkerTarget::Create,expected_version:0,idempotency:todo!(),capability:yixu_ports::authorized_metadata::MetadataCapability::Append,ids:todo!()};}",
            [],
            "struct `MarkerWrite` has no field named `ids`",
        ),
    ]
    results = [check_manifest(*case) for case in cases]

    metadata = json.loads(
        subprocess.check_output(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=ROOT, text=True)
    )
    packages = {package["name"]: package for package in metadata["packages"]}
    assert packages["yixu-sqlite-test-support"]["publish"] == [], "test support must remain unpublished"
    assert not {"metadata-lab", "fault-injection"} & set(packages["yixu-sqlite-foundation"]["features"])
    for root in ("yixu-application", "yixu-sqlite-foundation"):
        reached = normal_reachable(metadata, root)
        assert "yixu-sqlite-test-support" not in reached, (root, reached)
    results.extend(
        [
            {"case": "application_normal_graph_excludes_test_support", "result": "passed"},
            {"case": "sqlite_normal_graph_excludes_test_support", "result": "passed"},
            {"case": "test_support_publish_false", "result": "passed"},
        ]
    )
    out = ROOT / "verification"
    out.mkdir(exist_ok=True)
    (out / "external-api-probes.json").write_text(json.dumps(results, indent=2) + "\n")
    print(f"{len(cases)} compile-negative probes and 3 package-graph checks passed")


if __name__ == "__main__":
    main()
