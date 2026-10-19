"""Audit the path claims that the citation census is structurally blind to.

check_citations.py only reads a span shaped like `path:NNN`. A backtick span that names a
file with no line number is invisible to it, so a path pointing into the wrong package
passes every gate in this directory. That is not hypothetical: an entry in CHANGELOG.md
cited `tests/integration/security_tests.rs` for evidence read from
crates/velocity-mcp-edge/tests/integration/security_tests.rs, and the root crate owns a
`tests/` directory as well, so the citation reads as a real file in the wrong package.

The rule, in one sentence: a path whose first segment names a directory that exists at the
repository root is read in that directory, so it has to resolve there. Spans whose head
names nothing at the root get the abbreviations a reader can actually recover -- the crate
name of a workspace member, or a tail that matches exactly one file in the tree. A span
that fits neither is a hard finding.

Only path claims are read as paths: a span counts when its last segment carries a known
extension (file claim) or it ends in `/` (directory claim). That keeps prose slash-lists
(`DB_HOST/DB_PORT/...`) and CIDR literals out of the universe without needing a name-by-name
exemption. Directories are matched against the directory tree, files against the file tree --
`src/` and `plugins/` are true claims about directories, and treating them as file paths made
this instrument print 132 findings that were its own bug. Brace patterns are expanded and every
literal alternative must resolve, because `sdk/{python,typescript,go}/README.md` is three
promises. A wildcard claim is tested by family: `docs/*.md` names a set, so the set has to have
witnesses at that depth -- matching is segment-wise with equal depth, which is what stops a file
two levels down from satisfying a one-level pattern. A `<lang>`-style placeholder is read as a
wildcard over one segment and the translation is printed, because that is the claim its sentence
actually makes.

The shape gate has to agree with those two tiers. It originally rejected `*`, `?`, `<`, `>` and
`,`, so it dropped every glob and every multi-alternative brace span before dispatch reached them:
both branches were dead code, the brace-alternative count was structurally pinned at zero on any
input, and the example in the paragraph above was a span this gate could not see. A fixture tree
whose brace and glob cases are asserted one by one is the only thing that catches this kind of
blindness, because a real repository can be green while a tier never runs.

Spans with no separator are counted for ambiguity only: a bare `micropython.wasm` is a name,
not a path, and the one claim the gate can test is whether it is unique in the tree.

Dot-relative claims are read twice, because the two things a `.` prefix can mean are not
distinguishable from the span alone: a prose cross-reference is relative to the page that writes
it (`../docs/API.md` from `examples/README.md` means `docs/API.md`), while a command line is
relative to the directory the reader is told to stand in, which every command in these documents
takes to be the repository root. The gate therefore tries the page reading and the root reading and
is satisfied if either resolves; it does not adjudicate which the author meant, only that one of
them exists. Before this rule existed the shape gate rejected a leading `.` outright, so the
page-relative tier below could never fire on the spelling readers actually use -- and a
mis-spelled `../GETTING_STARTED.md` for a file that lives at `docs/GETTING_STARTED.md` was invisible
on every platform, which is how three broken links survived a gate whose purpose is to catch them.

Every headline figure counts claims, not printed lines, and the accounting line reports both units
beside the bucket sums. That split is itself a measurement: one brace span can name five families,
so the repository reading prints 17 wildcard lines for 13 wildcard claims, and a figure labelled
"claims" that counted lines would report a claim class nobody made. Each claim lands in exactly one
bucket, and a claim that lands in none is this instrument's own hard finding, not a silent gap.

Both gates decide what counts as a file claim from a suffix list, and a suffix missing from that
list is not a red row -- it is an unchecked claim, which no green can reveal. That is how every
`deploy/deploy-edge.bat:NNN` anchor in these documents went unread for the life of the citation
gate. `vocabulary_guard()` therefore tests the list itself from two directions: coverage against
the suffixes the tracked files actually carry, and drift against the sibling gate's list. A guard
that cannot read its sibling reports that as a finding rather than staying silent.

Usage: python scripts/check_doc_paths.py
Exit 0 when every file claim resolves and the vocabulary guard is clean, 1 otherwise.
"""

