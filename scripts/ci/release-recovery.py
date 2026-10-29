#!/usr/bin/env python3
# Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
# Licensed under the GNU Lesser General Public License Version 3.0.
# See https://www.gnu.org/licenses/lgpl-3.0.en.html for license details.
"""
Bind recovered release distributions to their original build and publication run.
"""

# The release job supplies PATH; fixed gh argument lists never invoke a shell
# ruff: noqa: S603, S607
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request
from http import HTTPStatus
from pathlib import Path
from typing import Any


PREDICATE_TYPE = "https://nautilustrader.io/attestations/release-recovery/v1"
PAGE_SIZE = 100


def _main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("prepare", "stage", "download", "verify"))
    parser.add_argument("path", type=Path)
    parser.add_argument("source", nargs="?")
    args = parser.parse_args()
    try:
        if args.command == "prepare":
            _prepare(args.path)
        elif args.command == "stage":
            _stage(args.path)
        elif args.command == "download":
            _download(args.path, _positive_integer(args.source))
        else:
            if not args.source:
                sys.stderr.write("Source run metadata is required\n")
                return 1
            _verify(args.path, Path(args.source))
    except (ValueError, OSError, KeyError, subprocess.CalledProcessError) as e:
        sys.stderr.write(f"Release recovery failed: {e}\n")
        return 1
    return 0


# Keep the signed inventory next to the checks that establish its original build
def _prepare(work: Path) -> None:  # noqa: C901
    source = _read(work / "source/source-run.json")
    repository = os.environ["GITHUB_REPOSITORY"]
    recovery_sha = os.environ["RELEASE_RECOVERY_SHA"]
    if not re.fullmatch(r"[0-9a-f]{40}", recovery_sha):
        raise ValueError("Invalid recovery commit")
    if (
        os.environ["GITHUB_WORKFLOW_REF"]
        != f"{repository}/.github/workflows/build.yml@refs/heads/master"
    ):
        raise ValueError("Publication recovery must run from build.yml on master")
    if os.environ["GITHUB_EVENT_NAME"] != "workflow_dispatch":
        raise ValueError("Publication recovery must be manually dispatched")

    originals = {}
    for path in (work / "source-artifacts").rglob("*"):
        if _distribution(path):
            digest = _digest(path)
            if path.name in originals and originals[path.name] != digest:
                raise ValueError(f"Conflicting original distribution: {path.name}")
            originals[path.name] = digest
    packages = work / "distributions"
    packages.mkdir()
    inventory = []
    for path in sorted((work / "assets").iterdir()):
        if not _distribution(path):
            continue
        digest = _digest(path)
        if originals.get(path.name) != digest:
            raise ValueError(f"Missing or mismatched original-run artifact: {path.name}")
        remote = work / "remote" / path.name
        if not remote.is_file() or _digest(remote) != digest:
            raise ValueError(f"Missing or mismatched draft release asset: {path.name}")
        shutil.copyfile(path, packages / path.name)
        inventory.append({"name": path.name, "sha256": digest})
    if not inventory:
        raise ValueError("No original distributions available")
    predicate = {
        "repository": repository,
        "tag": os.environ["TAG_NAME"],
        "source": {
            "sha": source["head_sha"],
            "run": source["id"],
            "attempt": source["run_attempt"],
        },
        "recovery": {
            "sha": recovery_sha,
            "run": _positive_integer(os.environ["GITHUB_RUN_ID"]),
            "attempt": _positive_integer(os.environ["GITHUB_RUN_ATTEMPT"]),
        },
        "artifacts": inventory,
    }
    predicate_path = work / "recovery-predicate.json"
    predicate_path.write_text(json.dumps(predicate, sort_keys=True) + "\n", encoding="utf-8")
    _output("work-dir", str(work))
    _output("packages-dir", str(packages))
    _output("predicate-path", str(predicate_path))
    _output("predicate-type", PREDICATE_TYPE)


