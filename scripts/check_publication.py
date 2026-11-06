"""Check the public source boundary and optionally create a clean source preview."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
PUBLIC_ROOTS = {".github", "crates", "docs", "examples", "licenses", "scripts", "src", "tests"}
PUBLIC_FILES = {".gitignore", "Cargo.lock", "Cargo.toml", "LICENSE", "README.md",
                "THIRD_PARTY_NOTICES.md", "pyproject.toml", "uv.lock"}
PRIVATE_PARTS = {".internal", "research", "corpus", ".venv", "target", "dist", "__pycache__", ".aws", ".codex"}
OWNER_PATH = re.compile(rb"(?:/(?:home|Users)/[^/\s\x00]+/|[A-Za-z]:[\\/]Users[\\/][^\\/\s\x00]+[\\/])")
JAPANESE = re.compile(r"[\u3040-\u30ff\u3400-\u9fff]")


def public_files() -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT, check=True, capture_output=True,
    )
    files = sorted({Path(p.decode("utf-8")) for p in result.stdout.split(b"\0") if p})
    for path in files:
        if PRIVATE_PARTS.intersection(path.parts):
            raise ValueError(f"private or generated path in public sources: {path}")
        if path.parts[0] not in PUBLIC_ROOTS and path.as_posix() not in PUBLIC_FILES:
            raise ValueError(f"unexpected public path; review before publishing: {path}")
        absolute = ROOT / path
        if absolute.is_symlink() or not absolute.is_file():
            raise ValueError(f"public path is not a regular file: {path}")
        if path.suffix.lower() in {".jt", ".pdf", ".xlsx", ".zip", ".so", ".pyd"}:
            raise ValueError(f"acquired or generated artifact in public sources: {path}")
        if OWNER_PATH.search(absolute.read_bytes()):
            raise ValueError(f"personal absolute path in public sources: {path}")
    ignored = subprocess.run(
        ["git", "check-ignore", "--no-index", ".internal/publication-probe"],
        cwd=ROOT, capture_output=True,
    )
    if ignored.returncode != 0:
        raise ValueError("the private directory must be excluded by Git")
    return files


def check_docs(files: list[Path]) -> None:
    selected = set(files)
    for path in files:
        if path.suffix != ".md":
            continue
        content = (ROOT / path).read_text(encoding="utf-8")
        if JAPANESE.search(content):
            raise ValueError(f"public documentation must default to English: {path}")
        if re.search(r"(?:\.internal/|research/|\bM[0-9]+\b)", content):
            raise ValueError(f"internal reference in public documentation: {path}")
        for target in re.findall(r"(?<!!)\[[^\]]*\]\(([^)]+)\)", content):
            target = target.strip().strip("<>")
            url = urlsplit(target)
            if url.scheme or url.netloc:
                continue
            resolved = (ROOT / path.parent / unquote(url.path)).resolve() if url.path else ROOT / path
            if not resolved.is_relative_to(ROOT):
                raise ValueError(f"documentation link escapes public sources: {path}: {target}")
            relative = resolved.relative_to(ROOT)
            if relative not in selected and not any(relative in p.parents for p in selected):
                raise ValueError(f"documentation link has no public target: {path}: {target}")
            if url.fragment and resolved.suffix == ".md":
                headings = re.findall(r"^#{1,6}\s+(.+)$", resolved.read_text(encoding="utf-8"), re.M)
                anchors = {re.sub(r"[^\w\- ]", "", h.lower()).replace(" ", "-") for h in headings}
                if unquote(url.fragment) not in anchors:
                    raise ValueError(f"documentation anchor does not exist: {path}: {target}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", type=Path, help="copy public sources to a new directory")
    parser.add_argument("--report", type=Path, help="write a file inventory with SHA-256 digests")
    args = parser.parse_args()
    files = public_files()
    check_docs(files)
    if args.stage:
        args.stage.mkdir(parents=True, exist_ok=False)
        for path in files:
            target = args.stage / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / path, target)
    if args.report:
        inventory = {p.as_posix(): hashlib.sha256((ROOT / p).read_bytes()).hexdigest() for p in files}
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(inventory, indent=2) + "\n", encoding="utf-8")
    print(f"Public source boundary and English documentation checks passed ({len(files)} files)")


if __name__ == "__main__":
    main()
