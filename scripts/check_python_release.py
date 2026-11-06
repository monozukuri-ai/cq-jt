"""Check shared versions, ABI metadata and distribution contents without importing code."""
from __future__ import annotations

import argparse
from email.parser import BytesParser
import hashlib
import json
from pathlib import Path, PurePosixPath
import tarfile
import tomllib
import zipfile

from check_publication import OWNER_PATH

ROOT = Path(__file__).resolve().parents[1]


def check_project() -> str:
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]
    version = workspace["package"]["version"]
    if workspace["package"]["license"] != "MIT":
        raise ValueError("the project license must be MIT")
    license_text = (ROOT / "LICENSE").read_bytes()
    if not license_text.startswith(b"MIT License\n"):
        raise ValueError("the root MIT license text is missing")
    for dependency in ("jt-core", "jt-py-bridge"):
        if workspace["dependencies"][dependency]["version"] != f"={version}":
            raise ValueError(f"workspace pin differs: {dependency}")
    for member in workspace["members"]:
        manifest = tomllib.loads((ROOT / member / "Cargo.toml").read_text())
        if manifest["package"]["version"] != {"workspace": True}:
            raise ValueError(f"crate does not inherit workspace version: {member}")
        if manifest["package"]["license"] != {"workspace": True}:
            raise ValueError(f"crate does not inherit the MIT license: {member}")
        if (ROOT / member / "LICENSE").read_bytes() != license_text:
            raise ValueError(f"crate license differs from the root MIT license: {member}")
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    for package in lock["package"]:
        if package["name"] in {"jt-core", "jt-py-bridge", "cq-jt-py"} and package["version"] != version:
            raise ValueError(f"Cargo.lock version differs: {package['name']}")
    licenses = json.loads((ROOT / "licenses/manifest.json").read_text())
    locked = {(p["name"], p["version"]) for p in lock["package"] if "source" in p}
    if {(p["name"], p["version"]) for p in licenses} != locked:
        raise ValueError("license manifest differs from locked dependencies")
    for dependency in licenses:
        for name in dependency["files"]:
            if not (ROOT / "licenses" / name).is_file():
                raise ValueError(f"missing dependency license: {name}")
    project = tomllib.loads((ROOT / "pyproject.toml").read_text())["project"]
    if project["license"] != "MIT":
        raise ValueError("Python package license must be MIT")
    if project.get("dynamic") != ["version"] or "version" in project:
        raise ValueError("Python version must come from Cargo")
    if project["requires-python"] != ">=3.11":
        raise ValueError("Python floor must match the cp311 ABI contract")
    for guide in (ROOT / "docs").glob("*.md"):
        if guide.read_bytes() != (ROOT / "src/cq_jt/docs" / guide.name).read_bytes():
            raise ValueError("packaged guide is stale; run scripts/sync_public_docs.py")
    return version


def check_members(names: list[str]) -> None:
    for name in names:
        parts = PurePosixPath(name).parts
        if any(p in {".internal", "research", "corpus", "tests", "scripts", ".venv", "target"} for p in parts):
            raise ValueError(f"private/development file in distribution: {name}")
        if name.endswith((".jt", ".pdf", ".xlsx", "docs/jt-research.md")):
            raise ValueError(f"research/CAD artifact in distribution: {name}")
    for suffix in tuple(f"docs/{p.name}" for p in (ROOT / "docs").glob("*.md")) + ("/LICENSE", "THIRD_PARTY_NOTICES.md"):
        if not any(name.endswith(suffix) for name in names):
            raise ValueError(f"missing distribution content: {suffix}")
    licenses = json.loads((ROOT / "licenses/manifest.json").read_text())
    for suffix in ["licenses/manifest.json"] + ["licenses/" + f for dependency in licenses for f in dependency["files"]]:
        if not any(name.endswith(suffix) for name in names):
            raise ValueError(f"missing packaged dependency license: {suffix}")


def check_content(name: str, content: bytes) -> None:
    if OWNER_PATH.search(content):
        raise ValueError(f"personal build/source path in distribution: {name}")


def check_guide(name: str, content: bytes) -> None:
    path = PurePosixPath(name)
    if path.parent.name == "docs" and path.suffix == ".md":
        canonical = ROOT / "docs" / path.name
        if not canonical.is_file() or content != canonical.read_bytes():
            raise ValueError(f"distribution documentation differs from public source: {name}")


def check_wheel(path: Path, version: str) -> None:
    if "-cp311-abi3-" not in path.name:
        raise ValueError(f"unexpected wheel ABI tag: {path.name}")
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        check_members(names)
        for name in names:
            content = archive.read(name)
            check_content(name, content)
            check_guide(name, content)
        metadata_paths = [n for n in names if n.endswith(".dist-info/METADATA")]
        if len(metadata_paths) != 1:
            raise ValueError("expected one wheel METADATA")
        metadata = BytesParser().parsebytes(archive.read(metadata_paths[0]))
        if metadata["Name"] != "cq-jt" or metadata["Version"] != version:
            raise ValueError("wheel project/version metadata mismatch")
        if metadata["License-Expression"] != "MIT":
            raise ValueError("wheel must declare MIT licensing")
        if metadata.get_all("Requires-Dist"):
            raise ValueError("model foundation must not require runtime Python dependencies")
        wheel_info = archive.read(metadata_paths[0].replace("METADATA", "WHEEL")).decode()
        if "Tag: cp311-abi3-" not in wheel_info:
            raise ValueError("WHEEL metadata does not contain the expected ABI tag")
        native = [n for n in names if n.startswith("cq_jt/_native") and n.endswith((".so", ".pyd"))]
        if len(native) != 1:
            raise ValueError("expected exactly one native extension")
        for name in ("cq_jt/py.typed", "cq_jt/_native.pyi", "cq_jt/model.py", "cq_jt/container.py", "cq_jt/payload.py", "cq_jt/structure.py", "cq_jt/mesh.py", "cq_jt/arrays.py", "cq_jt/export.py", "cq_jt/api.py", "cq_jt/__main__.py"):
            if name not in names:
                raise ValueError(f"missing Python package file: {name}")
        for name in ("README.md", "docs/model-api.md"):
            expected = (ROOT / name).read_bytes()
            if name == "README.md":
                if expected.strip() not in archive.read(metadata_paths[0]):
                    raise ValueError("wheel long description differs from README")
            else:
                matches = [n for n in names if n.endswith(name)]
                if len(matches) != 1 or archive.read(matches[0]) != expected:
                    raise ValueError("wheel public documentation differs")


