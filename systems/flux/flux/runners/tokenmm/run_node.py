#!/usr/bin/env python3
"""
Run a live TokenMM trading node using canonical MakerV3 strategy exports.
"""

from __future__ import annotations

import argparse
from contextlib import contextmanager
from contextlib import suppress
from decimal import Decimal
import logging
from pathlib import Path
import re
from types import SimpleNamespace
from typing import Any

import redis

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import DatabaseConfig
from nautilus_trader.config import MessageBusConfig
from nautilus_trader.config import TradingNodeConfig
from flux.common.strategy_contracts import StrategyContractEntry
from flux.common.strategy_contracts import decode_strategy_contracts
from flux.common.config import FLUX_DEFAULT_NAMESPACE
from flux.common.config import FLUX_SCHEMA_VERSION
from flux.execution.controller import VenueActivityOrigin
from flux.execution.events import ExecutionLifecycleEvent
from flux.execution.intents import ExecutionIntent
from flux.execution.intents import ExecutionLifecycleState
from flux.execution.transport import ControllerIntentCommandPayload
from flux.execution.transport import ControllerIntentRequest
from flux.execution.transport import ControllerIntentReply
from flux.execution.transport import UdsTransportPaths
from flux.execution.transport import REPLY_STATUS_REJECTED
from flux.execution.transport import send_request as send_transport_request
from flux.runners.live import resolve_strategy_venues
from flux.runners.shared.bootstrap import build_redis_client_kwargs
from flux.runners.shared.bootstrap import build_redis_database_config
from flux.runners.shared.bootstrap import load_runtime_config as load_shared_runtime_config
from flux.runners.shared.bootstrap import load_config as load_shared_config
from flux.runners.shared.bootstrap import merge_shared_tables as merge_shared_tables_from_bootstrap
from flux.runners.shared.bootstrap import resolve_flux_strategy_id as resolve_flux_strategy_id_from_bootstrap
from flux.runners.shared.bootstrap import resolve_mode as resolve_shared_mode
from flux.runners.shared.bootstrap import strategy_startup_lock
from flux.runners.shared.logging import build_node_logging_config
from flux.runners.shared.qty_units import resolve_runner_qty_unit
from flux.runners.shared.bootstrap import table as shared_table
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.strategies import FluxStrategySpec
from flux.strategies import get_strategy_spec
from flux.strategies.makerv4.strategy import ControllerStateFeedBridge
from flux.strategies.makerv3.constants import TOPIC_ORDER_INTENT
from flux.strategies.makerv3 import runtime_params as runtime_params_mod
from nautilus_trader.live.config import LiveDataEngineConfig
from nautilus_trader.live.config import LiveExecEngineConfig
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.live.node import TradingNodeFatalError
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


SAFE_MODES = frozenset({"paper", "testnet", "live"})
DEFAULT_LIVE_MESSAGE_BUS_AUTOTRIM_MINS = 30
TOKENMM_DESCRIPTOR = get_strategy_set_descriptor("tokenmm")
_MAKERV3_SPEC = get_strategy_spec("makerv3")
LOGGER = logging.getLogger(__name__)
MakerV3Strategy = _MAKERV3_SPEC.strategy_cls
MakerV3StrategyConfig = _MAKERV3_SPEC.config_cls
_ORDER_SIDE_VALUE_NAMES = {
    str(member.value): name.upper()
    for name, member in getattr(OrderSide, "__members__", {}).items()
}
_TIME_IN_FORCE_VALUE_NAMES = {
    str(member.value): name.upper()
    for name, member in getattr(TimeInForce, "__members__", {}).items()
}


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[5]


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _normalize_enum_text(value: Any, *, numeric_names: dict[str, str] | None = None) -> str | None:
    name = getattr(value, "name", None)
    if isinstance(name, str) and name.strip():
        return name.strip().upper()
    text = _optional_text(value)
    if text is None:
        return None
    normalized = text.upper()
    if numeric_names is not None:
        return numeric_names.get(normalized, normalized)
    return normalized


