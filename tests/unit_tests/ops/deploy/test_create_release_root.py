from __future__ import annotations

import os
import subprocess
from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def test_create_release_root_writes_metadata_and_current_symlink(tmp_path: Path) -> None:
    repo_root = _repo_root()
    source_root = tmp_path / "source"
    releases_root = tmp_path / "releases"
    source_root.mkdir()
    (source_root / "README.txt").write_text("pilot build\n", encoding="utf-8")
    (source_root / ".git").mkdir()
    (source_root / ".git" / "ignored").write_text("x\n", encoding="utf-8")

    script_path = repo_root / "ops/scripts/deploy/create_release_root.sh"
    result = subprocess.run(  # noqa: S603 - controlled test invocation of repo shell helper
        ["/usr/bin/bash", str(script_path)],
        check=True,
        capture_output=True,
        text=True,
        cwd=repo_root,
        env={
            **os.environ,
            "RELEASES_ROOT": str(releases_root),
            "DEPLOY_LANE": "pilot",
            "STACK_NAME": "equities",
            "SOURCE_ROOT": str(source_root),
            "RELEASE_ID": "20260326test",
            "SOURCE_REF": "abc1234",
        },
    )

    release_root = Path(result.stdout.strip())
    metadata_path = release_root / ".flux-release" / "release.env"
    current_link = releases_root / "pilot" / "equities" / "current"

    assert release_root.is_dir()
    assert (release_root / "README.txt").read_text(encoding="utf-8") == "pilot build\n"
    assert not (release_root / ".git").exists()
    assert metadata_path.is_file()
    metadata = metadata_path.read_text(encoding="utf-8")
    assert "DEPLOY_LANE=pilot" in metadata
    assert "STACK_NAME=equities" in metadata
    assert "RELEASE_ID=20260326test" in metadata
    assert "SOURCE_REF=abc1234" in metadata
    assert current_link.is_symlink()
    assert current_link.resolve() == release_root.resolve()
