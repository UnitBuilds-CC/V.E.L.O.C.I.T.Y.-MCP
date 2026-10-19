"""Audit built-in tool contracts: every parameter a tool's dispatch arm reads must be
declared in that tool's own input_schema, and every declared key should be read.

A parameter that is read but not declared is invisible to clients that build calls
from the advertised schema; a declared key nothing reads is a promise the server
does not keep. Both directions are printed, with the arm sites, for a human to judge.

Usage: python scripts/check_tool_schema_params.py [src/registry.rs ...]
Targets resolve against the repository root, not the current directory.
Exit 0 when every tool reconciles, 1 when any mismatch is printed.
"""

import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent


def close_block(lines, li, ci):
    """Given the line/col of an opening '{', return the line just past its match."""
    depth = 0
    for k in range(li, len(lines)):
        src = lines[k]
        for c in range(ci if k == li else 0, len(src)):
            if src[c] == "{":
                depth += 1
            elif src[c] == "}":
                depth -= 1
                if depth == 0:
                    return k, c
    return len(lines) - 1, len(lines[-1])


def prop_names(body):
    """Property names in a schema literal, each tagged with its `properties` nesting
    depth (0 = the tool's own top level, 1+ = inside items / additionalProperties)."""
    regions = []
    for pm in re.finditer(r'"properties":\s*\{', body):
        start = pm.end() - 1
        depth = 0
        end = None
        for k in range(start, len(body)):
            if body[k] == "{":
                depth += 1
            elif body[k] == "}":
                depth -= 1
                if depth == 0:
                    end = k
                    break
        if end is not None:
            regions.append((start + 1, end))
    names = {}
    for idx, (a, b) in enumerate(regions):
        nesting = sum(1 for j, (c, d) in enumerate(regions) if j != idx and c <= a and b <= d)
        region = body[a:b]
        depth, i = 0, 0
        while i < len(region):
            c = region[i]
            if c == "{":
                depth += 1
            elif c == "}":
                depth -= 1
            elif depth == 0 and c == '"':
                j = region.find('"', i + 1)
                if j < 0:
                    break
                key = region[i + 1 : j]
                if region[j + 1 :].lstrip().startswith(":") and re.fullmatch(r"[a-zA-Z_0-9]+", key):
                    names[key] = min(names.get(key, 99), nesting)
                i = j
            i += 1
    required = set()
    rm = re.search(r'"required":\s*\[([^\]]*)\]', body)
    if rm:
        required.update(re.findall(r'"([a-zA-Z_0-9]+)"', rm.group(1)))
    return names, required


def audit(path):
    target = Path(path)
    if not target.is_absolute():
        target = REPO / target
    label = target.relative_to(REPO).as_posix() if target.is_relative_to(REPO) else str(target)
    text = target.open(encoding="utf-8").read()
    lines = text.split("\n")
    test_start = next((i for i, s in enumerate(lines) if s.strip() == "#[cfg(test)]"), len(lines))

    # declared schemas: an `input_schema: json!({ ... })` field, named by the nearest
    # `name: "..."` above it (dynamic names, e.g. format!(), have no literal and skip).
    declared = {}
    for li, src in enumerate(lines):
        if li >= test_start:
            break
        m = re.match(r"\s*input_schema:\s*json!\(\{", src)
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
            declared[name] = prop_names("\n".join(lines[li : end_li + 1])) + (li + 1,)

    fn_spans = [
        (i + 1, fm.group(1))
        for i, s in enumerate(lines)
        for fm in [re.match(r"\s*(?:pub |async )*fn ([a-zA-Z_0-9]+)\s*[(<]", s)]
        if fm
    ]

    def fn_of(lineno):
        cur = None
        for st, nm in fn_spans:
            if st > lineno:
                break
            cur = nm
        return cur

    # dispatch arms: a quoted match label followed by a block, plus every quoted key
    # subscripted off any value inside that block.
    reads = {}
    for i, src in enumerate(lines):
        m = re.match(r'^(\s*)(?:Some\()?"([a-z_0-9]+)"(?:\))?\s*(?:=>|\|)', src)
        if not m or "=>" not in src:
            continue
        brace = src.find("{", m.end())
        if brace < 0:
            continue
        end_li, _ = close_block(lines, i, brace)
        body = "\n".join(lines[i : end_li + 1])
        keys = set()
        for km in re.finditer(r'\w+\s*\[\s*"([a-zA-Z_0-9]+)"\s*\]', body):
            keys.add(km.group(1))
        for km in re.finditer(r'\w+\s*\.\s*get\(\s*"([a-zA-Z_0-9]+)"', body):
            keys.add(km.group(2 - 1))
        reads.setdefault(m.group(2), []).append((i + 1, fn_of(i + 1), keys, i < test_start))

    bad = 0
    for name in sorted(declared):
        names, required, decl_line = declared[name]
        top = {k for k, d in names.items() if d == 0}
        occ = reads.get(name)
        if not occ:
            print(f"{label}: {name} (decl {decl_line}) declares {sorted(top)} but has no dispatch arm")
            continue
        read = set().union(*(k for _, _, k, prod in occ if prod))
        prod_sites = ", ".join(f"{ln} [{f}]" for ln, f, _, prod in occ if prod)
        unread = sorted(top - read)
        invisible = sorted(read - names.keys())
        if not unread and not invisible:
            continue
        bad += 1
        print(f"{label}: {name} (decl {decl_line}; arms {prod_sites})")
        if unread:
            req = [k for k in unread if k in required]
            print(f"    advertised, never read : {unread}"
                  + (f"  <-- also required {req}" if req else ""))
        if invisible:
            print(f"    read, never advertised   : {invisible}")
    armed = sum(1 for n in declared if any(prod for _, _, _, prod in reads.get(n, [])))
    return bad, len(declared), armed


def main():
    targets = sys.argv[1:] or ["src/registry.rs"]
    total = 0
    tools = 0
    armed = 0
    for t in targets:
        bad, n_tools, n_armed = audit(t)
        total += bad
        tools += n_tools
        armed += n_armed
    print(f"{tools} tools with a declared schema, {armed} with a production dispatch arm")
    if total:
        print(f"{total} tool(s) out of contract")
        return 1
    print("every declared parameter is read and every read parameter is declared")
    return 0


if __name__ == "__main__":
    sys.exit(main())
