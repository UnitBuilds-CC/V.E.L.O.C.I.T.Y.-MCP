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
            skip = section == "bench" or bool(re.search(r"(^|\.)dev-dependencies(\.|$)", section))
        if skip or (section == "package" and re.match(r"\s*readme\s*=", line)):
            continue
        lines.append(line)
    return "".join(lines)


def main():
    repo = Path(r"C:\Users\ian\Documents\MCP")
    target = repo / "target" / "edge-wasix-poc"
    artifact = Path("wasm32-wasmer-wasi/release/velocity-edge.wasm")
    final = repo / "nested-poc" / "poc.wasm"
    with tempfile.TemporaryDirectory(prefix="velocity-edge-poc-") as tmp:
        stage = Path(tmp)
        for name in ("velocity-mcp-core", "velocity-mcp-edge"):
            source = repo / "crates" / name
            dest = stage / "crates" / name
            shutil.copytree(source / "src", dest / "src")
            manifest = build_manifest(source / "Cargo.toml")
            if name == "velocity-mcp-edge":
                manifest += "\n[package.metadata]\nwasm-opt = false\n"
                # Prototype deps injected only into the PoC build (wasmer target);
                # flagship Cargo.toml stays clean. Single new table to avoid TOML dup.
                manifest += (
                    "\n[target.'cfg(all(target_arch = \"wasm32\", target_vendor = \"wasmer\"))'.dependencies]\n"
                    "wasmi = \"0.32\"\n"
                    "rustls = { version = \"0.23\", default-features = false, features = [\"ring\", \"logging\", \"std\", \"tls12\"] }\n"
                    "webpki-roots = \"0.26\"\n"
                    "sha2 = \"0.10\"\n"
                    "hmac = \"0.12\"\n"
                    "pbkdf2 = { version = \"0.12\", default-features = false, features = [\"hmac\"] }\n"
                    "base64 = \"0.22\"\n"
                )
            (dest / "Cargo.toml").write_text(manifest, encoding="utf-8")
        (stage / "Cargo.toml").write_text(
            '[workspace]\nmembers = ["crates/velocity-mcp-core", "crates/velocity-mcp-edge"]\n'
            'resolver = "2"\n\n[profile.release]\nopt-level = 2\npanic = "abort"\n', encoding="utf-8")
        (stage / ".cargo").mkdir()
        (stage / ".cargo" / "config.toml").write_text(
            '[source.crates-io]\nreplace-with = "wasix"\n\n[source.wasix]\n'
            'registry = "sparse+https://cargo-registry.wasix.org/"\n\n[http]\nmultiplexing = false\n',
            encoding="utf-8")
        shutil.copyfile(repo / "deploy" / "edge-wasix.lock", stage / "Cargo.lock")
        env = os.environ.copy()
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        env.update(
            CARGO_TARGET_DIR=str(target),
            RUSTFLAGS="-C opt-level=2 -C target-feature=+atomics,+bulk-memory,+mutable-globals,+reference-types,+multivalue -C panic=abort",
            CARGO_HTTP_MULTIPLEXING="false", CARGO_HTTP_TIMEOUT="30",
            CC_wasm32_wasmer_wasi=r"C:/wasi-sdk/bin/clang.exe --target=wasm32-wasmer-wasi",
            CXX_wasm32_wasmer_wasi=r"C:/wasi-sdk/bin/clang++.exe --target=wasm32-wasmer-wasi",
            AR_wasm32_wasmer_wasi=r"C:/wasi-sdk/bin/llvm-ar.exe",
            RANLIB_wasm32_wasmer_wasi=r"C:/wasi-sdk/bin/llvm-ranlib.exe",
            CFLAGS_wasm32_wasmer_wasi="--sysroot=C:/wasi-sdk/share/wasi-sysroot -matomics -mbulk-memory")
        r = subprocess.run(
            ["cargo", "wasix", "+wasix", "build", "--release", "-p", "velocity-mcp-edge", "--bin", "velocity-edge"],
            cwd=stage, env=env)
        if r.returncode != 0:
            return r.returncode
        final.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(target / artifact, final)
    print("BUILT:", final)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
