#!/usr/bin/env python3
"""
Run a live Equities trading node using the canonical equities strategy spec.
"""

from __future__ import annotations

import argparse
import inspect
import logging
from contextlib import contextmanager
from contextlib import suppress
from decimal import Decimal
from pathlib import Path
from typing import Any

import redis

from flux.common.config import FLUX_DEFAULT_NAMESPACE
from flux.common.config import FLUX_SCHEMA_VERSION
from flux.common.account_scopes import decode_account_scopes
from flux.common.strategy_contracts import decode_strategy_contracts
from flux.runners.live import resolve_strategy_venues
from flux.runners.shared.bootstrap import build_redis_client_kwargs
from flux.runners.shared.bootstrap import build_redis_database_config
from flux.runners.shared.bootstrap import load_config as load_shared_config
from flux.runners.shared.bootstrap import load_runtime_config as load_shared_runtime_config
from flux.runners.shared.bootstrap import merge_shared_tables as merge_shared_tables_from_bootstrap
from flux.runners.shared.bootstrap import (
    resolve_flux_strategy_id as resolve_flux_strategy_id_from_bootstrap,
)
from flux.runners.shared.bootstrap import resolve_mode as resolve_shared_mode
from flux.runners.shared.bootstrap import strategy_startup_lock
from flux.runners.shared.bootstrap import table as shared_table
from flux.runners.shared.logging import build_node_logging_config
from flux.runners.shared.qty_units import resolve_runner_qty_unit
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.strategies import FluxStrategySpec
from flux.strategies import get_strategy_spec
from flux.strategies.makerv3 import runtime_params as makerv3_runtime_params_mod
from flux.strategies.makerv4 import runtime_params as makerv4_runtime_params_mod
from flux.strategies.makerv4.instruments import hyperliquid_perp_to_ibkr_instrument_id
from flux.strategies.makerv4.reference_balances import IbkrReferenceBalanceSnapshotProviderConfig
from flux.strategies.makerv4.reference_balances import get_cached_ibkr_reference_balance_provider
from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import DatabaseConfig
from nautilus_trader.config import MessageBusConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.config import LiveDataEngineConfig
from nautilus_trader.live.config import LiveExecEngineConfig
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.live.node import TradingNodeFatalError
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


SAFE_MODES = frozenset({"paper", "testnet", "live"})
DEFAULT_LIVE_MESSAGE_BUS_AUTOTRIM_MINS = 30
EQUITIES_DESCRIPTOR = get_strategy_set_descriptor("equities")
_MAKERV3_SPEC = get_strategy_spec("makerv3")
_MAKERV4_SPEC = get_strategy_spec("makerv4")
LOGGER = logging.getLogger(__name__)
MakerV3Strategy = _MAKERV3_SPEC.strategy_cls
MakerV3StrategyConfig = _MAKERV3_SPEC.config_cls
MakerV4Strategy = _MAKERV4_SPEC.strategy_cls
MakerV4StrategyConfig = _MAKERV4_SPEC.config_cls


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _profile_owned_account_projections_enabled(config: dict[str, Any]) -> bool:
    portfolio_cfg = config.get("portfolio")
    if not isinstance(portfolio_cfg, dict):
        return True
    return bool(portfolio_cfg.get("profile_owned_account_projections", True))


def _dockerized_ib_gateway_config(ibkr_cfg: dict[str, Any]) -> DockerizedIBGatewayConfig | None:
    dockerized_gateway_cfg = ibkr_cfg.get("dockerized_gateway")
    if isinstance(dockerized_gateway_cfg, DockerizedIBGatewayConfig):
        return dockerized_gateway_cfg
    if isinstance(dockerized_gateway_cfg, dict):
        return DockerizedIBGatewayConfig(**dockerized_gateway_cfg)
    if dockerized_gateway_cfg is not None:
        raise ValueError("`node.venues.IBKR.dockerized_gateway` must be a TOML table")
    return None


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
    return load_shared_config(path, env_prefix=EQUITIES_DESCRIPTOR.env_prefix)


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
        table_names=("redis", "portfolio", "strategy_contracts", "account_scopes"),
    )


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    return shared_table(data, name)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run Equities trading node using flux production modules.",
    )
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--shared-config", type=Path, default=None)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--enable-execution", action="store_true")
    parser.add_argument("--log-level", default=None)
    return parser.parse_args()