import fnmatch
import os
import posixpath
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
# The walk covers every first-party markdown this repository authors: the three top-level doc
# surfaces plus the package READMEs that ship or are pointed at from the root page
# (`sdk/*/README.md`, `client/README.md`, `benches/README.md`, `bench_tools/*/README.md`).
# Vendored source trees are out of scope by design: their docs describe upstream directories, not
# this repository, and measuring them put 111 hard findings under `bench_tools/` alone.
MD_FILES = (sorted(REPO.glob("*.md")) + sorted(REPO.glob("docs/*.md"))
            + sorted(REPO.glob("examples/*.md")) + sorted(REPO.glob("sdk/*/README.md"))
            + sorted(REPO.glob("client/README.md")) + sorted(REPO.glob("benches/README.md"))
            + sorted(REPO.glob("bench_tools/*/README.md")))

SPAN = re.compile(r"`([^`\n]+)`")
SUFFIXES = frozenset(("rs", "toml", "md", "c", "h", "py", "mjs", "js", "sh", "yaml", "yml",
                      "json", "css", "html", "wasm", "txt", "lock", "bat", "ps1", "go", "ts",
                      "tsx", "lua", "rb", "hpp", "mk", "mod", "wasi", "log"))
EXT = "(?:" + "|".join(sorted(SUFFIXES)) + ")"
CITED = re.compile(r"[A-Za-z0-9_./-]+\." + EXT + r":\d+")
FILE_TAIL = re.compile(r"\." + EXT + r"$|/$")
GLOB = re.compile(r"[*?]|<[a-z_]+>")
WILD = re.compile(r"[*?]")
PLACEHOLDER = re.compile(r"<[a-z_]+>")
BRACE = re.compile(r"\{([^{}]*)\}")
# `*`, `?`, `<`, `>` and `,` are in the class because the glob and brace tiers below exist, and a
# span has to reach them to be reported. Spaces, colons and quotes stay out: that is what keeps
# `python scripts/check_citations.py`, `https://cargo-registry.wasix.org/` and
# `{VELOCITY_AUDIT_LOG_PATH or "audit_logs"}/<session-id>.json` from being read as paths, and a
# leading `/` keeps the runtime's absolute paths (`/etc/velocity-mcp/config.toml`) out too.
# The optional leading `./` / `../` group is what lets the page-relative tier see a claim at all;
# it requires a real segment after it, so a bare `./` (a cwd marker in a command) stays out.
PATHISH = re.compile(r"^(?:\.{1,2}/)+[A-Za-z0-9_*][A-Za-z0-9_./{}*?<>,\-]*$|"
                     r"^[A-Za-z0-9_*][A-Za-z0-9_./{}*?<>,\-]*$")
DOTREL = re.compile(r"^(?:\.{1,2}/)+")
SKIP_DIRS = {"target", ".git", "node_modules", "__pycache__", ".qoder"}

# File claims that name something outside this repository, or that a historical entry
# records on purpose, each with the reason it is not a defect. Anything the scanner
# produces that is not in one of these tiers is a finding, so an invented path gets caught
# and a real one only escapes when somebody has written down why it is allowed.
ALLOWED = {
    "src/win/async.c": "libuv's own tree, named as such in the sentence carrying it, and the "
                       "line number was dropped precisely because nobody read libuv's source",
    "src/linux_seccomp.rs": "records the mis-placed scp of the round that found it; the corrected "
                            "path is named in the same sentence, so this one is the finding itself",
    "deploy/deploy/prometheus/prometheus.yml": "the doubled prefix is the compose defect being "
                                               "reported, in the sentence that says none of these exist",
    "target/wasm32-wasmer-wasi/release/velocity-edge.wasm": "a build output the page's own build "
                                                            "command produces, not a source path",
    "target/criterion/": "criterion's own result directory under the cargo target root: the page's "
                        "commands are `cargo bench` and the same sentence claims the HTML reports the "
                        "`html_reports` feature turns on, so this is build output, not a source path -- "
                        "it is absent here only because no bench run sits on this tree",
    "tests/tmp_probe_numtoken.rs": "a probe binary the entry deleted after the run it recorded",
    "bench_nda/newrun.txt": "a scratch log the entry deleted after the run it recorded",
    "bench_nda/audit_probe.json": "a scratch export the entry deleted after the run it recorded",
    "dist/": "the TypeScript package's own build output: the sentence carrying it names "
             "`sdk/typescript/package.json` and `npm run build`, so `dist/` is package-relative",
    "sdk/python/velocity_mcp_client.egg-info/": "the entry records this directory as created by "
                                               "its own probe and then removed, and names the "
                                               ".gitignore pattern added for it",
    "github/workflows/ci.yml": "the mis-resolution the entry is reporting: it quotes what the "
                               "buggy `lstrip(\"./\")` resolved to, and names the true "
                               "`.github/workflows/ci.yml` in the same sentence",
    "marketplace/": "a directory the server creates in its own working directory at runtime, not "
                    "a repository path -- `src/transport/http.rs` passes "
                    "`std::path::Path::new(\"marketplace\")` to `Marketplace::new`",
    "*.egg-info/": "quoted as the .gitignore rule that round added, with `git check-ignore -v` "
                   "as its witness, not as a directory anyone claims sits in the tree -- the "
                   "egg-info directory it ignores was created by that probe and removed in the "
                   "same sentence",
}


