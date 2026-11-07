"""Release-boundary controls. The synthetic native entries are never executed."""
import io
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import urllib.error
import zipfile

import pytest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import prepare_release as release

VERSION = release.check_project()
TAG = f"v{VERSION}"
SHA = "a" * 40
TAGS = {
    "linux-x86_64": "manylinux_2_35_x86_64",
    "linux-aarch64": "manylinux_2_35_aarch64",
    "windows-x86_64": "win_amd64",
    "macos-aarch64": "macosx_11_0_arm64",
    "macos-x86_64": "macosx_10_12_x86_64",
}


def metadata():
    return (f"Metadata-Version: 2.4\nName: cq-jt\nVersion: {VERSION}\n"
            "License-Expression: MIT\nDescription-Content-Type: text/markdown\n\n").encode() + (ROOT / "README.md").read_bytes()


@pytest.fixture(scope="module")
def authored_artifacts(tmp_path_factory):
    root = tmp_path_factory.mktemp("authored-release")
    source_files = {ROOT / name for name in ("Cargo.toml", "Cargo.lock", "uv.lock", "pyproject.toml", "LICENSE", "README.md", "THIRD_PARTY_NOTICES.md")}
    for folder in ("licenses", "docs", "src", "crates"):
        for path in (ROOT / folder).rglob("*"):
            if path.is_file() and (path.suffix in {".rs", ".py", ".pyi", ".md", ".txt", ".json", ".toml"} or path.name in {"LICENSE", "py.typed"}):
                if "tests" not in path.relative_to(ROOT).parts:
                    source_files.add(path)
    source = root / f"cq_jt-{VERSION}.tar.gz"
    with tarfile.open(source, "w:gz") as archive:
        data = {p.relative_to(ROOT).as_posix(): p.read_bytes() for p in source_files}
        data["PKG-INFO"] = metadata()
        for name, content in sorted(data.items()):
            info = tarfile.TarInfo(f"cq_jt-{VERSION}/{name}")
            info.size = len(content)
            archive.addfile(info, io.BytesIO(content))
    artifacts = root / "artifacts"
    for platform, tag in TAGS.items():
        folder = artifacts / f"python-distributions-{platform}"
        folder.mkdir(parents=True)
        shutil.copyfile(source, folder / source.name)
        wheel = folder / f"cq_jt-{VERSION}-cp311-abi3-{tag}.whl"
        info = f"cq_jt-{VERSION}.dist-info"
        with zipfile.ZipFile(wheel, "w", compression=zipfile.ZIP_DEFLATED) as archive:
            archive.writestr(f"{info}/METADATA", metadata())
            archive.writestr(f"{info}/WHEEL", f"Wheel-Version: 1.0\nRoot-Is-Purelib: false\nTag: cp311-abi3-{tag}\n")
            archive.writestr("cq_jt/_native" + (".pyd" if platform.startswith("windows") else ".abi3.so"), b"authored release-boundary control")
            for path in sorted(source_files):
                relative = path.relative_to(ROOT)
                if relative.parts[0] == "src":
                    archive.writestr(str(relative.relative_to("src")).replace("\\", "/"), path.read_bytes())
                elif relative.parts[0] == "licenses" or relative.name in {"LICENSE", "THIRD_PARTY_NOTICES.md"}:
                    if relative.parts[0] != "crates":
                        archive.writestr(f"{info}/licenses/" + relative.as_posix(), path.read_bytes())
    return artifacts


@pytest.fixture
def artifacts(authored_artifacts, tmp_path):
    path = tmp_path / "artifacts"
    shutil.copytree(authored_artifacts, path)
    return path


def collect(artifacts, tmp_path):
    bundle = tmp_path / "bundle"
    manifest = release.collect(artifacts, bundle, TAG, SHA)
    return bundle, manifest


def test_collect_and_verify_exact_platform_set(artifacts, tmp_path):
    bundle, manifest = collect(artifacts, tmp_path)
    assert len(manifest["files"]) == 6
    assert release.verify(bundle, TAG, SHA) == manifest
    assert {line.split("  ")[1] for line in (bundle / "SHA256SUMS").read_text().splitlines()} == {Path(name).name for name in manifest["files"]}
    source = artifacts / "python-distributions-linux-x86_64" / f"cq_jt-{VERSION}.tar.gz"
    assert manifest["files"][f"dist/{source.name}"] == release.digest(source)
    with pytest.raises(ValueError, match="manifest"):
        release.verify(bundle, TAG, "b" * 40)
    with pytest.raises(FileExistsError):
        release.collect(artifacts, bundle, TAG, SHA)


@pytest.mark.parametrize("tag", ["v0.0.0", TAG + "-extra", "release/" + TAG, TAG + "\n"])
def test_wrong_tag_is_rejected(tag):
    with pytest.raises(ValueError, match="tag must be exactly"):
        release.identity(tag, SHA)


def test_missing_and_extra_platforms_are_rejected(artifacts, tmp_path):
    folder = artifacts / "python-distributions-windows-x86_64"
    folder.rename(artifacts / "unexpected")
    with pytest.raises(ValueError, match="platform artifacts"):
        collect(artifacts, tmp_path)


def test_duplicate_and_mislabeled_wheels_are_rejected(artifacts, tmp_path):
    folder = artifacts / "python-distributions-windows-x86_64"
    wheel = next(folder.glob("*.whl"))
    extra = wheel.with_name(wheel.name.replace("cp311", "cp312"))
    shutil.copyfile(wheel, extra)
    with pytest.raises(ValueError, match="one wheel"):
        collect(artifacts, tmp_path)
    extra.unlink()
    wheel.rename(wheel.with_name(wheel.name.replace("win_amd64", "manylinux_2_35_x86_64")))
    with pytest.raises(ValueError, match="filename and WHEEL tags differ"):
        collect(artifacts, tmp_path)


