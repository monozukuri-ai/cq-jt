"""Keep packaged Markdown guides equal to their canonical public sources."""
import argparse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    sources = sorted((ROOT / "docs").glob("*.md"))
    target_dir = ROOT / "src/cq_jt/docs"
    expected = {p.name for p in sources}
    existing = {p.name for p in target_dir.glob("*.md")}
    if existing - expected:
        parser.error("unexpected packaged guide; review it before synchronizing")
    for source in sources:
        target = target_dir / source.name
        if args.check:
            if not target.exists() or target.read_bytes() != source.read_bytes():
                parser.error("packaged guides are stale; run python scripts/sync_public_docs.py")
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(source.read_bytes())
    if args.check:
        print(f"Public guide copies are current ({len(sources)})")


if __name__ == "__main__":
    main()
