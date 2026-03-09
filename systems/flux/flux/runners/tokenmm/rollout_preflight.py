"""
Preflight checks for TokenMM production rollouts.
"""

from __future__ import annotations

import importlib
import sys
import tomllib
from pathlib import Path
from typing import Any


REQUIRED_RUNTIME_FILES = (
    Path("fluxboard/dist/index.html"),
    Path("pulse-ui/dist/index.html"),
)

REQUIRED_NATIVE_EXPORTS_BY_VENUE: dict[str, tuple[str, ...]] = {
    "BITGET": ("BitgetEnvironment",),
}


def _load_toml(path: Path) -> dict[str, Any]:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def _tokenmm_strategy_ids(repo_root: Path) -> list[str]:
    config = _load_toml(repo_root / "deploy/tokenmm/tokenmm.live.toml")
    raw_ids = config.get("api", {}).get("tokenmm_strategy_ids") or []
    return [str(item).strip() for item in raw_ids if str(item).strip()]


def _strategy_venues(repo_root: Path, strategy_id: str) -> set[str]:
    strategy_path = repo_root / "deploy/tokenmm/strategies" / f"{strategy_id}.toml"
    config = _load_toml(strategy_path)
    raw_venues = config.get("node", {}).get("venues") or {}
    return {
        str(venue).strip().upper()
        for venue in raw_venues
        if str(venue).strip()
    }


def _required_native_exports(repo_root: Path) -> set[str]:
    required: set[str] = set()
    for strategy_id in _tokenmm_strategy_ids(repo_root):
        for venue in _strategy_venues(repo_root, strategy_id):
            required.update(REQUIRED_NATIVE_EXPORTS_BY_VENUE.get(venue, ()))
    return required


def _import_nautilus_pyo3(repo_root: Path) -> Any:
    root_text = str(repo_root)
    if root_text not in sys.path:
        sys.path.insert(0, root_text)
    module = importlib.import_module("nautilus_trader.core.nautilus_pyo3")
    return module


def collect_rollout_preflight_errors(
    repo_root: Path,
    *,
    nautilus_pyo3: Any | None = None,
) -> list[str]:
    repo_root = repo_root.resolve()
    errors: list[str] = []

    for relative_path in REQUIRED_RUNTIME_FILES:
        candidate = repo_root / relative_path
        if not candidate.is_file():
            errors.append(
                f"Missing required runtime artifact: {relative_path}. Build it before rollout.",
            )

    required_exports = _required_native_exports(repo_root)
    if not required_exports:
        return errors

    module = nautilus_pyo3
    if module is None:
        try:
            module = _import_nautilus_pyo3(repo_root)
        except Exception as exc:  # pragma: no cover - exercised through CLI usage
            errors.append(f"Failed to import nautilus_pyo3 from checkout: {exc}")
            return errors

    for export_name in sorted(required_exports):
        if not hasattr(module, export_name):
            errors.append(
                f"Native rollout prerequisite missing: nautilus_pyo3.{export_name}. "
                "Run `make build` in this checkout before rollout.",
            )

    return errors


def main(argv: list[str] | None = None) -> int:
    del argv
    repo_root = Path(__file__).resolve().parents[5]
    errors = collect_rollout_preflight_errors(repo_root)
    if errors:
        for error in errors:
            print(f"[tokenmm-rollout-preflight] {error}", file=sys.stderr)
        return 1

    print("[tokenmm-rollout-preflight] OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
