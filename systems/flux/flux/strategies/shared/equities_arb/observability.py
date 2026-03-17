from __future__ import annotations

from collections.abc import Mapping
import sys
from typing import Any

from flux.strategies.shared.equities_arb.core import EquitiesArbFeeRules
from flux.strategies.shared.equities_arb.core import FeeAssumptions
from flux.strategies.shared.equities_arb.core import build_effective_ibkr_fee_bps
from flux.strategies.shared.equities_arb.core import build_fee_assumptions
from flux.strategies.shared.equities_arb.core import build_fee_aware_threshold_bps
from flux.strategies.shared.equities_arb.core import build_ibkr_ioc_limit
from flux.strategies.shared.equities_arb.core import build_maker_quote_price
from flux.strategies.shared.equities_arb.core import build_take_take_limit_price
from flux.strategies.shared.equities_arb.core import resolve_fee_rules
from flux.strategies.shared.equities_arb.core import validate_ibkr_quote
from flux.strategies.shared.quote_snapshot import (
    build_quote_snapshot_payload as shared_build_quote_snapshot_payload,
)


def build_fee_assumptions_payload(fee_assumptions: FeeAssumptions) -> dict[str, Any]:
    return {
        "ibkr_fee_plan": str(fee_assumptions.ibkr_fee_plan),
        "ibkr_fee_min_usd": float(fee_assumptions.ibkr_fee_min_usd),
        "hl_taker_fee_bps": float(fee_assumptions.hl_taker_fee_bps),
        "hl_maker_fee_bps": float(fee_assumptions.hl_maker_fee_bps),
        "assumed_hedge_fee_bps": float(fee_assumptions.assumed_hedge_fee_bps),
    }


def build_quote_snapshot_payload(
    *,
    maker_leg: Mapping[str, Any] | None,
    hedge_leg: Mapping[str, Any] | None,
    ref_leg: Mapping[str, Any] | None,
    mid_spread_bps: float | None = None,
    arb_bid_spread_bps: float | None = None,
    arb_ask_spread_bps: float | None = None,
    effective_spread_bps: float | None = None,
    quoted_spread_bps: float | None = None,
    expected_maker_fee_bps: float | None = None,
    assumed_hedge_fee_bps: float | None = None,
    hedge_ready: bool | None = None,
    hedge_route: str | None = None,
    effective_account_source: str | None = None,
    hedge_disabled_reason: str | None = None,
    ibkr_quote_age_ms: int | None = None,
    fee_snapshot_age_s: float | None = None,
    hedge_latency_ms: int | None = None,
    hedge_slippage_bps_vs_mid: float | None = None,
    ts_ms: int | None = None,
    fee_assumptions: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    payload = shared_build_quote_snapshot_payload(
        maker_leg=maker_leg,
        hedge_leg=hedge_leg,
        ref_leg=ref_leg,
        effective_spread_bps=effective_spread_bps,
        quoted_spread_bps=quoted_spread_bps,
        expected_maker_fee_bps=expected_maker_fee_bps,
        assumed_hedge_fee_bps=assumed_hedge_fee_bps,
        hedge_ready=hedge_ready,
        hedge_route=hedge_route,
        effective_account_source=effective_account_source,
        hedge_disabled_reason=hedge_disabled_reason,
        ibkr_quote_age_ms=ibkr_quote_age_ms,
        fee_snapshot_age_s=fee_snapshot_age_s,
        hedge_latency_ms=hedge_latency_ms,
        hedge_slippage_bps_vs_mid=hedge_slippage_bps_vs_mid,
        ts_ms=ts_ms,
    )
    if mid_spread_bps is not None:
        payload["mid_spread_bps"] = mid_spread_bps
    if arb_bid_spread_bps is not None:
        payload["arb_bid_spread_bps"] = arb_bid_spread_bps
    if arb_ask_spread_bps is not None:
        payload["arb_ask_spread_bps"] = arb_ask_spread_bps
    if fee_assumptions is None:
        return payload

    assumptions_payload = dict(fee_assumptions)
    payload["fee_assumptions"] = assumptions_payload
    if "hedge_leg" in payload:
        hedge_payload = dict(payload["hedge_leg"])
        hedge_payload["fee_assumptions"] = dict(assumptions_payload)
        payload["hedge_leg"] = hedge_payload
    return payload


if __name__ == "flux.strategies.shared.equities_arb.observability":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.equities_arb.observability",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.equities_arb.observability":
    sys.modules.setdefault(
        "flux.strategies.shared.equities_arb.observability",
        sys.modules[__name__],
    )


__all__ = [
    "EquitiesArbFeeRules",
    "FeeAssumptions",
    "build_effective_ibkr_fee_bps",
    "build_fee_assumptions",
    "build_fee_assumptions_payload",
    "build_fee_aware_threshold_bps",
    "build_ibkr_ioc_limit",
    "build_maker_quote_price",
    "build_quote_snapshot_payload",
    "build_take_take_limit_price",
    "resolve_fee_rules",
    "validate_ibkr_quote",
]
