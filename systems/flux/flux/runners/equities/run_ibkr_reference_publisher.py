#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
from typing import Any

from flux.runners.shared.bootstrap import build_redis_client
from flux.runners.shared.bootstrap import load_config as load_shared_config
from flux.runners.shared.bootstrap import resolve_mode as resolve_shared_mode
from flux.runners.shared.bootstrap import table as shared_table
from flux.runners.shared.ibkr_reference_publisher import (
    IbkrReferencePublisherService,
)
from flux.runners.shared.ibkr_reference_publisher import build_ibkr_reference_publisher_config
from flux.runners.shared.logging import configure_python_logging
from flux.runners.shared.strategy_set import get_strategy_set_descriptor


SAFE_MODES = frozenset({"paper", "testnet", "live"})
EQUITIES_DESCRIPTOR = get_strategy_set_descriptor("equities")


def _load_config(path: Path) -> dict[str, Any]:
    return load_shared_config(path, env_prefix=EQUITIES_DESCRIPTOR.env_prefix)


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    return shared_table(data, name)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the Equities shared IBKR reference publisher.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--log-level", default=None)
    return parser.parse_args()


def _resolve_mode(config: dict[str, Any], args: argparse.Namespace) -> str:
    return resolve_shared_mode(config, args, safe_modes=SAFE_MODES)


def main() -> None:
    args = _parse_args()
    config = _load_config(args.config)
    _resolve_mode(config, args)

    publisher_cfg = _table(config, "ibkr_reference_publisher")
    redis_cfg = _table(config, "redis")
    configure_python_logging(
        cli_level=args.log_level,
        config_level=publisher_cfg.get("log_level", "INFO"),
        service_env_var="FLUX_IBKR_REFERENCE_PUBLISHER_LOG_LEVEL",
    )

    service = IbkrReferencePublisherService(
        config=build_ibkr_reference_publisher_config(config),
        redis_client=build_redis_client(redis_cfg),
    )
    service.run_forever()


if __name__ == "__main__":
    main()
