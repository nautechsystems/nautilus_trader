from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
from typing import Any
from collections.abc import Mapping


def _to_decimal(value: Any, *, field_name: str) -> Decimal:
    try:
        return Decimal(str(value))
    except Exception as e:  # pragma: no cover - defensive type guard
        raise ValueError(f"Invalid decimal value for {field_name}: {value!r}") from e


@dataclass(frozen=True, slots=True)
class MakerV4FeeRules:
    maker_fee_source: str
    hedge_fee_source: str
    hedge_fee_plan: str
    maker_fee_bps: Decimal
    hedge_fee_bps: Decimal
    fee_snapshot_age_s: Decimal | None


def resolve_fee_rules(
    *,
    runtime_params: Mapping[str, Any],
    maker_fee_bps: Any | None,
    fee_snapshot_age_s: Any | None = None,
) -> MakerV4FeeRules:
    maker_fee_source = str(runtime_params.get("maker_fee_source", "config")).strip()
    hedge_fee_source = str(runtime_params.get("hedge_fee_source", "config")).strip()
    hedge_fee_plan = str(runtime_params.get("hedge_fee_plan", "ibkr_pro_tiered")).strip()
    if maker_fee_source not in {"config", "hyperliquid_api"}:
        raise ValueError(f"Unsupported maker fee source: {maker_fee_source!r}")
    if hedge_fee_source != "config":
        raise ValueError(f"Unsupported hedge fee source: {hedge_fee_source!r}")
    if hedge_fee_plan != "ibkr_pro_tiered":
        raise ValueError(f"Unsupported hedge fee plan: {hedge_fee_plan!r}")
    if maker_fee_bps is None:
        raise ValueError("`maker_fee_bps` is required when maker_fee_source uses configured fees")

    hedge_fee_bps = _to_decimal(
        runtime_params.get("assumed_hedge_fee_bps", "0"),
        field_name="assumed_hedge_fee_bps",
    )
    snapshot_age = (
        None
        if fee_snapshot_age_s is None
        else _to_decimal(fee_snapshot_age_s, field_name="fee_snapshot_age_s")
    )
    return MakerV4FeeRules(
        maker_fee_source=maker_fee_source,
        hedge_fee_source=hedge_fee_source,
        hedge_fee_plan=hedge_fee_plan,
        maker_fee_bps=_to_decimal(maker_fee_bps, field_name="maker_fee_bps"),
        hedge_fee_bps=hedge_fee_bps,
        fee_snapshot_age_s=snapshot_age,
    )


__all__ = [
    "MakerV4FeeRules",
    "resolve_fee_rules",
]
