from __future__ import annotations

from dataclasses import dataclass
from decimal import ROUND_CEILING
from decimal import ROUND_FLOOR
from decimal import Decimal
import importlib
import sys
from typing import Any

from flux.common.account_scopes import decode_account_scopes
from flux.common.strategy_contracts import decode_strategy_contracts
from flux.runners.shared.bootstrap import (
    resolve_flux_strategy_id as resolve_flux_strategy_id_from_bootstrap,
)
from flux.strategies.shared.equities_arb.instruments import (
    hyperliquid_perp_to_ibkr_instrument_id,
)


if __name__ == "flux.strategies.shared.equities_arb.core":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.equities_arb.core",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.equities_arb.core":
    sys.modules.setdefault("flux.strategies.shared.equities_arb.core", sys.modules[__name__])


def _to_decimal(value: Any, *, field_name: str) -> Decimal:
    try:
        return Decimal(str(value))
    except Exception as exc:  # pragma: no cover - defensive type guard
        raise ValueError(f"Invalid decimal value for {field_name}: {value!r}") from exc


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _strategy_param_set(strategy_spec: Any) -> str:
    return str(getattr(strategy_spec, "param_set", "")).strip().lower()


def _resolve_flux_strategy_id(config: dict[str, Any]) -> str:
    return resolve_flux_strategy_id_from_bootstrap(config)


def _runtime_params_module(module: Any, *, param_set: str) -> Any:
    if not hasattr(module, "PARAM_SET"):
        setattr(module, "PARAM_SET", param_set)
    return module


def _normalize_tick(value: Decimal, *, field_name: str) -> Decimal:
    if value <= 0:
        raise ValueError(f"`{field_name}` must be > 0")
    return value


def _normalize_side(side: str) -> str:
    normalized = str(side).strip().upper()
    if normalized not in {"BUY", "SELL"}:
        raise ValueError(f"Unsupported side: {side!r}")
    return normalized


def round_hyperliquid_price(price: Decimal, *, tick_size: Decimal, side: str) -> Decimal:
    tick = _normalize_tick(tick_size, field_name="tick_size")
    normalized_side = _normalize_side(side)
    rounding = ROUND_FLOOR if normalized_side == "BUY" else ROUND_CEILING
    steps = (price / tick).to_integral_value(rounding=rounding)
    return steps * tick


def round_ibkr_limit_price(price: Decimal, *, tick_size: Decimal, side: str) -> Decimal:
    tick = _normalize_tick(tick_size, field_name="tick_size")
    normalized_side = _normalize_side(side)
    rounding = ROUND_CEILING if normalized_side == "BUY" else ROUND_FLOOR
    steps = (price / tick).to_integral_value(rounding=rounding)
    return steps * tick


@dataclass(frozen=True, slots=True)
class EquitiesArbFeeRules:
    maker_fee_source: str
    hedge_fee_source: str
    hedge_fee_plan: str
    maker_fee_bps: Decimal
    hedge_fee_bps: Decimal
    fee_snapshot_age_s: Decimal | None


@dataclass(frozen=True, slots=True)
class FeeAssumptions:
    ibkr_fee_plan: str
    ibkr_fee_min_usd: Decimal
    hl_taker_fee_bps: Decimal
    hl_maker_fee_bps: Decimal
    assumed_hedge_fee_bps: Decimal


