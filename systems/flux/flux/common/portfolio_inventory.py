from __future__ import annotations

import json
from collections.abc import Mapping
from dataclasses import dataclass
from decimal import Decimal
from typing import Any


DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS = 3_000


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _decimal_to_text(value: Decimal | None) -> str | None:
    if value is None:
        return None
    return format(value, "f")


def _decimal_from_value(value: Any) -> Decimal | None:
    text = _optional_text(value)
    if text is None:
        return None
    try:
        return Decimal(text)
    except Exception:
        return None


@dataclass(frozen=True, slots=True)
class StrategyInventoryComponent:
    strategy_id: str
    portfolio_id: str
    base_currency: str
    local_qty_base: Decimal | None
    ts_ms: int
    local_position_qty_venue: Decimal | None = None
    local_position_qty_base: Decimal | None = None
    local_spot_qty: Decimal | None = None
    qty_conversion_status: str | None = None
    qty_conversion_source: str | None = None
    stale_after_ms: int = DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
    maker_instrument_id: str = ""
    state: str = ""

    @property
    def local_qty(self) -> Decimal | None:
        return self.local_qty_base

    @property
    def expires_at_ms(self) -> int:
        return self.ts_ms + max(0, int(self.stale_after_ms))

    def is_fresh(self, *, now_ms_value: int) -> bool:
        return bool(self.ts_ms > 0 and now_ms_value <= self.expires_at_ms and self.state != "on_stop")


def encode_component(component: StrategyInventoryComponent) -> str:
    local_qty_base = _decimal_to_text(component.local_qty_base)
    return json.dumps(
        {
            "strategy_id": component.strategy_id,
            "portfolio_id": component.portfolio_id,
            "base_currency": component.base_currency,
            "local_qty_base": local_qty_base,
            "local_qty": local_qty_base,
            "local_position_qty_venue": _decimal_to_text(component.local_position_qty_venue),
            "local_position_qty_base": _decimal_to_text(component.local_position_qty_base),
            "local_spot_qty": _decimal_to_text(component.local_spot_qty),
            "qty_conversion_status": component.qty_conversion_status,
            "qty_conversion_source": component.qty_conversion_source,
            "ts_ms": int(component.ts_ms),
            "stale_after_ms": int(component.stale_after_ms),
            "maker_instrument_id": component.maker_instrument_id,
            "state": component.state,
        },
        separators=(",", ":"),
    )


def decode_component(raw: Any) -> StrategyInventoryComponent | None:
    if raw is None:
        return None
    if isinstance(raw, bytes):
        raw = raw.decode("utf-8", errors="replace")
    if isinstance(raw, str):
        try:
            raw = json.loads(raw)
        except Exception:
            return None
    if not isinstance(raw, Mapping):
        return None
    strategy_id = _optional_text(raw.get("strategy_id"))
    portfolio_id = _optional_text(raw.get("portfolio_id"))
    base_currency = _optional_text(raw.get("base_currency"))
    if not strategy_id or not portfolio_id or not base_currency:
        return None
    return StrategyInventoryComponent(
        strategy_id=strategy_id,
        portfolio_id=portfolio_id,
        base_currency=base_currency.upper(),
        local_qty_base=_decimal_from_value(raw.get("local_qty_base") or raw.get("local_qty")),
        ts_ms=int(raw.get("ts_ms") or 0),
        local_position_qty_venue=_decimal_from_value(raw.get("local_position_qty_venue")),
        local_position_qty_base=_decimal_from_value(raw.get("local_position_qty_base")),
        local_spot_qty=_decimal_from_value(raw.get("local_spot_qty")),
        qty_conversion_status=_optional_text(raw.get("qty_conversion_status")),
        qty_conversion_source=_optional_text(raw.get("qty_conversion_source")),
        stale_after_ms=int(raw.get("stale_after_ms") or DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS),
        maker_instrument_id=_optional_text(raw.get("maker_instrument_id")) or "",
        state=_optional_text(raw.get("state")) or "",
    )


def encode_portfolio_inventory(payload: Mapping[str, Any]) -> str:
    return json.dumps(payload, separators=(",", ":"))


def decode_portfolio_inventory(raw: Any) -> dict[str, Any] | None:
    if raw is None:
        return None
    if isinstance(raw, bytes):
        raw = raw.decode("utf-8", errors="replace")
    if isinstance(raw, str):
        try:
            raw = json.loads(raw)
        except Exception:
            return None
    if not isinstance(raw, Mapping):
        return None
    return dict(raw)