def _normalize_order_side_text(value: Any) -> str | None:
    return _normalize_enum_text(value, numeric_names=_ORDER_SIDE_VALUE_NAMES)


def _normalize_time_in_force_text(value: Any) -> str | None:
    return _normalize_enum_text(value, numeric_names=_TIME_IN_FORCE_VALUE_NAMES)


def _coerce_order_side_enum(value: Any) -> Any:
    normalized = _normalize_order_side_text(value)
    if normalized == "BUY":
        return OrderSide.BUY
    if normalized == "SELL":
        return OrderSide.SELL
    return value


def _markout_component_id(benchmark_name: str) -> str:
    slug = re.sub(r"[^A-Za-z0-9]+", "-", benchmark_name).strip("-").upper()
    return f"MARKOUT-DB-{slug or 'DEFAULT'}"


def _build_markout_actor_configs(
    *,
    telemetry: dict[str, Any],
    markouts_db_path: str,
) -> list[dict[str, Any]]:
    base_config: dict[str, Any] = {"db_path": markouts_db_path}
    raw_horizons = telemetry.get("markout_horizons_s")
    if isinstance(raw_horizons, list | tuple):
        base_config["horizons_s"] = [int(value) for value in raw_horizons]

    raw_benchmarks = telemetry.get("markout_benchmarks")
    if isinstance(raw_benchmarks, list) and raw_benchmarks:
        actor_configs: list[dict[str, Any]] = []
        for raw_benchmark in raw_benchmarks:
            if not isinstance(raw_benchmark, dict):
                continue
            benchmark_name = _optional_text(raw_benchmark.get("benchmark_name"))
            if benchmark_name is None:
                continue
            benchmark_field = _optional_text(raw_benchmark.get("benchmark_field")) or "fv"
            actor_config = dict(base_config)
            actor_config.update(
                {
                    "component_id": _markout_component_id(benchmark_name),
                    "benchmark_name": benchmark_name,
                    "benchmark_field": benchmark_field,
                },
            )
            actor_configs.append(actor_config)
        if actor_configs:
            return actor_configs

    actor_config = dict(base_config)
    actor_config.update(
        {
            "component_id": _markout_component_id("fv_market_mid"),
            "benchmark_name": "fv_market_mid",
            "benchmark_field": "fv",
        },
    )
    return [actor_config]


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
        table_names=("redis", "portfolio", "telemetry_shipper", "controller", "strategy_contracts"),
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
        allow_partial_global_risk=bool(portfolio_cfg.get("allow_partial_global_risk", False)),
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


