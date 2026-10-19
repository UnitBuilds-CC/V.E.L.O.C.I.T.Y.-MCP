"""Check that every relative link in the first-party docs resolves, and that every
anchor claim names a heading that exists on the page carrying it.

The walk is a prune list, not a fixed set of globs: a page under a subdirectory was
otherwise never read. Vendored and generated trees are excluded on purpose -- their
bracket spans are often source code, and reading them mints false rows.

Three link forms carry a target and all three are claims, so all three are read: an inline
span ([text](target)), an autolink in angle brackets ([text](<target with space>)), and a
reference-style definition ([label]: target) whose target the page never repeats inline. A
definition is only a definition outside a code fence -- a bracketed line inside a sample is
source code, not a link.
"""
import re
import sys
from pathlib import Path
from urllib.parse import unquote

REPO = Path(__file__).resolve().parent.parent
PRUNE = {".git", ".qoder", "target", "node_modules", "dist", "build", "bench_tools", ".pytest_cache"}
# Prune on the path relative to the repository: on the absolute parts, a directory above
# this clone that happened to be named like a prune entry would delete the whole corpus
# from the universe while still printing a confident "all links resolve".
MD_FILES = sorted(p for p in REPO.rglob("*.md") if not set(p.relative_to(REPO).parts) & PRUNE)

# Either the angle-bracket form, whose target may contain a parenthesis, or the bare form.
LINK_RE = re.compile(r"\]\((?:<([^>]*)>|([^)]*))\)")
DEF_RE = re.compile(r"^ {0,3}\[[^\]]+\]:[ \t]+(<[^>]+>|\S+)")
HEADING_RE = re.compile(r"^ {0,3}#{1,6}\s+(.*?)\s*$")
FENCE_RE = re.compile("^ {0,3}(```|~~~)")

skip_prefixes = ("http://", "https://", "mailto:")


def slug(text):
    return re.sub(r"\s+", "-", re.sub(r"[^\w\s-]", "", text.lower()))


TITLE_RE = re.compile(r'^(\S+)[ \t]+["\']')


def target_of(raw):
    """The link's target, with any angle brackets and any title removed.

    GitHub reads both [t](<File With Space.md>) and [t](file.md "Title") as links, and both
    can arrive in the same capture group, so the bracket and the title are peeled here rather
    than assumed away by the regex.
    """
    raw = raw.strip()
    if raw.startswith("<"):
        return raw[1:].partition(">")[0]
    m = TITLE_RE.match(raw)
    return m.group(1) if m else raw


_SLUGS = {}


def heading_slugs(md):
    key = str(md)
    if key in _SLUGS:
        return _SLUGS[key]
    slugs, counts, in_fence = set(), {}, False
    for line in md.read_text(encoding="utf-8", errors="ignore").splitlines():
        if FENCE_RE.match(line):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        m = HEADING_RE.match(line)
        if not m:
            continue
        base = slug(m.group(1))
        n = counts.get(base, 0)
        counts[base] = n + 1
        slugs.add(base if n == 0 else "%s-%d" % (base, n))
    _SLUGS[key] = slugs
    return slugs


def link_targets(md):
    """Every target the page claims, inline or by definition, in document order.

    A fenced line is skipped only for the definition pass: an inline span in a fence is
    still rendered as a link by GitHub when the fence is a markdown sample, and the corpus
    has no fenced inline span that is not a real claim.
    """
    text = md.read_text(encoding="utf-8", errors="ignore")
    for bracketed, bare in LINK_RE.findall(text):
        yield bracketed or bare
    in_fence = False
    for line in text.splitlines():
        if FENCE_RE.match(line):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        m = DEF_RE.match(line)
        if m:
            yield m.group(1)


broken = []
dead_anchors = []
total_links = 0
total_anchors = 0

for md in MD_FILES:
    for raw in link_targets(md):
        target = target_of(raw)
        if any(target.startswith(p) for p in skip_prefixes):
            continue
        head, _, fragment = target.partition("#")
        head = head.strip()
        fragment = fragment.strip().lower()
        if head:
            total_links += 1
            # GitHub resolves a percent-encoded path, so a page named "Docs With Space.md"
            # is reachable as Docs%20With%20Space.md; the filesystem needs the decoded form.
            resolved = (md.parent / unquote(head)).resolve()
            if not resolved.exists():
                broken.append((md, "%s  (no such file)" % target))
                continue
            page = resolved if fragment else None
        else:
            total_anchors += 1
            page = md
        if fragment and page is not None and page.is_file() and unquote(fragment) not in heading_slugs(page):
            dead_anchors.append((md, "%s  (no heading anchors to it)" % target))

if broken or dead_anchors:
    print("BROKEN LINKS: %d/%d   DEAD ANCHORS: %d/%d" % (len(broken), total_links, len(dead_anchors), total_anchors))
    for src, tgt in broken:
        print("  link   %s: %s" % (src.relative_to(REPO), tgt))
    for src, tgt in dead_anchors:
        print("  anchor %s: %s" % (src.relative_to(REPO), tgt))
    sys.exit(1)

print("All %d relative links and %d anchor claims resolve OK across %d files."
      % (total_links, total_anchors, len(MD_FILES)))
