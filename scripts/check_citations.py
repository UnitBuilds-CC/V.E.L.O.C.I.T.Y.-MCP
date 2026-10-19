"""Check that every file:line citation in the docs points at a line that exists.

Two hard classes fail the script, because both are provable breakage:
the cited file does not exist, or the cited line is past its end.
Two further classes are printed as advisories and need a human to read them:
the cited line is blank, or none of the code tokens the prose names appear on it.
A fifth class is printed and counted but never fails: an exact target listed in
ALLOWED, i.e. a citation that is not a claim about this repository. It is
exit-0-able only because that list is small, keyed on the whole cited target
rather than its path, and self-auditing -- an entry nothing cites is reported as
dead, so the list cannot quietly outlive the citation that needed it.
"""

import re
import sys
from pathlib import Path

# A cited line can contain anything the docs quote -- on 2026-10-11 a `->`/`<-` wire
# transcript being cited crashed this script on Windows, because printing the line that
# a citation names is the last thing it does and the console default was cp1252. The
# verdict must not depend on the console, so report in UTF-8 and replace what cannot show.
if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")

REPO = Path(__file__).resolve().parent.parent
PRUNE = {".git", ".qoder", "target", "node_modules", "dist", "build", "bench_tools", ".pytest_cache"}

CITATION_RE = re.compile(
    r"([A-Za-z0-9_./-]+\.(?:rs|toml|md|c|h|py|mjs|js|sh|yaml|yml|json|css|html|wasm|txt|lock|bat|ps1|go|ts|tsx|lua|rb|hpp|mk|mod|wasi|log)):(\d+)(?:(?:\.\.|-)(\d+))?"
)
BLANK_RE = re.compile(r"^[\s\x09\x0a\x0d\x20]*$")
SPAN_RE = re.compile(r"`([^`\n]+)`")
IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]{2,}")

# A citation is a claim about one line, so the prose usually names something
# expected to sit on it. Keep the shortest useful forms and drop noise:
# file paths, byte counts, version numbers and JSON members are not identifiers.
STOPWORDS = {
    "the", "and", "for", "with", "not", "from", "http", "https", "json",
    "jsonrpc", "utf", "sha", "usize", "i64", "u32", "f64", "ok", "err",
    "Some", "None", "Vec", "String", "true", "false", "pub", "fn", "let",
    "mut", "self", "match", "if", "else", "async", "await", "struct",
}

# Citations that are not claims about a file in this repository, so that
# "MISSING FILE" is true of them and means nothing. Each entry is an admission
# that this script goes quiet on a whole shape, which is why the reason is
# mandatory: an entry that cannot say why it is here is a way to hide a defect.
# Keyed on the cited target alone, never on the citing line, so editing a doc
# cannot silently re-open a closed row -- and cannot silently open a new one
# either, because a new row only passes if it names one of these exact targets.
ALLOWED = {
    "lru-0.12.5/src/lib.rs:1109":
        "crates.io vendored source, quoted as upstream evidence",
    "lru-0.18.5/src/lib.rs:1292":
        "crates.io vendored source, quoted as upstream evidence",
    "tower-http-0.6.11/src/cors/allow_origin.rs:61":
        "crates.io vendored source, quoted as upstream evidence",
    "src/win/async.c:94":
        "upstream libuv abort site, quoted and named as deliberately unresolvable",
    "http.rs:731":
        "argument of a quoted grep command, not a repo pointer",
    "transport.go:13":
        "quoted `go test` build output, not a repo pointer -- the line it names is real at "
        "sdk/go/transport.go:13, the gorilla/websocket import the missing go.sum entry is about",
    "wshttp531.log:591":
        "a probe transcript this entry deleted after the run it recorded, cited by its own line "
        "number as evidence rather than as a file in the tree",
}


def claim_tokens(md_line):
    """Code tokens the prose expects to find on the cited line, if any."""
    tokens = set()
    for span in SPAN_RE.findall(md_line):
        if span.endswith("/") or ":" in span or "/" in span:
            continue
        for word in IDENT_RE.findall(span):
            if word not in STOPWORDS:
                tokens.add(word)
    return tokens


def read_lines(path):
    try:
        return path.read_text(encoding="utf-8", errors="ignore").splitlines()
    except OSError:
        return None


def resolve(cited, from_md):
    direct = REPO / cited
    if direct.exists():
        return direct
    sibling = (from_md.parent / cited).resolve()
    if sibling.exists():
        return sibling
    return None


def owned_nested(md):
    """The one nested page under a pruned directory that this repository authors."""
    parts = md.relative_to(REPO).parts
    return len(parts) == 3 and parts[0] == "bench_tools" and parts[2] == "README.md"


# The same ownership walk the sibling doc gates use: the tree minus vendored and generated
# directories, plus the one nested README tracked here. The two-glob walk this file replaced never
# opened the seven first-party package READMEs, so a citation in one was unchecked by construction
# and no green census could have shown it.
MD_FILES = sorted(md for md in REPO.rglob("*.md")
                  if not set(md.relative_to(REPO).parts) & PRUNE or owned_nested(md))

