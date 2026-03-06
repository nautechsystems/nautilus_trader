#!/usr/bin/env python3
"""
Run a live TokenMM trading node using canonical MakerV3 strategy exports.
"""

from __future__ import annotations

import argparse
import fcntl
import os
import tomllib
from contextlib import contextmanager
from decimal import Decimal
from pathlib import Path
from typing import Any

import redis

from nautilus_trader.config import CacheConfig
from nautilus_trader.config import DatabaseConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import MessageBusConfig
from nautilus_trader.config import TradingNodeConfig
from flux.common.config import FLUX_DEFAULT_NAMESPACE
from flux.common.config import FLUX_SCHEMA_VERSION
from flux.runners.live import resolve_strategy_venues
from flux.runners.tokenmm.redis_runtime import apply_redis_env_overrides
from flux.strategies import MakerV3Strategy
from flux.strategies import MakerV3StrategyConfig
from flux.strategies.makerv3 import runtime_params as runtime_params_mod
from nautilus_trader.live.config import LiveExecEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


SAFE_MODES = frozenset({"paper", "testnet", "live"})


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


def _load_config(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    if not isinstance(data, dict):
        raise ValueError(f"Config root must be a table: {path}")
    return apply_redis_env_overrides(data)


def _merge_shared_tables(
    *,
    config: dict[str, Any],
    shared_config: dict[str, Any],
    table_names: tuple[str, ...],
) -> dict[str, Any]:
    merged = dict(config)
    for table_name in table_names:
        if table_name in merged:
            continue
        value = shared_config.get(table_name)
        if isinstance(value, dict):
            merged[table_name] = dict(value)
    return merged


def _load_runtime_config(path: Path, *, shared_config_path: Path | None = None) -> dict[str, Any]:
    config = _load_config(path)
    if shared_config_path is None:
        return config

    shared_config = _load_config(shared_config_path)
    return _merge_shared_tables(
        config=config,
        shared_config=shared_config,
        table_names=("redis", "portfolio"),
    )


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    value = data.get(name, {})
    if not isinstance(value, dict):
        raise ValueError(f"[{name}] must be a TOML table")
    return value


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
    flux = _table(config, "flux")
    mode = str(args.mode or flux.get("mode", "paper")).strip().lower()
    if mode not in SAFE_MODES:
        raise ValueError(f"Invalid mode {mode!r}; expected one of {sorted(SAFE_MODES)}")
    if mode == "live" and not args.confirm_live:
        raise ValueError("Live mode requires explicit --confirm-live")
    return mode


def _attach_runtime_params_manager(
    *,
    strategy: Any,
    redis_cfg: dict[str, Any],
    namespace: str,
    schema_version: str,
) -> None:
    redis_client = redis.Redis(
        host=str(redis_cfg.get("host", "127.0.0.1")),
        port=int(redis_cfg.get("port", 6380)),
        db=int(redis_cfg.get("db", 0)),
        username=_optional_text(redis_cfg.get("username")),
        password=_optional_text(redis_cfg.get("password")),
        ssl=bool(redis_cfg.get("ssl", False)),
        socket_connect_timeout=float(redis_cfg.get("connect_timeout_secs", 5.0)),
        socket_timeout=float(redis_cfg.get("read_timeout_secs", 5.0)),
        decode_responses=False,
    )
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
    portfolio_id = _optional_text(portfolio_cfg.get("portfolio_id"))
    if portfolio_id is None:
        return
    redis_client = redis.Redis(
        host=str(redis_cfg.get("host", "127.0.0.1")),
        port=int(redis_cfg.get("port", 6380)),
        db=int(redis_cfg.get("db", 0)),
        username=_optional_text(redis_cfg.get("username")),
        password=_optional_text(redis_cfg.get("password")),
        ssl=bool(redis_cfg.get("ssl", False)),
        socket_connect_timeout=float(redis_cfg.get("connect_timeout_secs", 5.0)),
        socket_timeout=float(redis_cfg.get("read_timeout_secs", 5.0)),
        decode_responses=False,
    )
    strategy.configure_portfolio_inventory_feed(
        redis_client=redis_client,
        portfolio_id=portfolio_id,
        namespace=namespace,
        schema_version=schema_version,
        stale_after_ms=int(portfolio_cfg.get("inventory_stale_after_ms", 3_000)),
    )


def _redis_database_config(redis_cfg: dict[str, Any]) -> DatabaseConfig:
    return DatabaseConfig(
        type="redis",
        host=str(redis_cfg.get("host", "127.0.0.1")),
        port=int(redis_cfg.get("port", 6380)),
        username=_optional_text(redis_cfg.get("username")),
        password=_optional_text(redis_cfg.get("password")),
        ssl=bool(redis_cfg.get("ssl", False)),
    )


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


def _resolve_flux_strategy_id(config: dict[str, Any]) -> str:
    identity = _table(config, "identity")
    return _optional_text(identity.get("strategy_id")) or "makerv3"


@contextmanager
def _strategy_startup_lock(
    config: dict[str, Any],
    *,
    lock_dir: Path | None = None,
):
    strategy_id = _resolve_flux_strategy_id(config)
    root = lock_dir or (_repo_root() / ".run" / "tokenmm-strategy-locks")
    root.mkdir(parents=True, exist_ok=True)
    lock_path = root / f"{strategy_id}.lock"
    lock_handle = lock_path.open("a+", encoding="utf-8")
    try:
        try:
            fcntl.flock(lock_handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            lock_handle.seek(0)
            owner = lock_handle.read().strip()
            detail = f" ({owner})" if owner else ""
            raise RuntimeError(
                f"TokenMM strategy `{strategy_id}` is already running{detail}",
            ) from exc

        lock_handle.seek(0)
        lock_handle.truncate()
        lock_handle.write(f"pid={os.getpid()}\n")
        lock_handle.flush()
        yield
    finally:
        try:
            fcntl.flock(lock_handle.fileno(), fcntl.LOCK_UN)
        finally:
            lock_handle.close()


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
    redis_database = _redis_database_config(redis_cfg)
    strategy_venues = resolve_strategy_venues(
        config=config,
        mode=mode,
        enable_execution=enable_execution,
    )
    maker_instrument_id = strategy_venues.execution_instrument_id
    reference_instrument_id = strategy_venues.reference_instrument_id

    config_node = TradingNodeConfig(
        trader_id=TraderId(trader_id),
        logging=LoggingConfig(
            log_level=str(node_cfg.get("log_level", "INFO")),
            use_pyo3=True,
        ),
        exec_engine=LiveExecEngineConfig(
            reconciliation=bool(node_cfg.get("exec_reconciliation", True)),
            reconciliation_lookback_mins=reconciliation_lookback_mins,
            reconciliation_instrument_ids=[maker_instrument_id],
            reconciliation_startup_delay_secs=reconciliation_startup_delay_secs,
            filter_unclaimed_external_orders=filter_unclaimed_external_orders,
            filter_position_reports=filter_position_reports,
        ),
        cache=CacheConfig(
            database=redis_database,
            flush_on_start=bool(node_cfg.get("cache_flush_on_start", False)),
        ),
        message_bus=MessageBusConfig(
            database=redis_database,
            encoding="json",
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

    strategy = MakerV3Strategy(
        config=MakerV3StrategyConfig(
            strategy_id=str(strategy_cfg.get("strategy_id", "MAKERV3-001")),
            maker_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
            external_strategy_id=external_strategy_id,
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
