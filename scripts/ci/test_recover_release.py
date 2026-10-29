#!/usr/bin/env python3
# Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
# Licensed under the GNU Lesser General Public License Version 3.0.
# See https://www.gnu.org/licenses/lgpl-3.0.en.html for license details.
"""
Check release recovery, read-only verification, and interrupted publication.
"""

# Test assertions use literal counts and subprocess exit codes
# ruff: noqa: PLR2004
import base64
import hashlib
import io
import json
import os
import runpy
import shutil
import subprocess
import tempfile
import textwrap
import urllib.error
from collections.abc import Callable
from pathlib import Path
from typing import Any
from unittest.mock import patch


repo = Path(__file__).resolve().parents[2]
bash = shutil.which("bash")
if bash is None:
    raise RuntimeError("bash is required to run release recovery tests")
mock_source = r"""#!/usr/bin/env python3
import fnmatch
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys

root = Path(os.environ["MOCK_ROOT"])
tool = Path(sys.argv[0]).name
args = sys.argv[1:]
with (root / "calls.jsonl").open("a") as f:
    f.write(json.dumps([tool, args]) + "\n")


def option(name, default=None):
    return args[args.index(name) + 1] if name in args else default


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def state():
    return json.loads((root / "state.json").read_text())


def save(data):
    (root / "state.json").write_text(json.dumps(data))


sha = "1" * 40
repository = os.environ["GITHUB_REPOSITORY"]
if args == ["--version"]:
    print(tool + " test version")
elif tool == "git":
    assert args == ["rev-parse", "HEAD"], args
    print(os.environ.get("CHECKOUT_SHA", sha))
elif tool == "sleep":
    pass
elif tool == "cargo":
    assert args == ["metadata", "--no-deps", "--format-version=1"], args
    print(
        json.dumps(
            {
                "packages": [
                    {"name": "nautilus-core", "version": "1.2.3", "source": None, "publish": None}
                ]
            }
        )
    )
elif tool == "uv":
    assert "pypi_attestations" in args and "--provenance-file" in args, args
    assert "--isolated" in args and "--managed-python" in args and "-I" in args, args
    assert Path(option("--provenance-file")).is_file()
    if os.environ.get("FAIL_REGISTRY"):
        print("provenance mismatch: wrong publisher identity", file=sys.stderr)
        sys.exit(42)
elif tool == "curl":
    url = args[-1]
    if url == "https://rekor.sigstore.dev/api/v1/log":
        sys.exit(47 if os.environ.get("FAIL_REKOR") else 0)
    if url.endswith("/actions/runs/123/artifacts"):
        print(
            json.dumps(
                {
                    "artifacts": [
                        {"id": 1, "name": "release-sdist"},
                        {"id": 2, "name": "release-verification-123-1"},
                        {"id": 3, "name": "release-publication-123-2"},
                        {"id": 4, "name": "release-wheel-linux"},
                    ]
                }
            )
        )
        sys.exit(0)
    if "/actions/artifacts/" in url:
        assert option("-X") == "DELETE", args
        print("204")
        sys.exit(0)
    output = Path(option("--output"))
    packages = sorted((root / "original").glob("*.whl")) + sorted(
        (root / "original").glob("*.tar.gz")
    )
    if os.environ.get("MATCH_REMOTE_PYPI"):
        packages = [p for p in packages if (root / "remote" / p.name).is_file()]
    crate = root / "crate"
    if url == "https://pypi.org/pypi/nautilus_trader/json":
        data = {
            "releases": {
                "1.2.3": [
                    {
                        "filename": p.name,
                        "digests": {"sha256": digest(p)},
                        "url": "https://files.pythonhosted.org/" + p.name,
                    }
                    for p in packages
                ]
            }
        }
    elif url.startswith("https://files.pythonhosted.org/"):
        shutil.copyfile(root / "original" / url.rsplit("/", 1)[-1], output)
        sys.exit(0)
    elif "/provenance" in url:
        data = {
            "attestation_bundles": [
                {
                    "publisher": {
                        "kind": "GitHub",
                        "repository": repository,
                        "workflow": "build.yml",
                        "environment": "release",
                    }
                }
            ]
        }
    elif url == "https://crates.io/api/v1/crates/nautilus-core/versions":
        data = {
            "versions": [
                {
                    "num": "1.2.3",
                    "checksum": digest(crate),
                    "trustpub_data": {"provider": "github", "repository": repository, "sha": sha},
                    "published_by": None,
                }
            ]
        }
    elif url.startswith("https://static.crates.io/"):
        shutil.copyfile(crate, output)
        sys.exit(0)
    elif url.startswith("https://index.crates.io/"):
        data = {"vers": "1.2.3", "cksum": digest(crate)}
    else:
        raise AssertionError(url)
    output.write_text(json.dumps(data))
elif tool == "gh":
    if args[:3] == ["attestation", "verify", "--help"]:
        print("--source-digest --source-ref --bundle")
    elif args[:2] == ["attestation", "verify"]:
        assert option("--source-digest") == ("7" * 40 if "--predicate-type" in args else sha), args
        assert option("--source-ref") == "refs/heads/master", args
        assert (
            option("--cert-identity")
            == "https://github.com/" + repository + "/.github/workflows/build.yml@refs/heads/master"
        ), args
        if "--bundle" in args:
            bundle = Path(option("--bundle"))
            assert bundle.suffix in (".json", ".jsonl"), "Unsupported GitHub CLI bundle extension"
            package = Path(args[2])
            assert bundle.read_bytes() == package.with_name(package.name + ".sigstore").read_bytes()
            if os.environ.get("PUBLICATION_RECOVERY"):
                if "--predicate-type" not in args:
                    sys.exit(43)
            else:
                assert json.loads(bundle.read_text()) == {
                    "dsseEnvelope": {"payload": "signed payload", "signatures": ["signature"]}
                }
        if os.environ.get("FAIL_PROVENANCE"):
            sys.exit(43)
    elif args[0] == "api":
        route = args[1]
        if "/actions/runs/456" in route:
            if "/jobs?" in route:
                data = {"total_count": 1,
                        "jobs": [{"name": "recover-pypi", "conclusion": "success"}]}
            elif "/artifacts?" in route:
                data = {"total_count": 1, "artifacts": [{
                    "name": "release-recovery-attestations-456-3", "expired": False,
                    "workflow_run": {"id": 456, "head_sha": "7" * 40},
                }]}
            else:
                data = {"id": 456, "repository": {"full_name": repository},
                        "head_repository": {"full_name": repository},
                        "path": ".github/workflows/build.yml", "event": "workflow_dispatch",
                        "head_branch": "master", "head_sha": "7" * 40,
                        "run_attempt": 3, "status": "completed"}
            print(json.dumps(data))
        elif "/jobs?" in route:
            print(
                json.dumps(
                    {"name": "tag-release", "conclusion": os.environ.get("SOURCE_GATE", "success")}
                )
            )
        elif "/artifacts?" in route:
            if not os.environ.get("EXPIRED_ARTIFACTS"):
                print(
                    json.dumps(
                        {
                            "name": "release-sdist",
                            "expired": False,
                            "workflow_run": {"id": 123, "head_sha": sha},
                        }
                    )
                )
        elif "/commits/" in route:
            print(json.dumps({"sha": "2" * 40 if os.environ.get("WRONG_TAG") else sha}))
        elif "/actions/runs/123/attempts/" in route:
            attempt = int(route.rsplit("/", 1)[1])
            print(json.dumps({"head_sha": sha, "run_attempt": attempt, "status": "completed"}))
        else:
            assert route.endswith("/actions/runs/123"), route
            count_file = root / "source-reads"
            count = int(count_file.read_text()) + 1 if count_file.exists() else 1
            count_file.write_text(str(count))
            attempt = 2 if os.environ.get("CHANGED_SOURCE") and count > 1 else 1
            print(
                json.dumps(
                    {
                        "id": 123,
                        "repository": {"full_name": repository},
                        "head_repository": {
                            "full_name": os.environ.get("SOURCE_REPOSITORY", repository)
                        },
                        "event": os.environ.get("SOURCE_EVENT", "push"),
                        "head_branch": os.environ.get("SOURCE_BRANCH", "master"),
                        "path": os.environ.get("SOURCE_WORKFLOW", ".github/workflows/build.yml"),
                        "status": os.environ.get("SOURCE_STATUS", "completed"),
                        "head_sha": sha,
                        "run_attempt": attempt,
                    }
                )
            )
    elif args[:2] == ["run", "download"]:
        assert args[2] in ("123", "456")
        if args[2] == "456":
            assert option("--name") == "release-recovery-attestations-456-3"
        destination = Path(option("--dir"))
        destination.mkdir(parents=True, exist_ok=True)
        for p in (root / ("original" if args[2] == "123" else "publication")).iterdir():
            shutil.copyfile(p, destination / p.name)
    elif args[:2] == ["release", "view"]:
        data = state()
        if option("--json") == "body":
            print(data["body"])
        elif option("--json") == "body,isDraft":
            if os.environ.get("PUBLISH_BEFORE_NOTES"):
                data["isDraft"] = False
                save(data)
            print(json.dumps({"body": data["body"], "isDraft": data["isDraft"]}))
        elif option("--json") == "isDraft,url":
            print(str(data["isDraft"]).lower() + "\thttps://example.invalid/release")
        else:
            data["assets"] = [
                {
                    "name": p.name,
                    "state": os.environ.get("ASSET_STATE", "uploaded"),
                    "size": p.stat().st_size,
                }
                for p in sorted((root / "remote").iterdir())
            ]
            print(json.dumps(data))
    elif args[:2] == ["release", "download"]:
        destination = Path(option("--dir"))
        destination.mkdir(parents=True, exist_ok=True)
        patterns = [args[i + 1] for i, x in enumerate(args) if x == "--pattern"] or ["*"]
        for p in (root / "remote").iterdir():
            if any(fnmatch.fnmatch(p.name, pattern) for pattern in patterns):
                shutil.copyfile(p, destination / p.name)
    elif args[:2] == ["release", "upload"]:
        assert "--clobber" not in args or os.environ.get("ALLOW_CLOBBER"), args
        for value in args[3:]:
            p = Path(value)
            if not p.is_file():
                continue
            target = root / "remote" / p.name
            assert not target.exists() or "--clobber" in args, "rewriting existing asset " + p.name
            shutil.copyfile(p, target)
            if os.environ.get("FAIL_AFTER_UPLOAD") and not (root / "upload-failed").exists():
                (root / "upload-failed").touch()
                sys.exit(44)
    elif args[:2] == ["release", "edit"]:
        data = state()
        if "--notes-file" in args:
            data["body"] = Path(option("--notes-file")).read_text()
        else:
            assert "--draft=false" in args, args
            if os.environ.get("FAIL_PUBLISH"):
                sys.exit(46)
            data["isDraft"] = False
        save(data)
    elif args[:2] == ["release", "verify"]:
        if "--help" in args and os.environ.get("UNSUPPORTED_SEAL_HELP"):
            sys.exit(2)
        if "--help" in args:
            print(
                "  gh release <command>"
                if os.environ.get("PARENT_SEAL_HELP")
                else "  gh release verify [<tag>] [flags]"
            )
        if "--help" not in args:
            assert state()["isDraft"] is False
            if os.environ.get("FAIL_SEAL"):
                sys.exit(45)
    else:
        raise AssertionError(args)
else:
    raise AssertionError((tool, args))
"""


