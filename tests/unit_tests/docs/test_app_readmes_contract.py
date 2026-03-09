from pathlib import Path


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def test_fluxboard_readme_points_to_current_runbooks_and_shells() -> None:
    readme = _read(_repo_root() / "fluxboard/README.md")

    assert "apps/fluxboard/docs/tokenmm_runbook.md" in readme
    assert "examples/live/makerv3_single_leg/README.md" not in readme
    assert "/tokenmm/*" in readme
    assert "/equities/*" in readme


def test_pulse_ui_readme_documents_build_test_and_base_path_contract() -> None:
    readme = _read(_repo_root() / "pulse-ui/README.md")

    assert "pnpm --dir pulse-ui build" in readme
    assert "pnpm --dir pulse-ui test" in readme
    assert "/pulse/*" in readme
    assert "PULSE_UI_BASE_PATH" in readme
    assert "flux.runners.tokenmm.run_api" in readme
    assert "flux.runners.equities.run_api" in readme