# Keep final source and registry checks together with the files staged for publication
def _stage(work: Path) -> None:  # noqa: C901
    predicate = _read(work / "recovery-predicate.json")
    repository = predicate["repository"]
    source = _github_json(f"repos/{repository}/actions/runs/{predicate['source']['run']}")
    tag = _github_json(f"repos/{repository}/commits/{predicate['tag']}")
    if not (
        source["status"] == "completed"
        and source["head_sha"] == predicate["source"]["sha"]
        and source["run_attempt"] == predicate["source"]["attempt"]
        and tag["sha"] == source["head_sha"]
    ):
        raise ValueError("Original source run or release tag changed before publication")
    release = subprocess.run(
        ["gh", "release", "view", predicate["tag"], "--repo", repository, "--json", "isDraft"],
        check=True,
        capture_output=True,
        text=True,
    )
    if json.loads(release.stdout)["isDraft"] is not True:
        raise ValueError("Publication recovery requires a draft release")
    version = predicate["tag"][1:]
    url = f"https://pypi.org/pypi/nautilus_trader/{version}/json"
    request = urllib.request.Request(url, headers={"User-Agent": "NautilusTrader release recovery"})
    try:
        with urllib.request.urlopen(request, timeout=30) as response:  # noqa: S310 (fixed HTTPS origin)
            remote = json.load(response)["urls"]
    except urllib.error.HTTPError as e:
        if e.code != HTTPStatus.NOT_FOUND:
            raise
        remote = []
    expected = {item["name"]: item["sha256"] for item in predicate["artifacts"]}
    published = {}
    for item in remote:
        name = item["filename"]
        digest = item["digests"]["sha256"]
        if name in published or expected.get(name) != digest:
            raise ValueError(f"Unexpected or mismatched PyPI distribution: {name}")
        published[name] = digest
    destination = work / "publish/dist"
    destination.mkdir(parents=True, exist_ok=True)
    if any(destination.iterdir()):
        raise ValueError("Publication staging directory must be empty")
    for name, digest in expected.items():
        path = work / "distributions" / name
        if _digest(path) != digest:
            raise ValueError(f"Distribution changed before publication: {name}")
        if name not in published:
            attestation = path.with_name(name + ".publish.attestation")
            if not attestation.is_file() or attestation.stat().st_size == 0:
                raise ValueError(f"Missing PyPI publish attestation: {name}")
            shutil.copyfile(path, destination / name)
            shutil.copyfile(attestation, destination / attestation.name)
    _output("missing", str(len(published) != len(expected)).lower())


def _download(work: Path, run_id: int) -> None:
    source = _read(work / "source/source-run.json")
    repository = source["repository"]["full_name"]
    run = _github_json(f"repos/{repository}/actions/runs/{run_id}")
    _publication_run(repository, run_id, run["run_attempt"], run["head_sha"])
    name = f"release-recovery-attestations-{run_id}-{run['run_attempt']}"
    metadata = _github_json(
        f"repos/{repository}/actions/runs/{run_id}/artifacts?per_page={PAGE_SIZE}",
    )
    if metadata["total_count"] > PAGE_SIZE:
        raise ValueError("Unexpected publication recovery artifact inventory")
    matches = [item for item in metadata["artifacts"] if item["name"] == name]
    if len(matches) != 1 or matches[0]["expired"]:
        raise ValueError("Missing or expired publication recovery attestations")
    artifact = matches[0]
    if (
        artifact["workflow_run"]["id"] != run_id
        or artifact["workflow_run"]["head_sha"] != run["head_sha"]
    ):
        raise ValueError("Publication artifact source mismatch")
    destination = work / "publication-attestations"
    subprocess.run(
        [
            "gh",
            "run",
            "download",
            str(run_id),
            "--repo",
            repository,
            "--name",
            name,
            "--dir",
            str(destination),
        ],
        check=True,
    )
    for path in destination.rglob("*"):
        if not path.is_file():
            continue
        if path.is_symlink() or not path.name.endswith((".sigstore", ".intoto.jsonl")):
            raise ValueError("Unexpected publication recovery artifact")
        target = work / "assets" / path.name
        if not target.exists():
            shutil.copyfile(path, target)


