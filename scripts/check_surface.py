"""Compile-negative release-boundary and opaque-authorization probes."""

from __future__ import annotations

import json
import os
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
            f"[dependencies]\n{dependency_lines}\n[workspace]\n"
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
            "b2_read_api_is_not_public",
            {"yixu-sqlite-foundation": (sqlite, 'default-features = false')},
            "use yixu_sqlite_foundation::auth::persistence::profile10::read::ReadQuery; fn main(){}",
            ["--all-features"], "could not find `auth`",
        ),
        (
            "b2_borrow_trait_is_not_public",
            {"yixu-application": (application, 'default-features = false')},
            "use yixu_application::internal_content::ContentRead; fn main(){}",
            ["--all-features"], "could not find `internal_content`",
        ),
        (
            "p10_real_content_engine_is_private_with_all_features",
            {"yixu-sqlite-foundation": (sqlite, 'default-features = false')},
            "use yixu_sqlite_foundation::auth::persistence::profile10::Engine; fn main(){}",
            ["--all-features"], "could not find `auth`",
        ),
        (
            "p10_internal_application_has_no_production_module",
            {"yixu-application": (application, 'default-features = false')},
            "use yixu_application::internal_content::InternalContentApplication; fn main(){}",
            ["--all-features"], "could not find `internal_content`",
        ),
        (
            "body_crypto_helper_unavailable_to_public_dependency",
            {"yixu-sqlite-foundation": (sqlite, 'default-features = false')},
            "use yixu_sqlite_foundation::auth::persistence::content_crypto; fn main(){}",
            ["--all-features"], "could not find `auth`",
        ),
        (
            "w2_real_local_vault_unavailable_to_public_dependency",
            {"yixu-sqlite-foundation": (sqlite, 'default-features = false')},
            "use yixu_sqlite_foundation::auth::persistence::dpapi::LocalVault; fn main(){}",
            ["--all-features"], "could not find `auth`",
        ),
        (
            "m3b_persistence_unavailable_with_all_features",
            {"yixu-sqlite-foundation": (sqlite, 'default-features = false')},
            "use yixu_sqlite_foundation::auth::persistence::Store; fn main(){}",
            ["--all-features"], "could not find `auth`",
        ),
        (
            "m3b_linux_store_unavailable_to_dependency",
            {"yixu-m3-lab": ((ROOT / "crates/m3-lab").as_posix(), 'default-features = false')},
            "use yixu_m3_lab::linux_store::Store; fn main(){}",
            ["--all-features"], "could not find `linux_store`",
        ),
        (
            "m3_fake_backend_unavailable_with_all_features",
            {"yixu-m3-lab": ((ROOT / "crates/m3-lab").as_posix(), 'default-features = false')},
            "use yixu_m3_lab::experiment::FakeKeyVault; fn main(){}",
            ["--all-features"], "could not find `experiment`",
        ),
        (
            "m3_release_permit_cannot_be_forged",
            {"yixu-ports": (ports, 'default-features = false')},
            "fn main(){let _=yixu_ports::security_m3::ReleasePermit{_private:()};}",
            ["--all-features"], "field `_private`",
        ),
        (
            "m3_protection_context_cannot_be_forged",
            {"yixu-ports": (ports, 'default-features = false')},
            "fn main(){let _=yixu_ports::security_m3::ProtectionContext{binding:todo!()};}",
            [], "field `binding`",
        ),
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
    # All new probes remain separate from runtime SQLite scenario counts.
    for field in ("memory_id", "revision_id", "parent_revision", "revision_number", "ids", "body"):
        cases.append((
            f"memory_request_cannot_choose_{field}",
            {"yixu-ports": (ports, 'default-features = false')},
            f"fn attempt(c:&mut yixu_ports::memory::MemoryWrite){{c.{field}=todo!();}} fn main(){{}}",
            [],
            f"no field `{field}`",
        ))
    cases.append((
        "memory_private_adapter_unavailable_even_with_support_unification",
        {"yixu-sqlite-foundation": (sqlite, 'default-features = false'),
         "yixu-sqlite-test-support": (support, 'features = ["synthetic-fixtures"]')},
        "use yixu_sqlite_foundation::auth::AuthDatabase; fn main(){}",
        ["--all-features"], "could not find `auth`",
    ))
    cases.append((
        "memory_facade_has_no_repository_escape_hatch",
        {"yixu-ports": (ports, 'default-features = false'), "yixu-application": (application, 'default-features = false')},
        "use yixu_ports::memory::AuthorizedMemory; fn attempt<S:AuthorizedMemory>(a:&mut yixu_application::memory::MemoryApplication<S>){a.store=todo!();} fn main(){}",
        [], "field `store`",
    ))
    for method in ("history", "resolve_conflict"):
        cases.append((
            f"unimplemented_memory_{method}_has_no_ingress",
            {"yixu-ports": (ports, 'default-features = false'), "yixu-application": (application, 'default-features = false')},
            f"use yixu_ports::memory::AuthorizedMemory; fn attempt<S:AuthorizedMemory>(a:&mut yixu_application::memory::MemoryApplication<S>){{a.{method}();}} fn main(){{}}",
            [], f"no method named `{method}`",
        ))
    for field in ("body", "terms", "locator", "admin", "key_ref"):
        cases.append((
            f"m2_query_has_no_{field}_override",
            {"yixu-ports": (ports, 'default-features = false')},
            f"fn attempt(q:&mut yixu_ports::memory_m2::MetadataQuery){{q.{field}=todo!();}}fn main(){{}}",
            [], f"no field `{field}`",
        ))
    cases.append((
        "m2_signing_authority_is_private_even_with_support_unification",
        {"yixu-sqlite-foundation": (sqlite, 'default-features = false'), "yixu-sqlite-test-support": (support, 'features = ["synthetic-fixtures"]')},
        "use yixu_sqlite_foundation::auth::m2::tokens::TokenAuthority;fn main(){}",
        ["--all-features"], "could not find `auth`",
    ))
    cases.append((
        "m2_facade_cannot_expose_repository",
        {"yixu-ports": (ports, 'default-features = false'), "yixu-application": (application, 'default-features = false')},
        "use yixu_ports::memory_m2::MemoryM2;fn attempt<S:MemoryM2>(a:&mut yixu_application::memory_m2::MemoryM2Application<S>){a.store=todo!();}fn main(){}",
        [], "field `store`",
    ))
    cases.append((
        "w3_broker_and_plaintext_unavailable_even_with_all_features",
        {"yixu-sqlite-foundation": (sqlite, 'default-features = false')},
        "use yixu_sqlite_foundation::auth::persistence::broker::Broker;fn main(){}",
        ["--all-features"], "could not find `auth`",
    ))
    for private_type in ("TerminalEvidence", "Operation"):
        cases.append((
            f"w4a_{private_type}_has_no_request_constructor",
            {"yixu-sqlite-foundation": (sqlite, 'default-features = false')},
            f"use yixu_sqlite_foundation::auth::persistence::output::{private_type};fn main(){{}}",
            ["--all-features"], "could not find `auth`",
        ))
    ipc_deps = {"yixu-ipc": ((ROOT / "crates/ipc").as_posix(), 'default-features = false')}
    for name, source, diagnostic in (
        ("private_session", "use yixu_ipc::AuthenticatedSession; fn main(){}", "struct `AuthenticatedSession` is private"),
        ("private_assembly", "fn main(){yixu_ipc::Core::new([0;16],[0;16],todo!());}", "associated function `new` is private"),
        ("no_fake_auth", "use yixu_ipc_test_support::prove; fn main(){}", "unresolved import `yixu_ipc_test_support`"),
        ("no_native_settlement", "fn main(){yixu_ipc::Core::default().complete_native(todo!());}", "no method named `complete_native`"),
        ("r4_no_synthetic_broker", "use yixu_ipc::tests::r4::Broker; fn main(){}", "could not find `tests`"),
        ("r4_no_synthetic_completion", "use yixu_ipc::SyntheticCompletion; fn main(){}", "unresolved import `yixu_ipc::SyntheticCompletion`"),
        ("r4_no_direct_release_intent_write", "fn main(){yixu_ipc::Core::default().arm(todo!());}", "no method named `arm`"),
        ("r5_no_closure_module", "use yixu_ipc::tests::r4::r5::Closure; fn main(){}", "could not find `tests`"),
        ("r5_no_effect_receipt", "use yixu_ipc::SyntheticEffect; fn main(){}", "unresolved import `yixu_ipc::SyntheticEffect`"),
        ("r5_no_security_mutation", "fn main(){yixu_ipc::Core::default().apply_security_change(todo!());}", "no method named `apply_security_change`"),
    ):
        cases.append((f"ipc_{name}", ipc_deps, source, ["--all-features"], diagnostic))
    cases.append((
        "ipc_scheduler_descriptor_cannot_mint_business_auth",
        {**ipc_deps, "yixu-ports": (ports, 'default-features = false')},
        "fn attempt(s:yixu_ipc::Scheduled)->yixu_ports::AuthContext{s.into()} fn main(){}",
        ["--all-features"], "AuthContext: From<Scheduled>",
    ))
    results = [check_manifest(*case) for case in cases]

    metadata = json.loads(
        subprocess.check_output(["cargo", "metadata", "--locked", "--offline", "--all-features", "--format-version", "1"], cwd=ROOT, text=True)
    )
    packages = {package["name"]: package for package in metadata["packages"]}
    assert packages["yixu-sqlite-test-support"]["publish"] == [], "test support must remain unpublished"
    assert not {"metadata-lab", "fault-injection"} & set(packages["yixu-sqlite-foundation"]["features"])
    for root in ("yixu-application", "yixu-sqlite-foundation"):
        reached = normal_reachable(metadata, root)
        assert "yixu-sqlite-test-support" not in reached, (root, reached)
        assert not {"yixu-m3-lab", "chacha20poly1305", "zeroize"} & reached, (root, reached)
        assert not {"yixu-ipc", "yixu-ipc-test-support"} & reached, (root, reached)
    assert packages["yixu-ipc-test-support"]["publish"] == []
    assert not {
        "yixu-ipc-test-support", "hmac", "rusqlite", "windows-sys", "yixu-application",
        "yixu-ports", "yixu-sqlite-foundation",
    } & normal_reachable(metadata, "yixu-ipc")
    results.extend(
        [
            {"case": "application_normal_graph_excludes_test_support", "result": "passed"},
            {"case": "sqlite_normal_graph_excludes_test_support", "result": "passed"},
            {"case": "test_support_publish_false", "result": "passed"},
            {"case": "ipc_normal_graph_isolated_from_fixture_and_runtime", "result": "passed"},
        ]
    )
    out = Path(os.environ.get("YIXU_VERIFICATION_OUT", str(ROOT / "verification")))
    out.mkdir(exist_ok=True)
    (out / "external-api-probes.json").write_text(json.dumps(results, indent=2) + "\n")
    print(f"{len(cases)} compile-negative probes and 4 package-graph checks passed")


if __name__ == "__main__":
    main()
