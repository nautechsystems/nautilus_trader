"""Manage MakerV3 runtime parameter wiring and safe application."""

from __future__ import annotations

from collections.abc import Mapping
from decimal import Decimal
from typing import Any

from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_DEFAULTS
from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_REGISTRY
from nautilus_trader.flux.strategies.makerv3 import inventory as inventory_mod
from nautilus_trader.flux.strategies.makerv3 import pricing as pricing_mod
from nautilus_trader.flux.strategies.makerv3 import publisher as publisher_mod
from nautilus_trader.flux.strategies.makerv3.constants import ALERT_COOLDOWN_RUNTIME_PARAMS_FAILURE_MS
from nautilus_trader.flux.strategies.makerv3.constants import ALERT_KEY_RUNTIME_PARAMS_FAILURE


_to_decimal = pricing_mod.to_decimal
_to_decimal_or_none = pricing_mod.to_decimal_or_none
_to_int_or_default = pricing_mod.to_int_or_default


def parse_bool_text(value: Any) -> bool | None:
    """Return a parsed boolean value for common truthy/falsey runtime payloads."""
    if value is None:
        return None
    text = str(value).strip().lower()
    if text in {"1", "true", "t", "yes", "y", "on", "enabled"}:
        return True
    if text in {"0", "false", "f", "no", "n", "off", "disabled"}:
        return False
    return None


RUNTIME_PARAM_SCHEMA: dict[str, dict[str, Any]] = {
    name: dict(spec)
    for name, spec in MAKERV3_RUNTIME_PARAM_REGISTRY.schema.items()
}

RUNTIME_PARAM_TYPES: dict[str, str] = {
    name: (
        "bool"
        if spec.get("type") == "boolean"
        else "int"
        if spec.get("type") == "integer"
        else "decimal"
    )
    for name, spec in RUNTIME_PARAM_SCHEMA.items()
}

_QUOTE_RUNTIME_INT_NAMES: tuple[str, ...] = (
    "max_age_ms",
    "n_orders1",
    "n_orders2",
    "n_orders3",
)
_QUOTE_RUNTIME_DECIMAL_NAMES: tuple[str, ...] = (
    "des_qty_global",
    "max_qty_global",
    "max_skew_bps_global",
    "des_qty_local",
    "max_qty_local",
    "max_skew_bps_local",
    "linear_offset_bps",
    "bid_edge1",
    "ask_edge1",
    "bid_edge2",
    "ask_edge2",
    "bid_edge3",
    "ask_edge3",
    "place_edge1",
    "place_edge2",
    "place_edge3",
    "distance1",
    "distance2",
    "distance3",
)
_INVENTORY_SKEW_RUNTIME_PARAMS = set(inventory_mod.INVENTORY_SKEW_RUNTIME_PARAMS)


def coerce_runtime_param_value(name: str, value: Any) -> Any:
    """Coerce a raw runtime param payload into its canonical Python type."""
    coerced = MAKERV3_RUNTIME_PARAM_REGISTRY.coerce_value(name, value)
    schema = RUNTIME_PARAM_SCHEMA.get(name)
    if schema is None:
        raise ValueError(f"Unsupported runtime param: {name!r}")
    if schema.get("type") == "number":
        parsed = _to_decimal_or_none(coerced)
        if parsed is None:
            raise ValueError(f"Invalid decimal value for {name}: {value!r}")
        return parsed
    return coerced


def initial_runtime_params(config: Any) -> dict[str, Any]:
    """Build the initial runtime parameter set derived from strategy config."""
    runtime_defaults: dict[str, Any] = dict(MAKERV3_RUNTIME_PARAM_DEFAULTS)
    runtime_defaults["qty"] = config.active_order_qty
    runtime_params: dict[str, Any] = {}
    for name in MAKERV3_RUNTIME_PARAM_REGISTRY.names:
        configured_value = getattr(config, name, runtime_defaults[name])
        if configured_value is None:
            configured_value = runtime_defaults[name]
        runtime_params[name] = coerce_runtime_param_value(name, configured_value)
    return runtime_params