def tree():
    """(file paths, directory paths), repo-root relative, POSIX separators.

    Every directory is recorded before pruning so a claim about `node_modules/` is judged
    against whether that directory really exists, not against whether this gate indexes files
    inside it.
    """
    found, dirs_found = [], []
    for root, dirs, files in os.walk(str(REPO)):
        rel = (Path(root) / ".").relative_to(REPO).as_posix()
        base = "" if rel == "." else rel + "/"
        for d in dirs:
            dirs_found.append(base + d)
        dirs[:] = [d for d in dirs if d not in SKIP_DIRS]
        for f in files:
            found.append((Path(root) / f).relative_to(REPO).as_posix())
    return found, dirs_found


def expand_braces(span):
    out = [span]
    for _ in range(span.count("{")):
        nxt = []
        for cand in out:
            m = BRACE.search(cand)
            if not m:
                nxt.append(cand)
                continue
            nxt.extend(cand[:m.start()] + o + cand[m.end():] for o in m.group(1).split(","))
        out = nxt
    return out


def family(kind, needle):
    """Every tree entry the pattern names, matched segment by segment at equal depth.

    Depth equality is what makes this a test: `docs/*.md` may not be satisfied by a file two
    levels down, because a wildcard claim names one directory's contents.
    """
    segs = (needle.rstrip("/") if kind == "dir" else needle).split("/")
    pool = dir_list if kind == "dir" else files
    hits = []
    for path in pool:
        parts = path.split("/")
        if len(parts) == len(segs) and all(fnmatch.fnmatch(a, b) for a, b in zip(parts, segs)):
            hits.append(path)
    return sorted(hits)


files, dir_list = tree()
by_path = set(files)
by_dir = set(dir_list)


def dot_readings(key, kind, citing_dir):
    """(repository-relative path, tier) pairs a dot claim can mean, in the order it means them.

    `./x` is a command run from the directory the reader was told to stand in, which every command
    block here takes to be the repository root, so the root reading is tried first. `../x` is a
    prose cross-reference and is read from the page that writes it -- `../docs/API.md` inside
    `examples/README.md` is `docs/API.md`. The second reading is offered as a fallback rather than
    discarded: this instrument cannot tell a sentence from a shell line, and a claim that holds
    under either reading is a claim that holds. A reading that walks above the root is not a claim
    about this repository at all.
    """
    trail = "/" if kind == "dir" else ""
    bare = key.rstrip("/")
    out = []
    if bare.startswith("./"):
        out.append((posixpath.normpath(bare[2:]) + trail, "root"))
    page = posixpath.normpath(posixpath.join(citing_dir, bare))
    if page.startswith(".."):
        return out
    if (page + trail) not in [c for c, _ in out]:
        out.append((page + trail, "selfdir"))
    return out

bases = {}
def tail_index(paths):
    idx = {}
    for f in paths:
        parts = f.split("/")
        for i in range(len(parts)):
            idx.setdefault("/".join(parts[i:]), set()).add(f)
    return idx

for f in files:
    bases.setdefault(f.split("/")[-1], set()).add(f)
tails = tail_index(files)
dir_tails = tail_index(dir_list)
CRATES = {p.name for p in (REPO / "crates").glob("*") if p.is_dir()} if (REPO / "crates").is_dir() else set()