cache = {}
missing_file = []
out_of_range = []
blank = []
mismatch = []
allowed = []
ok = 0
total, lenient = 0, []

for md in MD_FILES:
    text = read_lines(md)
    if text is None:
        continue
    for lineno, line in enumerate(text, 1):
        for cited, target, hi in CITATION_RE.findall(line):
            total += 1
            key = "%s:%s" % (cited, target)
            if key in ALLOWED:
                allowed.append((md.relative_to(REPO), lineno, key))
                continue
            path = resolve(cited, md)
            if path is None:
                missing_file.append((md.relative_to(REPO), lineno, cited, target))
                continue
            try:
                rel = str(path.relative_to(REPO)).replace("\\", "/")
            except ValueError:
                # A cited path spelled with `..` can resolve outside the tree, and a target that
                # is not in this repository is a missing-file claim, not a reason to crash the gate.
                missing_file.append((md.relative_to(REPO), lineno, cited, target))
                continue
            if rel not in cache:
                cache[rel] = read_lines(path)
            lines = cache[rel]
            n = int(target)
            # `file:0` is not a line: without the lower bound it reads lines[-1], the last row, and counts as resolving.
            if lines is None or n < 1 or n > len(lines):
                out_of_range.append(
                    (md.relative_to(REPO), lineno, rel, n,
                     0 if lines is None else len(lines))
                )
                continue
            row = lines[n - 1]
            if BLANK_RE.match(row):
                blank.append((md.relative_to(REPO), lineno, rel, n))
                continue
            if hi:
                # A range citation claims its upper endpoint as plainly as its lower one, and this gate stopped
                # after the lower, so `file:100-900` on a 120-line file read as green. M is checked for existence
                # and for being populated only -- a range closes on whatever ends the span, usually a brace, so
                # demanding this line's tokens at M would report prose shape, not drift. It never enters `ok`: one
                # citation resolves once, so the census keeps counting citations rather than lines.
                m = int(hi)
                if m < 1 or m > len(lines):
                    out_of_range.append((md.relative_to(REPO), lineno, rel, m, len(lines)))
                    continue
                if BLANK_RE.match(lines[m - 1]):
                    blank.append((md.relative_to(REPO), lineno, rel, m))
                    continue
            tokens = claim_tokens(line)
            if tokens and not any(tok in row for tok in tokens):
                mismatch.append(
                    (md.relative_to(REPO), lineno, rel, n,
                     sorted(tokens), row.strip()[:80])
                )
            else:
                ok += 1
                # A row can also resolve on an ACCIDENT: the prose names `all` and the line contains it only as
                # the tail of `call`, or names `size` and the line has it inside `usize`. Reclassifying those as
                # advisory was measured on 2026-10-14 and rejected: it demoted 11 rows, ten of which are correct
                # citations redeemed by accident and one a genuinely wrong anchor, since repaired. The accidents
                # happen because `tokens` is keyed on the whole markdown line, so a row citing six targets aims all
                # six at every span written on it. What must not happen is an accident staying invisible -- this
                # round's own insert moved a frozen CHANGELOG pointer onto a line holding `all` inside `call`, and
                # the report named it. So the verdict stands, and the way it matched is printed.
                if tokens and not any(
                        re.search(r"(?<![A-Za-z0-9])" + re.escape(tok) + r"(?![A-Za-z0-9])", row)
                        for tok in tokens):
                    lenient.append(
                        (md.relative_to(REPO), lineno, rel, n,
                         sorted((t, re.search(r"[A-Za-z0-9]*" + re.escape(t) + r"[A-Za-z0-9]*", row).group(0))
                                for t in tokens if t in row),
                         row.strip()[:80]))

hard = len(missing_file) + len(out_of_range)
print(
    "citations: %d checked, %d resolve to a populated line, "
    "%d hard failures, %d advisory, %d allowed"
    % (total, ok, hard, len(blank) + len(mismatch), len(allowed))
)

for src, line, key in allowed:
    print("  allowed  %s:%d cites %s -- %s" % (src, line, key, ALLOWED[key]))

for key in sorted(set(ALLOWED) - {k for _, _, k in allowed}):
    print("  DEAD ALLOWLIST ENTRY (nothing cites it, delete it): %s" % key)

for src, line, cited, target in missing_file:
    print("  MISSING FILE   %s:%d cites %s:%s (file not in repo)" % (src, line, cited, target))
for src, line, rel, n, length in out_of_range:
    print("  OUT OF RANGE   %s:%d cites %s:%d (file has %d lines)" % (src, line, rel, n, length))
for src, line, rel, n in blank:
    print("  advisory blank %s:%d cites %s:%d" % (src, line, rel, n))
for src, line, rel, n, tokens, row in mismatch:
    print("  advisory token %s:%d cites %s:%d" % (src, line, rel, n))
    print("      looks for: %s" % ", ".join(tokens[:8]))
    print("      line says: %s" % row)