def _verify(package: Path, source_path: Path) -> None:
    source = _read(source_path)
    bundle = package.with_name(package.name + ".sigstore")
    envelope = _read(bundle)["dsseEnvelope"]
    statement = json.loads(base64.b64decode(envelope["payload"], validate=True))
    if statement["predicateType"] != PREDICATE_TYPE:
        raise ValueError("Expected a release recovery predicate")
    predicate = statement["predicate"]
    repository = source["repository"]["full_name"]
    if predicate["repository"] != repository or predicate["tag"] != os.environ["TAG_NAME"]:
        raise ValueError("Recovery predicate repository or tag mismatch")
    source_attempt = _positive_integer(predicate["source"]["attempt"])
    expected_source = {
        "sha": source["head_sha"],
        "run": source["id"],
        "attempt": source_attempt,
    }
    if predicate["source"] != expected_source:
        raise ValueError("Recovery predicate does not identify the selected original build")
    recovery = predicate["recovery"]
    if not re.fullmatch(r"[0-9a-f]{40}", recovery["sha"]):
        raise ValueError("Invalid recovery commit")
    run_id = _positive_integer(recovery["run"])
    attempt = _positive_integer(recovery["attempt"])
    selected = os.environ.get("PYPI_RECOVERY_RUN_ID", "")
    if selected and _positive_integer(selected) != run_id:
        raise ValueError("Attestation does not belong to the selected publication recovery")
    with tempfile.TemporaryDirectory() as directory:
        bundle_json = Path(directory) / "bundle.json"
        shutil.copyfile(bundle, bundle_json)
        subprocess.run(
            [
                "gh",
                "attestation",
                "verify",
                str(package),
                "--bundle",
                str(bundle_json),
                "--repo",
                repository,
                "--source-digest",
                recovery["sha"],
                "--source-ref",
                "refs/heads/master",
                "--cert-identity",
                f"https://github.com/{repository}/.github/workflows/build.yml@refs/heads/master",
                "--cert-oidc-issuer",
                "https://token.actions.githubusercontent.com",
                "--predicate-type",
                PREDICATE_TYPE,
            ],
            check=True,
        )
    original = _github_json(
        f"repos/{repository}/actions/runs/{source['id']}/attempts/{source_attempt}",
    )
    if (
        original["head_sha"] != source["head_sha"]
        or original["run_attempt"] != source_attempt
        or original["status"] != "completed"
    ):
        raise ValueError("Original build attempt identity mismatch")
    _publication_run(repository, run_id, attempt, recovery["sha"])
    inventory = predicate["artifacts"]
    names = [item["name"] for item in inventory]
    if len(names) != len(set(names)):
        raise ValueError("Duplicate recovery distribution")
    matches = [item for item in inventory if item["name"] == package.name]
    if matches != [{"name": package.name, "sha256": _digest(package)}]:
        raise ValueError("Recovery attestation artifact binding mismatch")


def _publication_run(repository: str, run_id: int, attempt: int, sha: str) -> None:
    route = f"repos/{repository}/actions/runs/{run_id}/attempts/{attempt}"
    run = _github_json(route)
    if not (
        run["repository"]["full_name"] == repository
        and run["head_repository"]["full_name"] == repository
        and run["path"] == ".github/workflows/build.yml"
        and run["event"] == "workflow_dispatch"
        and run["head_branch"] == "master"
        and run["head_sha"] == sha
        and run["run_attempt"] == attempt
        and run["status"] == "completed"
    ):
        raise ValueError("Publication recovery workflow identity mismatch")
    jobs = _github_json(f"{route}/jobs?per_page={PAGE_SIZE}")
    if jobs["total_count"] > PAGE_SIZE or not any(
        job["name"] == "recover-pypi" and job["conclusion"] == "success" for job in jobs["jobs"]
    ):
        raise ValueError("Publication recovery did not complete successfully")


def _github_json(route: str) -> dict[str, Any]:
    result = subprocess.run(["gh", "api", route], check=True, capture_output=True, text=True)
    return json.loads(result.stdout)


def _read(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def _distribution(path: Path) -> bool:
    return path.is_file() and path.name.endswith((".whl", ".tar.gz"))


def _digest(path: Path) -> str:
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"Expected a regular release artifact: {path.name}")
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def _positive_integer(value: object) -> int:
    if isinstance(value, bool) or not re.fullmatch(r"[1-9][0-9]*", str(value)):
        raise ValueError("Expected a positive run or attempt identifier")
    return int(str(value))


def _output(name: str, value: str) -> None:
    with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as stream:
        stream.write(f"{name}={value}\n")


if __name__ == "__main__":
    raise SystemExit(_main())