def _enforce_live_startup_reconciliation_guardrails(
    *,
    mode: str,
    node_cfg: dict[str, Any],
    enable_execution: bool,
) -> None:
    if mode != "live" or not enable_execution:
        return

    if not bool(node_cfg.get("exec_reconciliation", True)):
        raise ValueError(
            "live TokenMM nodes with execution enabled require exec_reconciliation=true",
        )

    if bool(node_cfg.get("filter_position_reports", False)):
        raise ValueError(
            "live TokenMM nodes with execution enabled require filter_position_reports=false",
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


def _strategy_contract_for_strategy(
    config: dict[str, Any],
    *,
    external_strategy_id: str,
) -> StrategyContractEntry | None:
    for contract in decode_strategy_contracts(config.get("strategy_contracts") or []):
        if contract.strategy_id == external_strategy_id:
            return contract
    return None


def _controller_managed_contract_for_strategy(
    config: dict[str, Any],
    *,
    external_strategy_id: str,
) -> StrategyContractEntry | None:
    contract = _strategy_contract_for_strategy(
        config,
        external_strategy_id=external_strategy_id,
    )
    if contract is None or contract.controller_scope_id is None:
        return None
    controller_cfg = config.get("controller")
    if not isinstance(controller_cfg, dict):
        return None
    managed_strategy_ids = controller_cfg.get("managed_strategy_ids")
    if isinstance(managed_strategy_ids, list | tuple) and external_strategy_id not in {
        str(item).strip() for item in managed_strategy_ids if str(item).strip()
    }:
        return None
    return contract


def _strategy_timestamp_ns(strategy: Any) -> int:
    clock = getattr(strategy, "clock", None)
    if clock is None:
        clock = getattr(strategy, "_clock", None)
    timestamp_ns = getattr(clock, "timestamp_ns", None)
    if callable(timestamp_ns):
        with suppress(Exception):
            return int(timestamp_ns())
    return 0


def _strategy_runtime_id(strategy: Any) -> str:
    for attr in ("runtime_strategy_id", "_external_strategy_id"):
        value = _optional_text(getattr(strategy, attr, None))
        if value is not None:
            return value
    config = getattr(strategy, "config", None)
    strategy_id = _optional_text(getattr(config, "external_strategy_id", None))
    if strategy_id is not None:
        return strategy_id
    strategy_id = _optional_text(getattr(config, "strategy_id", None))
    if strategy_id is not None:
        return strategy_id
    return "tokenmm"


def _set_order_client_order_id(order: Any, client_order_id: str) -> None:
    try:
        setattr(order, "client_order_id", client_order_id)
        return
    except Exception:
        pass
    with suppress(Exception):
        object.__setattr__(order, "client_order_id", client_order_id)


def _order_post_only(order: Any) -> bool | None:
    for attr in ("is_post_only", "post_only"):
        value = getattr(order, attr, None)
        if callable(value):
            with suppress(Exception):
                return bool(value())
        if value is not None:
            return bool(value)
    return None


def _controller_order_snapshot(row: dict[str, Any]) -> Any:
    pending_cancel = bool(row.get("pending_cancel", False))
    return SimpleNamespace(
        client_order_id=str(row.get("client_order_id", "")).strip(),
        instrument_id=row.get("instrument_id"),
        side=_coerce_order_side_enum(row.get("side")),
        quantity=row.get("quantity"),
        price=row.get("price"),
        post_only=bool(row.get("post_only", True)),
        is_pending_cancel=lambda: pending_cancel,
        is_closed=lambda: False,
    )


def _build_controller_intent_publisher(
    *,
    strategy: Any,
    controller_scope_id: str,
    transport_root_dir: Path,
    send_request_fn=send_transport_request,
):
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id=controller_scope_id,
        root_dir=transport_root_dir,
    )

    def _publish_intent(order: Any) -> ControllerIntentReply:
        original_client_order_id = str(getattr(order, "client_order_id", "") or "").strip()
        if not original_client_order_id:
            raise ValueError("controller-managed TokenMM orders require client_order_id")
        request = ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id=original_client_order_id,
                controller_scope_id=controller_scope_id,
                strategy_id=_strategy_runtime_id(strategy),
            ),
            requested_at_ns=_strategy_timestamp_ns(strategy),
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id=str(getattr(order, "instrument_id")),
                side=_normalize_order_side_text(getattr(order, "side", None)),
                quantity=(
                    None
                    if getattr(order, "quantity", None) is None
                    else str(getattr(order, "quantity"))
                ),
                limit_price=(
                    None if getattr(order, "price", None) is None else str(getattr(order, "price"))
                ),
                post_only=_order_post_only(order),
                time_in_force=_normalize_time_in_force_text(getattr(order, "time_in_force", None)),
            ),
        )
        reply = send_request_fn(
            paths=paths,
            request=request,
            timeout_s=1.0,
        )
        if reply.status == REPLY_STATUS_REJECTED:
            raise RuntimeError(reply.reason or "controller rejected managed TokenMM request")
        if reply.claim is None:
            raise RuntimeError("controller accepted the request without returning a claim")
        _set_order_client_order_id(order, reply.claim.client_order_id)
        apply_lifecycle = getattr(strategy, "apply_controller_lifecycle_event", None)
        if callable(apply_lifecycle):
            apply_lifecycle(
                ExecutionLifecycleEvent.from_claim(
                    claim=reply.claim,
                    lifecycle_state=ExecutionLifecycleState.ACCEPTED,
                    venue_activity_origin=VenueActivityOrigin.CONTROLLER,
                ),
            )
        return reply

    return _publish_intent