def resolve_fee_rules(
    *,
    runtime_params: dict[str, Any] | Any,
    maker_fee_bps: Any | None,
    fee_snapshot_age_s: Any | None = None,
) -> EquitiesArbFeeRules:
    maker_fee_source = str(runtime_params.get("maker_fee_source", "hyperliquid_api")).strip()
    hedge_fee_source = str(runtime_params.get("hedge_fee_source", "config")).strip()
    hedge_fee_plan = str(runtime_params.get("hedge_fee_plan", "ibkr_pro_tiered")).strip()
    if maker_fee_source not in {"hyperliquid_api", "config"}:
        raise ValueError(f"Unsupported maker fee source: {maker_fee_source!r}")
    if hedge_fee_source != "config":
        raise ValueError(f"Unsupported hedge fee source: {hedge_fee_source!r}")
    if hedge_fee_plan != "ibkr_pro_tiered":
        raise ValueError(f"Unsupported hedge fee plan: {hedge_fee_plan!r}")
    if maker_fee_bps is None:
        raise ValueError("`maker_fee_bps` is required when maker_fee_source=hyperliquid_api")

    hedge_fee_bps = _to_decimal(
        runtime_params.get("assumed_hedge_fee_bps", "0"),
        field_name="assumed_hedge_fee_bps",
    )
    snapshot_age = (
        None
        if fee_snapshot_age_s is None
        else _to_decimal(fee_snapshot_age_s, field_name="fee_snapshot_age_s")
    )
    return EquitiesArbFeeRules(
        maker_fee_source=maker_fee_source,
        hedge_fee_source=hedge_fee_source,
        hedge_fee_plan=hedge_fee_plan,
        maker_fee_bps=_to_decimal(maker_fee_bps, field_name="maker_fee_bps"),
        hedge_fee_bps=hedge_fee_bps,
        fee_snapshot_age_s=snapshot_age,
    )


def build_fee_assumptions(
    *,
    ibkr_fee_plan: str,
    ibkr_fee_min_usd: Any,
    hl_taker_fee_bps: Any,
    hl_maker_fee_bps: Any,
    assumed_hedge_fee_bps: Any,
) -> FeeAssumptions:
    normalized_ibkr_fee_plan = str(ibkr_fee_plan).strip().lower()
    if normalized_ibkr_fee_plan not in {"fixed", "tiered"}:
        raise ValueError(f"Unsupported ibkr fee plan: {ibkr_fee_plan!r}")
    return FeeAssumptions(
        ibkr_fee_plan=normalized_ibkr_fee_plan,
        ibkr_fee_min_usd=_to_decimal(ibkr_fee_min_usd, field_name="ibkr_fee_min_usd"),
        hl_taker_fee_bps=_to_decimal(hl_taker_fee_bps, field_name="hl_taker_fee_bps"),
        hl_maker_fee_bps=_to_decimal(hl_maker_fee_bps, field_name="hl_maker_fee_bps"),
        assumed_hedge_fee_bps=_to_decimal(
            assumed_hedge_fee_bps,
            field_name="assumed_hedge_fee_bps",
        ),
    )


def build_fee_aware_threshold_bps(
    *,
    target_edge_bps: Decimal,
    hl_fee_bps: Decimal,
    ibkr_fee_bps: Decimal,
    offset_bps: Decimal = Decimal("0"),
) -> Decimal:
    return target_edge_bps + hl_fee_bps + ibkr_fee_bps + offset_bps


def build_effective_ibkr_fee_bps(
    *,
    fee_assumptions: FeeAssumptions,
    hedge_notional_usd: Decimal,
) -> Decimal:
    normalized_notional = abs(hedge_notional_usd)
    if normalized_notional <= 0:
        return fee_assumptions.assumed_hedge_fee_bps

    min_fee_bps = (
        fee_assumptions.ibkr_fee_min_usd / normalized_notional
    ) * Decimal("10000")
    if fee_assumptions.ibkr_fee_plan == "fixed":
        return fee_assumptions.assumed_hedge_fee_bps + min_fee_bps
    return max(fee_assumptions.assumed_hedge_fee_bps, min_fee_bps)


def build_take_take_limit_price(
    *,
    side: str,
    maker_bid: Decimal | None,
    maker_ask: Decimal | None,
    reference_bid: Decimal | None,
    reference_ask: Decimal | None,
    target_edge_bps: Decimal,
    hl_taker_fee_bps: Decimal,
    hedge_fee_bps: Decimal,
) -> Decimal | None:
    normalized_side = str(side).strip().upper()
    if normalized_side not in {"BUY", "SELL"}:
        raise ValueError(f"Unsupported side: {side!r}")
    if maker_bid is None or maker_ask is None or reference_bid is None or reference_ask is None:
        return None
    if maker_ask <= maker_bid or reference_ask <= reference_bid:
        return None

    reference_mid = (reference_bid + reference_ask) / Decimal("2")
    if reference_mid <= 0:
        return None

    required_threshold_bps = build_fee_aware_threshold_bps(
        target_edge_bps=target_edge_bps,
        hl_fee_bps=hl_taker_fee_bps,
        ibkr_fee_bps=hedge_fee_bps,
    )
    if normalized_side == "BUY":
        available_edge_bps = ((reference_bid - maker_ask) / reference_mid) * Decimal("10000")
        return maker_ask if available_edge_bps >= required_threshold_bps else None

    available_edge_bps = ((maker_bid - reference_ask) / reference_mid) * Decimal("10000")
    return maker_bid if available_edge_bps >= required_threshold_bps else None


