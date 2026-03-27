#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
import sys


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[3]


sys.path.insert(0, str(_repo_root() / "systems/flux"))

from flux.runners.equities.node_groups import load_equities_node_groups  # noqa: E402


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="List grouped equities node ids and config paths for the installer.",
    )
    parser.add_argument(
        "--shared-config",
        type=Path,
        default=None,
        help="Path to deploy/equities/equities.live.toml.",
    )
    parser.add_argument(
        "--strategies-dir",
        type=Path,
        default=None,
        help="Path to deploy/equities/strategies.",
    )
    return parser.parse_args()


def main() -> int:
    args = _parse_args()
    groups = load_equities_node_groups(
        live_config_path=args.shared_config,
        strategies_dir=args.strategies_dir,
    )
    for group in groups:
        print("\t".join((group.node_group_id, *(str(path) for path in group.config_paths))))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