def _resolve_mode(config: dict[str, Any], args: argparse.Namespace) -> str:
    return resolve_shared_mode(config, args, safe_modes=SAFE_MODES)


def _attach_runtime_params_manager(
    *,
    strategy: Any,
    redis_cfg: dict[str, Any],
    namespace: str,
    schema_version: str,
    strategy_spec: FluxStrategySpec,
) -> None:
    redis_client = redis.Redis(**build_redis_client_kwargs(redis_cfg))
    runtime_params_mod = _runtime_params_module(strategy_spec)
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
    portfolio_id = _optional_text(portfolio_cfg.get("portfolio_id")) or EQUITIES_DESCRIPTOR.default_portfolio_id
    redis_client = redis.Redis(**build_redis_client_kwargs(redis_cfg))
    strategy.configure_portfolio_inventory_feed(
        redis_client=redis_client,
        portfolio_id=portfolio_id,
        namespace=namespace,
        schema_version=schema_version,
        stale_after_ms=int(portfolio_cfg.get("inventory_stale_after_ms", 3_000)),
        allow_partial_global_risk=bool(portfolio_cfg.get("allow_partial_global_risk", False)),
    )


def _attach_profile_account_projection_feed(
    *,
    strategy: Any,
    redis_cfg: dict[str, Any],
    namespace: str,
    schema_version: str,
) -> None:
    configure_projection_feed = getattr(strategy, "configure_profile_account_projection_feed", None)
    if not callable(configure_projection_feed):
        return
    account_scope_id = _optional_text(getattr(strategy.config, "execution_account_scope_id", None))
    if account_scope_id is None:
        return
    redis_client = redis.Redis(**build_redis_client_kwargs(redis_cfg))
    configure_projection_feed(
        redis_client=redis_client,
        profile_id=EQUITIES_DESCRIPTOR.profile,
        account_scope_id=account_scope_id,
        namespace=namespace,
        schema_version=schema_version,
    )


def _attach_reference_balance_snapshot_provider(
    *,
    strategy: Any,
    config: dict[str, Any],
    strategy_spec: FluxStrategySpec,
) -> None:
    if (
        _uses_profile_account_projection(strategy_spec)
        and _profile_owned_account_projections_enabled(config)
    ):
        return

    configure_provider = getattr(strategy, "configure_reference_balance_snapshot_provider", None)
    if not callable(configure_provider):
        return

    venues_cfg = _table(config, "venues")
    reference_venue = (_optional_text(venues_cfg.get("reference_venue")) or "").upper()
    if reference_venue != "IBKR":
        return

    node_cfg = _table(config, "node")
    venue_entries = node_cfg.get("venues")
    if not isinstance(venue_entries, dict):
        return
    ibkr_cfg = venue_entries.get("IBKR")
    if not isinstance(ibkr_cfg, dict):
        return
    adapter_id = (_optional_text(ibkr_cfg.get("adapter")) or "ibkr").lower()
    if adapter_id not in {"ibkr", "interactive_brokers"}:
        return

    dockerized_gateway = _dockerized_ib_gateway_config(ibkr_cfg)
    provider = get_cached_ibkr_reference_balance_provider(
        IbkrReferenceBalanceSnapshotProviderConfig(
            ibg_host=_optional_text(ibkr_cfg.get("ibg_host")) or "127.0.0.1",
            ibg_port=(
                None
                if dockerized_gateway is not None or ibkr_cfg.get("ibg_port") is None
                else int(ibkr_cfg.get("ibg_port"))
            ),
            ibg_client_id=int(ibkr_cfg.get("ibg_client_id", 1)),
            dockerized_gateway=dockerized_gateway,
            connection_timeout=int(ibkr_cfg.get("connection_timeout", 300)),
            request_timeout_secs=int(ibkr_cfg.get("request_timeout_secs", 60)),
            account_id=_optional_text(ibkr_cfg.get("account_id")),
        ),
    )
    configure_provider(provider)


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


def _resolve_strategy_param_set(config: dict[str, Any]) -> str:
    strategy_cfg = _table(config, "strategy")
    return _optional_text(strategy_cfg.get("param_set")) or _MAKERV3_SPEC.strategy_id