class _TokenmmControllerManagedBridge:
    def __init__(
        self,
        *,
        strategy: Any,
        controller_scope_id: str,
        redis_client: Any,
        namespace: str,
        schema_version: str,
        transport_root_dir: Path,
    ) -> None:
        self._strategy = strategy
        self._controller_scope_id = controller_scope_id
        self._paths = UdsTransportPaths.for_controller_scope(
            controller_scope_id=controller_scope_id,
            root_dir=transport_root_dir,
        )
        self._publish_place = _build_controller_intent_publisher(
            strategy=strategy,
            controller_scope_id=controller_scope_id,
            transport_root_dir=transport_root_dir,
        )
        self._feed = ControllerStateFeedBridge(
            redis_client=redis_client,
            controller_scope_id=controller_scope_id,
            strategy_id=_strategy_runtime_id(strategy),
            namespace=namespace,
            schema_version=schema_version,
        )
        self._managed_order_rows: dict[str, dict[str, Any]] = {}
        self._cancel_intent_seq = 0
        self._feed.bind(
            lifecycle_callback=self._apply_lifecycle_event,
            canonical_state_callback=self._apply_canonical_state,
        )

    def publish_place(self, order: Any, *_args, **_kwargs) -> None:
        reply = self._publish_place(order)
        assert reply.claim is not None
        self._managed_order_rows[reply.claim.client_order_id] = {
            "client_order_id": reply.claim.client_order_id,
            "instrument_id": str(getattr(order, "instrument_id")),
            "side": _normalize_order_side_text(getattr(order, "side", None)),
            "quantity": (
                None if getattr(order, "quantity", None) is None else str(getattr(order, "quantity"))
            ),
            "price": None if getattr(order, "price", None) is None else str(getattr(order, "price")),
            "post_only": bool(_order_post_only(order)),
            "pending_cancel": False,
        }

    def publish_cancel(self, order: Any, *_args, **_kwargs) -> None:
        from flux.execution.transport import ControllerIntentCommandPayload

        self._feed.sync_once()
        client_order_id = str(getattr(order, "client_order_id", "") or "").strip()
        if not client_order_id:
            raise ValueError("controller-managed TokenMM cancels require client_order_id")
        requested_at_ns = _strategy_timestamp_ns(self._strategy)
        self._cancel_intent_seq += 1
        request = ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id=(
                    f"cancel:{client_order_id}:{requested_at_ns}:{self._cancel_intent_seq}"
                ),
                controller_scope_id=self._controller_scope_id,
                strategy_id=_strategy_runtime_id(self._strategy),
            ),
            requested_at_ns=requested_at_ns,
            command=ControllerIntentCommandPayload(
                command_type="cancel",
                order_role="maker",
                instrument_id=str(getattr(order, "instrument_id")),
                target_client_order_id=client_order_id,
            ),
        )
        reply = send_transport_request(
            paths=self._paths,
            request=request,
            timeout_s=1.0,
        )
        if reply.status == REPLY_STATUS_REJECTED:
            raise RuntimeError(reply.reason or "controller rejected managed TokenMM cancel")
        row = self._managed_order_rows.get(client_order_id)
        if row is not None:
            row["pending_cancel"] = True

    def managed_orders(self) -> list[Any]:
        self._feed.sync_once()
        pending_cancel_ids = {
            str(value)
            for value in getattr(self._strategy, "_pending_cancel_client_order_ids", set()) or set()
            if str(value)
        }
        return [
            _controller_order_snapshot(row)
            for client_order_id, row in self._managed_order_rows.items()
            if not bool(row.get("pending_cancel", False)) and client_order_id not in pending_cancel_ids
        ]

    def _apply_canonical_state(self, payload: dict[str, Any]) -> None:
        managed_rows = payload.get("managed_maker_orders")
        if not isinstance(managed_rows, list):
            return
        next_rows: dict[str, dict[str, Any]] = {}
        for row in managed_rows:
            if not isinstance(row, dict):
                continue
            client_order_id = str(row.get("client_order_id", "")).strip()
            if not client_order_id:
                continue
            next_rows[client_order_id] = dict(row)
        self._managed_order_rows = next_rows
        tracked_ids = getattr(self._strategy, "_managed_client_order_ids", None)
        if isinstance(tracked_ids, set):
            tracked_ids.clear()
            tracked_ids.update(next_rows)
        clear_pending_cancel = getattr(self._strategy, "_clear_pending_cancel", None)
        if callable(clear_pending_cancel):
            for client_order_id in list(getattr(self._strategy, "_pending_cancel_client_order_ids", set()) or set()):
                if str(client_order_id) not in next_rows:
                    clear_pending_cancel(client_order_id)

    def _apply_lifecycle_event(self, event: Any) -> None:
        _ = event