def test_modified_archive_and_bundle_contents_are_rejected(artifacts, tmp_path):
    bundle, _ = collect(artifacts, tmp_path)
    wheel = next((bundle / "dist").glob("*.whl"))
    original = wheel.read_bytes()
    wheel.write_bytes(original + b"modified transfer")
    with pytest.raises(ValueError, match="manifest"):
        release.verify(bundle, TAG, SHA)
    wheel.write_bytes(original)
    (bundle / "private.txt").write_text("unexpected file")
    with pytest.raises(ValueError, match="bundle contents"):
        release.verify(bundle, TAG, SHA)


def test_checksum_and_manifest_identity_are_verified(artifacts, tmp_path):
    bundle, manifest = collect(artifacts, tmp_path)
    sums = bundle / "SHA256SUMS"
    original = sums.read_text()
    sums.write_text(("1" if original[0] == "0" else "0") + original[1:])
    with pytest.raises(ValueError, match="SHA256SUMS"):
        release.verify(bundle, TAG, SHA)
    sums.write_text(original)
    manifest["files"]["../outside"] = "0" * 64
    (bundle / "manifest.json").write_text(json.dumps(manifest))
    with pytest.raises(ValueError, match="manifest"):
        release.verify(bundle, TAG, SHA)


def test_identical_pypi_retries_are_allowed_and_conflicts_fail(artifacts, tmp_path):
    _, manifest = collect(artifacts, tmp_path)
    name, value = next(iter(manifest["files"].items()))
    published = {"info": {"name": "cq-jt", "version": VERSION}, "urls": [{"filename": Path(name).name, "digests": {"sha256": value}}]}
    release.check_pypi(manifest, lambda: None)
    release.check_pypi(manifest, lambda: published)
    published["urls"][0]["digests"]["sha256"] = "0" * 64
    with pytest.raises(ValueError, match="conflicting file"):
        release.check_pypi(manifest, lambda: published)


@pytest.mark.parametrize("status", [404, 500])
def test_pypi_network_errors_are_not_treated_as_existing_success(monkeypatch, status):
    def fail(*args, **kwargs):
        raise urllib.error.HTTPError("https://pypi.org/", status, "control", {}, None)
    monkeypatch.setattr(release.urllib.request, "urlopen", fail)
    manifest = {"version": VERSION}
    if status == 404:
        release.check_pypi(manifest)
    else:
        with pytest.raises(urllib.error.HTTPError):
            release.check_pypi(manifest)


def github_control(monkeypatch, remote):
    def run(args, **kwargs):
        assert args[1:3] == ["release", "view"]
        return subprocess.CompletedProcess(args, 0 if remote["exists"] else 1,
            json.dumps({"isDraft": remote["draft"], "assets": [{"name": n} for n in remote["assets"]]}), "")

    def output(args, **kwargs):
        if args[1] == "api":
            if remote.get("annotated") and "/git/ref/tags/" in args[2]:
                return json.dumps({"object": {"type": "tag", "sha": "c" * 40}})
            return json.dumps({"object": {"type": "commit", "sha": remote.get("sha", SHA)}})
        verb = args[2]
        if verb == "create":
            assert "--verify-tag" in args and "--draft" in args
            remote["exists"] = True
        elif verb == "download":
            name = args[args.index("--pattern") + 1]
            folder = Path(args[args.index("--dir") + 1])
            (folder / name).write_bytes(remote["assets"][name])
        elif verb == "upload":
            path = Path(args[4])
            assert path.name not in remote["assets"]
            remote["assets"][path.name] = path.read_bytes()
            remote["uploads"] += 1
        elif verb == "edit":
            assert len(remote["assets"]) == 8 and "--draft=false" in args
            remote["draft"] = False
        else:
            raise AssertionError(args)
        return ""
    monkeypatch.setattr(release.subprocess, "run", run)
    monkeypatch.setattr(release.subprocess, "check_output", output)


@pytest.mark.parametrize("partial", [False, True])
def test_github_release_retries_preserve_identical_assets(artifacts, tmp_path, monkeypatch, partial):
    bundle, manifest = collect(artifacts, tmp_path)
    assets = {"SHA256SUMS": (bundle / "SHA256SUMS").read_bytes()} if partial else {}
    remote = {"exists": partial, "draft": True, "assets": assets, "uploads": 0}
    github_control(monkeypatch, remote)
    release.publish_github(bundle, manifest, "monozukuri-ai/cq-jt")
    assert not remote["draft"] and remote["uploads"] == 8 - int(partial)
    remote["annotated"] = True
    release.publish_github(bundle, manifest, "monozukuri-ai/cq-jt")
    assert remote["uploads"] == 8 - int(partial)


def test_github_asset_conflict_aborts_before_any_upload(artifacts, tmp_path, monkeypatch):
    bundle, manifest = collect(artifacts, tmp_path)
    remote = {"exists": True, "draft": True, "assets": {"SHA256SUMS": b"conflicting"}, "uploads": 0}
    github_control(monkeypatch, remote)
    with pytest.raises(ValueError, match="conflicting GitHub Release asset"):
        release.publish_github(bundle, manifest, "monozukuri-ai/cq-jt")
    assert remote["uploads"] == 0 and remote["draft"]
    remote["sha"] = "b" * 40
    with pytest.raises(ValueError, match="qualified commit"):
        release.publish_github(bundle, manifest, "monozukuri-ai/cq-jt")