def classify(citing_dir, span, kind):
    """(tier, detail) for one path claim; `kind` selects the file or the directory tree.

    A directory claim carries its trailing slash in prose (`src/`) but the tree records it
    without one, so the slash is stripped before lookup. The span is reported as written.
    """
    index, tails_index = (by_path, tails) if kind == "file" else (by_dir, dir_tails)
    key = span if kind == "file" else span.rstrip("/")
    prefix = "" if citing_dir in (".", "") else citing_dir + "/"
    head = span.split("/")[0]
    if DOTREL.match(key):
        tried = []
        for cand, tier in dot_readings(key, kind, citing_dir):
            if cand in index:
                return tier, cand
            on_disk = (REPO / cand).is_dir() if kind == "dir" else (REPO / cand).is_file()
            if on_disk:
                return "disk", ("%s on disk, outside the git index (the tree walk prunes build "
                                "output)" % ("directory" if kind == "dir" else "file"))
            tried.append(cand)
        if not tried:
            return "hard", "the claim walks out of the repository, so it names no path in it"
        return "hard", "resolves under neither reading: tried %s" % ", ".join("`%s`" % c for c in tried)
    if key in index:
        return "root", ""
    if head in CRATES and ("crates/%s/%s" % (head, key)) in index:
        return "crate", "crates/%s/%s" % (head, key)
    # `velocity-mcp-edge/src/main.rs` is the other way a reader abbreviates a workspace member:
    # the crate name spelled where the `crates/` directory would be.
    if ("crates/" + key) in index:
        return "crate", "crates/" + key
    # Judge the exact claim against reality before arguing about abbreviations. The index prunes
    # build trees, so a build output the docs cite -- `bench_tools/rust_wasm/target/...wasm`,
    # 56,310 bytes -- has to resolve by stat, not by exemption.
    on_disk = (REPO / key).is_dir() if kind == "dir" else (REPO / key).is_file()
    if on_disk:
        return "disk", ("%s on disk, outside the git index (the tree walk prunes build output)"
                        % ("directory" if kind == "dir" else "file"))
    if (REPO / head).is_dir():
        return "hard", ("head `%s/` is a repository-root directory, so the claim is read there "
                        "and %s" % (head, "no such file sits under it" if kind == "file"
                                     else "no such directory exists"))
    if prefix and (prefix + key) in index:
        return "selfdir", citing_dir
    if key in tails_index and len(tails_index[key]) == 1:
        return "tail", next(iter(tails_index[key]))
    if key in tails_index:
        return "advisory", "%d %ss end with this tail: %s" % (
            len(tails_index[key]), kind, ", ".join(sorted(tails_index[key])[:3]))
    return "hard", "no %s in the tree has this path or ends with it" % kind


checked = file_claims = dir_claims = brace_ok = 0
tiers = {"root": 0, "crate": 0, "selfdir": 0, "tail": 0, "disk": 0}
# One bucket per claim. A brace span that names five families prints five lines but remains one
# claim, so the headline figures count claims and the accounting line reports both units: counting
# printed lines and labelling them "claims" overstates the family expansions (17 lines, 13 claims).
buckets = {"hard": 0, "allowed": 0, "advisory": 0, "wildcard": 0, "brace": 0}
hard, advisories, allowed, ambiguous = [], [], [], set()


def pattern(where, lineno, span, shown, kind):
    """Test one wildcard claim: the family it names has to have witnesses.

    `shown` is the alternative as written, `span` is what the document actually says (a brace
    pattern can name several families at once). A `<lang>`-style placeholder is read as a wildcard
    over one segment, which is the claim its sentence makes, and the translation is printed.
    Returns which bucket the claim lands in, so the caller can count claims rather than lines.
    """
    translated = PLACEHOLDER.sub("*", shown.rstrip("/") if kind == "dir" else shown)
    hits = family(kind, translated)
    if hits:
        advisories.append("wildcard      %s:%d `%s` -> %d %s%s match, e.g. %s%s"
                          % (where, lineno, shown, len(hits), kind,
                             "" if len(hits) == 1 else "s", ", ".join(hits[:2]),
                             "" if translated == shown else "  (read as `%s`)" % translated))
        return "wildcard"
    if span in ALLOWED:
        allowed.append((where, lineno, span))
        return "allowed"
    hard.append((where, lineno, span, "no %s in the tree matches this pattern%s"
                 % ("directory" if kind == "dir" else "file",
                    "" if translated == shown else " (read as `%s`)" % translated)))
    return "hard"



