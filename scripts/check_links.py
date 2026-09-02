"""Check that every relative link in the docs resolves to an actual file."""
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
MD_FILES = list(REPO.glob("*.md")) + list(REPO.glob("docs/*.md"))

LINK_RE = re.compile(r"\]\(([^)]+)\)")
ANCHOR_RE = re.compile(r"#.*$")

skip_prefixes = ("http://", "https://", "mailto:", "#")

broken = []
total = 0

for md in MD_FILES:
    text = md.read_text(encoding="utf-8", errors="ignore")
    for raw in LINK_RE.findall(text):
        target = raw.split(")", 1)[0]  # handle [text](file "title")
        if any(target.startswith(p) for p in skip_prefixes):
            continue
        if target.startswith("<"):
            continue
        total += 1
        # Strip anchor fragment
        path_part = ANCHOR_RE.sub("", target).strip()
        if not path_part:
            continue
        resolved = (md.parent / path_part).resolve()
        if not resolved.exists():
            broken.append((md.relative_to(REPO), target))

if broken:
    print(f"BROKEN LINKS: {len(broken)}/{total}")
    for src, tgt in broken:
        print(f"  {src}: {tgt}")
    sys.exit(1)
else:
    print(f"All {total} relative links resolve OK across {len(MD_FILES)} files.")