def validate_ibkr_quote(
    *,
    bid: Decimal | None,
    ask: Decimal | None,
    quote_age_ms: int | None = None,
    max_quote_age_ms: int | None = None,
    max_spread_bps: Decimal | None = None,
) -> str | None:
    if bid is None:
        return "missing_bid"
    if ask is None:
        return "missing_ask"
    if ask <= bid:
        return "locked_or_crossed"

    if max_quote_age_ms is not None and quote_age_ms is not None and quote_age_ms > max_quote_age_ms:
        return "stale_quote"

    if max_spread_bps is not None:
        mid = (bid + ask) / Decimal("2")
        if mid <= 0:
            return "missing_midpoint"
        spread_bps = ((ask - bid) / mid) * Decimal("10000")
        if spread_bps > max_spread_bps:
            return "spread_too_wide"

    return None


def build_ibkr_ioc_limit(
    *,
    side: str,
    bid: Decimal | None,
    ask: Decimal | None,
    cross_mid_bps: Decimal,
    max_cross_bps: Decimal | None = None,
    tick_size: Decimal,
    quote_age_ms: int | None = None,
    max_quote_age_ms: int | None = None,
    max_spread_bps: Decimal | None = None,
) -> Decimal | None:
    invalid_reason = validate_ibkr_quote(
        bid=bid,
        ask=ask,
        quote_age_ms=quote_age_ms,
        max_quote_age_ms=max_quote_age_ms,
        max_spread_bps=max_spread_bps,
    )
    if invalid_reason is not None:
        return None

    assert bid is not None
    assert ask is not None

    normalized_side = str(side).strip().upper()
    if normalized_side not in {"BUY", "SELL"}:
        raise ValueError(f"Unsupported side: {side!r}")

    mid = (bid + ask) / Decimal("2")
    effective_cross_mid_bps = cross_mid_bps
    if max_cross_bps is not None and effective_cross_mid_bps > max_cross_bps:
        effective_cross_mid_bps = max_cross_bps
    cross_ratio = effective_cross_mid_bps / Decimal("10000")
    raw_price = mid * (
        Decimal("1") + cross_ratio if normalized_side == "BUY" else Decimal("1") - cross_ratio
    )
    rounded_price = round_ibkr_limit_price(
        raw_price,
        tick_size=tick_size,
        side=normalized_side,
    )

    if normalized_side == "BUY":
        return min(rounded_price, ask)
    return max(rounded_price, bid)


def build_maker_quote_price(
    *,
    side: str,
    reference_mid: Decimal,
    target_edge_bps: Decimal,
    maker_fee_bps: Decimal,
    hedge_fee_bps: Decimal,
    offset_bps: Decimal,
    tick_size: Decimal,
) -> Decimal:
    if reference_mid <= 0:
        raise ValueError("`reference_mid` must be > 0")

    normalized_side = str(side).strip().upper()
    if normalized_side not in {"BUY", "SELL"}:
        raise ValueError(f"Unsupported side: {side!r}")

    total_bps = build_fee_aware_threshold_bps(
        target_edge_bps=target_edge_bps,
        hl_fee_bps=maker_fee_bps,
        ibkr_fee_bps=hedge_fee_bps,
        offset_bps=offset_bps,
    )
    ratio = total_bps / Decimal("10000")
    raw_price = (
        reference_mid * (Decimal("1") - ratio)
        if normalized_side == "BUY"
        else reference_mid * (Decimal("1") + ratio)
    )
    return round_hyperliquid_price(raw_price, tick_size=tick_size, side=normalized_side)


