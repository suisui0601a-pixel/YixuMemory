"""Real external-crate compile probes, on NTFS; no mock authorization claims."""
from pathlib import Path
import json
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def main() -> None:
    results = []
    cases = [
        ("default_lab_hidden", "yixu-sqlite-foundation", "crates/sqlite", [],
         "use yixu_sqlite_foundation::experimental::MetadataLab; fn main() {}", "E0432"),
        ("all_features_old_api_hidden", "yixu-sqlite-foundation", "crates/sqlite", ["metadata-lab", "fault-injection"],
         "use yixu_sqlite_foundation::MetadataLab; fn main() {}", "E0432"),
        ("internal_storage_hidden", "yixu-sqlite-foundation", "crates/sqlite", ["metadata-lab"],
         "use yixu_sqlite_foundation::storage::MetadataLab; fn main() {}", "E0603"),
        ("context_cannot_be_forged", "yixu-ports", "crates/ports", [],
         "fn main() { let _ = yixu_ports::AuthContext { principal: todo!(), scope: todo!() }; }", "E0451"),
        ("bootstrap_cannot_be_forged", "yixu-ports", "crates/ports", [],
         "fn main() { let _ = yixu_ports::BootstrapAuthority { _private: () }; }", "E0451"),
    ]
    for name, package, relative, features, source, diagnostic in cases:
        with tempfile.TemporaryDirectory(prefix="yixu-external-") as temp:
            path = Path(temp)
            (path / "src").mkdir()
            (path / "src/main.rs").write_text(source)
            dep_path = json.dumps((ROOT / relative).as_posix())
            (path / "Cargo.toml").write_text(
                '[package]\nname="external-probe"\nversion="0.0.0"\nedition="2024"\n'
                f'[dependencies]\n{package}={{path={dep_path},default-features=false,features={json.dumps(features)}}}\n'
            )
            result = subprocess.run(["cargo", "check", "--offline", "--manifest-path", str(path / "Cargo.toml")],
                                    text=True, capture_output=True, check=False)
            assert result.returncode != 0 and diagnostic in result.stderr, (name, result.stderr)
            results.append({"case": name, "exit": result.returncode, "expected_diagnostic": diagnostic,
                            "stderr": result.stderr})
    out = ROOT / "verification"
    out.mkdir(exist_ok=True)
    (out / "external-api-probes.json").write_text(json.dumps(results, indent=2) + "\n")
    print(f"{len(results)} external compile-negative probes passed")

if __name__ == "__main__":
    main()
