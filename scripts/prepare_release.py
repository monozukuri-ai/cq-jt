"""Collect, verify and publish the exact distributions qualified by the CI run."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import urllib.error
import urllib.request
import zipfile

from check_python_release import check_project, check_sdist, check_wheel

PLATFORMS = {
    "linux-x86_64": r"manylinux_2_\d+_x86_64",
    "linux-aarch64": r"manylinux_2_\d+_aarch64",
    "windows-x86_64": r"win_amd64",
    "macos-aarch64": r"macosx_\d+_\d+_arm64",
    "macos-x86_64": r"macosx_\d+_\d+_x86_64",
}


def identity(tag: str | None, source_sha: str) -> dict:
    version = check_project()
    if not re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", version):
        raise ValueError("release requires a stable x.y.z workspace version")
    expected = f"v{version}"
    if tag is not None and tag != expected:
        raise ValueError(f"release tag must be exactly {expected}")
    if not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise ValueError("source SHA must be a full commit ID")
    return {"schema_version": 1, "version": version, "tag": expected, "source_sha": source_sha}


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def regular_files(folder: Path) -> list[Path]:
    files = sorted(folder.iterdir())
    if any(not p.is_file() or p.is_symlink() for p in files):
        raise ValueError(f"expected only regular files: {folder}")
    return files


def platform_of(wheel: Path, version: str) -> str:
    prefix = f"cq_jt-{version}-cp311-abi3-"
    if not wheel.name.startswith(prefix) or not wheel.name.endswith(".whl"):
        raise ValueError(f"unexpected wheel version or ABI: {wheel.name}")
    platform_tag = wheel.name[len(prefix):-4]
    matches = [name for name, pattern in PLATFORMS.items() if re.fullmatch(pattern, platform_tag)]
    if len(matches) != 1:
        raise ValueError(f"unexpected wheel platform: {wheel.name}")
    check_wheel(wheel, version)
    with zipfile.ZipFile(wheel) as archive:
        name = next(n for n in archive.namelist() if n.endswith(".dist-info/WHEEL"))
        tags = [line[5:] for line in archive.read(name).decode().splitlines() if line.startswith("Tag: ")]
        if tags != [f"cp311-abi3-{platform_tag}"]:
            raise ValueError(f"wheel filename and WHEEL tags differ: {wheel.name}")
    return matches[0]


def distribution_files(folder: Path, version: str) -> list[Path]:
    files = regular_files(folder)
    wheels = [p for p in files if p.suffix == ".whl"]
    sources = [p for p in files if p.name == f"cq_jt-{version}.tar.gz"]
    if len(files) != 6 or len(wheels) != 5 or len(sources) != 1:
        raise ValueError("release requires exactly five wheels and one sdist")
    if {platform_of(p, version) for p in wheels} != set(PLATFORMS):
        raise ValueError("release must cover all five platforms exactly once")
    check_sdist(sources[0], version)
    return files


def checksums(files: dict[str, str]) -> str:
    return "".join(f"{value}  {Path(name).name}\n" for name, value in sorted(files.items()))


def collect(artifacts: Path, bundle: Path, tag: str, source_sha: str) -> dict:
    manifest = identity(tag, source_sha)
    folders = list(artifacts.iterdir())
    expected = {f"python-distributions-{name}" for name in PLATFORMS}
    if {p.name for p in folders} != expected or any(not p.is_dir() or p.is_symlink() for p in folders):
        raise ValueError("missing or unexpected CI platform artifacts")
    selected = []
    for platform in PLATFORMS:
        files = regular_files(artifacts / f"python-distributions-{platform}")
        wheels = [p for p in files if p.suffix == ".whl"]
        sources = [p for p in files if p.name == f"cq_jt-{manifest['version']}.tar.gz"]
        if len(files) != 2 or len(wheels) != 1 or len(sources) != 1:
            raise ValueError(f"expected one wheel and sdist in {platform} artifact")
        if platform_of(wheels[0], manifest["version"]) != platform:
            raise ValueError(f"wheel does not match its CI platform: {platform}")
        check_sdist(sources[0], manifest["version"])
        selected.append(wheels[0])
        # Keep each platform's archive separate. Never merge colliding sdist names.
        if platform == "linux-x86_64":
            selected.append(sources[0])
    bundle.mkdir(parents=True, exist_ok=False)
    output = bundle / "dist"
    output.mkdir()
    for path in selected:
        shutil.copyfile(path, output / path.name)
    files = distribution_files(output, manifest["version"])
    manifest["files"] = {f"dist/{p.name}": digest(p) for p in files}
    (bundle / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (bundle / "SHA256SUMS").write_text(checksums(manifest["files"]), encoding="utf-8")
    return manifest


def verify(bundle: Path, tag: str, source_sha: str) -> dict:
    expected = identity(tag, source_sha)
    if {p.name for p in bundle.iterdir()} != {"dist", "manifest.json", "SHA256SUMS"}:
        raise ValueError("unexpected release bundle contents")
    if any(p.is_symlink() for p in bundle.iterdir()):
        raise ValueError("release bundle must not contain symlinks")
    manifest = json.loads((bundle / "manifest.json").read_text(encoding="utf-8"))
    files = distribution_files(bundle / "dist", expected["version"])
    expected["files"] = {f"dist/{p.name}": digest(p) for p in files}
    if manifest != expected:
        raise ValueError("release manifest, source identity or distribution digests differ")
    if (bundle / "SHA256SUMS").read_text(encoding="utf-8") != checksums(expected["files"]):
        raise ValueError("SHA256SUMS differs from the qualified distributions")
    return manifest


def check_pypi(manifest: dict, fetch=None) -> None:
    """Allow retries only when existing PyPI files have identical names and hashes."""
    if fetch is None:
        def fetch():
            try:
                with urllib.request.urlopen(f"https://pypi.org/pypi/cq-jt/{manifest['version']}/json", timeout=30) as response:
                    return json.load(response)
            except urllib.error.HTTPError as error:
                if error.code == 404:
                    return None
                raise
    published = fetch()
    if published is None:
        return
    if published["info"]["name"].replace("_", "-") != "cq-jt" or published["info"]["version"] != manifest["version"]:
        raise ValueError("unexpected PyPI project or version")
    for file in published["urls"]:
        if manifest["files"].get(f"dist/{file['filename']}") != file["digests"]["sha256"]:
            raise ValueError(f"conflicting file already on PyPI: {file['filename']}")


def publish_github(bundle: Path, manifest: dict, repository: str) -> None:
    """Resume uploads without replacing any existing release asset."""
    if not re.fullmatch(r"[\w.-]+/[\w.-]+", repository):
        raise ValueError("expected an owner/repository name")
    tag = manifest["tag"]

    def gh(*args):
        return subprocess.check_output(["gh", *args], text=True)

    obj = json.loads(gh("api", f"repos/{repository}/git/ref/tags/{tag}"))["object"]
    while obj["type"] == "tag":
        obj = json.loads(gh("api", f"repos/{repository}/git/tags/{obj['sha']}"))["object"]
    if obj["type"] != "commit" or obj["sha"] != manifest["source_sha"]:
        raise ValueError("remote release tag does not identify the qualified commit")
    command = ["gh", "release", "view", tag, "--repo", repository, "--json", "isDraft,assets"]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode:
        gh("release", "create", tag, "--repo", repository, "--verify-tag", "--draft", "--title", tag, "--generate-notes")
        result = subprocess.run(command, capture_output=True, text=True, check=True)
    release = json.loads(result.stdout)
    paths = [bundle / name for name in manifest["files"]] + [bundle / "manifest.json", bundle / "SHA256SUMS"]
    expected = {p.name: p for p in paths}
    existing = {asset["name"] for asset in release["assets"]}
    if existing - expected.keys():
        raise ValueError("GitHub Release contains unexpected assets")
    # Check all existing files before adding any missing files.
    with tempfile.TemporaryDirectory(prefix="cq-jt-release-assets-") as directory:
        for name in sorted(existing):
            gh("release", "download", tag, "--repo", repository, "--pattern", name, "--dir", directory)
            if digest(Path(directory) / name) != digest(expected[name]):
                raise ValueError(f"conflicting GitHub Release asset: {name}")
    for name in sorted(expected.keys() - existing):
        gh("release", "upload", tag, str(expected[name]), "--repo", repository)
    if release["isDraft"]:
        gh("release", "edit", tag, "--repo", repository, "--draft=false")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    metadata = subparsers.add_parser("metadata")
    metadata.add_argument("--tag")
    metadata.add_argument("--github-output", type=Path)
    for name in ("collect", "verify", "check-pypi", "github"):
        sub = subparsers.add_parser(name)
        sub.add_argument("--bundle", type=Path, required=True)
        sub.add_argument("--tag", required=True)
        sub.add_argument("--source-sha", required=True)
        if name == "collect":
            sub.add_argument("--artifacts", type=Path, required=True)
        if name == "github":
            sub.add_argument("--repository", default=os.environ.get("GITHUB_REPOSITORY"), required=not os.environ.get("GITHUB_REPOSITORY"))
    args = parser.parse_args()
    if args.command == "metadata":
        sha = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
        manifest = identity(args.tag or None, sha)
        if args.github_output:
            with args.github_output.open("a", encoding="utf-8") as output:
                output.write("".join(f"{key}={manifest[key]}\n" for key in ("version", "tag", "source_sha")))
    elif args.command == "collect":
        manifest = collect(args.artifacts, args.bundle, args.tag, args.source_sha)
    else:
        manifest = verify(args.bundle, args.tag, args.source_sha)
        if args.command == "check-pypi":
            check_pypi(manifest)
        elif args.command == "github":
            publish_github(args.bundle, manifest, args.repository)
    print(json.dumps(manifest, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
