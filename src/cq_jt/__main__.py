"""Inspect JT container framing through the public Rust-backed API."""
import argparse
from dataclasses import asdict
import json
import sys

from . import (__version__, decode_file, inspect_file, JtError, ReadLimits,
               INSPECT_JSON_SCHEMA_VERSION, PAYLOAD_JSON_SCHEMA_VERSION, read_structure_file,
               StructureOptions, read_file, ReadOptions, MeshOptions, ResolverOptions, ExportOptions, export_glb)


def decode_report(document):
    segments = []
    for segment in document.segments:
        item = asdict(segment)
        item["guid_raw_hex"] = item.pop("guid_raw").hex()
        for element in item["elements"]:
            element["type_id_raw_hex"] = element.pop("type_id").hex()
        segments.append(item)
    return {"schema_version": PAYLOAD_JSON_SCHEMA_VERSION, "outer_decoded": True,
            "element_payloads_decoded": False, "geometry_decoded": False,
            "container": document.container.to_dict(),
            "total_output_bytes": document.total_output_bytes, "segments": segments,
            "diagnostics": [asdict(d) for d in document.diagnostics]}


def nonnegative(value: str) -> int:
    result = int(value)
    if result < 0:
        raise argparse.ArgumentTypeError("limit must be nonnegative")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description="Inspect JT header, TOC and segment boundaries")
    parser.add_argument("--version", action="version", version=__version__)
    commands = parser.add_subparsers(dest="command")
    inspect = commands.add_parser("inspect", help="inspect a JT container without expanding payloads")
    inspect.add_argument("path")
    inspect.add_argument("--json", action="store_true", dest="as_json")
    inspect.add_argument("--max-input-bytes", type=nonnegative, default=ReadLimits().max_input_bytes)
    inspect.add_argument("--max-segments", type=nonnegative, default=ReadLimits().max_segments)
    decode = commands.add_parser("decode", help="expand outer streams and list opaque logical elements")
    decode.add_argument("path")
    decode.add_argument("--json", action="store_true", dest="as_json")
    for name in ReadLimits.__dataclass_fields__:
        decode.add_argument("--" + name.replace("_", "-"), type=nonnegative, default=getattr(ReadLimits(), name))
    structure = commands.add_parser("structure", help="read supported JT hierarchy, placement and LSG properties")
    structure.add_argument("path")
    structure.add_argument("--json", action="store_true", dest="as_json")
    structure.add_argument("--mode", choices=["strict", "best_effort"], default="strict")
    for name in ReadLimits.__dataclass_fields__:
        structure.add_argument("--" + name.replace("_", "-"), type=nonnegative, default=getattr(ReadLimits(), name))
    for name in StructureOptions.__dataclass_fields__:
        if name == "experimental_jt106":
            structure.add_argument("--experimental-jt106", action="store_true", help="enable the unqualified specification-only JT 10.6 profile")
        elif name != "mode":
            structure.add_argument("--" + name.replace("_", "-"), type=nonnegative, default=getattr(StructureOptions(), name))
    mesh = commands.add_parser("mesh", help="read selected supported JT meshes and report metadata")
    mesh.add_argument("path")
    mesh.add_argument("--json", action="store_true", dest="as_json")
    mesh.add_argument("--mode", choices=["strict", "best_effort"], default="strict")
    mesh.add_argument("--lod", default="highest", help="highest, lowest or nonnegative child index")
    for cls in (ReadLimits, StructureOptions, MeshOptions):
        for name in cls.__dataclass_fields__:
            if name == "experimental_jt106":
                mesh.add_argument("--experimental-jt106", action="store_true", help="enable the unqualified specification-only JT 10.6 profile")
            elif name != "mode":
                mesh.add_argument("--" + name.replace("_", "-"), type=nonnegative, default=getattr(cls(), name))
    convert = commands.add_parser("convert", help="convert the selected JT mesh scene to GLB")
    convert.add_argument("path")
    convert.add_argument("output")
    convert.add_argument("--json", action="store_true", dest="as_json")
    convert.add_argument("--mode", choices=["strict", "best_effort"], default="strict")
    convert.add_argument("--lod", default="highest")
    convert.add_argument("--allow-partial", action="store_true")
    convert.add_argument("--overwrite", action="store_true")
    convert.add_argument("--meters-per-unit", type=float)
    convert.add_argument("--default-color", type=float, nargs=4, metavar=("R", "G", "B", "A"))
    convert.add_argument("--color-space", choices=["linear", "srgb"], default="linear")
    convert.add_argument("--up-axis", choices=["preserve", "y", "z"], default="preserve")
    convert.add_argument("--max-position-error-meters", type=float, default=ExportOptions().max_position_error_meters)
    for cls in (ReadLimits, StructureOptions, MeshOptions):
        for name in cls.__dataclass_fields__:
            if name == "experimental_jt106":
                convert.add_argument("--experimental-jt106", action="store_true", help="enable the unqualified specification-only JT 10.6 profile")
            elif name != "mode":
                convert.add_argument("--" + name.replace("_", "-"), type=nonnegative, default=getattr(cls(), name))
    for name in ("max_output_bytes", "max_json_bytes", "max_vertices", "max_nodes", "max_materials", "max_mesh_variants"):
        flag = "--" + (name.replace("max_", "max_export_", 1) if name in {"max_vertices", "max_nodes"} else name).replace("_", "-")
        convert.add_argument(flag, type=nonnegative, dest="export_" + name, default=getattr(ExportOptions(), name))
    for command in (mesh,convert):
        command.add_argument("--no-resolve",action="store_true",help="do not open external JT partitions")
        command.add_argument("--resolve-root",help="allowed root directory; defaults to the input directory")
        command.add_argument("--assembly-meters-per-unit",type=float,help="supplement missing units during assembly reading")
        for name in ("max_files","max_depth","max_references","max_total_input_bytes"):
            command.add_argument("--external-"+name.replace("_","-"),dest="resolver_"+name,type=nonnegative,default=getattr(ResolverOptions(),name))
    args = parser.parse_args()
    if args.command is None:
        parser.print_help()
        return 0
    values = {name: getattr(args, name, getattr(ReadLimits(), name)) for name in ReadLimits.__dataclass_fields__}
    if any(value >= 2**(32 if name in {"max_segments", "max_elements", "max_xz_blocks"} else 64)
           for name, value in values.items()):
        parser.error("limits must fit u64 bytes and u32 counts")
    try:
        if args.command in {"mesh", "convert"}:
            lod = args.lod
            if lod not in {"highest", "lowest"}:
                try:
                    lod = int(lod)
                except ValueError:
                    parser.error("LOD must be highest, lowest or a nonnegative child index")
                if not 0 <= lod < 2**32:
                    parser.error("LOD index must fit u32")
            sopts = {name: getattr(args, name) for name in StructureOptions.__dataclass_fields__}
            mopts = {name: getattr(args, name) for name in MeshOptions.__dataclass_fields__}
            for name, value in {**sopts, **mopts}.items():
                if name != "mode" and value >= 2**(32 if name in {"max_nodes", "max_instances", "max_depth", "max_references", "max_meshes", "max_codec_depth"} else 64):
                    parser.error("mesh and structure limits exceed integer width")
            if mopts["max_codec_depth"] > 64:
                parser.error("max_codec_depth must not exceed 64")
            import math
            rlimits={name:getattr(args,"resolver_"+name) for name in ("max_files","max_depth","max_references","max_total_input_bytes")}
            if any(v>=2**(64 if n=="max_total_input_bytes" else 32) for n,v in rlimits.items()) or rlimits["max_depth"]>64:
                parser.error("external resolver limits exceed their bounds")
            if args.assembly_meters_per_unit is not None and (not math.isfinite(args.assembly_meters_per_unit) or args.assembly_meters_per_unit<=0):
                parser.error("assembly units must be finite and positive")
            resolver=ResolverOptions(enabled=not args.no_resolve,root=args.resolve_root,meters_per_unit=args.assembly_meters_per_unit,**rlimits)
            if args.command == "convert":
                import math
                if (args.meters_per_unit is not None and (not math.isfinite(args.meters_per_unit) or args.meters_per_unit <= 0)) or not math.isfinite(args.max_position_error_meters) or args.max_position_error_meters < 0:
                    parser.error("units must be positive and precision tolerance nonnegative and finite")
                if args.default_color is not None and any(not math.isfinite(v) or not 0 <= v <= 1 for v in args.default_color):
                    parser.error("default-color components must be within 0..1")
                limits = {name: getattr(args, "export_" + name) for name in ("max_output_bytes", "max_json_bytes", "max_vertices", "max_nodes", "max_materials", "max_mesh_variants")}
                if any(v >= 2**(32 if name in {"max_nodes", "max_materials", "max_mesh_variants"} else 64) for name, v in limits.items()):
                    parser.error("export limits exceed their integer width")
                options = ExportOptions(allow_partial=args.allow_partial, meters_per_unit=args.meters_per_unit,
                                        default_color=None if args.default_color is None else tuple(args.default_color),
                                        color_space=args.color_space, up_axis=args.up_axis,
                                        max_position_error_meters=args.max_position_error_meters, **limits)
            scene = read_file(args.path, limits=ReadLimits(**values), options=ReadOptions(lod, StructureOptions(**sopts), MeshOptions(**mopts),resolver))
            if args.command == "convert":
                report = export_glb(scene, args.output, options=options, overwrite=args.overwrite)
                if args.as_json:
                    print(json.dumps({"input": args.path, "output": args.output, **report.to_dict()}, ensure_ascii=False, sort_keys=True))
                else:
                    print(f"{args.output}: {report.instance_count} placements, {report.geometry_count} shared geometries, {report.placed_triangle_count} placed triangles, {report.byte_length} bytes; source_complete={report.source_complete}")
                return 0
            if args.as_json:
                print(json.dumps(scene.to_model().to_dict(), ensure_ascii=False, sort_keys=True))
            else:
                version=scene.structure.source.version
                print(f"{args.path}: JT {version.major}.{version.minor} mesh, {len(scene.meshes)} shared meshes, {len(scene.instances)} placements; complete={scene.complete}")
                for item in scene.meshes:
                    print(f"  {item.id}: {item.vertex_count} vertices, {item.triangle_count} triangles, {item.normal_count} normals")
            return 0
        if args.command == "structure":
            options = {name: getattr(args, name) for name in StructureOptions.__dataclass_fields__}
            if any(value >= 2**(64 if name == "max_string_bytes" else 32) for name, value in options.items() if name != "mode"):
                parser.error("structure limits must fit u32 counts and u64 string bytes")
            scene = read_structure_file(args.path, limits=ReadLimits(**values), options=StructureOptions(**options))
            if args.as_json:
                print(json.dumps(scene.to_model().to_dict(), ensure_ascii=False, sort_keys=True))
            else:
                version=scene.source.version
                print(f"{args.path}: JT {version.major}.{version.minor} structure, {len(scene.parts)} part definitions, {len(scene.part_instances)} part placements; complete={scene.complete}; geometry not decoded")
                for part in scene.parts:
                    print(f"  {part.id}: {part.name!r}; {len(part.shape_node_ids)} shape nodes")
            return 0
        if args.command == "decode":
            document = decode_file(args.path, limits=ReadLimits(**values))
            if args.as_json:
                print(json.dumps(decode_report(document), ensure_ascii=False, sort_keys=True))
            else:
                segments = document.segments
                print(f"{args.path}: {len(segments)} segments, {document.total_output_bytes} output bytes; geometry not decoded")
                for segment in segments:
                    print(f"  {segment.segment_index}: type {segment.segment_type}, {segment.codec}, {len(segment.elements)} logical headers")
            return 0
        info = inspect_file(args.path, limits=ReadLimits(**values))
    except (JtError, OSError) as error:
        source = getattr(error, "source", None)
        report = {"schema_version": INSPECT_JSON_SCHEMA_VERSION, "path": args.path,
                  {"inspect": "container_checks", "decode": "decode_checks", "structure": "structure_checks", "mesh": "mesh_checks", "convert": "convert_checks"}[args.command]: "failed", "payload_decoded": False,
                  "error": {"code": getattr(error, "code", "io_error"), "message": str(error),
                            "source": None if source is None else asdict(source),
                            "offset": getattr(error, "offset", None)}}
        if args.command == "convert":
            report.pop("payload_decoded")
            report["output"] = args.output
        if args.as_json:
            print(json.dumps(report, ensure_ascii=False, sort_keys=True))
        else:
            print(f"{args.path}: {report['error']['code']}: {error}", file=sys.stderr)
        return 1
    if args.as_json:
        print(json.dumps(info.to_dict(), ensure_ascii=False, sort_keys=True))
    else:
        print(f"{args.path}: JT {info.version.major}.{info.version.minor}, {info.byte_order.value} endian, {info.size_bytes} bytes")
        print(f"TOC at {info.toc_offset}; {info.segment_count} segments; framing checks passed; payloads not decoded")
        for kind, count in info.segment_type_counts.items():
            name = next(s.type_name for s in info.segments if s.segment_type == kind)
            print(f"  {kind}: {name}: {count}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