def resolve_runtime_params_module(strategy_spec: Any):
    param_set = _strategy_param_set(strategy_spec)
    if param_set:
        try:
            module = importlib.import_module(f"flux.strategies.{param_set}.runtime_params")
            return _runtime_params_module(module, param_set=param_set)
        except ModuleNotFoundError as exc:
            # Future split families still reuse Makerv3/Makerv4 runtime params in wave 1.
            exc_name = str(getattr(exc, "name", "") or "")
            if not exc_name.startswith(f"flux.strategies.{param_set}"):
                raise

    legacy_param_set = "makerv4" if supports_immediate_hedge(strategy_spec) else "makerv3"
    module = importlib.import_module(f"flux.strategies.{legacy_param_set}.runtime_params")
    return _runtime_params_module(module, param_set=legacy_param_set)


def runtime_params_module_for_strategy(strategy_spec: Any):
    return resolve_runtime_params_module(strategy_spec)


def supports_immediate_hedge(strategy_spec: Any) -> bool:
    capabilities = getattr(strategy_spec, "capabilities", None)
    if capabilities is not None and hasattr(capabilities, "supports_immediate_hedge"):
        return bool(capabilities.supports_immediate_hedge)
    return _strategy_param_set(strategy_spec) == "makerv4"


def strategy_supports_immediate_hedge(strategy_spec: Any) -> bool:
    return supports_immediate_hedge(strategy_spec)


def uses_profile_account_projection(strategy_spec: Any) -> bool:
    capabilities = getattr(strategy_spec, "capabilities", None)
    if capabilities is not None and hasattr(capabilities, "uses_profile_account_projection"):
        return bool(capabilities.uses_profile_account_projection)
    return True


def strategy_uses_profile_account_projection(strategy_spec: Any) -> bool:
    return uses_profile_account_projection(strategy_spec)


def strategy_allowed_instrument_ids(
    *,
    strategy_spec: Any,
    maker_instrument_id: Any,
    reference_instrument_id: Any,
) -> list[Any]:
    if supports_immediate_hedge(strategy_spec):
        return [maker_instrument_id, reference_instrument_id]
    return [maker_instrument_id]


