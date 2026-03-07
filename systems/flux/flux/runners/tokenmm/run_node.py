#!/usr/bin/env python3
"""
Run a live TokenMM trading node using canonical MakerV3 strategy exports.
"""

from __future__ import annotations

import argparse
from contextlib import contextmanager
from contextlib import suppress
from decimal import Decimal
from pathlib import Path
from typing import Any

import redis

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import DatabaseConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import MessageBusConfig
from nautilus_trader.config import TradingNodeConfig
from flux.common.config import FLUX_DEFAULT_NAMESPACE
from flux.common.config import FLUX_SCHEMA_VERSION
from flux.runners.live import resolve_strategy_venues
from flux.runners.shared.bootstrap import build_redis_client_kwargs
from flux.runners.shared.bootstrap import build_redis_database_config
from flux.runners.shared.bootstrap import load_runtime_config as load_shared_runtime_config
from flux.runners.shared.bootstrap import load_config as load_shared_config
from flux.runners.shared.bootstrap import merge_shared_tables as merge_shared_tables_from_bootstrap
from flux.runners.shared.bootstrap import resolve_flux_strategy_id as resolve_flux_strategy_id_from_bootstrap
from flux.runners.shared.bootstrap import resolve_mode as resolve_shared_mode
from flux.runners.shared.bootstrap import strategy_startup_lock
from flux.runners.shared.bootstrap import table as shared_table
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.strategies import FluxStrategySpec
from flux.strategies import get_strategy_spec
from flux.strategies.makerv3 import runtime_params as runtime_params_mod
from nautilus_trader.live.config import LiveDataEngineConfig
from nautilus_trader.live.config import LiveExecEngineConfig
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


SAFE_MODES = frozenset({"paper", "testnet", "live"})
DEFAULT_LIVE_MESSAGE_BUS_AUTOTRIM_MINS = 30
TOKENMM_DESCRIPTOR = get_strategy_set_descriptor("tokenmm")
_MAKERV3_SPEC = get_strategy_spec("makerv3")
MakerV3Strategy = _MAKERV3_SPEC.strategy_cls
MakerV3StrategyConfig = _MAKERV3_SPEC.config_cls


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _client_order_id_config(instrument_id: InstrumentId) -> dict[str, Any]:
    venue = str(instrument_id.venue).upper()
    if venue == "OKX":
        return {"use_hyphens_in_client_order_ids": False}
    return {}


def _register_cash_borrowing_venues(*, exec_clients: dict[Any, Any]) -> None:
    for venue, client_config in exec_clients.items():
        if not bool(getattr(client_config, "allow_cash_borrowing", False)):
            continue
        with suppress(KeyError):
            AccountFactory.register_cash_borrowing(str(venue))


def _load_config(path: Path) -> dict[str, Any]:
    return load_shared_config(path, env_prefix=TOKENMM_DESCRIPTOR.env_prefix)


def _merge_shared_tables(
    *,
    config: dict[str, Any],
    shared_config: dict[str, Any],
    table_names: tuple[str, ...],
) -> dict[str, Any]:
    return merge_shared_tables_from_bootstrap(
        config=config,
        shared_config=shared_config,
        table_names=table_names,
    )


def _load_runtime_config(path: Path, *, shared_config_path: Path | None = None) -> dict[str, Any]:
    return load_shared_runtime_config(
        path,
        shared_config_path=shared_config_path,
        load_config=_load_config,
        table_names=("redis", "portfolio"),
    )


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    return shared_table(data, name)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run TokenMM trading node using flux production modules.",
    )
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--shared-config", type=Path, default=None)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--enable-execution", action="store_true")
    return parser.parse_args()


def _resolve_mode(config: dict[str, Any], args: argparse.Namespace) -> str:
    return resolve_shared_mode(config, args, safe_modes=SAFE_MODES)


def _attach_runtime_params_manager(
    *,
    strategy: Any,
    redis_cfg: dict[str, Any],
    namespace: str,
    schema_version: str,
) -> None:
    redis_client = redis.Redis(**build_redis_client_kwargs(redis_cfg))
    strategy.set_params_manager_factory(
        runtime_params_mod.params_manager_factory(
            redis_client=redis_client,
            namespace=namespace,
            schema_version=schema_version,
        ),
    )


