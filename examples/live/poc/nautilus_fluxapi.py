#!/usr/bin/env python3
# -*- coding: utf-8 -*-
from __future__ import annotations

import os
from typing import Any

import redis

from nautilus_trader.flux.api import ContractCatalogEntry
from nautilus_trader.flux.api import StrategyMetadata
from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.common.config import FLUX_DEFAULT_NAMESPACE
from nautilus_trader.flux.common.config import FLUX_SCHEMA_VERSION
from nautilus_trader.flux.common.config import FluxConfig
from nautilus_trader.flux.common.config import FluxIdentityConfig
from nautilus_trader.flux.common.config import FluxRedisConfig
from nautilus_trader.flux.common.config import FluxVenuesConfig

try:
    from examples.live.poc.contracts import INSTRUMENT_CONTRACTS
    from examples.live.poc.contracts import STRATEGY_ID
except ImportError:  # pragma: no cover
    from contracts import INSTRUMENT_CONTRACTS
    from contracts import STRATEGY_ID


DEFAULT_REDIS_HOST = "127.0.0.1"
DEFAULT_REDIS_PORT = 6380
DEFAULT_REDIS_DB = 0


def _env_bool(name: str, *, default: bool) -> bool:
    text = os.getenv(name)
    if text is None:
        return default
    return text.strip().lower() in {"1", "true", "t", "yes", "y", "on"}


def _symbol_parts(symbol: str) -> tuple[str, str]:
    text = str(symbol).strip().upper()
    if "/" in text:
        base, quote = text.split("/", maxsplit=1)
        return base, quote
    if "_" in text:
        base, quote = text.split("_", maxsplit=1)
        return base, quote
    return text, ""


def _build_contract_catalog() -> tuple[ContractCatalogEntry, ...]:
    return tuple(
        ContractCatalogEntry(
            exchange=str(item.chainsaw_exchange),
            symbol=str(item.chainsaw_symbol),
        )
        for item in INSTRUMENT_CONTRACTS
    )


def _build_strategy_metadata(contracts: tuple[ContractCatalogEntry, ...]) -> StrategyMetadata:
    first_symbol = contracts[0].symbol if contracts else ""
    default_base, default_quote = _symbol_parts(first_symbol)
    return StrategyMetadata(
        strategy_class=os.getenv("FLUX_STRATEGY_CLASS", "maker_v3"),
        strategy_groups=os.getenv("FLUX_STRATEGY_GROUPS", "tokenmm"),
        base_asset=os.getenv("FLUX_BASE_ASSET", default_base or "BASE"),
        quote_asset=os.getenv("FLUX_QUOTE_ASSET", default_quote or "QUOTE"),
    )


def _build_flux_config(contracts: tuple[ContractCatalogEntry, ...]) -> FluxConfig:
    if not contracts:
        raise ValueError("No contracts available for API startup.")

    strategy_id = os.getenv("FLUX_STRATEGY_ID", STRATEGY_ID)
    execution = contracts[0]
    reference = contracts[1] if len(contracts) > 1 else contracts[0]

    identity = FluxIdentityConfig(
        namespace=os.getenv("FLUX_NAMESPACE", FLUX_DEFAULT_NAMESPACE),
        schema_version=os.getenv("FLUX_SCHEMA_VERSION", FLUX_SCHEMA_VERSION),
        strategy_id=strategy_id,
        strategy_instance_id=os.getenv("FLUX_STRATEGY_INSTANCE_ID", strategy_id),
        trader_id=os.getenv("FLUX_TRADER_ID", "flux_api"),
        external_strategy_id=os.getenv("FLUX_EXTERNAL_STRATEGY_ID", strategy_id),
    )

    redis_config = FluxRedisConfig(
        host=os.getenv("FLUX_REDIS_HOST", DEFAULT_REDIS_HOST),
        port=int(os.getenv("FLUX_REDIS_PORT", str(DEFAULT_REDIS_PORT))),
        db=int(os.getenv("FLUX_REDIS_DB", str(DEFAULT_REDIS_DB))),
        username=os.getenv("FLUX_REDIS_USERNAME") or None,
        password=os.getenv("FLUX_REDIS_PASSWORD") or None,
    )

    venues = FluxVenuesConfig(
        execution_venue=os.getenv("FLUX_EXECUTION_VENUE", execution.exchange),
        reference_venue=os.getenv("FLUX_REFERENCE_VENUE", reference.exchange),
        execution_symbol=os.getenv("FLUX_EXECUTION_SYMBOL", execution.symbol),
        reference_symbol=os.getenv("FLUX_REFERENCE_SYMBOL", reference.symbol),
    )

    mode = os.getenv("FLUX_MODE", "paper")
    confirm_live = _env_bool("FLUX_CONFIRM_LIVE", default=False)
    return FluxConfig(
        mode=mode,
        confirm_live=confirm_live,
        identity=identity,
        redis=redis_config,
        venues=venues,
    )


def _build_redis_client(config: FluxConfig) -> redis.Redis:
    return redis.Redis(
        host=config.redis.host,
        port=config.redis.port,
        db=config.redis.db,
        username=config.redis.username,
        password=config.redis.password,
        socket_connect_timeout=config.redis.connect_timeout_secs,
        socket_timeout=config.redis.read_timeout_secs,
        decode_responses=False,
    )


def build_app() -> Any:
    contracts = _build_contract_catalog()
    flux_config = _build_flux_config(contracts)
    redis_client = _build_redis_client(flux_config)
    metadata = _build_strategy_metadata(contracts)
    return create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contracts,
        strategy_metadata=metadata,
    )


def main() -> None:
    app = build_app()
    host = os.getenv("HOST", "0.0.0.0")
    port = int(os.getenv("PORT", "5022"))
    app.run(host=host, port=port, debug=False, use_reloader=False)


if __name__ == "__main__":
    main()