def _fixture(root: Path) -> None:
    for name in ("bin", "original", "remote", "python"):
        (root / name).mkdir(parents=True)
    (root / "state.json").write_text(
        json.dumps({"isDraft": True, "tagName": "v1.2.3", "body": "Release notes"}),
    )
    (root / "python/pyproject.toml").write_text('[project]\nversion = "1.2.3"\n')
    (root / "crate").write_bytes(b"published crate\n")
    for py in ("cp313", "cp314"):
        for platform in (
            "manylinux_2_34_x86_64",
            "manylinux_2_34_aarch64",
            "macosx_11_0_arm64",
            "win_amd64",
        ):
            name = f"nautilus_trader-1.2.3-{py}-{py}-{platform}.whl"
            (root / "original" / name).write_bytes(name.encode())
    (root / "original/nautilus_trader-1.2.3.tar.gz").write_bytes(b"published sdist\n")
    for package in list((root / "original").iterdir()):
        envelope = {"payload": "signed payload", "signatures": ["signature"]}
        package.with_name(package.name + ".sigstore").write_text(
            json.dumps({"dsseEnvelope": envelope}),
        )
        package.with_name(package.name + ".intoto.jsonl").write_text(json.dumps(envelope))
    mock = root / "bin/tool"
    mock.write_text(mock_source)
    mock.chmod(0o755)
    for name in ("gh", "cargo", "uv", "curl", "git", "sleep"):
        (root / "bin" / name).symlink_to(mock)


