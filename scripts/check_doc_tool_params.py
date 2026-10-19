"""Audit the docs direction of built-in tool contracts: every parameter a markdown page
publishes for a tool must be declared in that tool's own input_schema, its requiredness
must agree, and every argument key in a published example call must be declared. A
documented knob the schema does not declare is a promise the server drops on the floor:
the dispatch arm never looks at it and the client is never told.

Companion to scripts/check_tool_schema_params.py, which reconciles each schema against the
dispatch arms. That gate cannot see these pages and this one cannot see the code, so
together they close schema -> code -> docs in both directions.

Two registries are read because the docs describe two products: src/registry.rs for the
server, crates/velocity-mcp-edge/src/tools.rs for the Edge deployment, and docs/edge_*.md
plus the Edge test report are checked against the latter.

Usage: python scripts/check_doc_tool_params.py
Exit 0 when every published parameter reconciles, 1 when any mismatch is printed.
"""

import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_tool_schema_params import close_block, prop_names  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
MD_FILES = sorted(REPO.glob("*.md")) + sorted(REPO.glob("docs/*.md")) + sorted(REPO.glob("examples/*.md"))

HEADING = re.compile(r"^#{2,4}\s+([a-z_0-9]+)\s*$")
PARAMS_LABEL = re.compile(r"^\s*\**Parameters:\**\s*$")
ROW = re.compile(r"^\s*\|\s*`?([A-Za-z_0-9]+)`?\s*\|\s*(Yes|No)\s*\|")

# Tool names a page may legitimately call that neither registry declares, with the reason.
# Anything outside this set is a finding, so an invented tool name gets caught.
EXTERNAL = {
    "analyze_logs_go": "declared by the plugin manifest that docs/DYNAMIC_GO_TOOLS.md itself "
                       "shows at line 116, so the page's call names its own tool",
    "edge_ping": "plugin tool that a 2026-09 Edge PoC instance registered at runtime; "
                 "WASMER_EDGE_TEST_REPORT.md records what that deployment listed",
    "nonexistent_tool": "deliberate negative example in docs/API.md's error-surface block",
    "your_tool_name": "placeholder in a request template, not a tool",
    "tool_name": "placeholder in a request template, not a tool",
}


def load_registry(rel_path, field):
    """{tool: (top-level properties, required, declaration line)} from `field: <json macro>({...})`."""
    path = REPO / rel_path
    lines = path.read_text(encoding="utf-8").split("\n")
    test_start = next((i for i, s in enumerate(lines) if s.strip() in ("#[cfg(test)]", "#[cfg(all(test, unix))]")), len(lines))
    out = {}
    for li, src in enumerate(lines):
        if li >= test_start:
            break
        m = re.match(r"\s*" + field + r":\s*(?:serde_json::)?json!\(\{", src)
        if not m:
            continue
        end_li, _ = close_block(lines, li, src.index("{", m.start()))
        name = None
        for back in range(li, max(-1, li - 10), -1):
            nm = re.search(r'\bname:\s*"([^"]+)"', lines[back])
            if nm:
                name = nm.group(1)
                break
        if name is not None:
            props, required = prop_names("\n".join(lines[li : end_li + 1]))
            out[name] = ({k for k, d in props.items() if d == 0}, set(required), li + 1)
    return out


REGISTRIES = {
    "server": load_registry("src/registry.rs", "input_schema"),
    "edge": load_registry(Path("crates/velocity-mcp-edge/src/tools.rs").as_posix(), "input_schema"),
}


def registry_for(md_path):
    """Which product a page documents, from its own path."""
    rel = md_path.relative_to(REPO).as_posix()
    if rel.startswith("docs/edge_") or rel == "WASMER_EDGE_TEST_REPORT.md":
        return "edge"
    return "server"


def objects_in(text):
    """Every string-aware balanced {...} region, as (offset, slice). Braces inside JSON
    strings do not open a region, so an escaped inner request parses as one value."""
    out = []
    i, n = 0, len(text)
    while i < n:
        if text[i] != "{":
            i += 1
            continue
        depth = 0
        in_str = False
        esc = False
        k = i
        while k < n:
            c = text[k]
            if in_str:
                if esc:
                    esc = False
                elif c == "\\":
                    esc = True
                elif c == '"':
                    in_str = False
            elif c == '"':
                in_str = True
            elif c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
                if depth == 0:
                    break
            k += 1
        if depth == 0:
            out.append((i, text[i : k + 1]))
            i = k + 1
        else:
            i += 1
    return out