def _resolve_strategy_spec(config: dict[str, Any]) -> FluxStrategySpec:
    return get_strategy_spec(_resolve_strategy_param_set(config))


def _runtime_params_module(strategy_spec: FluxStrategySpec):
    if strategy_spec.param_set == "makerv4":
        return makerv4_runtime_params_mod
    return makerv3_runtime_params_mod


def _supports_immediate_hedge(strategy_spec: FluxStrategySpec) -> bool:
    capabilities = getattr(strategy_spec, "capabilities", None)
    if capabilities is not None and hasattr(capabilities, "supports_immediate_hedge"):
        return bool(capabilities.supports_immediate_hedge)
    return getattr(strategy_spec, "param_set", "") == "makerv4"


def _uses_profile_account_projection(strategy_spec: FluxStrategySpec) -> bool:
    capabilities = getattr(strategy_spec, "capabilities", None)
    if capabilities is not None and hasattr(capabilities, "uses_profile_account_projection"):
        return bool(capabilities.uses_profile_account_projection)
    return True


def _strategy_allowed_instrument_ids(
    *,
    strategy_spec: FluxStrategySpec,
    maker_instrument_id: InstrumentId,
    reference_instrument_id: InstrumentId,
) -> list[InstrumentId]:
    if _supports_immediate_hedge(strategy_spec):
        return [maker_instrument_id, reference_instrument_id]
    return [maker_instrument_id]


def _external_strategy_id(config: dict[str, Any]) -> str:
    identity_cfg = config.get("identity")
    return (
        _optional_text(identity_cfg.get("external_strategy_id"))
        if isinstance(identity_cfg, dict)
        else None
    ) or _resolve_flux_strategy_id(config)


def _strategy_contract_for_strategy_id(config: dict[str, Any], *, strategy_id: str):
    for contract in decode_strategy_contracts(config.get("strategy_contracts") or []):
        if contract.strategy_id == strategy_id:
            return contract
    return None


def _ibkr_scope_overrides_for_contract(
    *,
    config: dict[str, Any],
    contract: Any,
    ibkr_cfg: dict[str, Any],
) -> dict[str, Any]:
    ibkr_scope_overrides: dict[str, Any] = {}
    scope_configs = {
        scope.scope_id: scope
        for scope in decode_account_scopes(config.get("account_scopes") or [])
    }
    scope_id = contract.hedge_account_scope_id or contract.reference_account_scope_id
    scope = scope_configs.get(scope_id)
    if scope is None or scope.provider.lower() != "ibkr":
        return ibkr_scope_overrides
    if scope.ibg_host is not None:
        ibkr_scope_overrides["ibg_host"] = scope.ibg_host
    if scope.ibg_port is not None and scope.dockerized_gateway is None:
        ibkr_scope_overrides["ibg_port"] = scope.ibg_port
    if scope.ibg_client_id is not None and ibkr_cfg.get("ibg_client_id") in (None, ""):
        ibkr_scope_overrides["ibg_client_id"] = scope.ibg_client_id
    if scope.account_id is not None:
        ibkr_scope_overrides["account_id"] = scope.account_id
    if scope.dockerized_gateway is not None:
        ibkr_scope_overrides["dockerized_gateway"] = dict(scope.dockerized_gateway)
    return ibkr_scope_overrides


