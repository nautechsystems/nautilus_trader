from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
from typing import Any

from flux.strategies.makerv4.rounding import round_maker_price
from flux.strategies.makerv4.rounding import round_ibkr_limit_price


@dataclass(frozen=True, slots=True)
class FeeAssumptions:
    ibkr_fee_plan: str
    ibkr_fee_min_usd: Decimal
    maker_taker_fee_bps: Decimal
    maker_maker_fee_bps: Decimal
    assumed_hedge_fee_bps: Decimal


def _to_decimal(value: Any, *, field_name: str) -> Decimal:
    try:
        return Decimal(str(value))
    except Exception as exc:  # pragma: no cover - defensive guard
        raise ValueError(f"Invalid decimal value for {field_name}: {value!r}") from exc


def build_fee_assumptions(
    *,
    ibkr_fee_plan: str,
    ibkr_fee_min_usd: Any,
    maker_taker_fee_bps: Any,
    maker_maker_fee_bps: Any,
    assumed_hedge_fee_bps: Any,
) -> FeeAssumptions:
    normalized_ibkr_fee_plan = str(ibkr_fee_plan).strip().lower()
    if normalized_ibkr_fee_plan not in {"fixed", "tiered"}:
        raise ValueError(f"Unsupported ibkr fee plan: {ibkr_fee_plan!r}")
    return FeeAssumptions(
        ibkr_fee_plan=normalized_ibkr_fee_plan,
        ibkr_fee_min_usd=_to_decimal(ibkr_fee_min_usd, field_name="ibkr_fee_min_usd"),
        maker_taker_fee_bps=_to_decimal(
            maker_taker_fee_bps,
            field_name="maker_taker_fee_bps",
        ),
        maker_maker_fee_bps=_to_decimal(
            maker_maker_fee_bps,
            field_name="maker_maker_fee_bps",
        ),
        assumed_hedge_fee_bps=_to_decimal(
            assumed_hedge_fee_bps,
            field_name="assumed_hedge_fee_bps",
        ),
    )


def build_fee_aware_threshold_bps(
    *,
    target_edge_bps: Decimal,
    maker_fee_bps: Decimal,
    ibkr_fee_bps: Decimal,
    offset_bps: Decimal = Decimal("0"),
) -> Decimal:
    return target_edge_bps + maker_fee_bps + ibkr_fee_bps + offset_bps


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
    maker_taker_fee_bps: Decimal,
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
        maker_fee_bps=maker_taker_fee_bps,
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
    raw_price = mid * (Decimal("1") + cross_ratio if normalized_side == "BUY" else Decimal("1") - cross_ratio)
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
        maker_fee_bps=maker_fee_bps,
        ibkr_fee_bps=hedge_fee_bps,
        offset_bps=offset_bps,
    )
    ratio = total_bps / Decimal("10000")
    raw_price = (
        reference_mid * (Decimal("1") - ratio)
        if normalized_side == "BUY"
        else reference_mid * (Decimal("1") + ratio)
    )
    return round_maker_price(raw_price, tick_size=tick_size, side=normalized_side)


__all__ = [
    "FeeAssumptions",
    "build_fee_assumptions",
    "build_fee_aware_threshold_bps",
    "build_effective_ibkr_fee_bps",
    "build_ibkr_ioc_limit",
    "build_maker_quote_price",
    "build_take_take_limit_price",
    "validate_ibkr_quote",
]