def normalize_inventory_by_asset(
    inventory_by_asset: Mapping[str, Mapping[str, Any]],
) -> dict[str, dict[str, Any]]:
    normalized: dict[str, dict[str, Any]] = {}
    for asset_id, payload in inventory_by_asset.items():
        canonical_asset_id = _optional_text(asset_id)
        if canonical_asset_id is None:
            continue
        canonical_asset_id = canonical_asset_id.upper()
        row = dict(payload) if isinstance(payload, Mapping) else {}
        base_currency = _optional_text(row.get("base_currency")) or canonical_asset_id
        row["base_currency"] = base_currency.upper()
        normalized[canonical_asset_id] = row
    return dict(sorted(normalized.items()))


def aggregate_components(
    *,
    portfolio_id: str,
    base_currency: str,
    components: Mapping[str, StrategyInventoryComponent | None],
    required_strategy_ids: set[str],
    now_ms_value: int,
    stale_after_ms: int = DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS,
    aggregation_mode: str = "strict",
) -> dict[str, Any]:
    total = Decimal(0)
    fresh_any = False
    usable_component_count = 0
    component_rows: list[dict[str, Any]] = []
    missing_required: list[str] = []
    stale_required: list[str] = []
    null_qty_required: list[str] = []
    mode = str(aggregation_mode or "strict").strip().lower()
    if mode not in {"strict", "partial"}:
        mode = "strict"

    for strategy_id, component in components.items():
        if component is None:
            row = {
                "strategy_id": strategy_id,
                "missing": True,
                "stale": False,
                "fresh": False,
                "required": strategy_id in required_strategy_ids,
                "local_qty_base": None,
                "local_qty": None,
                "local_position_qty_venue": None,
                "local_position_qty_base": None,
                "local_spot_qty": None,
                "qty_conversion_status": None,
                "qty_conversion_source": None,
                "ts_ms": 0,
            }
            if strategy_id in required_strategy_ids:
                missing_required.append(strategy_id)
            component_rows.append(row)
            continue

        fresh = component.is_fresh(now_ms_value=now_ms_value)
        stale = not fresh
        local_qty_base = component.local_qty_base
        if fresh and local_qty_base is not None:
            total += local_qty_base
            fresh_any = True
            usable_component_count += 1
        elif strategy_id in required_strategy_ids:
            if stale:
                stale_required.append(strategy_id)
            elif local_qty_base is None:
                null_qty_required.append(strategy_id)
            else:
                missing_required.append(strategy_id)

        local_qty_base_text = _decimal_to_text(local_qty_base)
        component_rows.append(
            {
                "strategy_id": strategy_id,
                "missing": False,
                "stale": stale,
                "fresh": fresh,
                "required": strategy_id in required_strategy_ids,
                "local_qty_base": local_qty_base_text,
                "local_qty": local_qty_base_text,
                "local_position_qty_venue": _decimal_to_text(component.local_position_qty_venue),
                "local_position_qty_base": _decimal_to_text(component.local_position_qty_base),
                "local_spot_qty": _decimal_to_text(component.local_spot_qty),
                "qty_conversion_status": component.qty_conversion_status,
                "qty_conversion_source": component.qty_conversion_source,
                "ts_ms": component.ts_ms,
                "maker_instrument_id": component.maker_instrument_id,
                "state": component.state,
            },
        )

    component_rows.sort(key=lambda item: str(item["strategy_id"]))
    missing_required = sorted(set(missing_required))
    stale_required = sorted(set(stale_required))
    null_qty_required = sorted(set(null_qty_required))
    degraded = bool(missing_required or stale_required or null_qty_required)
    global_qty_base: str | None
    if mode == "partial":
        global_qty_base = _decimal_to_text(total) if fresh_any else None
    else:
        global_qty_base = _decimal_to_text(total) if fresh_any and not degraded else None
    payload: dict[str, Any] = {
        "portfolio_id": portfolio_id,
        "base_currency": base_currency.upper(),
        "global_qty_base": global_qty_base,
        "global_qty": global_qty_base,
        "aggregation_mode": mode,
        "global_qty_base_complete": not degraded,
        "global_qty_complete": not degraded,
        "ts_ms": int(now_ms_value),
        "stale_after_ms": int(stale_after_ms),
        "components": component_rows,
        "missing_required": missing_required,
        "stale_required": stale_required,
        "null_qty_required": null_qty_required,
        "usable_component_count": usable_component_count,
        "expected_component_count": len(component_rows),
        "degraded": degraded,
    }
    return payload
