"""Convert a supported JT file; no optional Python dependencies required."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

import cq_jt as jt


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--lod", choices=("highest", "lowest"), default="highest")
    parser.add_argument("--up-axis", choices=("preserve", "y", "z"), default="preserve")
    parser.add_argument("--meters-per-unit", type=float)
    parser.add_argument("--default-color", nargs=4, type=float, metavar=("R", "G", "B", "A"))
    parser.add_argument("--overwrite", action="store_true")
    args = parser.parse_args()
    options = jt.ExportOptions(
        up_axis=args.up_axis,
        meters_per_unit=args.meters_per_unit,
        default_color=tuple(args.default_color) if args.default_color is not None else None,
    )
    try:
        report = jt.convert(args.input, args.output,
                            read_options=jt.ReadOptions(lod=args.lod),
                            export_options=options, overwrite=args.overwrite)
    except (jt.JtError, OSError, ValueError) as error:
        parser.exit(1, f"{getattr(error, 'code', type(error).__name__)}: {error}\n")
    print(json.dumps(report.to_dict(), indent=2))


if __name__ == "__main__":
    main()
