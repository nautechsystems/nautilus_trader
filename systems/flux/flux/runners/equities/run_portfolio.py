#!/usr/bin/env python3
from __future__ import annotations

import argparse
import logging
from pathlib import Path
from typing import Any

from flux.runners.shared.bootstrap import load_config as load_shared_config
from flux.runners.shared.bootstrap import resolve_mode as resolve_shared_mode
from flux.runners.shared.bootstrap import table as shared_table
from flux.runners.shared.portfolio_runner import parse_required_strategy_ids
from flux.runners.shared.portfolio_runner import parse_strategy_ids
from flux.runners.shared.portfolio_runner import portfolio_base_assets
from flux.runners.shared.portfolio_runner import StrategySetPortfolioAggregator
from flux.runners.shared.strategy_set import get_strategy_set_descriptor


SAFE_MODES = frozenset({"paper", "testnet", "live"})
EQUITIES_DESCRIPTOR = get_strategy_set_descriptor("equities")


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _load_config(path: Path) -> dict[str, Any]:
    return load_shared_config(path, env_prefix=EQUITIES_DESCRIPTOR.env_prefix)


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    return shared_table(data, name)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run Equities portfolio inventory aggregator.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--log-level", default=None)
    return parser.parse_args()


def _resolve_mode(config: dict[str, Any], args: argparse.Namespace) -> str:
    return resolve_shared_mode(config, args, safe_modes=SAFE_MODES)


def _equities_strategy_ids(api_cfg: dict[str, Any]) -> list[str]:
    return parse_strategy_ids(api_cfg, descriptor=EQUITIES_DESCRIPTOR)


def _required_strategy_ids(api_cfg: dict[str, Any], *, fallback: list[str]) -> list[str]:
    return parse_required_strategy_ids(
        api_cfg,
        descriptor=EQUITIES_DESCRIPTOR,
        fallback=fallback,
    )


def _portfolio_base_assets(config: dict[str, Any]) -> list[str]:
    return portfolio_base_assets(config, fallback=["PLUME"])


class EquitiesPortfolioAggregator(StrategySetPortfolioAggregator):
    def __init__(self, *, config: dict[str, Any], mode: str, logger: logging.Logger) -> None:
        super().__init__(
            config=config,
            mode=mode,
            logger=logger,
            descriptor=EQUITIES_DESCRIPTOR,
        )


def main() -> None:
    args = _parse_args()
    config = _load_config(args.config)
    mode = _resolve_mode(config, args)
    portfolio_cfg = _table(config, "portfolio")
    log_level = str(args.log_level or portfolio_cfg.get("log_level", "INFO")).upper()
    logging.basicConfig(
        level=getattr(logging, log_level, logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s - %(message)s",
    )
    aggregator = EquitiesPortfolioAggregator(
        config=config,
        mode=mode,
        logger=logging.getLogger("nautilus-equities-portfolio"),
    )
    aggregator.run()


if __name__ == "__main__":
    main()