def _attach_controller_managed_binance_bridge(
    *,
    strategy: Any,
    controller_scope_id: str,
    redis_cfg: dict[str, Any],
    namespace: str,
    schema_version: str,
) -> None:
    redis_client = redis.Redis(**build_redis_client_kwargs(redis_cfg))
    bridge = _TokenmmControllerManagedBridge(
        strategy=strategy,
        controller_scope_id=controller_scope_id,
        redis_client=redis_client,
        namespace=namespace,
        schema_version=schema_version,
        transport_root_dir=_repo_root() / ".run",
    )
    strategy.submit_order = bridge.publish_place
    strategy.cancel_order = bridge.publish_cancel
    strategy._managed_orders = bridge.managed_orders
    setattr(strategy, "_controller_managed_binance_bridge", bridge)


def _strategy_config_kwargs(
    *,
    strategy_spec: FluxStrategySpec,
    strategy_cfg: dict[str, Any],
    contract: StrategyContractEntry | None,
    maker_instrument_id: InstrumentId,
    reference_instrument_id: InstrumentId,
    external_strategy_id: str,
    order_qty: Decimal,
    qty_unit: str,
    qty: Decimal | None,
) -> dict[str, Any]:
    kwargs: dict[str, Any] = {
        "strategy_id": str(strategy_cfg.get("strategy_id", "MAKERV3-001")),
        "maker_instrument_id": maker_instrument_id,
        "reference_instrument_id": reference_instrument_id,
        "external_strategy_id": external_strategy_id,
        "allowed_submit_instrument_ids": [maker_instrument_id],
        "external_order_claims": [maker_instrument_id],
        "manage_stop": bool(strategy_cfg.get("manage_stop", False)),
        "order_qty": order_qty,
        "qty_unit": qty_unit,
        "qty": qty,
        "bot_on": bool(strategy_cfg.get("bot_on", False)),
        "des_qty_global": float(strategy_cfg.get("des_qty_global", 0.0)),
        "max_qty_global": float(strategy_cfg.get("max_qty_global", 40_000.0)),
        "max_skew_bps_global": float(strategy_cfg.get("max_skew_bps_global", 20.0)),
        "des_qty_local": float(strategy_cfg.get("des_qty_local", 0.0)),
        "max_qty_local": float(strategy_cfg.get("max_qty_local", 0.0)),
        "max_skew_bps_local": float(strategy_cfg.get("max_skew_bps_local", 0.0)),
        "linear_offset_bps": float(strategy_cfg.get("linear_offset_bps", 0.0)),
        "max_age_ms": int(strategy_cfg.get("max_age_ms", 10_000)),
        "bid_edge1": float(strategy_cfg.get("bid_edge1", 10.0)),
        "ask_edge1": float(strategy_cfg.get("ask_edge1", 10.0)),
        "place_edge1": float(strategy_cfg.get("place_edge1", 2.0)),
        "distance1": float(strategy_cfg.get("distance1", 2.0)),
        "n_orders1": int(strategy_cfg.get("n_orders1", 5)),
        "bid_edge2": float(strategy_cfg.get("bid_edge2", 25.0)),
        "ask_edge2": float(strategy_cfg.get("ask_edge2", 25.0)),
        "place_edge2": float(strategy_cfg.get("place_edge2", 2.0)),
        "distance2": float(strategy_cfg.get("distance2", 5.0)),
        "n_orders2": int(strategy_cfg.get("n_orders2", 0)),
        "bid_edge3": float(strategy_cfg.get("bid_edge3", 50.0)),
        "ask_edge3": float(strategy_cfg.get("ask_edge3", 50.0)),
        "place_edge3": float(strategy_cfg.get("place_edge3", 2.0)),
        "distance3": float(strategy_cfg.get("distance3", 5.0)),
        "n_orders3": int(strategy_cfg.get("n_orders3", 0)),
        "quote_fail_critical_after_count": int(
            strategy_cfg.get("quote_fail_critical_after_count", 3),
        ),
        "quote_fail_critical_after_s": float(
            strategy_cfg.get("quote_fail_critical_after_s", 60.0),
        ),
        "spot_cash_borrowing_policy": str(
            strategy_cfg.get("spot_cash_borrowing_policy", "none"),
        ),
        "force_bot_off_on_start": bool(
            strategy_cfg.get("force_bot_off_on_start", False),
        ),
        "cancel_all_instrument_orders": bool(
            strategy_cfg.get("cancel_all_instrument_orders", False),
        ),
        **_client_order_id_config(maker_instrument_id),
    }
    if contract is not None:
        kwargs["portfolio_asset_id"] = contract.portfolio_asset_id
        kwargs["execution_account_scope_id"] = contract.execution_account_scope_id
    accepted: set[str] = set()
    for cls in reversed(getattr(strategy_spec.config_cls, "__mro__", ())):
        accepted.update(getattr(cls, "__annotations__", {}))
    return {key: value for key, value in kwargs.items() if not accepted or key in accepted}