def check_sdist(path: Path, version: str) -> None:
    with tarfile.open(path) as archive:
        names = archive.getnames()
        check_members(names)
        for member in archive.getmembers():
            if member.isfile():
                content = archive.extractfile(member).read()
                check_content(member.name, content)
                check_guide(member.name, content)
            elif not member.isdir():
                raise ValueError(f"unexpected link or special file in sdist: {member.name}")
        if any(name.endswith((".so", ".pyd")) for name in names):
            raise ValueError("source distribution must not bundle a compiled extension")
        pkg_info = [n for n in names if n.count("/") == 1 and n.endswith("/PKG-INFO")]
        if len(pkg_info) != 1:
            raise ValueError("expected one sdist PKG-INFO")
        metadata = BytesParser().parsebytes(archive.extractfile(pkg_info[0]).read())
        if metadata["Name"] != "cq-jt" or metadata["Version"] != version:
            raise ValueError("sdist project/version metadata mismatch")
        if metadata["License-Expression"] != "MIT":
            raise ValueError("sdist must declare MIT licensing")
        for suffix in ("/Cargo.lock", "/uv.lock", "/pyproject.toml", "crates/jt-core/src/lib.rs", "crates/jt-core/src/container.rs", "crates/jt-core/src/compression.rs", "crates/jt-core/src/payload.rs", "crates/jt-core/src/io.rs", "crates/jt-core/src/error.rs", "crates/jt-core/src/limits.rs", "crates/jt-py-bridge/src/lib.rs", "crates/jt-py-bridge/src/inspect.rs", "crates/jt-py-bridge/src/payload.rs", "crates/cq-jt-py/src/lib.rs"):
            if not any(name.endswith(suffix) for name in names):
                raise ValueError(f"sdist is missing build input: {suffix}")
        for suffix in ("crates/jt-core/src/structure/mod.rs", "crates/jt-core/src/structure/reader.rs", "crates/jt-core/src/structure/resolve.rs", "crates/jt-core/src/structure/value.rs", "crates/jt-py-bridge/src/structure.rs", "crates/cq-jt-py/src/structure.rs"):
            if not any(name.endswith(suffix) for name in names):
                raise ValueError(f"sdist is missing structure source: {suffix}")
        for suffix in ("crates/jt-core/src/mesh/mod.rs", "crates/jt-core/src/mesh/codec.rs", "crates/jt-core/src/mesh/hash.rs", "crates/jt-core/src/mesh/reader.rs", "crates/jt-core/src/mesh/topology.rs", "crates/jt-core/src/mesh/scene.rs", "crates/jt-core/src/mesh/resolver.rs", "crates/jt-py-bridge/src/mesh.rs", "crates/cq-jt-py/src/mesh.rs"):
            if not any(name.endswith(suffix) for name in names):
                raise ValueError(f"sdist is missing mesh source: {suffix}")
        for suffix in ("crates/jt-core/src/export/mod.rs", "crates/jt-core/src/export/glb.rs", "crates/jt-core/src/export/geometry.rs", "crates/jt-core/src/export/material.rs", "crates/jt-core/src/export/output.rs", "crates/jt-py-bridge/src/export.rs", "crates/cq-jt-py/src/export.rs"):
            if not any(name.endswith(suffix) for name in names):
                raise ValueError(f"sdist is missing export source: {suffix}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dist", type=Path, required=True)
    parser.add_argument("--wheels", type=int, default=1)
    parser.add_argument("--sdists", type=int, default=1)
    parser.add_argument("--tag", help="optional candidate tag to verify; does not create or publish a tag")
    args = parser.parse_args()
    version = check_project()
    if args.tag is not None and args.tag != f"v{version}":
        parser.error("tag must match the shared workspace version")
    wheels = sorted(args.dist.glob("*.whl"))
    sources = sorted(args.dist.glob("*.tar.gz"))
    if len(wheels) != args.wheels or len(sources) != args.sdists:
        raise ValueError(f"expected {args.wheels} wheel(s), {args.sdists} sdist(s); found {len(wheels)}, {len(sources)}")
    for path in wheels:
        check_wheel(path, version)
    for path in sources:
        check_sdist(path, version)
    for path in wheels + sources:
        print(f"{path.name}: {hashlib.sha256(path.read_bytes()).hexdigest()}")
    print(f"Distribution checks passed for {version}")


if __name__ == "__main__":
    main()