# Printed last because it is the class a reader cannot reconstruct from the headline: these cites DO resolve, and
# they resolve on letters rather than on the thing named. A stale pointer that arrives here is indistinguishable
# from a correct one by count alone, so the count is published and every row is named.
print("  lenient (substring-only) matches: %d of %d resolving citations" % (len(lenient), ok))
for src, line, rel, n, pairs, row in lenient:
    print("  LENIENT  %s:%d cites %s:%d" % (src, line, rel, n))
    print("      named token -> word actually holding it: %s"
          % ", ".join("%s in %s" % (t, w) for t, w in pairs[:8]))
    print("      line says: %s" % row)

# ── Attribution report: verdicts unchanged, the mechanism printed ───────────
# `claim_tokens` is keyed on the whole markdown line, so on a line carrying several citations every cite is
# redeemed by every span written anywhere on it. Priced on 2026-10-14 on the tree then reading 703 resolving and
# 188 advisory: 90 of those resolving rows name something of their own that is absent from the line they cite, and
# 82 of those advisories carry no token of their own at all, while re-tiering moves only 703 -> 695 / 188 -> 196 --
# across 172 tier changes. Re-verdicting on that attribution was measured and REJECTED: the dominant shape is
# prose that sends two cites to the same file ("`symbol` at `a.rs:1` and `a.rs:9`"), where no positional rule
# can know which anchor the words meant, so demoting those rows manufactures false alarms rather than finding
# drift. What it did find is one stale anchor that the union rule kept green -- a guard cited at the line of
# its own panic site instead of its own -- repaired in the same pass. So the tiers stand, as with the
# substring-only class above, and what is published is the way each row was redeemed.
def span_identifiers(span):
    """One code span's claim, under the same skip rule and identifier class `claim_tokens` uses."""
    if span.endswith("/") or ":" in span or "/" in span:
        return set()
    return {w for w in IDENT_RE.findall(span) if w not in STOPWORDS}


sibling_redeemed, unclaimed = [], []
attrib_mismatch = 0

for md in MD_FILES:
    text = read_lines(md)
    if text is None:
        continue
    for lineno, line in enumerate(text, 1):
        cites = list(CITATION_RE.finditer(line))
        # A single citation already owns every span on its line, so there is no mechanism to report.
        if len(cites) < 2:
            continue
        union = claim_tokens(line)
        owned = [set() for _ in cites]
        for span in SPAN_RE.finditer(line):
            tokens = span_identifiers(span.group(1))
            if not tokens:
                continue
            ahead = [i for i, c in enumerate(cites) if c.start() >= span.end()]
            if ahead:
                owned[min(ahead, key=lambda i: cites[i].start() - span.end())] |= tokens
                continue
            behind = [i for i, c in enumerate(cites) if c.end() <= span.start()]
            if behind:
                owned[min(behind, key=lambda i: span.start() - cites[i].end())] |= tokens
            else:
                # A span that overlaps every citation cannot be attributed to one, so it stays with all of them.
                for own in owned:
                    own |= tokens
        # The windows must rebuild the line's own token set, or this report is measuring its own artefact.
        if union != set().union(*owned):
            attrib_mismatch += 1
        for i, cite in enumerate(cites):
            if "%s:%s" % (cite.group(1), cite.group(2)) in ALLOWED:
                continue
            path = resolve(cite.group(1), md)
            if path is None:
                continue
            try:
                rel = str(path.relative_to(REPO)).replace("\\", "/")
            except ValueError:
                continue
            if rel not in cache:
                cache[rel] = read_lines(path)
            lines = cache[rel]
            n = int(cite.group(2))
            if lines is None or n < 1 or n > len(lines):
                continue
            row = lines[n - 1]
            if BLANK_RE.match(row) or not union:
                continue
            hit = {t for t in union if t in row}
            own = owned[i]
            if not hit:
                if not own:
                    unclaimed.append((md.relative_to(REPO), lineno, rel, n, len(cites)))
                continue
            if own and not any(t in row for t in own):
                sibling_redeemed.append((md.relative_to(REPO), lineno, rel, n, sorted(own),
                                         sorted(hit - own),
                                         sum(1 for j, other in enumerate(cites)
                                             if j != i and other.group(1) == cite.group(1))))

print("  attribution: %d of %d resolving rows redeem only on a co-located citation's spans, "
      "%d of %d advisories name no token of their own"
      % (len(sibling_redeemed), ok, len(unclaimed), len(mismatch)))
if attrib_mismatch:
    print("  ATTRIBUTION SELF-CHECK BROKEN on %d lines: per-cite windows do not rebuild the line's token set"
          % attrib_mismatch)
for src, line, rel, n, own, borrowed, same in sibling_redeemed:
    print("  SIBLING-REDEEMED %s:%d cites %s:%d (same-file siblings on the line: %d)" % (src, line, rel, n, same))
    print("      its own spans name: %s | redeemed only by a neighbour's: %s"
          % (", ".join(own[:6]), ", ".join(borrowed[:6])))
for src, line, rel, n, cites in unclaimed:
    print("  UNCLAIMED  %s:%d cites %s:%d -- one of %d citations on a line whose spans all point elsewhere"
          % (src, line, rel, n, cites))

sys.exit(1 if hard else 0)
