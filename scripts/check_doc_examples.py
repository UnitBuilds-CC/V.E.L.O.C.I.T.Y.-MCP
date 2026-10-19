#!/usr/bin/env python3
"""Parse every fenced ```json block in this repository's own markdown and report the ones
that are not valid JSON.

Why this exists: 2026-10-11, while replaying docs/API.md's examples against a live server,
five blocks labelled ```json turned out not to be JSON at all -- three schematic responses
using a `{ ... }` elision and two `->`/`<-` wire transcripts. A reader cannot paste either
kind, and the label claims a validity the content does not have. The fix for those five was
to make them parse or to relabel the fence; this script is what keeps them from coming back.

Deliberately narrow about content: only ```json is checked. ```jsonc, ```text, ```toml and
```rust are skipped, because a labelled-with-comments or transcript block is allowed to be
unreadable by a JSON parser -- that is what its label says.

Universe, and why it changed: this used to walk the whole directory tree behind a hand-picked
skip list, which opened 337 markdown pages to gate 96 of them. 285 of the pages it read are
vendored upstream source trees, where the two censuses that did try reading them measured 111
path findings and 209 link and anchor findings that no author here can fix, and 20 more are
generated knowledge cards. So the walk is now the sibling gates' ownership rule instead of a
skip list: the pruned recursive walk the link gate uses, plus the first-level README under each
vendored tool directory, which is written here even though the tree under it is not. Generated
pages stay read and stay reported, but only as advisories -- hand-editing them would be
overwritten by the next generation.

Usage:  python scripts/check_doc_examples.py [--quiet]
Exit:   0 when every gated ```json block parses, 1 otherwise.
"""

import io
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

# Build outputs, dependency caches and the vendored tool trees. `.qoder` is not skipped, it is
# classified as generated below; `.pytest_cache` is a test artefact, not a page.
PRUNE = {".git", ".qoder", "target", "node_modules", "dist", "build", "bench_tools", ".pytest_cache"}
GENERATED = ".qoder"

OPEN_RE = re.compile(r"^```json\s*$")
CLOSE_RE = re.compile(r"^```\s*$")


def rel(path):
    return path.relative_to(ROOT).as_posix()


def owned_nested(path):
    """A vendored tool directory's own README -- `bench_tools/<tool>/README.md`, and nothing
    deeper -- is written here even though the tree under it is not."""
    parts = path.relative_to(ROOT).parts
    return len(parts) == 3 and parts[0] == "bench_tools" and parts[2] == "README.md"


def markdown_files():
    """(gated, advisory, unread) -- owned pages, generated pages, and the count of non-owned
    pages the walk does not open."""
    gated, advisory, unread = [], [], 0
    for path in ROOT.rglob("*.md"):
        parts = set(path.relative_to(ROOT).parts)
        if GENERATED in parts:
            advisory.append(path)
        elif not parts & PRUNE or owned_nested(path):
            gated.append(path)
        else:
            unread += 1
    return sorted(map(rel, gated)), sorted(map(rel, advisory)), unread


def json_blocks(rel_path):
    path = ROOT / rel_path
    try:
        lines = io.open(path, encoding="utf-8", errors="replace").read().split("\n")
    except OSError:
        return
    i = 0
    while i < len(lines):
        if OPEN_RE.match(lines[i]):
            j = i + 1
            body = []
            while j < len(lines) and not CLOSE_RE.match(lines[j]):
                body.append(lines[j])
                j += 1
            if j >= len(lines):
                yield i + 2, "\n".join(body), "unterminated fence"
            else:
                yield i + 2, "\n".join(body), None
            i = j + 1
        else:
            i += 1


def scan(files):
    total = 0
    failures = []
    for rel_path in files:
        for start_line, body, syntax_error in json_blocks(rel_path):
            total += 1
            detail = syntax_error or ""
            if not syntax_error:
                try:
                    json.loads(body)
                except ValueError as exc:
                    detail = str(exc)
            if detail:
                failures.append("%s:%d  %s" % (rel_path, start_line, detail))
    return total, failures


def main():
    quiet = "--quiet" in sys.argv[1:]
    gated_files, generated_files, unread = markdown_files()
    gated_total, failures = scan(gated_files)
    # The generated wiki is written by a knowledge-card pass, not by hand, so a bad block
    # there is reported but does not fail the gate.
    gen_total, generated = scan(generated_files)
    print("fenced json blocks: %d gated across %d owned page(s), %d advisory across %d generated "
          "page(s), %d non-owned page(s) not opened" % (gated_total, len(gated_files), gen_total,
                                                         len(generated_files), unread))
    for f in failures:
        print("  BAD %s" % f)
    for g in generated:
        print("  advisory (generated wiki, not gated) %s" % g)
    if failures:
        print("%d of %d gated json examples do not parse (%d more in generated files, advisory)"
              % (len(failures), gated_total, len(generated)))
        return 1
    if not quiet:
        print("every gated json example parses (%d advisory blocks in generated files skipped)"
              % len(generated))
    return 0


if __name__ == "__main__":
    sys.exit(main())