def _attach_portfolio_inventory_feed(
    *,
    strategy: Any,
    config: dict[str, Any],
    redis_cfg: dict[str, Any],
    namespace: str,
    schema_version: str,
) -> None:
    portfolio_cfg = _table(config, "portfolio")
    portfolio_id = _optional_text(portfolio_cfg.get("portfolio_id")) or TOKENMM_DESCRIPTOR.default_portfolio_id
    redis_client = redis.Redis(**build_redis_client_kwargs(redis_cfg))
    strategy.configure_portfolio_inventory_feed(
        redis_client=redis_client,
        portfolio_id=portfolio_id,
        namespace=namespace,
        schema_version=schema_version,
        stale_after_ms=int(portfolio_cfg.get("inventory_stale_after_ms", 3_000)),
    )


def _redis_database_config(redis_cfg: dict[str, Any]) -> DatabaseConfig:
    return build_redis_database_config(redis_cfg)


def _resolve_reconciliation_settings(*, mode: str, node_cfg: dict[str, Any]) -> tuple[int, float]:
    lookback_mins = int(node_cfg.get("exec_reconciliation_lookback_mins", 0))
    startup_delay_secs = float(node_cfg.get("exec_reconciliation_startup_delay_secs", 10.0))
    if mode == "live":
        lookback_mins = max(0, lookback_mins)
        startup_delay_secs = max(10.0, startup_delay_secs)
    return lookback_mins, startup_delay_secs


def _resolve_execution_filter_settings(node_cfg: dict[str, Any]) -> tuple[bool, bool]:
    return (
        bool(node_cfg.get("filter_unclaimed_external_orders", False)),
        bool(node_cfg.get("filter_position_reports", False)),
    )


def _optional_int(node_cfg: dict[str, Any], field_name: str) -> int | None:
    value = node_cfg.get(field_name)
    if value is None:
        return None
    return int(value)


def _resolve_message_bus_autotrim_mins(*, mode: str, node_cfg: dict[str, Any]) -> int | None:
    raw_value = node_cfg.get("message_bus_autotrim_mins")
    if raw_value is None:
        return DEFAULT_LIVE_MESSAGE_BUS_AUTOTRIM_MINS if mode == "live" else None

    value = int(raw_value)
    if value > 0:
        return value
    return DEFAULT_LIVE_MESSAGE_BUS_AUTOTRIM_MINS if mode == "live" else None


def _resolve_graceful_shutdown_on_exception(*, mode: str, node_cfg: dict[str, Any]) -> bool:
    return bool(node_cfg.get("graceful_shutdown_on_exception", mode == "live"))


def _resolve_flux_strategy_id(config: dict[str, Any]) -> str:
    return resolve_flux_strategy_id_from_bootstrap(config)


def _resolve_strategy_spec() -> FluxStrategySpec:
    spec = get_strategy_spec("makerv3")
    if (
        MakerV3Strategy is _MAKERV3_SPEC.strategy_cls
        and MakerV3StrategyConfig is _MAKERV3_SPEC.config_cls
    ):
        return spec
    if MakerV3Strategy is spec.strategy_cls and MakerV3StrategyConfig is spec.config_cls:
        return spec
    return FluxStrategySpec(
        name=getattr(spec, "name", "makerv3"),
        strategy_cls=MakerV3Strategy,
        config_cls=MakerV3StrategyConfig,
        param_set=getattr(spec, "param_set", "makerv3"),
        strategy_family=getattr(spec, "strategy_family", "maker_v3"),
        strategy_version=getattr(spec, "strategy_version", "v3"),
    )


@contextmanager
def _strategy_startup_lock(
    config: dict[str, Any],
    *,
    lock_dir: Path | None = None,
):
    with strategy_startup_lock(
        config,
        descriptor=TOKENMM_DESCRIPTOR,
        repo_root=_repo_root(),
        lock_dir=lock_dir,
    ):
        yield


