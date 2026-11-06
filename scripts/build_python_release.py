"""Build a release wheel with local source paths remapped to neutral prefixes."""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import shlex
import subprocess
import sys


def build_environment(project: Path) -> dict[str, str]:
    environment = os.environ.copy()
    encoded = environment.get("CARGO_ENCODED_RUSTFLAGS")
    flags = encoded.split("\x1f") if encoded else shlex.split(environment.get("RUSTFLAGS", ""))
    # rustc applies the last matching prefix. Keep the most specific paths last.
    prefixes = [
        (Path.home(), "/source/user"),
        (Path(environment.get("CARGO_HOME", Path.home() / ".cargo")), "/source/cargo"),
        (project, "/source/cq-jt"),
    ]
    for path, replacement in prefixes:
        resolved = str(path.resolve())
        for spelling in dict.fromkeys((resolved, resolved.replace("\\", "/"))):
            flags.append(f"--remap-path-prefix={spelling}={replacement}")
    environment["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    environment.pop("RUSTFLAGS", None)
    return environment


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, default=Path("dist"))
    parser.add_argument("--interpreter", default=sys.executable)
    args = parser.parse_args()
    project = Path(__file__).resolve().parents[1]
    subprocess.run(
        ["maturin", "build", "--release", "--locked", "--out", str(args.out.resolve()),
         "--interpreter", args.interpreter],
        cwd=project, env=build_environment(project), check=True,
    )


if __name__ == "__main__":
    main()