for md in MD_FILES:
    where = md.relative_to(REPO).as_posix()
    # POSIX, not str(): the tree is indexed with forward separators, and on Windows str(Path(...))
    # yields `sdk\typescript`, so a page two directories deep could never reach the page-relative
    # tier -- its claims silently fell through to the unique-tail tier instead.
    citing_dir = Path(where).parent.as_posix()
    for lineno, line in enumerate(md.read_text(encoding="utf-8", errors="replace").split("\n"), 1):
        for span in SPAN.findall(line):
            if CITED.fullmatch(span) or not PATHISH.match(span):
                continue
            if "/" not in span:
                if FILE_TAIL.search(span) and len(bases.get(span, ())) > 1:
                    ambiguous.add(span)
                continue
            if not FILE_TAIL.search(span):
                continue
            kind = "dir" if span.endswith("/") else "file"
            checked += 1
            if kind == "dir":
                dir_claims += 1
            else:
                file_claims += 1
            if DOTREL.match(span) and (BRACE.search(span) or GLOB.search(span)):
                # A dot-relative family is tested at the reading its own spelling points at; the
                # literal tiers try both readings themselves, so only the pattern test needs this.
                readings = dot_readings(span, kind, citing_dir)
                if readings:
                    span = readings[0][0]
            if BRACE.search(span):
                index = by_dir if kind == "dir" else by_path
                alternatives = expand_braces(span)
                verdicts = [pattern(where, lineno, span, alt, kind)
                            for alt in alternatives if GLOB.search(alt)]
                literal = [a for a in alternatives if not GLOB.search(a)]
                bad = [a for a in literal
                       if (a.rstrip("/") if kind == "dir" else a) not in index]
                if bad:
                    hard.append((where, lineno, span, "brace alternative resolves nowhere: " + ", ".join(bad)))
                    buckets["hard"] += 1
                elif "hard" in verdicts:
                    buckets["hard"] += 1
                elif "wildcard" in verdicts:
                    buckets["wildcard"] += 1
                    buckets["advisory"] += 1
                elif "allowed" in verdicts:
                    buckets["allowed"] += 1
                elif literal:
                    brace_ok += len(literal)
                    buckets["brace"] += 1
                continue
            if GLOB.search(span):
                verdict = pattern(where, lineno, span, span, kind)
                if verdict == "wildcard":
                    buckets["wildcard"] += 1
                    buckets["advisory"] += 1
                else:
                    buckets[verdict] += 1
                continue
            tier, detail = classify(citing_dir, span, kind)
            if tier == "root":
                tiers["root"] += 1
            elif tier == "crate":
                tiers["crate"] += 1
                buckets["advisory"] += 1
                advisories.append("crate-qualified %s:%d `%s` -> %s" % (where, lineno, span, detail))
            elif tier == "selfdir":
                tiers["selfdir"] += 1
            elif tier == "tail":
                tiers["tail"] += 1
                buckets["advisory"] += 1
                advisories.append("tail match    %s:%d `%s` -> %s" % (where, lineno, span, detail))
            elif tier == "disk":
                tiers["disk"] += 1
                buckets["advisory"] += 1
                advisories.append("build output  %s:%d `%s` -> %s" % (where, lineno, span, detail))
            elif tier == "advisory":
                buckets["advisory"] += 1
                advisories.append("ambiguous tail %s:%d `%s` -- %s" % (where, lineno, span, detail))
            elif span in ALLOWED:
                buckets["allowed"] += 1
                allowed.append((where, lineno, span))
            else:
                buckets["hard"] += 1
                hard.append((where, lineno, span, detail))