def effective_bot_on(strategy: Any) -> bool:
    """Return the authoritative bot-on state after runtime overrides."""
    return bool(strategy._runtime_params.get("bot_on", strategy.config.bot_on))


def runtime_decimal(strategy: Any, name: str) -> Decimal:
    """Return a runtime decimal param, defaulting to config."""
    return _to_decimal(strategy._runtime_params.get(name, getattr(strategy.config, name)))


def runtime_int(strategy: Any, name: str) -> int:
    """Return a runtime integer param, defaulting to config."""
    value = strategy._runtime_params.get(name, getattr(strategy.config, name))
    try:
        return int(value)
    except Exception:
        return int(getattr(strategy.config, name))


def runtime_bool(strategy: Any, name: str) -> bool:
    """Return a runtime boolean param, defaulting to config."""
    value = strategy._runtime_params.get(name, getattr(strategy.config, name))
    parsed = parse_bool_text(value)
    if parsed is None:
        return bool(getattr(strategy.config, name))
    return parsed


def quote_runtime_params_snapshot(strategy: Any) -> dict[str, Any]:
    """Return a pre-coerced runtime snapshot used by the quote engine."""
    runtime = strategy._runtime_params
    snapshot: dict[str, Any] = {}
    for name in _QUOTE_RUNTIME_DECIMAL_NAMES:
        snapshot[name] = _to_decimal(runtime.get(name, getattr(strategy.config, name)))
    for name in _QUOTE_RUNTIME_INT_NAMES:
        snapshot[name] = _to_int_or_default(
            runtime.get(name, getattr(strategy.config, name)),
            getattr(strategy.config, name),
        )
    return snapshot


def params_manager_factory(
    *,
    redis_client: Any,
    namespace: str = "flux",
    schema_version: str = "v1",
    defaults: Mapping[str, Any] | None = None,
) -> Any:
    """Build a params-manager factory bound to MakerV3 runtime schema."""
    base_defaults: dict[str, Any] = dict(MAKERV3_RUNTIME_PARAM_DEFAULTS)
    if defaults:
        for name, value in defaults.items():
            base_defaults[str(name)] = value

    def _factory(strategy: Any) -> Any:
        from nautilus_trader.flux.params.manager import FluxParamsManager

        strategy_runtime_params = getattr(strategy, "_runtime_params", {})
        runtime_defaults = {
            name: coerce_runtime_param_value(
                name,
                strategy_runtime_params.get(name, base_defaults[name]),
            )
            for name in MAKERV3_RUNTIME_PARAM_REGISTRY.names
        }
        return FluxParamsManager(
            redis_client=redis_client,
            strategy_id=strategy.runtime_strategy_id,
            namespace=namespace,
            schema_version=schema_version,
            schema=RUNTIME_PARAM_SCHEMA,
            defaults=runtime_defaults,
            param_set=MAKERV3_RUNTIME_PARAM_REGISTRY.param_set,
        )

    return _factory


def ensure_params_manager_identity(strategy: Any, manager: Any | None) -> None:
    """Validate that a manager is bound to the correct strategy identity."""
    if manager is None:
        return
    manager_strategy_id = getattr(manager, "strategy_id", None)
    if manager_strategy_id is None:
        return
    manager_strategy_id_str = str(manager_strategy_id)
    if manager_strategy_id_str != strategy.runtime_strategy_id:
        raise ValueError(
            "Configured params manager strategy_id mismatch: "
            f"{manager_strategy_id_str!r} != {strategy.runtime_strategy_id!r}",
        )


def set_params_manager(strategy: Any, manager: Any | None) -> None:
    """Attach an explicit runtime params manager instance."""
    ensure_params_manager_identity(strategy, manager)
    strategy._params_manager = manager