def _build_telemetry_actor_configs(config: dict[str, Any]) -> list[ImportableActorConfig]:
    telemetry = config.get("telemetry_shipper")
    if not isinstance(telemetry, dict):
        return []
    if not bool(telemetry.get("enable_local_persistence", False)):
        return []

    actors: list[ImportableActorConfig] = []
    balance_snapshots_db_path = _optional_text(telemetry.get("balance_snapshots_db_path"))
    if balance_snapshots_db_path is not None:
        actors.append(
            ImportableActorConfig(
                actor_path=(
                    "nautilus_trader.flux.persistence.balance_snapshots.actor:"
                    "FluxBalanceSnapshotPersistenceActor"
                ),
                config_path=(
                    "nautilus_trader.flux.persistence.balance_snapshots.config:"
                    "FluxBalanceSnapshotPersistenceActorConfig"
                ),
                config={"db_path": balance_snapshots_db_path},
            ),
        )

    fills_db_path = _optional_text(telemetry.get("fills_db_path"))
    if fills_db_path is not None:
        actors.append(
            ImportableActorConfig(
                actor_path="nautilus_trader.persistence.fills.actor:ExecutionFillPersistenceActor",
                config_path=(
                    "nautilus_trader.persistence.fills.config:ExecutionFillPersistenceActorConfig"
                ),
                config={
                    "db_path": fills_db_path,
                    "action_intent_topic": TOPIC_ORDER_INTENT,
                },
            ),
        )

    orders_db_path = _optional_text(telemetry.get("orders_db_path"))
    if orders_db_path is not None:
        actors.append(
            ImportableActorConfig(
                actor_path="nautilus_trader.persistence.orders.actor:OrderActionPersistenceActor",
                config_path=(
                    "nautilus_trader.persistence.orders.config:OrderActionPersistenceActorConfig"
                ),
                config={
                    "db_path": orders_db_path,
                    "action_intent_topic": TOPIC_ORDER_INTENT,
                },
            ),
        )

    quote_cycles_db_path = _optional_text(telemetry.get("quote_cycles_db_path"))
    if quote_cycles_db_path is not None:
        actors.append(
            ImportableActorConfig(
                actor_path=(
                    "nautilus_trader.flux.persistence.quote_cycles.actor:"
                    "QuoteCyclePersistenceActor"
                ),
                config_path=(
                    "nautilus_trader.flux.persistence.quote_cycles.config:"
                    "QuoteCyclePersistenceActorConfig"
                ),
                config={"db_path": quote_cycles_db_path},
            ),
        )

    markouts_db_path = _optional_text(telemetry.get("markouts_db_path"))
    if markouts_db_path is not None:
        for actor_config in _build_markout_actor_configs(
            telemetry=telemetry,
            markouts_db_path=markouts_db_path,
        ):
            actors.append(
                ImportableActorConfig(
                    actor_path=(
                        "nautilus_trader.flux.persistence.markouts.actor:"
                        "ExecutionMarkoutPersistenceActor"
                    ),
                    config_path=(
                        "nautilus_trader.flux.persistence.markouts.config:"
                        "ExecutionMarkoutPersistenceActorConfig"
                    ),
                    config=actor_config,
                ),
            )

    return actors


