"""Audit markdown table structure: every row of a pipe table must have the header's cell count.

The citation census checks that an anchor points at a line that exists; it says nothing about
whether the line is still a well-formed row. A merged row -- one row's text running into the next
because the newline between them was lost -- keeps every citation it carries resolvable and renders
as garbage. Both directions are printed: a cell-count mismatch is a hard failure, since GFM silently
drops the surplus cells and the reader sees a table that no longer says what the bytes say; a row
that omits its closing pipe is an advisory, since it renders but disagrees with the corpus's style.

Usage: python scripts/check_md_tables.py [file.md ...]
Exit 0 when every table reconciles, 1 when any hard finding is printed.
"""

import re
import sys
from pathlib import Path

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

REPO = Path(__file__).resolve().parent.parent
# The ownership rule the sibling doc gates use, so the corpora stay comparable: the tree minus
# vendored, generated and build directories, plus the one nested page this repository authors.
# The two-glob walk this replaces could only ever see the root and docs/, so a table in a
# package README was unchecked by construction and no green census could have shown it.
PRUNE = {".git", ".qoder", "target", "node_modules", "dist", "build", "bench_tools", ".pytest_cache"}
FENCE_RE = re.compile(r"^\s*(```|~~~)")
# A GFM delimiter cell is 1+ dashes with optional alignment colons *inside* the dashes, so
# `---`, `:--`, `--:` and `:--:` are all legal. Requiring three dashes with the colons
# outside would reject `|:--:|` and drop a real table's header, which is how the first
# version of this gate mis-advised docs/API.md's method census.
DELIM_CELL_RE = re.compile(r"^:?-+:?$")


def is_delim(line):
    s = line.strip()
    if "|" not in s:
        return False
    parts = UNESCAPED_PIPE.split(s)
    if parts and not parts[0].strip():
        parts = parts[1:]
    if parts and not parts[-1].strip():
        parts = parts[:-1]
    return bool(parts) and all(DELIM_CELL_RE.match(p.strip()) for p in parts)


UNESCAPED_PIPE = re.compile(r"(?<!\\)\|")


def cells(line):
    """Pipe-table cells, honouring the one escape GFM defines inside a row: a backslashed
    pipe is data, not a separator. A leading pipe and a trailing pipe are the row's bounds,
    so they are removed before splitting -- an empty cell between two real pipes stays a cell."""
    s = line.strip()
    parts = UNESCAPED_PIPE.split(s)
    if parts and not parts[0].strip():
        parts = parts[1:]
    closed = bool(parts) and not parts[-1].strip()
    if closed:
        parts = parts[:-1]
    return [p.replace("\\|", "|").strip() for p in parts], closed


def audit(path):
    text = path.read_bytes().decode("utf-8", errors="replace").replace("\r\n", "\n")
    lines = text.split("\n")
    hard = adv = tables = rows = 0
    i = 0
    inside_fence = False
    while i < len(lines):
        src = lines[i]
        if FENCE_RE.match(src):
            inside_fence = not inside_fence
            i += 1
            continue
        if inside_fence or not src.lstrip().startswith("|"):
            i += 1
            continue
        # A maximal run of pipe-initial lines is one candidate table.
        j = i
        while j < len(lines) and lines[j].lstrip().startswith("|") and not FENCE_RE.match(lines[j]):
            j += 1
        block = list(range(i, j))
        if len(block) >= 2 and is_delim(lines[i + 1]):
            tables += 1
            header, header_closed = cells(lines[i])
            width = len(header)
            for ln in block:
                row, closed = cells(lines[ln])
                if ln == i:
                    continue
                rows += 1
                if ln == i + 1:
                    delim = [c for c in row if c]
                    if len(row) != width:
                        print(f"{path}: line {ln + 1} delimiter row has {len(row)} columns, "
                              f"header at line {i + 1} has {width}")
                        hard += 1
                    elif not delim:
                        print(f"{path}: line {ln + 1} delimiter row has no column markers")
                        adv += 1
                    continue
                if len(row) != width:
                    print(f"{path}: line {ln + 1} has {len(row)} cells, header at line {i + 1} "
                          f"({header[0]!r}) has {width}")
                    hard += 1
                elif not closed and header_closed:
                    print(f"{path}: advisory, line {ln + 1} opens a row and does not close it "
                          f"({width} cells)")
                    adv += 1
            # A GFM table ends at the first blank line, so a non-pipe-initial line that closes
            # no pipe is never a row -- yet if it carries exactly the header's cell count it is
            # a row whose leading pipe was lost, the sibling of the merged row this gate exists
            # for. The corpus is clean at both widths (0 non-blank lines in any table's gap),
            # so this fires only on the defect.
            k = j
            while k < len(lines) and lines[k].strip() and not lines[k].lstrip().startswith("|") \
                    and not FENCE_RE.match(lines[k]):
                gap, _ = cells(lines[k])
                if "|" in lines[k] and len(gap) == width:
                    print(f"{path}: line {k + 1} continues a table row but does not open one "
                          f"({width} cells, matching the header at line {i + 1})")
                    hard += 1
                k += 1
        elif len(block) >= 2:
            print(f"{path}: advisory, lines {i + 1}-{j} are pipe-initial but line {i + 2} is not "
                  f"a delimiter row, so this is not a table")
            adv += 1
        i = j
    return hard, adv, tables, rows


def owned_nested(md):
    """The one nested page under a pruned directory that this repository authors."""
    parts = md.relative_to(REPO).parts
    return len(parts) == 3 and parts[0] == "bench_tools" and parts[2] == "README.md"


def owned_md():
    return sorted(md for md in REPO.rglob("*.md")
                  if not set(md.relative_to(REPO).parts) & PRUNE or owned_nested(md))


def main():
    targets = [Path(a) for a in sys.argv[1:]] or owned_md()
    hard = adv = tables = rows = 0
    for t in targets:
        h, a, tb, r = audit(t)
        hard += h
        adv += a
        tables += tb
        rows += r
    print(f"{len(targets)} file(s), {tables} table(s), {rows} body row(s) checked, "
          f"{hard} hard, {adv} advisory")
    if hard:
        return 1
    print("every table's rows have the header's cell count")
    return 0


if __name__ == "__main__":
    sys.exit(main())