def set_params_manager_factory(strategy: Any, factory: Any | None) -> None:
    """Attach a lazy factory used to construct a params manager on demand."""
    if factory is None:
        strategy._params_manager_factory = None
        return
    if not callable(factory):
        raise TypeError("`factory` must be callable")
    strategy._params_manager_factory = factory


def ensure_params_manager(strategy: Any) -> Any | None:
    """Return a params manager, creating it via the configured factory if needed."""
    if strategy._params_manager is not None:
        return strategy._params_manager
    factory = strategy._params_manager_factory
    if factory is None:
        return None
    manager = factory(strategy)
    set_params_manager(strategy, manager)
    return strategy._params_manager


def apply_runtime_param_updates(strategy: Any, updates: dict[str, Any]) -> None:
    """Apply a validated runtime param update payload atomically."""
    coerced_updates: dict[str, Any] = {}
    for name, raw_value in updates.items():
        coerced_updates[str(name)] = coerce_runtime_param_value(str(name), raw_value)

    qty_changed = "qty" in coerced_updates
    next_order_qty: Any | None = None
    if qty_changed:
        qty = _to_decimal(coerced_updates["qty"])
        if qty <= 0:
            raise ValueError("`qty` must be > 0")
        if strategy._maker_instrument is not None:
            try:
                next_order_qty = strategy._maker_instrument.make_qty(qty)
            except Exception as exc:
                raise RuntimeError(
                    f"Failed to convert runtime qty to instrument quantity for "
                    f"{strategy._external_strategy_id}: qty={qty}",
                ) from exc

    strategy._runtime_params.update(coerced_updates)
    if next_order_qty is not None:
        strategy._order_qty = next_order_qty
    if any(name in _INVENTORY_SKEW_RUNTIME_PARAMS for name in coerced_updates):
        strategy._invalidate_inventory_skew_cache()


def refresh_runtime_params(strategy: Any, *, now_ns: int | None = None, force: bool = False) -> None:
    """Refresh runtime params from the configured manager if due."""
    if now_ns is None:
        now_ns = int(strategy.clock.timestamp_ns())
    if not force and now_ns - strategy._last_params_refresh_ns < strategy.PARAMS_REFRESH_INTERVAL_MS * 1_000_000:
        return
    strategy._last_params_refresh_ns = now_ns

    manager = ensure_params_manager(strategy)
    if manager is None:
        return
    updates_fn = getattr(manager, "load", None)
    if not callable(updates_fn):
        raise RuntimeError("Configured params manager does not provide load()")
    apply_runtime_param_updates(strategy, updates_fn())


def fail_fast_runtime_params(strategy: Any, *, context: str, exc: Exception) -> None:
    """Emit diagnostics and stop the strategy after a runtime params failure."""
    if strategy._runtime_params_failed:
        return

    strategy._runtime_params_failed = True
    error_type = type(exc).__name__
    error_message = str(exc)
    event_payload = {
        "context": context,
        "error_type": error_type,
        "error_message": error_message,
    }

    logger = getattr(strategy, "log", None)
    if logger is not None:
        log_error = getattr(logger, "error", None)
        if callable(log_error):
            try:
                log_error(
                    publisher_mod.to_json_safe(
                        {
                            "event": "runtime_params_failure",
                            "strategy_id": strategy._external_strategy_id,
                            **event_payload,
                        },
                    ),
                )
            except Exception:
                pass

    try:
        strategy._publish_event("runtime_params_failure", **event_payload)
    except Exception:
        pass

    try:
        strategy._publish_actionable_alert(
            alert_key=ALERT_KEY_RUNTIME_PARAMS_FAILURE,
            message=(
                f"runtime_params_failure[{context}] "
                f"{error_type}: {error_message}"
            ),
            level="error",
            reason_code=ALERT_KEY_RUNTIME_PARAMS_FAILURE,
            cooldown_ms=ALERT_COOLDOWN_RUNTIME_PARAMS_FAILURE_MS,
        )
    except Exception:
        pass

    strategy.stop()