def build_node(config: dict[str, Any], *, mode: str, force_enable_execution: bool) -> TradingNode:
    """
    Build and return a configured trading node for TokenMM.
    """
    flux = _table(config, "flux")
    identity = _table(config, "identity")
    redis_cfg = _table(config, "redis")
    node_cfg = _table(config, "node")
    strategy_cfg = _table(config, "strategy")

    strategy_id = _resolve_flux_strategy_id(config)
    external_strategy_id = _optional_text(identity.get("external_strategy_id")) or strategy_id
    trader_id = _optional_text(identity.get("trader_id")) or "MAKER-PAPER-001"
    namespace = _optional_text(flux.get("namespace")) or FLUX_DEFAULT_NAMESPACE
    schema_version = _optional_text(flux.get("schema_version")) or FLUX_SCHEMA_VERSION

    enable_execution = bool(node_cfg.get("enable_execution", force_enable_execution))
    reconciliation_lookback_mins, reconciliation_startup_delay_secs = (
        _resolve_reconciliation_settings(mode=mode, node_cfg=node_cfg)
    )
    filter_unclaimed_external_orders, filter_position_reports = _resolve_execution_filter_settings(
        node_cfg,
    )
    message_bus_autotrim_mins = _resolve_message_bus_autotrim_mins(mode=mode, node_cfg=node_cfg)
    graceful_shutdown_on_exception = _resolve_graceful_shutdown_on_exception(
        mode=mode,
        node_cfg=node_cfg,
    )
    redis_database = _redis_database_config(redis_cfg)
    strategy_venues = resolve_strategy_venues(
        config=config,
        mode=mode,
        enable_execution=enable_execution,
    )
    _register_cash_borrowing_venues(exec_clients=strategy_venues.exec_clients)
    maker_instrument_id = strategy_venues.execution_instrument_id
    reference_instrument_id = strategy_venues.reference_instrument_id

    config_node = TradingNodeConfig(
        trader_id=TraderId(trader_id),
        logging=LoggingConfig(
            log_level=str(node_cfg.get("log_level", "INFO")),
            use_pyo3=True,
        ),
        data_engine=LiveDataEngineConfig(
            graceful_shutdown_on_exception=graceful_shutdown_on_exception,
        ),
        risk_engine=LiveRiskEngineConfig(
            graceful_shutdown_on_exception=graceful_shutdown_on_exception,
        ),
        exec_engine=LiveExecEngineConfig(
            reconciliation=bool(node_cfg.get("exec_reconciliation", True)),
            reconciliation_lookback_mins=reconciliation_lookback_mins,
            reconciliation_instrument_ids=[maker_instrument_id],
            reconciliation_startup_delay_secs=reconciliation_startup_delay_secs,
            filter_unclaimed_external_orders=filter_unclaimed_external_orders,
            filter_position_reports=filter_position_reports,
            graceful_shutdown_on_exception=graceful_shutdown_on_exception,
            purge_closed_orders_interval_mins=_optional_int(
                node_cfg,
                "purge_closed_orders_interval_mins",
            ),
            purge_closed_orders_buffer_mins=_optional_int(
                node_cfg,
                "purge_closed_orders_buffer_mins",
            ),
            purge_closed_positions_interval_mins=_optional_int(
                node_cfg,
                "purge_closed_positions_interval_mins",
            ),
            purge_closed_positions_buffer_mins=_optional_int(
                node_cfg,
                "purge_closed_positions_buffer_mins",
            ),
            purge_account_events_interval_mins=_optional_int(
                node_cfg,
                "purge_account_events_interval_mins",
            ),
            purge_account_events_lookback_mins=_optional_int(
                node_cfg,
                "purge_account_events_lookback_mins",
            ),
            purge_from_database=bool(node_cfg.get("purge_from_database", False)),
        ),
        cache=CacheConfig(
            database=redis_database,
            flush_on_start=bool(node_cfg.get("cache_flush_on_start", False)),
        ),
        message_bus=MessageBusConfig(
            database=redis_database,
            encoding="json",
            autotrim_mins=message_bus_autotrim_mins,
            use_trader_prefix=False,
            use_trader_id=False,
            use_instance_id=False,
            streams_prefix=f"{namespace}:{schema_version}:in:stream:{mode}:{strategy_id}",
            stream_per_topic=True,
            types_filter=[OrderBookDeltas],
        ),
        data_clients=strategy_venues.data_clients,
        exec_clients=strategy_venues.exec_clients,
        timeout_connection=float(node_cfg.get("timeout_connection", 20.0)),
        timeout_reconciliation=float(node_cfg.get("timeout_reconciliation", 30.0)),
        timeout_portfolio=float(node_cfg.get("timeout_portfolio", 10.0)),
        timeout_disconnection=float(node_cfg.get("timeout_disconnection", 10.0)),
        timeout_post_stop=float(node_cfg.get("timeout_post_stop", 5.0)),
    )

    order_qty = Decimal(str(strategy_cfg.get("order_qty", "1000")))
    qty_raw = strategy_cfg.get("qty", strategy_cfg.get("order_qty", "1000"))
    qty = Decimal(str(qty_raw)) if qty_raw is not None else None
    strategy_spec = _resolve_strategy_spec()

    strategy = strategy_spec.strategy_cls(
        config=strategy_spec.config_cls(
            strategy_id=str(strategy_cfg.get("strategy_id", "MAKERV3-001")),
            maker_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
            external_strategy_id=external_strategy_id,
            allowed_submit_instrument_ids=[maker_instrument_id],
            external_order_claims=[maker_instrument_id],
            manage_stop=bool(strategy_cfg.get("manage_stop", mode == "live")),
            order_qty=order_qty,
            qty=qty,
            bot_on=bool(strategy_cfg.get("bot_on", False)),
            des_qty_global=float(strategy_cfg.get("des_qty_global", 0.0)),
            max_qty_global=float(strategy_cfg.get("max_qty_global", 40_000.0)),
            max_skew_bps_global=float(strategy_cfg.get("max_skew_bps_global", 20.0)),
            des_qty_local=float(strategy_cfg.get("des_qty_local", 0.0)),
            max_qty_local=float(strategy_cfg.get("max_qty_local", 0.0)),
            max_skew_bps_local=float(strategy_cfg.get("max_skew_bps_local", 0.0)),
            linear_offset_bps=float(strategy_cfg.get("linear_offset_bps", 0.0)),
            max_age_ms=int(strategy_cfg.get("max_age_ms", 10_000)),
            bid_edge1=float(strategy_cfg.get("bid_edge1", 10.0)),
            ask_edge1=float(strategy_cfg.get("ask_edge1", 10.0)),
            place_edge1=float(strategy_cfg.get("place_edge1", 2.0)),
            distance1=float(strategy_cfg.get("distance1", 2.0)),
            n_orders1=int(strategy_cfg.get("n_orders1", 5)),
            bid_edge2=float(strategy_cfg.get("bid_edge2", 25.0)),
            ask_edge2=float(strategy_cfg.get("ask_edge2", 25.0)),
            place_edge2=float(strategy_cfg.get("place_edge2", 2.0)),
            distance2=float(strategy_cfg.get("distance2", 5.0)),
            n_orders2=int(strategy_cfg.get("n_orders2", 0)),
            bid_edge3=float(strategy_cfg.get("bid_edge3", 50.0)),
            ask_edge3=float(strategy_cfg.get("ask_edge3", 50.0)),
            place_edge3=float(strategy_cfg.get("place_edge3", 2.0)),
            distance3=float(strategy_cfg.get("distance3", 5.0)),
            n_orders3=int(strategy_cfg.get("n_orders3", 0)),
            quote_fail_critical_after_count=int(
                strategy_cfg.get("quote_fail_critical_after_count", 3),
            ),
            quote_fail_critical_after_s=float(
                strategy_cfg.get("quote_fail_critical_after_s", 60.0),
            ),
            spot_cash_borrowing_policy=str(
                strategy_cfg.get("spot_cash_borrowing_policy", "none"),
            ),
            force_bot_off_on_start=bool(
                strategy_cfg.get("force_bot_off_on_start", False),
            ),
            cancel_all_instrument_orders=bool(
                strategy_cfg.get("cancel_all_instrument_orders", False),
            ),
            **_client_order_id_config(maker_instrument_id),
        ),
    )
    _attach_runtime_params_manager(
        strategy=strategy,
        redis_cfg=redis_cfg,
        namespace=namespace,
        schema_version=schema_version,
    )
    _attach_portfolio_inventory_feed(
        strategy=strategy,
        config=config,
        redis_cfg=redis_cfg,
        namespace=namespace,
        schema_version=schema_version,
    )

    node = TradingNode(config=config_node)
    node.trader.add_strategy(strategy)
    for venue, factory in strategy_venues.data_factories.items():
        node.add_data_client_factory(venue, factory)
    for venue, factory in strategy_venues.exec_factories.items():
        node.add_exec_client_factory(venue, factory)
    node.build()
    return node


def main() -> None:
    """
    Parse CLI arguments and run the TokenMM trading node.
    """
    args = _parse_args()
    config = _load_runtime_config(args.config, shared_config_path=args.shared_config)
    mode = _resolve_mode(config, args)

    node = build_node(
        config,
        mode=mode,
        force_enable_execution=bool(args.enable_execution),
    )

    with _strategy_startup_lock(config):
        try:
            node.run()
        finally:
            node.dispose()


if __name__ == "__main__":
    main()