def _effective_venue_resolution_config(
    *,
    config: dict[str, Any],
    strategy_spec: FluxStrategySpec,
) -> dict[str, Any]:
    if not _supports_immediate_hedge(strategy_spec):
        return config

    node_cfg = config.get("node")
    if not isinstance(node_cfg, dict):
        return config

    venue_entries = node_cfg.get("venues")
    if not isinstance(venue_entries, dict):
        return config

    ibkr_cfg = venue_entries.get("IBKR")
    if not isinstance(ibkr_cfg, dict):
        return config

    external_strategy_id = _external_strategy_id(config)
    contract = _strategy_contract_for_strategy_id(
        config,
        strategy_id=external_strategy_id,
    )

    maker_venue_name = "HYPERLIQUID"
    maker_cfg = venue_entries.get(maker_venue_name)
    desired_maker_instrument_id: str | None = None
    desired_reference_instrument_id: str | None = None
    ibkr_scope_overrides: dict[str, Any] = {}
    if contract is not None:
        maker_venue_name = contract.maker_venue.upper()
        maker_cfg = venue_entries.get(maker_venue_name)
        desired_maker_instrument_id = contract.maker_instrument_id
        desired_reference_instrument_id = contract.reference_instrument_id
        ibkr_scope_overrides = _ibkr_scope_overrides_for_contract(
            config=config,
            contract=contract,
            ibkr_cfg=ibkr_cfg,
        )
    else:
        if not isinstance(maker_cfg, dict):
            return config
        maker_instrument_id = _optional_text(maker_cfg.get("instrument_id"))
        if maker_instrument_id is None:
            return config
        strategy_cfg = _table(config, "strategy")
        desired_reference_instrument_id = hyperliquid_perp_to_ibkr_instrument_id(
            maker_instrument_id,
            primary_exchange=str(strategy_cfg.get("ibkr_primary_exchange", "NASDAQ")),
        )

    needs_reference_rewrite = (
        _optional_text(ibkr_cfg.get("instrument_id")) != desired_reference_instrument_id
    )
    needs_execution_promotion = not bool(ibkr_cfg.get("execution", False))
    needs_scope_overlay = bool(ibkr_scope_overrides)
    needs_maker_rewrite = (
        desired_maker_instrument_id is not None
        and isinstance(maker_cfg, dict)
        and _optional_text(maker_cfg.get("instrument_id")) != desired_maker_instrument_id
    )
    if (
        not needs_reference_rewrite
        and not needs_execution_promotion
        and not needs_scope_overlay
        and not needs_maker_rewrite
    ):
        return config

    effective_node_cfg = dict(node_cfg)
    effective_venue_entries = dict(venue_entries)
    if needs_maker_rewrite and isinstance(maker_cfg, dict):
        effective_venue_entries[maker_venue_name] = {
            **maker_cfg,
            "instrument_id": desired_maker_instrument_id,
        }
    effective_venue_entries["IBKR"] = {
        **ibkr_cfg,
        **ibkr_scope_overrides,
        "instrument_id": desired_reference_instrument_id,
        "execution": True,
    }
    effective_node_cfg["venues"] = effective_venue_entries
    return {
        **config,
        "node": effective_node_cfg,
    }


def _strategy_config_accepts(config_cls: type[object], field_name: str) -> bool:
    try:
        signature = inspect.signature(config_cls)
    except (TypeError, ValueError):
        return False
    if field_name in signature.parameters:
        return True
    return any(
        parameter.kind is inspect.Parameter.VAR_KEYWORD
        for parameter in signature.parameters.values()
    )


def _optional_strategy_config_kwargs(
    *,
    config: dict[str, Any],
    external_strategy_id: str,
    strategy_spec: FluxStrategySpec,
    strategy_cfg: dict[str, Any],
) -> dict[str, Any]:
    candidates: dict[str, Any] = {
        "reference_use_quote_ticks": bool(
            strategy_cfg.get("reference_use_quote_ticks", False),
        ),
        "outside_rth_hedge_enabled": bool(strategy_cfg.get("outside_rth_hedge_enabled", False)),
        "hedge_price_tick_size": Decimal(str(strategy_cfg.get("hedge_price_tick_size", "0.01"))),
        "hedge_min_share_increment": Decimal(
            str(strategy_cfg.get("hedge_min_share_increment", "1")),
        ),
        "max_ibkr_quote_age_ms": int(strategy_cfg.get("max_ibkr_quote_age_ms", 1_000)),
        "max_ibkr_spread_bps": Decimal(str(strategy_cfg.get("max_ibkr_spread_bps", "25"))),
        "ibkr_primary_exchange": str(strategy_cfg.get("ibkr_primary_exchange", "NASDAQ")),
    }
    for contract in decode_strategy_contracts(config.get("strategy_contracts") or []):
        if contract.strategy_id != external_strategy_id:
            continue
        candidates["portfolio_asset_id"] = contract.portfolio_asset_id
        candidates["execution_account_scope_id"] = contract.execution_account_scope_id
        break
    return {
        field_name: value
        for field_name, value in candidates.items()
        if _strategy_config_accepts(strategy_spec.config_cls, field_name)
    }


