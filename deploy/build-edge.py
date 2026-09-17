#!/usr/bin/env python3
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile


def build_manifest(path):
    lines = []
    section = ""
    skip = False
    for line in path.read_text(encoding="utf-8").splitlines(keepends=True):
        header = re.match(r"^\s*\[\[?(.+?)\]\]?\s*(?:#.*)?$", line)
        if header:
            section = header.group(1).strip()
            skip = section == "bench" or bool(
                re.search(r"(^|\.)dev-dependencies(\.|$)", section)
            )
        if skip or (section == "package" and re.match(r"\s*readme\s*=", line)):
            continue
        lines.append(line)
    return "".join(lines)


def main():
    repo = Path(__file__).resolve().parent.parent
    target = repo / "target" / "edge-wasix-build"
    artifact = Path("wasm32-wasmer-wasi/release/velocity-edge.wasm")
    final = repo / "target" / artifact
    temp_root = Path(tempfile.gettempdir()).resolve()
    if temp_root == repo or repo in temp_root.parents:
        temp_root = repo.parent

    # Stage outside the repo to isolate the WASIX registry and lock from native Cargo.
    with tempfile.TemporaryDirectory(prefix="velocity-edge-wasix-", dir=temp_root) as tmp:
        stage = Path(tmp)
        for name in ("velocity-mcp-core", "velocity-mcp-edge"):
            source = repo / "crates" / name
            dest = stage / "crates" / name
            shutil.copytree(source / "src", dest / "src")
            manifest = build_manifest(source / "Cargo.toml")
            if name == "velocity-mcp-edge":
                manifest += "\n[package.metadata]\nwasm-opt = false\n"
            (dest / "Cargo.toml").write_text(manifest, encoding="utf-8")

        (stage / "Cargo.toml").write_text(
            '[workspace]\n'
            'members = ["crates/velocity-mcp-core", "crates/velocity-mcp-edge"]\n'
            'resolver = "2"\n\n'
            '[profile.release]\n'
            'opt-level = 2\n'
            'panic = "abort"\n',
            encoding="utf-8",
        )
        (stage / ".cargo").mkdir()
        (stage / ".cargo" / "config.toml").write_text(
            '[source.crates-io]\n'
            'replace-with = "wasix"\n\n'
            '[source.wasix]\n'
            'registry = "sparse+https://cargo-registry.wasix.org/"\n\n'
            '[http]\n'
            'multiplexing = false\n',
            encoding="utf-8",
        )
        shutil.copyfile(repo / "deploy" / "edge-wasix.lock", stage / "Cargo.lock")
        env = os.environ.copy()
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        env.update(
            CARGO_TARGET_DIR=str(target),
            RUSTFLAGS="-C opt-level=2 -C target-feature=+atomics,+bulk-memory,+mutable-globals,+reference-types,+multivalue -C panic=abort",
            CARGO_HTTP_MULTIPLEXING="false",
            CARGO_HTTP_TIMEOUT="30",
        )
        result = subprocess.run(
            ["cargo", "wasix", "+wasix", "build", "--release", "--locked",
             "-p", "velocity-mcp-edge", "--bin", "velocity-edge"],
            cwd=stage,
            env=env,
        )
        if result.returncode != 0:
            return result.returncode
        final.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(target / artifact, final)
    print(final)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