def _prepare_telemetry_paths(config: dict[str, Any]) -> None:
    telemetry = config.get("telemetry_shipper")
    if not isinstance(telemetry, dict):
        return
    if not bool(telemetry.get("enable_local_persistence", False)):
        return

    for key in (
        "balance_snapshots_db_path",
        "fills_db_path",
        "orders_db_path",
        "quote_cycles_db_path",
        "markouts_db_path",
        "portfolio_inventory_db_path",
        "state_db_path",
    ):
        path_value = _optional_text(telemetry.get(key))
        if path_value is None:
            continue
        Path(path_value).expanduser().parent.mkdir(parents=True, exist_ok=True)


def _resolve_strategy_spec() -> FluxStrategySpec:
    return get_strategy_spec("makerv3")


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


def build_node(
    config: dict[str, Any],
    *,
    mode: str,
    force_enable_execution: bool,
    log_level_override: str | None = None,
) -> TradingNode:
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
    managed_contract = _controller_managed_contract_for_strategy(
        config,
        external_strategy_id=external_strategy_id,
    )

    enable_execution = bool(node_cfg.get("enable_execution", force_enable_execution))
    if managed_contract is not None:
        enable_execution = False
    _enforce_live_startup_reconciliation_guardrails(
        mode=mode,
        node_cfg=node_cfg,
        enable_execution=enable_execution,
    )
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
    exec_reconciliation_enabled = bool(node_cfg.get("exec_reconciliation", True))
    if managed_contract is not None:
        exec_reconciliation_enabled = False
    _register_cash_borrowing_venues(exec_clients=strategy_venues.exec_clients)
    maker_instrument_id = strategy_venues.execution_instrument_id
    reference_instrument_id = strategy_venues.reference_instrument_id

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
            reconciliation=exec_reconciliation_enabled,
            generate_missing_orders=bool(node_cfg.get("exec_generate_missing_orders", False)),
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
        actors=_build_telemetry_actor_configs(config),
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
    qty_unit = resolve_runner_qty_unit(
        strategy_cfg,
        strategy_id=external_strategy_id,
        logger=LOGGER,
    )
    strategy_spec = _resolve_strategy_spec()
    strategy_contract = _strategy_contract_for_strategy(
        config,
        external_strategy_id=external_strategy_id,
    )

    strategy = strategy_spec.strategy_cls(
        config=strategy_spec.config_cls(
            **_strategy_config_kwargs(
                strategy_spec=strategy_spec,
                strategy_cfg=strategy_cfg,
                contract=strategy_contract,
                maker_instrument_id=maker_instrument_id,
                reference_instrument_id=reference_instrument_id,
                external_strategy_id=external_strategy_id,
                order_qty=order_qty,
                qty_unit=qty_unit,
                qty=qty,
            ),
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
    if managed_contract is not None and managed_contract.controller_scope_id is not None:
        _attach_controller_managed_binance_bridge(
            strategy=strategy,
            controller_scope_id=managed_contract.controller_scope_id,
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
    _prepare_telemetry_paths(config)

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