def _invoke(
    root: Path,
    mode: str = "recover",
    expected: int = 0,
    **extra: str,
) -> subprocess.CompletedProcess[str]:
    env = {
        **os.environ,
        "PATH": str(root / "bin") + os.pathsep + os.environ["PATH"],
        "MOCK_ROOT": str(root),
        "GITHUB_REPOSITORY": "nautechsystems/nautilus_trader",
        "GH_TOKEN": "mock",
        "PYPI_ATTESTATIONS_VERSION": "0.0.30",
        "GH_RELEASE_UPLOAD_ATTEMPTS": "1",
        "GH_RELEASE_VERIFY_ATTEMPTS": "1",
        "GH_RELEASE_PUBLISH_ATTEMPTS": "1",
        "REKOR_CHECK_ATTEMPTS": "1",
        **extra,
    }
    result = subprocess.run(
        [
            bash,
            str(repo / "scripts/ci/recover-release.bash"),
            "v1.2.3",
            "123",
            mode,
            str(root / "diagnostics"),
        ],
        cwd=root,
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    (root / "output.log").write_text(result.stdout + result.stderr)
    if result.returncode != expected:
        raise AssertionError(
            f"Expected {expected}, received {result.returncode}:\n{result.stdout}{result.stderr}",
        )
    return result


def _calls(root: Path) -> list[tuple[str, list[str]]]:
    return [
        (tool, args)
        for tool, args in (
            json.loads(line) for line in (root / "calls.jsonl").read_text().splitlines()
        )
    ]


def _writes(root: Path) -> list[list[str]]:
    return [
        args
        for tool, args in _calls(root)
        if tool == "gh" and args[:2] in (["release", "upload"], ["release", "edit"])
    ]


RECOVERY = runpy.run_path(str(Path(__file__).with_name("release-recovery.py")))
GLOBALS = RECOVERY["_prepare"].__globals__
SOURCE_SHA = "1" * 40
RECOVERY_SHA = "7" * 40
REPOSITORY = "nautechsystems/nautilus_trader"


def _expect_failure(call: Callable[..., object], *args: object) -> None:
    try:
        call(*args)
    except (ValueError, subprocess.CalledProcessError):
        return
    raise AssertionError("Expected release recovery to reject invalid evidence")


# Keep each evidence mutation beside the original fixture it changes
def _check_recovery(root: Path) -> None:  # noqa: PLR0915
    (root / "source").mkdir()
    (root / "assets").mkdir()
    (root / "remote").mkdir()
    (root / "source-artifacts/wheel").mkdir(parents=True)
    source = {
        "id": 123,
        "head_sha": SOURCE_SHA,
        "run_attempt": 2,
        "status": "completed",
        "repository": {"full_name": REPOSITORY},
    }
    source_path = root / "source/source-run.json"
    source_path.write_text(json.dumps(source))
    name = "nautilus_trader-2.0.0rc8-cp313-cp313-win_amd64.whl"
    original = root / "source-artifacts/wheel" / name
    original.write_bytes(b"original wheel content")
    (root / "assets" / name).write_bytes(original.read_bytes())
    (root / "remote" / name).write_bytes(original.read_bytes())
    env = {
        "GITHUB_REPOSITORY": REPOSITORY,
        "GITHUB_WORKFLOW_REF": f"{REPOSITORY}/.github/workflows/build.yml@refs/heads/master",
        "GITHUB_EVENT_NAME": "workflow_dispatch",
        "GITHUB_RUN_ID": "456",
        "GITHUB_RUN_ATTEMPT": "3",
        "RELEASE_RECOVERY_SHA": RECOVERY_SHA,
        "GITHUB_OUTPUT": str(root / "outputs"),
        "TAG_NAME": "v2.0.0rc8",
        "PYPI_RECOVERY_RUN_ID": "456",
    }
    with patch.dict(os.environ, env):
        RECOVERY["_prepare"](root)
        predicate = json.loads((root / "recovery-predicate.json").read_text())
        digest = RECOVERY["_digest"](original)
        assert predicate == {
            "repository": REPOSITORY,
            "tag": "v2.0.0rc8",
            "source": {"sha": SOURCE_SHA, "run": 123, "attempt": 2},
            "recovery": {"sha": RECOVERY_SHA, "run": 456, "attempt": 3},
            "artifacts": [{"name": name, "sha256": digest}],
        }
        package = root / "distributions" / name
        package.with_name(name + ".publish.attestation").write_text("publish signature")

        def stage_metadata(route: str) -> dict[str, Any]:
            if route.endswith("/commits/v2.0.0rc8"):
                return {"sha": SOURCE_SHA}
            assert route.endswith("/actions/runs/123"), route
            return source

        with (
            patch.dict(GLOBALS, {"_github_json": stage_metadata}),
            patch(
                "subprocess.run",
                return_value=subprocess.CompletedProcess([], 0, '{"isDraft": true}'),
            ),
            patch(
                "urllib.request.urlopen",
                side_effect=urllib.error.HTTPError("", 404, "", {}, None),
            ),
        ):
            RECOVERY["_stage"](root)
        assert (root / "publish/dist" / name).read_bytes() == original.read_bytes()
        assert (
            root / "publish/dist" / (name + ".publish.attestation")
        ).read_text() == "publish signature"

        statement = {"predicateType": RECOVERY["PREDICATE_TYPE"], "predicate": predicate}
        bundle = package.with_name(name + ".sigstore")
        bundle.write_text(
            json.dumps(
                {
                    "dsseEnvelope": {
                        "payload": base64.b64encode(json.dumps(statement).encode()).decode(),
                    },
                },
            ),
        )
        run = {
            "repository": {"full_name": REPOSITORY},
            "head_repository": {"full_name": REPOSITORY},
            "path": ".github/workflows/build.yml",
            "event": "workflow_dispatch",
            "head_branch": "master",
            "head_sha": RECOVERY_SHA,
            "run_attempt": 3,
            "status": "completed",
        }

        def github_json(route: str) -> dict[str, Any]:
            if "/actions/runs/123/attempts/" in route:
                if not route.endswith("/2"):
                    raise subprocess.CalledProcessError(1, ["gh", "api", route])
                return {"head_sha": SOURCE_SHA, "run_attempt": 2, "status": "completed"}
            prefix = f"repos/{REPOSITORY}/actions/runs/456/attempts/3"
            if route == prefix:
                return run
            assert route == prefix + "/jobs?per_page=100", route
            return {"total_count": 1, "jobs": [{"name": "recover-pypi", "conclusion": "success"}]}

        def verify_bundle(args: list[str], *, check: bool) -> subprocess.CompletedProcess:
            assert check is True
            bundle_json = Path(args[args.index("--bundle") + 1])
            assert bundle_json.suffix == ".json"
            assert bundle_json.read_bytes() == bundle.read_bytes()
            return subprocess.CompletedProcess(args, 0)

        with (
            patch.dict(GLOBALS, {"_github_json": github_json}),
            patch("subprocess.run", side_effect=verify_bundle) as verify,
        ):
            RECOVERY["_verify"](package, source_path)
            command = verify.call_args.args[0]
            assert command[command.index("--source-digest") + 1] == RECOVERY_SHA
            assert command[command.index("--predicate-type") + 1] == RECOVERY["PREDICATE_TYPE"]
            original_bundle = bundle.read_text()
            source["run_attempt"] = 3
            source_path.write_text(json.dumps(source))
            RECOVERY["_verify"](package, source_path)
            source["run_attempt"] = 2
            source_path.write_text(json.dumps(source))
            for key, value in (("sha", "2" * 40), ("run", 124), ("attempt", 999)):
                saved = predicate["source"][key]
                predicate["source"][key] = value
                bundle.write_text(
                    json.dumps(
                        {
                            "dsseEnvelope": {
                                "payload": base64.b64encode(
                                    json.dumps(statement).encode(),
                                ).decode(),
                            },
                        },
                    ),
                )
                _expect_failure(RECOVERY["_verify"], package, source_path)
                predicate["source"][key] = saved
            bundle.write_text(original_bundle)
            with patch.dict(os.environ, {"PYPI_RECOVERY_RUN_ID": "789"}):
                _expect_failure(RECOVERY["_verify"], package, source_path)
            for key, value in (
                ("head_sha", SOURCE_SHA),
                ("event", "push"),
                ("head_branch", "develop"),
            ):
                saved = run[key]
                run[key] = value
                _expect_failure(RECOVERY["_verify"], package, source_path)
                run[key] = saved
            with patch.dict(
                GLOBALS,
                {
                    "_github_json": lambda route: (
                        github_json(route)
                        if "/jobs?" not in route
                        else {
                            "total_count": 1,
                            "jobs": [{"name": "recover-pypi", "conclusion": "failure"}],
                        }
                    ),
                },
            ):
                _expect_failure(RECOVERY["_verify"], package, source_path)
            with patch("subprocess.run", side_effect=subprocess.CalledProcessError(1, "gh")):
                _expect_failure(RECOVERY["_verify"], package, source_path)
            package.write_bytes(b"changed wheel")
            _expect_failure(RECOVERY["_verify"], package, source_path)
            package.write_bytes(original.read_bytes())
        original.write_bytes(b"different source artifact")
        another = root / "missing-original"
        (another / "source").mkdir(parents=True)
        (another / "source/source-run.json").write_text(json.dumps(source))
        (another / "assets").mkdir()
        (another / "assets" / name).write_bytes(package.read_bytes())
        _expect_failure(RECOVERY["_prepare"], another)

        response = {"urls": [{"filename": name, "digests": {"sha256": "bad"}}]}
        with (
            patch.dict(GLOBALS, {"_github_json": stage_metadata}),
            patch(
                "subprocess.run",
                return_value=subprocess.CompletedProcess([], 0, '{"isDraft": true}'),
            ),
            patch("urllib.request.urlopen", return_value=io.StringIO(json.dumps(response))),
        ):
            _expect_failure(RECOVERY["_stage"], root)
        shutil.rmtree(root / "publish")
        response["urls"][0]["digests"]["sha256"] = digest
        with (
            patch.dict(GLOBALS, {"_github_json": stage_metadata}),
            patch(
                "subprocess.run",
                return_value=subprocess.CompletedProcess([], 0, '{"isDraft": true}'),
            ),
            patch("urllib.request.urlopen", return_value=io.StringIO(json.dumps(response))),
        ):
            RECOVERY["_stage"](root)
        assert list((root / "publish/dist").iterdir()) == []
        assert (root / "outputs").read_text().splitlines()[-1] == "missing=false"


def _check_download(root: Path) -> None:
    (root / "source").mkdir(parents=True)
    (root / "source/source-run.json").write_text(
        json.dumps({"repository": {"full_name": REPOSITORY}}),
    )
    (root / "assets").mkdir()
    (root / "assets/wheel.whl.sigstore").write_text("original bundle")
    run = {"run_attempt": 3, "head_sha": RECOVERY_SHA}
    metadata = {
        "total_count": 1,
        "artifacts": [
            {
                "name": "release-recovery-attestations-456-3",
                "expired": False,
                "workflow_run": {"id": 456, "head_sha": RECOVERY_SHA},
            },
        ],
    }

    def github_json(route: str) -> dict[str, Any]:
        if route.endswith("/actions/runs/456"):
            return run
        assert route.endswith("/actions/runs/456/artifacts?per_page=100"), route
        return metadata

    def download(command: list[str], **_kwargs: object) -> subprocess.CompletedProcess[str]:
        assert command[:4] == ["gh", "run", "download", "456"]
        assert command[command.index("--repo") + 1] == REPOSITORY
        assert command[command.index("--name") + 1] == "release-recovery-attestations-456-3"
        destination = Path(command[command.index("--dir") + 1])
        destination.mkdir()
        (destination / "wheel.whl.sigstore").write_text("later bundle")
        (destination / "wheel.whl.intoto.jsonl").write_text("later envelope")
        return subprocess.CompletedProcess(command, 0)

    with (
        patch.dict(GLOBALS, {"_github_json": github_json, "_publication_run": lambda *_args: None}),
        patch("subprocess.run", side_effect=download) as retrieve,
    ):
        RECOVERY["_download"](root, 456)
        assert (root / "assets/wheel.whl.sigstore").read_text() == "original bundle"
        assert (root / "assets/wheel.whl.intoto.jsonl").read_text() == "later envelope"
        assert retrieve.call_count == 1
        artifact = metadata["artifacts"][0]
        for key, value in (("expired", True), ("name", "release-recovery-attestations-456-2")):
            saved = artifact[key]
            artifact[key] = value
            _expect_failure(RECOVERY["_download"], root, 456)
            artifact[key] = saved
        artifact["workflow_run"]["head_sha"] = SOURCE_SHA
        _expect_failure(RECOVERY["_download"], root, 456)
        assert retrieve.call_count == 1


def _check_signing(root: Path) -> None:
    root.mkdir()
    executable = root / "uv"
    executable.write_text(
        '#!/usr/bin/env bash\nset -euo pipefail\nprintf "%s\\n" "$@" > "$SIGNING_ARGUMENTS"\n',
    )
    executable.chmod(0o755)
    artifact = root / "original.whl"
    artifact.write_bytes(b"original artifact")
    script_dir = Path(__file__).parent
    version = subprocess.check_output(
        [bash, str(script_dir.parent / "tool-version.sh"), "pypi-attestations"],
        text=True,
    ).strip()
    log = root / "arguments"
    subprocess.run(
        [bash, str(script_dir / "sign-pypi-attestations.bash"), str(artifact)],
        check=True,
        capture_output=True,
        text=True,
        env={
            **os.environ,
            "PATH": str(root) + os.pathsep + os.environ["PATH"],
            "SIGNING_ARGUMENTS": str(log),
            "PYPI_ATTESTATIONS_VERSION": version,
            "PYPI_ATTESTATION_SIGN_ATTEMPTS": "1",
            "PYTHONPATH": str(root / "poison"),
        },
    )
    assert log.read_text().splitlines() == [
        "run",
        "--isolated",
        "--no-project",
        "--no-build",
        "--managed-python",
        "--python",
        "3.13",
        "--with",
        f"pypi-attestations=={version}",
        "--",
        "python",
        "-I",
        "-m",
        "pypi_attestations",
        "sign",
        str(artifact),
    ]


with tempfile.TemporaryDirectory(prefix="release-recovery-test-") as directory:
    base = Path(directory)
    units = base / "publication-units"
    units.mkdir()
    _check_recovery(units)
    _check_download(base / "evidence-download")
    _check_signing(base / "signing")
    publication = (
        (repo / ".github/workflows/build.yml")
        .read_text()
        .split("\n  publish-github-release:\n", 1)[1]
    )
    pipelines = [step for step in publication.split("\n      - ") if "tee " in step]
    assert len(pipelines) == 3
    pipeline_root = base / "publication-pipelines"
    (pipeline_root / "scripts/ci").mkdir(parents=True)
    diagnostics = pipeline_root / "diagnostics"
    diagnostics.mkdir()
    for step in pipelines:
        assert "\n        shell: bash\n" in step
        script = textwrap.dedent(step.split("\n        run: |\n", 1)[1])
        command = next(line for line in script.splitlines() if line.startswith("bash "))
        (pipeline_root / command.split()[1]).write_text("exit 42\n")
        result = subprocess.run(
            [bash, "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", script],
            cwd=pipeline_root,
            env={
                **os.environ,
                "RELEASE_VERIFICATION_DIAGNOSTICS_DIR": str(diagnostics),
            },
            capture_output=True,
            check=False,
        )
        assert result.returncode == 42

    success = base / "success"
    _fixture(success)
    _invoke(success)
    verified = [
        args
        for tool, args in _calls(success)
        if tool == "gh" and args[:2] == ["attestation", "verify"] and "--help" not in args
    ]
    assert len(verified) == 9
    assert all("--bundle" in args for args in verified)
    downloads = [
        args
        for tool, args in _calls(success)
        if tool == "gh" and args[:2] == ["release", "download"]
    ]
    assert len(downloads) == 1
    assert [downloads[0][i + 1] for i, arg in enumerate(downloads[0]) if arg == "--pattern"] == [
        "dist-manifest.json",
        "crates-manifest.json",
    ]
    bodies = [
        args
        for tool, args in _calls(success)
        if tool == "gh"
        and args[:2] == ["release", "view"]
        and "body" in args[args.index("--json") + 1].split(",")
    ]
    assert len(bodies) == 1
    assert json.loads((success / "state.json").read_text())["isDraft"] is False
    assert len(list((success / "remote").iterdir())) == 39
    manifest = json.loads((success / "remote/dist-manifest.json").read_text())
    packages = sorted(
        [
            p
            for p in (success / "original").iterdir()
            if p.suffix == ".whl" or p.name.endswith(".tar.gz")
        ],
    )
    assert manifest["schema_version"] == 1
    assert manifest["tag"] == "v1.2.3"
    assert manifest["artifacts"] == [
        {
            "name": p.name,
            "sha256": hashlib.sha256(p.read_bytes()).hexdigest(),
            "size": p.stat().st_size,
        }
        for p in packages
    ]
    before = {p.name: p.read_bytes() for p in (success / "remote").iterdir()}
    write_count = len(_writes(success))
    _invoke(success, EXPIRED_ARTIFACTS="1")
    _invoke(success, mode="verify", EXPIRED_ARTIFACTS="1")
    assert len(_writes(success)) == write_count, "Published verification changed the release"
    assert {p.name: p.read_bytes() for p in (success / "remote").iterdir()} == before

    preparation = base / "prepare-pypi"
    _fixture(preparation)
    for package in (preparation / "original").iterdir():
        if package.name.endswith((".whl", ".tar.gz")):
            shutil.copyfile(package, preparation / "remote" / package.name)
    _invoke(
        preparation,
        mode="prepare-pypi",
        GITHUB_SHA="7" * 40,
        GITHUB_WORKFLOW_REF="nautechsystems/nautilus_trader/.github/workflows/build.yml@refs/heads/master",
        GITHUB_EVENT_NAME="workflow_dispatch",
        GITHUB_RUN_ID="456",
        GITHUB_RUN_ATTEMPT="3",
        GITHUB_OUTPUT=str(preparation / "outputs"),
    )
    assert _writes(preparation) == []
    prepared = next((preparation / "diagnostics").glob("recovery.*/recovery-predicate.json"))
    predicate = json.loads(prepared.read_text())
    assert predicate["source"] == {"sha": "1" * 40, "run": 123, "attempt": 1}
    assert predicate["recovery"] == {"sha": "7" * 40, "run": 456, "attempt": 3}
    assert predicate["artifacts"] == [
        {"name": p.name, "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}
        for p in sorted((preparation / "remote").iterdir())
    ]
    assert not any(tool == "uv" for tool, args in _calls(preparation) if args != ["--version"])

    publication_recovery = base / "publication-recovery"
    _fixture(publication_recovery)
    (publication_recovery / "publication").mkdir()
    for original in (publication_recovery / "original").iterdir():
        if original.name.endswith((".sigstore", ".intoto.jsonl")):
            original.unlink()
    predicate["artifacts"] = [
        {"name": p.name, "sha256": hashlib.sha256(p.read_bytes()).hexdigest()}
        for p in sorted((publication_recovery / "original").iterdir())
    ]
    statement = {
        "predicateType": "https://nautilustrader.io/attestations/release-recovery/v1",
        "predicate": predicate,
    }
    envelope = {
        "payload": base64.b64encode(json.dumps(statement).encode()).decode(),
        "signatures": ["signature"],
    }
    for package in (publication_recovery / "original").iterdir():
        shutil.copyfile(package, publication_recovery / "remote" / package.name)
        (publication_recovery / "publication" / (package.name + ".sigstore")).write_text(
            json.dumps({"dsseEnvelope": envelope}),
        )
        (publication_recovery / "publication" / (package.name + ".intoto.jsonl")).write_text(
            json.dumps(envelope),
        )
    _invoke(publication_recovery, PYPI_RECOVERY_RUN_ID="456", PUBLICATION_RECOVERY="1")
    published = json.loads((publication_recovery / "state.json").read_text())
    assert published["isDraft"] is False
    assert (
        '--predicate-type "https://nautilustrader.io/attestations/release-recovery/v1"'
        in published["body"]
    )
    assert len(list((publication_recovery / "remote").iterdir())) == 39
    writes_before = len(_writes(publication_recovery))
    _invoke(
        publication_recovery,
        mode="verify",
        PYPI_RECOVERY_RUN_ID="",
        PUBLICATION_RECOVERY="1",
    )
    assert len(_writes(publication_recovery)) == writes_before

    for name, env, expected in (
        ("wrong-tag", {"WRONG_TAG": "1"}, 1),
        ("wrong-source", {"SOURCE_EVENT": "pull_request"}, 1),
        ("wrong-repository", {"SOURCE_REPOSITORY": "someone/other"}, 1),
        ("wrong-branch", {"SOURCE_BRANCH": "develop"}, 1),
        ("wrong-workflow", {"SOURCE_WORKFLOW": ".github/workflows/other.yml"}, 1),
        ("incomplete-source", {"SOURCE_STATUS": "in_progress"}, 1),
        ("wrong-checkout", {"CHECKOUT_SHA": "2" * 40}, 1),
        ("unsupported-seal-tool", {"UNSUPPORTED_SEAL_HELP": "1"}, 2),
        ("parent-seal-help", {"PARENT_SEAL_HELP": "1"}, 1),
        ("failed-source-gate", {"SOURCE_GATE": "failure"}, 1),
        ("provenance", {"FAIL_PROVENANCE": "1"}, 1),
        ("registry", {"FAIL_REGISTRY": "1"}, 42),
        ("missing-originals", {"EXPIRED_ARTIFACTS": "1"}, 1),
        ("changed-source", {"CHANGED_SOURCE": "1"}, 1),
    ):
        root = base / name
        _fixture(root)
        _invoke(root, expected=expected, **env)
        assert _writes(root) == [], name + " changed the release before verification"
        assert not list((root / "remote").iterdir())
        assert next(
            (root / "diagnostics").glob("recovery.*/exit-status"),
        ).read_text().strip() == str(expected)
        if name == "registry":
            evidence = list((root / "diagnostics").glob("recovery.*/verification/registries.*"))
            assert len(evidence) == 1
            project = json.loads((evidence[0] / "pypi-project.json").read_text())
            assert project == {
                "releases": {
                    "1.2.3": [
                        {
                            "filename": p.name,
                            "digests": {"sha256": hashlib.sha256(p.read_bytes()).hexdigest()},
                            "url": "https://files.pythonhosted.org/" + p.name,
                        }
                        for p in packages
                    ],
                },
            }
            logs = list(
                (root / "diagnostics").glob("recovery.*/verification/retry.*/attempt-1.log"),
            )
            assert [p.read_text() for p in logs] == [
                "provenance mismatch: wrong publisher identity\n",
            ]

    notes = base / "notes-rerun"
    _fixture(notes)
    _invoke(notes, expected=46, FAIL_PUBLISH="1")
    old_notes = json.loads((notes / "state.json").read_text())["body"]
    assert old_notes.count("<!-- release-checksums:start -->") == 1
    assert old_notes.count("<!-- release-checksums:end -->") == 1
    edits_before = sum("--notes-file" in args for args in _writes(notes))
    calls_before = len(_calls(notes))
    _invoke(notes)
    downloads = [
        args
        for tool, args in _calls(notes)[calls_before:]
        if tool == "gh" and args[:2] == ["release", "download"]
    ]
    assert len(downloads) == 2
    assert "--pattern" not in downloads[0]
    assert [downloads[1][i + 1] for i, arg in enumerate(downloads[1]) if arg == "--pattern"] == [
        "dist-manifest.json",
        "crates-manifest.json",
    ]
    assert sum("--notes-file" in args for args in _writes(notes)) == edits_before
    assert json.loads((notes / "state.json").read_text())["body"] == old_notes

    normal_notes = base / "normal-notes"
    _fixture(normal_notes)
    _invoke(normal_notes, expected=46, FAIL_PUBLISH="1")
    data = json.loads((normal_notes / "state.json").read_text())
    data["body"] = "Release notes\n\n"
    (normal_notes / "state.json").write_text(json.dumps(data))
    asset_dir = normal_notes / "normal-assets"
    shutil.copytree(normal_notes / "remote", asset_dir)
    env = {
        **os.environ,
        "PATH": str(normal_notes / "bin") + os.pathsep + os.environ["PATH"],
        "MOCK_ROOT": str(normal_notes),
        "GITHUB_REPOSITORY": "nautechsystems/nautilus_trader",
        "GH_TOKEN": "mock",
        "TAG_NAME": "v1.2.3",
        "ALLOW_CLOBBER": "1",
    }
    subprocess.run(
        [
            bash,
            str(repo / "scripts/ci/publish-release-checksums.sh"),
            "--publish-existing",
            str(asset_dir),
        ],
        env=env,
        capture_output=True,
        check=True,
    )
    old_notes = json.loads((normal_notes / "state.json").read_text())["body"]
    edits_before = sum("--notes-file" in args for args in _writes(normal_notes))
    _invoke(normal_notes)
    assert sum("--notes-file" in args for args in _writes(normal_notes)) == edits_before
    assert json.loads((normal_notes / "state.json").read_text())["body"] == old_notes
    count = len(_writes(normal_notes))
    result = subprocess.run(
        [
            bash,
            str(repo / "scripts/ci/publish-release-checksums.sh"),
            "--update-notes",
            str(asset_dir),
        ],
        env=env,
        capture_output=True,
        check=False,
    )
    assert result.returncode == 1
    assert b"draft" in result.stderr
    assert len(_writes(normal_notes)) == count

    sealed_before_notes = base / "sealed-before-notes"
    _fixture(sealed_before_notes)
    _invoke(sealed_before_notes, expected=1, PUBLISH_BEFORE_NOTES="1")
    assert not any(args[:2] == ["release", "edit"] for args in _writes(sealed_before_notes))

    cleanup = base / "artifact-cleanup"
    _fixture(cleanup)
    subprocess.run(
        [bash, str(repo / "scripts/ci/publish-wheels-delete-artifacts.sh")],
        env={
            **os.environ,
            "PATH": str(cleanup / "bin") + os.pathsep + os.environ["PATH"],
            "MOCK_ROOT": str(cleanup),
            "GITHUB_REPOSITORY": "nautechsystems/nautilus_trader",
            "GITHUB_TOKEN": "mock",
            "GITHUB_RUN_ID": "123",
        },
        capture_output=True,
        check=True,
    )
    assert [
        args[-1].rsplit("/", 1)[-1]
        for tool, args in _calls(cleanup)
        if tool == "curl" and "DELETE" in args
    ] == ["1", "4"]

    for field, value, expected in (
        ("generated_at", "2000-01-01T00:00:00Z", 0),
        ("tag", "v9.9.9", 1),
    ):
        preserved = base / ("manifest-" + field)
        _fixture(preserved)
        shutil.copytree(success / "remote", preserved / "remote", dirs_exist_ok=True)
        (preserved / "state.json").write_bytes((success / "state.json").read_bytes())
        for name in ("dist-manifest.json", "crates-manifest.json"):
            path = preserved / "remote" / name
            data = json.loads(path.read_text())
            data[field] = value
            path.write_text(json.dumps(data))
        before = {p.name: p.read_bytes() for p in (preserved / "remote").iterdir()}
        _invoke(preserved, mode="verify", expected=expected, EXPIRED_ARTIFACTS="1")
        assert _writes(preserved) == []
        assert {p.name: p.read_bytes() for p in (preserved / "remote").iterdir()} == before

    rekor = base / "rekor-failure"
    _fixture(rekor)
    _invoke(rekor, expected=47, FAIL_REKOR="1")
    assert json.loads((rekor / "state.json").read_text())["isDraft"] is True
    assert not any("--draft=false" in args for args in _writes(rekor))
    assert next((rekor / "diagnostics").glob("recovery.*/checkpoint")).read_text() == "rekor\n"
    _invoke(rekor)

    bundle = base / "invalid-bundle"
    _fixture(bundle)
    next((bundle / "original").glob("*.sigstore")).write_text("wrong bundle\n")
    _invoke(bundle, expected=1)
    assert _writes(bundle) == []

    envelope = base / "invalid-envelope"
    _fixture(envelope)
    next((envelope / "original").glob("*.intoto.jsonl")).write_text('{"payload":"wrong"}\n')
    _invoke(envelope, expected=1)
    assert _writes(envelope) == []

    for suffix in (".whl", ".tar.gz"):
        incomplete = base / ("incomplete-" + suffix)
        _fixture(incomplete)
        shutil.copytree(success / "remote", incomplete / "remote", dirs_exist_ok=True)
        (incomplete / "state.json").write_bytes((success / "state.json").read_bytes())
        missing = next((incomplete / "remote").glob("*" + suffix))
        missing_name = missing.name
        for path in list((incomplete / "remote").iterdir()):
            if path.name == missing_name or path.name.startswith(missing_name + "."):
                path.unlink()
        manifest_path = incomplete / "remote/dist-manifest.json"
        data = json.loads(manifest_path.read_text())
        data["artifacts"] = [item for item in data["artifacts"] if item["name"] != missing_name]
        manifest_path.write_text(json.dumps(data))
        sums = incomplete / "remote/SHA256SUMS"
        sums.write_text(
            "\n".join(line for line in sums.read_text().splitlines() if missing_name not in line)
            + "\n",
        )
        _invoke(
            incomplete,
            mode="verify",
            expected=1,
            EXPIRED_ARTIFACTS="1",
            MATCH_REMOTE_PYPI="1",
        )
        assert _writes(incomplete) == []

    partial = base / "partial"
    _fixture(partial)
    _invoke(partial, expected=44, FAIL_AFTER_UPLOAD="1", GH_RELEASE_UPLOAD_ATTEMPTS="5")
    partial_bytes = {p.name: p.read_bytes() for p in (partial / "remote").iterdir()}
    assert len(partial_bytes) == 1
    assert sum(args[:2] == ["release", "upload"] for args in _writes(partial)) == 1
    _invoke(partial)
    for name, contents in partial_bytes.items():
        assert (partial / "remote" / name).read_bytes() == contents

    mismatch = base / "mismatch"
    _fixture(mismatch)
    package = next((mismatch / "original").glob("*.whl"))
    (mismatch / "remote" / package.name).write_bytes(b"wrong bytes")
    _invoke(mismatch, expected=1)
    assert _writes(mismatch) == []
    assert (mismatch / "remote" / package.name).read_bytes() == b"wrong bytes"

    for contents, state in ((b"", "uploaded"), (b"partial", "starter")):
        incomplete_upload = base / ("incomplete-upload-" + state)
        _fixture(incomplete_upload)
        name = next((incomplete_upload / "original").glob("*.whl")).name
        (incomplete_upload / "remote" / name).write_bytes(contents)
        result = _invoke(incomplete_upload, expected=1, ASSET_STATE=state)
        assert "Incomplete release asset: " + name in result.stdout + result.stderr
        assert _writes(incomplete_upload) == []

    sealed = base / "sealed-failure"
    _fixture(sealed)
    _invoke(sealed, expected=45, FAIL_SEAL="1")
    assert json.loads((sealed / "state.json").read_text())["isDraft"] is False
    count = len(_writes(sealed))
    _invoke(sealed, EXPIRED_ARTIFACTS="1")
    assert len(_writes(sealed)) == count

print("Release recovery tests passed")
