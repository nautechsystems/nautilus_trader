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
    local_qty: Decimal | None
    ts_ms: int
    stale_after_ms: int = DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
    maker_instrument_id: str = ""
    state: str = ""

    @property
    def expires_at_ms(self) -> int:
        return self.ts_ms + max(0, int(self.stale_after_ms))

    def is_fresh(self, *, now_ms_value: int) -> bool:
        return bool(self.ts_ms > 0 and now_ms_value <= self.expires_at_ms and self.state != "on_stop")


def encode_component(component: StrategyInventoryComponent) -> str:
    return json.dumps(
        {
            "strategy_id": component.strategy_id,
            "portfolio_id": component.portfolio_id,
            "base_currency": component.base_currency,
            "local_qty": _decimal_to_text(component.local_qty),
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
        local_qty=_decimal_from_value(raw.get("local_qty")),
        ts_ms=int(raw.get("ts_ms") or 0),
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


def aggregate_components(
    *,
    portfolio_id: str,
    base_currency: str,
    components: Mapping[str, StrategyInventoryComponent | None],
    required_strategy_ids: set[str],
    now_ms_value: int,
    stale_after_ms: int = DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS,
) -> dict[str, Any]:
    total = Decimal(0)
    fresh_any = False
    component_rows: list[dict[str, Any]] = []
    missing_required: list[str] = []

    for strategy_id, component in components.items():
        if component is None:
            row = {
                "strategy_id": strategy_id,
                "missing": True,
                "stale": False,
                "fresh": False,
                "required": strategy_id in required_strategy_ids,
                "local_qty": None,
                "ts_ms": 0,
            }
            if strategy_id in required_strategy_ids:
                missing_required.append(strategy_id)
            component_rows.append(row)
            continue

        fresh = component.is_fresh(now_ms_value=now_ms_value)
        stale = not fresh
        local_qty = component.local_qty
        if fresh and local_qty is not None:
            total += local_qty
            fresh_any = True
        elif strategy_id in required_strategy_ids:
            missing_required.append(strategy_id)

        component_rows.append(
            {
                "strategy_id": strategy_id,
                "missing": False,
                "stale": stale,
                "fresh": fresh,
                "required": strategy_id in required_strategy_ids,
                "local_qty": _decimal_to_text(local_qty),
                "ts_ms": component.ts_ms,
                "maker_instrument_id": component.maker_instrument_id,
                "state": component.state,
            },
        )

    component_rows.sort(key=lambda item: str(item["strategy_id"]))
    missing_required = sorted(set(missing_required))
    payload: dict[str, Any] = {
        "portfolio_id": portfolio_id,
        "base_currency": base_currency.upper(),
        "global_qty": _decimal_to_text(total) if fresh_any and not missing_required else None,
        "ts_ms": int(now_ms_value),
        "stale_after_ms": int(stale_after_ms),
        "components": component_rows,
        "missing_required": missing_required,
        "degraded": bool(missing_required),
    }
    return payload