def vocabulary_guard():
    """Return one finding per way this directory's file-type list can be wrong.

    Both doc gates decide what counts by a suffix list, and those lists were duplicated until
    2026-10-14 -- the citation copy lacked `bat`, so every `deploy/deploy-edge.bat:NNN` claim in the
    docs was unchecked by construction and no green could reveal it. Two directions are tested:

    1. coverage: every suffix carried by a tracked file that has lines must be read. A new language
       in the tree therefore shows up here rather than as silent blindness;
    2. drift: check_citations.py must read the same set this gate reads, because the two universes
       are disjoint by span shape, not by file type.

    Dotfiles (`.gitignore`) are excluded because their whole name is the suffix, and line-less
    containers (images, archives, native objects, source maps) are excluded because a line number
    into one is not a claim. If the sibling's list cannot be read at all, that is itself a finding:
    a guard that quietly has no opinion is the defect this function exists to catch.
    """
    findings = []
    try:
        listing = subprocess.run(["git", "ls-files"], cwd=str(REPO), capture_output=True,
                                 text=True, errors="ignore", timeout=60)
    except (OSError, subprocess.SubprocessError):
        listing = None
    if listing is None or listing.returncode != 0:
        findings.append("vocabulary coverage unverified: `git ls-files` did not run")
    else:
        lineless = {"png", "gif", "jpg", "jpeg", "webp", "ico", "bmp", "woff", "woff2", "ttf",
                    "otf", "eot", "so", "dll", "dylib", "exe", "lib", "obj", "o", "a", "zip",
                    "gz", "tgz", "bz2", "xz", "7z", "tar", "map", "pdf", "mp3", "mp4", "wav"}
        tracked = set()
        for path in listing.stdout.splitlines():
            base = os.path.basename(path.strip())
            if not base or "." not in base or base.startswith("."):
                continue
            suffix = base.rsplit(".", 1)[1].lower()
            if suffix.isalpha() and len(suffix) <= 12 and suffix not in lineless:
                tracked.add(suffix)
        unread = sorted(tracked - SUFFIXES)
        if unread:
            findings.append("%d tracked text suffix(es) no doc gate reads: %s"
                            % (len(unread), ", ".join("." + s for s in unread)))
    sibling = REPO / "scripts" / "check_citations.py"
    other = None
    if sibling.is_file():
        text = sibling.read_text(encoding="utf-8", errors="replace")
        m = re.search(r"CITATION_RE = re\.compile\(\s*\n?\s*r\"[^\n]*\(\?:([a-z0-9|]+)\)", text)
        if m:
            other = set(m.group(1).split("|"))
    if other is None:
        findings.append("cannot read check_citations.py's suffix list, so the two gates are "
                        "uncompared -- re-point this check at wherever the list now lives")
    elif other != SUFFIXES:
        findings.append("check_citations.py reads a different file-type set: it lacks %s, this gate "
                        "lacks %s -- the two universes are split by span shape, not by suffix"
                        % (", ".join(sorted("." + s for s in SUFFIXES - other)) or "nothing",
                           ", ".join(sorted("." + s for s in other - SUFFIXES)) or "nothing"))
    return findings


accounted = (tiers["root"] + tiers["selfdir"] + buckets["advisory"] + buckets["allowed"]
             + buckets["hard"] + buckets["brace"])
if accounted != checked:
    hard.append(("scripts/check_doc_paths.py", 0, "claim accounting",
                 "internal defect: %d of %d claims land in no bucket" % (checked - accounted, checked)))
print("paths: %d path claims checked (%d file, %d directory), %d resolve at the repository "
      "root, %d hard failures, %d advisory claims, %d allowed claims, %d wildcard claims "
      "matched a family"
      % (checked, file_claims, dir_claims, tiers["root"], buckets["hard"], buckets["advisory"],
         buckets["allowed"], buckets["wildcard"]))
print("  recovered by abbreviation: %d crate-qualified, %d relative to the page naming them, "
      "%d unique tail, %d brace alternatives each resolved; %d more resolve on disk only (a build "
      "output the index prunes) -- all of these are printed as advisories, not failures"
      % (tiers["crate"], tiers["selfdir"], tiers["tail"], brace_ok, tiers["disk"]))
print("  accounting: %d claims accounted in one bucket each, against %d checked -- the buckets print "
      "%d hard lines, %d allowed lines and %d advisory lines, including %d fully resolved brace "
      "spans, because one brace span can name several families"
      % (accounted, checked, len(hard), len(allowed), len(advisories), buckets["brace"]))
print("  bare names used as labels: %d of them match more than one file in the tree (%s)"
      % (len(ambiguous), ", ".join(sorted(ambiguous)[:8]) + ("..." if len(ambiguous) > 8 else "")))

vocab = vocabulary_guard()
print("  vocabulary: this gate reads %d suffixes; the tracked-file census and check_citations.py's "
      "own list were both compared against it and produced %d finding(s)"
      % (len(SUFFIXES), len(vocab)))

for where, lineno, span in allowed:
    print("  allowed  %s:%d `%s` -- %s" % (where, lineno, span, ALLOWED[span]))
for key in sorted(set(ALLOWED) - {s for _, _, s in allowed}):
    print("  DEAD ALLOWLIST ENTRY (nothing cites it, delete it): %s" % key)
for line in advisories:
    print("  " + line)
for where, lineno, span, detail in hard:
    print("  HARD  %s:%d `%s` -- %s" % (where, lineno, span, detail))
for detail in vocab:
    print("  HARD  scripts/check_doc_paths.py:0 `vocabulary guard` -- %s" % detail)

sys.exit(1 if hard or vocab else 0)
