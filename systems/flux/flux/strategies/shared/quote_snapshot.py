from __future__ import annotations

import sys
from collections.abc import Mapping
from typing import Any

if __name__ == "flux.strategies.shared.quote_snapshot":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.quote_snapshot",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.quote_snapshot":
    sys.modules.setdefault("flux.strategies.shared.quote_snapshot", sys.modules[__name__])


def build_quote_snapshot_payload(
    *,
    maker_leg: Mapping[str, Any] | None,
    hedge_leg: Mapping[str, Any] | None,
    ref_leg: Mapping[str, Any] | None,
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
) -> dict[str, Any]:
    payload: dict[str, Any] = {}
    if maker_leg is not None:
        payload["maker_leg"] = dict(maker_leg)
    if hedge_leg is not None:
        payload["hedge_leg"] = dict(hedge_leg)
    if ref_leg is not None:
        payload["ref_leg"] = dict(ref_leg)

    scalar_fields = {
        "effective_spread_bps": effective_spread_bps,
        "quoted_spread_bps": quoted_spread_bps,
        "expected_maker_fee_bps": expected_maker_fee_bps,
        "assumed_hedge_fee_bps": assumed_hedge_fee_bps,
        "hedge_ready": hedge_ready,
        "hedge_route": hedge_route,
        "effective_account_source": effective_account_source,
        "hedge_disabled_reason": hedge_disabled_reason,
        "ibkr_quote_age_ms": ibkr_quote_age_ms,
        "fee_snapshot_age_s": fee_snapshot_age_s,
        "hedge_latency_ms": hedge_latency_ms,
        "hedge_slippage_bps_vs_mid": hedge_slippage_bps_vs_mid,
        "ts_ms": ts_ms,
    }
    for field_name, value in scalar_fields.items():
        if value is not None:
            payload[field_name] = value
    return payload


__all__ = [
    "build_quote_snapshot_payload",
]