def calls_of(value):
    """(tool, sorted argument keys) for every tools/call shape in a parsed value. A request
    naming another method (prompts/get, ...) is not a tool call and its subtree is not read."""
    found = []
    stack = [value]
    while stack:
        v = stack.pop()
        if isinstance(v, dict):
            method = v.get("method")
            if isinstance(method, str) and method != "tools/call":
                continue
            tool, args = v.get("name"), v.get("arguments")
            if isinstance(tool, str) and isinstance(args, dict):
                found.append((tool, sorted(args)))
            stack.extend(v.values())
        elif isinstance(v, list):
            stack.extend(v)
    return found


def check_tables(rel, lines, declared, findings):
    """Each `#### <tool>` section's **Parameters:** table against that tool's schema."""
    tables = 0
    tool = None
    for i, src in enumerate(lines):
        h = HEADING.match(src)
        if h:
            tool = h.group(1)
            continue
        if not PARAMS_LABEL.match(src) or tool not in declared:
            continue
        j = i + 1
        while j < len(lines) and not ROW.match(lines[j]):
            if HEADING.match(lines[j]) or not lines[j].strip().startswith("|") and j > i + 1 and lines[j].strip():
                break
            j += 1
        rows = []
        while j < len(lines):
            m = ROW.match(lines[j])
            if not m:
                break
            rows.append((m.group(1), m.group(2) == "Yes", j + 1))
            j += 1
        if not rows:
            continue
        tables += 1
        top, required, decl_line = declared[tool]
        names = {n for n, _, _ in rows}
        phantom = False
        for n, req, ln in rows:
            if n not in top:
                phantom = True
                findings.append(
                    f"{rel}:{ln} documents `{n}` for {tool} (schema decl at {decl_line} has {sorted(top)})"
                )
                continue
            if req != (n in required):
                findings.append(
                    f"{rel}:{ln} marks `{n}` Required={req} for {tool} but the schema requires {sorted(required)}"
                )
        for n in sorted(top - names):
            findings.append(f"{rel}:{i+1} declares `{n}` for {tool} but this page does not document it")
    return tables


def main():
    findings = []
    tables = 0
    examples = 0
    external = {}
    unparsed = 0

    for path in MD_FILES:
        rel = path.relative_to(REPO).as_posix()
        declared = REGISTRIES[registry_for(path)]
        text = path.read_text(encoding="utf-8", errors="replace")
        lines = text.splitlines()
        tables += check_tables(rel, lines, declared, findings)

        for off, blob in objects_in(text):
            if '"arguments"' not in blob:
                continue
            try:
                value = json.loads(blob)
            except Exception:
                unparsed += 1
                continue
            line = text[:off].count("\n") + 1
            for tool, keys in calls_of(value):
                if tool not in declared:
                    if tool in EXTERNAL:
                        external[tool] = external.get(tool, 0) + 1
                    else:
                        findings.append(
                            f"{rel}:{line} example calls {tool!r}, which the {registry_for(path)} registry does not declare"
                        )
                    continue
                top, _, decl_line = declared[tool]
                for k in keys:
                    if k not in top:
                        findings.append(
                            f"{rel}:{line} example passes `{k}` to {tool}, which declares {sorted(top)} (decl {decl_line})"
                        )
                examples += 1

    print(
        f"{len(REGISTRIES['server'])} server tools and {len(REGISTRIES['edge'])} Edge tools declare a schema; "
        f"{tables} parameter tables and {examples} example calls checked"
    )
    if unparsed:
        print(f"{unparsed} brace-balanced blocks carrying \"arguments\" did not parse as JSON and were skipped")
    if external:
        print("examples naming a tool outside both registries, each with a reason: "
              + ", ".join(f"{t} x{n} ({EXTERNAL[t]})" for t, n in sorted(external.items())))
    if findings:
        print(f"\n{len(findings)} finding(s):")
        for f in findings:
            print("  " + f)
        return 1
    print("every published parameter name and example argument is declared by that tool's schema")
    return 0


if __name__ == "__main__":
    sys.exit(main())