@contextmanager
def _strategy_startup_lock(
    config: dict[str, Any],
    *,
    lock_dir: Path | None = None,
):
    with strategy_startup_lock(
        config,
        descriptor=EQUITIES_DESCRIPTOR,
        repo_root=_repo_root(),
        lock_dir=lock_dir,
    ):
        yield


def build_node(
    config: dict[str, Any],
    *,
    mode: str,
    force_enable_execution: bool,
    log_level_override: str | None = None,
) -> TradingNode:
    """
    Build and return a configured trading node for Equities.
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
    strategy_spec = _resolve_strategy_spec(config)
    venue_resolution_config = _effective_venue_resolution_config(
        config=config,
        strategy_spec=strategy_spec,
    )

    enable_execution = bool(node_cfg.get("enable_execution", False) or force_enable_execution)
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
        config=venue_resolution_config,
        mode=mode,
        enable_execution=enable_execution,
    )
    _register_cash_borrowing_venues(exec_clients=strategy_venues.exec_clients)
    maker_instrument_id = strategy_venues.execution_instrument_id
    reference_instrument_id = strategy_venues.reference_instrument_id
    allowed_instrument_ids = _strategy_allowed_instrument_ids(
        strategy_spec=strategy_spec,
        maker_instrument_id=maker_instrument_id,
        reference_instrument_id=reference_instrument_id,
    )

    config_node = TradingNodeConfig(
        trader_id=TraderId(trader_id),
        logging=build_node_logging_config(
            cli_level=log_level_override,
            config_level=node_cfg.get("log_level", "INFO"),
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
            reconciliation_instrument_ids=allowed_instrument_ids,
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

    order_qty = Decimal(str(strategy_cfg.get("order_qty", "1")))
    qty_raw = strategy_cfg.get("qty", strategy_cfg.get("order_qty", "1"))
    qty = Decimal(str(qty_raw)) if qty_raw is not None else None
    qty_unit = resolve_runner_qty_unit(
        strategy_cfg,
        strategy_id=external_strategy_id,
        logger=LOGGER,
    )

    strategy = strategy_spec.strategy_cls(
        config=strategy_spec.config_cls(
            strategy_id=str(strategy_cfg.get("strategy_id", "MAKERV3-001")),
            maker_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
            external_strategy_id=external_strategy_id,
            allowed_submit_instrument_ids=allowed_instrument_ids,
            external_order_claims=allowed_instrument_ids,
            manage_stop=bool(strategy_cfg.get("manage_stop", False)),
            order_qty=order_qty,
            qty_unit=qty_unit,
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
            **_optional_strategy_config_kwargs(
                config=config,
                external_strategy_id=external_strategy_id,
                strategy_spec=strategy_spec,
                strategy_cfg=strategy_cfg,
            ),
            **_client_order_id_config(maker_instrument_id),
        ),
    )
    _attach_runtime_params_manager(
        strategy=strategy,
        redis_cfg=redis_cfg,
        namespace=namespace,
        schema_version=schema_version,
        strategy_spec=strategy_spec,
    )
    _attach_portfolio_inventory_feed(
        strategy=strategy,
        config=config,
        redis_cfg=redis_cfg,
        namespace=namespace,
        schema_version=schema_version,
    )
    _attach_profile_account_projection_feed(
        strategy=strategy,
        redis_cfg=redis_cfg,
        namespace=namespace,
        schema_version=schema_version,
    )
    _attach_reference_balance_snapshot_provider(
        strategy=strategy,
        config=venue_resolution_config,
        strategy_spec=strategy_spec,
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
    Parse CLI arguments and run the Equities trading node.
    """
    args = _parse_args()
    config = _load_runtime_config(args.config, shared_config_path=args.shared_config)
    mode = _resolve_mode(config, args)

    node = build_node(
        config,
        mode=mode,
        force_enable_execution=bool(args.enable_execution),
        log_level_override=args.log_level,
    )

    with _strategy_startup_lock(config):
        fatal_error: TradingNodeFatalError | None = None
        try:
            node.run()
        except TradingNodeFatalError as exc:
            fatal_error = exc
        finally:
            node.dispose()
        if fatal_error is not None:
            raise SystemExit(fatal_error.exit_code)


if __name__ == "__main__":
    main()