def effective_venue_resolution_config(
    *,
    config: dict[str, Any],
    strategy_spec: Any,
) -> dict[str, Any]:
    if not strategy_supports_immediate_hedge(strategy_spec):
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

    identity_cfg = config.get("identity")
    external_strategy_id = (
        _optional_text(identity_cfg.get("external_strategy_id"))
        if isinstance(identity_cfg, dict)
        else None
    ) or _resolve_flux_strategy_id(config)
    strategy_contract = None
    for contract in decode_strategy_contracts(config.get("strategy_contracts") or []):
        if contract.strategy_id == external_strategy_id:
            strategy_contract = contract
            break

    maker_venue_name = (
        _optional_text(getattr(strategy_contract, "maker_venue", None))
        if strategy_contract is not None
        else None
    ) or "HYPERLIQUID"
    maker_cfg = venue_entries.get(maker_venue_name)
    if not isinstance(maker_cfg, dict):
        return config

    maker_instrument_id = (
        _optional_text(getattr(strategy_contract, "maker_instrument_id", None))
        if strategy_contract is not None
        else None
    ) or _optional_text(maker_cfg.get("instrument_id"))
    if maker_instrument_id is None:
        return config

    reference_instrument_id = (
        _optional_text(getattr(strategy_contract, "reference_instrument_id", None))
        if strategy_contract is not None
        else None
    )
    if reference_instrument_id is None:
        strategy_cfg = config.get("strategy")
        if not isinstance(strategy_cfg, dict):
            return config
        reference_instrument_id = hyperliquid_perp_to_ibkr_instrument_id(
            maker_instrument_id,
            primary_exchange=str(strategy_cfg.get("ibkr_primary_exchange", "NASDAQ")),
        )

    ibkr_scope_overrides: dict[str, Any] = {}
    scope_configs = {
        scope.scope_id: scope
        for scope in decode_account_scopes(config.get("account_scopes") or [])
    }
    if strategy_contract is not None:
        scope_id = strategy_contract.hedge_account_scope_id or strategy_contract.reference_account_scope_id
        scope = scope_configs.get(scope_id)
        if scope is None or scope.provider.lower() != "ibkr":
            scope = None
        if scope is not None:
            if scope.ibg_host is not None:
                ibkr_scope_overrides["ibg_host"] = scope.ibg_host
            if scope.ibg_port is not None:
                ibkr_scope_overrides["ibg_port"] = scope.ibg_port
            if scope.ibg_client_id is not None and ibkr_cfg.get("ibg_client_id") in (None, ""):
                ibkr_scope_overrides["ibg_client_id"] = scope.ibg_client_id
            if scope.account_id is not None:
                ibkr_scope_overrides["account_id"] = scope.account_id
            if scope.dockerized_gateway is not None:
                ibkr_scope_overrides["dockerized_gateway"] = dict(scope.dockerized_gateway)

    execution_venues_cfg = config.get("venues")
    execution_venue = (
        _optional_text(execution_venues_cfg.get("execution_venue"))
        if isinstance(execution_venues_cfg, dict)
        else None
    )
    maker_cfg_is_executing = bool(maker_cfg.get("execution", False))
    stale_execution_flags = any(
        venue_name not in {maker_venue_name, "IBKR"}
        and isinstance(venue_cfg, dict)
        and bool(venue_cfg.get("execution", False))
        for venue_name, venue_cfg in venue_entries.items()
    )
    needs_maker_rewrite = _optional_text(maker_cfg.get("instrument_id")) != maker_instrument_id
    needs_top_level_execution_promotion = execution_venue != maker_venue_name
    needs_maker_execution_promotion = not maker_cfg_is_executing
    needs_reference_rewrite = _optional_text(ibkr_cfg.get("instrument_id")) != reference_instrument_id
    needs_ibkr_execution_promotion = not bool(ibkr_cfg.get("execution", False))
    needs_scope_overlay = bool(ibkr_scope_overrides)
    if not (
        needs_maker_rewrite
        or needs_top_level_execution_promotion
        or needs_maker_execution_promotion
        or stale_execution_flags
        or needs_reference_rewrite
        or needs_ibkr_execution_promotion
        or needs_scope_overlay
    ):
        return config

    effective_venues_cfg = (
        dict(execution_venues_cfg)
        if isinstance(execution_venues_cfg, dict)
        else {}
    )
    effective_venues_cfg["execution_venue"] = maker_venue_name

    effective_node_cfg = dict(node_cfg)
    effective_venue_entries: dict[str, Any] = {}
    for venue_name, venue_cfg in venue_entries.items():
        if not isinstance(venue_cfg, dict):
            effective_venue_entries[venue_name] = venue_cfg
            continue
        if venue_name == maker_venue_name:
            effective_venue_entries[venue_name] = {
                **venue_cfg,
                "instrument_id": maker_instrument_id,
                "execution": True,
            }
            continue
        if venue_name == "IBKR":
            effective_venue_entries[venue_name] = {
                **venue_cfg,
                **ibkr_scope_overrides,
                "instrument_id": reference_instrument_id,
                "execution": True,
            }
            continue
        if bool(venue_cfg.get("execution", False)):
            effective_venue_entries[venue_name] = {
                **venue_cfg,
                "execution": False,
            }
            continue
        effective_venue_entries[venue_name] = venue_cfg

    effective_node_cfg["venues"] = effective_venue_entries
    return {
        **config,
        "venues": effective_venues_cfg,
        "node": effective_node_cfg,
    }


__all__ = [
    "EquitiesArbFeeRules",
    "FeeAssumptions",
    "build_effective_ibkr_fee_bps",
    "build_fee_assumptions",
    "build_fee_aware_threshold_bps",
    "build_ibkr_ioc_limit",
    "build_maker_quote_price",
    "build_take_take_limit_price",
    "effective_venue_resolution_config",
    "resolve_fee_rules",
    "resolve_runtime_params_module",
    "runtime_params_module_for_strategy",
    "strategy_allowed_instrument_ids",
    "strategy_supports_immediate_hedge",
    "strategy_uses_profile_account_projection",
    "supports_immediate_hedge",
    "uses_profile_account_projection",
    "validate_ibkr_quote",
]
