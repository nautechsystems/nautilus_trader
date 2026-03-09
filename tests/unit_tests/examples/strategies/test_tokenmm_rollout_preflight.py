from __future__ import annotations

from pathlib import Path
from types import SimpleNamespace

from flux.runners.tokenmm.rollout_preflight import collect_rollout_preflight_errors


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def _write_repo_fixture(
    repo_root: Path,
    *,
    strategy_id: str = "plumeusdt_bitget_perp_makerv3",
    strategy_venues: tuple[str, ...] = ("BITGET", "BINANCE_SPOT"),
    include_fluxboard_dist: bool = True,
    include_pulse_dist: bool = True,
) -> None:
    _write(
        repo_root / "deploy/tokenmm/tokenmm.live.toml",
        f"""
[api]
tokenmm_strategy_ids = ["{strategy_id}"]
""".strip()
        + "\n",
    )

    venues_block = "\n".join(f"[node.venues.{venue}]\n" for venue in strategy_venues)
    _write(
        repo_root / f"deploy/tokenmm/strategies/{strategy_id}.toml",
        venues_block,
    )

    if include_fluxboard_dist:
        _write(repo_root / "fluxboard/dist/index.html", "<html>fluxboard</html>\n")
    if include_pulse_dist:
        _write(repo_root / "pulse-ui/dist/index.html", "<html>pulse</html>\n")


def test_collect_rollout_preflight_errors_requires_built_fluxboard_and_pulse_assets(
    tmp_path: Path,
) -> None:
    _write_repo_fixture(
        tmp_path,
        include_fluxboard_dist=False,
        include_pulse_dist=False,
    )

    errors = collect_rollout_preflight_errors(
        tmp_path,
        nautilus_pyo3=SimpleNamespace(BitgetEnvironment=object()),
    )

    assert any("fluxboard/dist/index.html" in error for error in errors)
    assert any("pulse-ui/dist/index.html" in error for error in errors)


def test_collect_rollout_preflight_errors_requires_bitget_export_when_bitget_is_enabled(
    tmp_path: Path,
) -> None:
    _write_repo_fixture(tmp_path)

    errors = collect_rollout_preflight_errors(
        tmp_path,
        nautilus_pyo3=SimpleNamespace(),
    )

    assert any("BitgetEnvironment" in error for error in errors)


def test_collect_rollout_preflight_errors_passes_when_assets_and_native_exports_are_present(
    tmp_path: Path,
) -> None:
    _write_repo_fixture(tmp_path)

    errors = collect_rollout_preflight_errors(
        tmp_path,
        nautilus_pyo3=SimpleNamespace(BitgetEnvironment=object()),
    )

    assert errors == []


def test_collect_rollout_preflight_errors_requires_live_runtime_modules(
    tmp_path: Path,
) -> None:
    _write_repo_fixture(tmp_path)

    def _fake_import(name: str) -> object:
        if name == "redis":
            raise ModuleNotFoundError("No module named 'redis'")
        return object()

    errors = collect_rollout_preflight_errors(
        tmp_path,
        nautilus_pyo3=SimpleNamespace(BitgetEnvironment=object()),
        import_module=_fake_import,
    )

    assert any("redis" in error for error in errors)
