#!/usr/bin/env python3
from __future__ import annotations

import argparse
import logging
from pathlib import Path
from typing import Any

from flux.common.strategy_contracts import execution_account_scope_by_strategy_id
from flux.persistence.portfolio_inventory_snapshots.sqlite import PortfolioInventorySnapshotWriter
from flux.runners.shared.bootstrap import load_config as load_shared_config
from flux.runners.shared.bootstrap import resolve_mode as resolve_shared_mode
from flux.runners.shared.bootstrap import table as shared_table
from flux.runners.shared.logging import configure_python_logging
from flux.runners.shared.portfolio_runner import parse_required_strategy_ids
from flux.runners.shared.portfolio_runner import parse_strategy_ids
from flux.runners.shared.portfolio_runner import portfolio_base_assets
from flux.runners.shared.portfolio_runner import StrategySetPortfolioAggregator
from flux.runners.shared.profile_accounts import build_profile_account_provider_bindings
from flux.runners.shared.strategy_set import get_strategy_set_descriptor


SAFE_MODES = frozenset({"paper", "testnet", "live"})
TOKENMM_DESCRIPTOR = get_strategy_set_descriptor("tokenmm")


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _load_config(path: Path) -> dict[str, Any]:
    return load_shared_config(path, env_prefix=TOKENMM_DESCRIPTOR.env_prefix)


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    return shared_table(data, name)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run TokenMM portfolio inventory aggregator.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--log-level", default=None)
    return parser.parse_args()


def _resolve_mode(config: dict[str, Any], args: argparse.Namespace) -> str:
    return resolve_shared_mode(config, args, safe_modes=SAFE_MODES)


def _tokenmm_strategy_ids(api_cfg: dict[str, Any]) -> list[str]:
    return parse_strategy_ids(api_cfg, descriptor=TOKENMM_DESCRIPTOR)


def _required_strategy_ids(api_cfg: dict[str, Any], *, fallback: list[str]) -> list[str]:
    return parse_required_strategy_ids(
        api_cfg,
        descriptor=TOKENMM_DESCRIPTOR,
        fallback=fallback,
    )


def _portfolio_base_assets(config: dict[str, Any]) -> list[str]:
    return portfolio_base_assets(config, fallback=["PLUME"])


class TokenMMPortfolioAggregator(StrategySetPortfolioAggregator):
    def __init__(self, *, config: dict[str, Any], mode: str, logger: logging.Logger) -> None:
        super().__init__(
            config=config,
            mode=mode,
            logger=logger,
            descriptor=TOKENMM_DESCRIPTOR,
        )
        self._profile_account_bindings = build_profile_account_provider_bindings(config=config)
        self.account_scope_ids = [
            binding.account_scope_id
            for binding in self._profile_account_bindings
        ]
        self._execution_account_scope_by_strategy_id = execution_account_scope_by_strategy_id(
            config.get("strategy_contracts") or [],
            allowlist=self._strategy_ids,
        )
        self._snapshot_writer = _build_portfolio_inventory_snapshot_writer(config)


def _build_portfolio_inventory_snapshot_writer(
    config: dict[str, Any],
) -> PortfolioInventorySnapshotWriter | None:
    telemetry = config.get("telemetry_shipper")
    if not isinstance(telemetry, dict):
        return None
    if not bool(telemetry.get("enable_local_persistence", False)):
        return None

    db_path = _optional_text(telemetry.get("portfolio_inventory_db_path"))
    if db_path is None:
        return None

    Path(db_path).expanduser().parent.mkdir(parents=True, exist_ok=True)
    return PortfolioInventorySnapshotWriter(
        db_path=db_path,
        unchanged_heartbeat_ms=int(telemetry.get("portfolio_inventory_unchanged_heartbeat_ms", 60_000)),
    )


def main() -> None:
    args = _parse_args()
    config = _load_config(args.config)
    mode = _resolve_mode(config, args)
    portfolio_cfg = _table(config, "portfolio")
    configure_python_logging(
        cli_level=args.log_level,
        config_level=portfolio_cfg.get("log_level", "INFO"),
        service_env_var="FLUX_PORTFOLIO_LOG_LEVEL",
    )
    aggregator = TokenMMPortfolioAggregator(
        config=config,
        mode=mode,
        logger=logging.getLogger("nautilus-tokenmm-portfolio"),
    )
    aggregator.run()


if __name__ == "__main__":
    main()
