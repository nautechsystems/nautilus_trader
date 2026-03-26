from __future__ import annotations

import json
import math
from collections.abc import Mapping
from collections.abc import Sequence
from typing import Any

from flux.api._payloads_balances import build_balances_rows
from flux.api._payloads_balances import merge_portfolio_balances_rows
from flux.api._payloads_common import load_json
from flux.api._payloads_common import safe_float
from flux.common.portfolio_inventory import DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS
from flux.common.portfolio_inventory import StrategyInventoryComponent
from flux.common.portfolio_inventory import aggregate_components
from flux.common.portfolio_inventory import normalize_inventory_by_asset


def _coerce_finite_float(value: Any) -> float | None:
    out = safe_float(value)
    if out is None or not math.isfinite(out):
        return None
    return out


def _format_money_display(value: float) -> str:
    return f"{'-$' if value < 0 else '$'}{abs(value):.2f}"


def _balance_row_qty(row: Mapping[str, Any]) -> float | None:
    return _coerce_finite_float(
        row.get("signed_qty")
        if str(row.get("kind")).strip().lower() == "position"
        else row.get("total") or row.get("quantity") or row.get("signed_qty") or row.get("free"),
    )


def _coherent_balance_rows(rows: Sequence[Mapping[str, Any]]) -> list[dict[str, Any]]:
    coherent: list[dict[str, Any]] = []
    for source_row in rows:
        row = dict(source_row)
        mark = _coerce_finite_float(row.get("mark_raw") or row.get("mark"))
        qty = _balance_row_qty(row)
        if mark is not None:
            row["mark_raw"] = mark
            if qty is not None:
                row["mv_raw"] = qty * mark
        coherent.append(row)
    return coherent


def _balances_totals(rows: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    total_mv = 0.0
    for row in rows:
        mv = _coerce_finite_float(
            row.get("mv_raw")
            or row.get("mv")
            or row.get("notional")
            or row.get("notional_quote")
            or row.get("notional_usd"),
        )
        if mv is not None:
            total_mv += mv
    return {
        "mv_raw": total_mv,
        "mv_display": _format_money_display(total_mv),
    }


def build_balance_rows_by_strategy(
    *,
    raw_snapshots_by_strategy: Mapping[str, Any],
) -> dict[str, list[dict[str, Any]]]:
    rows_by_strategy: dict[str, list[dict[str, Any]]] = {}
    for strategy_id, raw_snapshot in raw_snapshots_by_strategy.items():
        rows_by_strategy[strategy_id] = build_balances_rows(
            raw_snapshot=load_json(raw_snapshot),
            strategy_id=strategy_id,
        )
    return rows_by_strategy


def build_portfolio_balance_rows(
    *,
    portfolio_id: str,
    balance_rows_by_strategy: Mapping[str, Sequence[Mapping[str, Any]]],
    shared_position_groups_by_strategy: Mapping[str, str] | None = None,
) -> list[dict[str, Any]]:
    return _coherent_balance_rows(
        merge_portfolio_balances_rows(
            rows_by_strategy=balance_rows_by_strategy,
            portfolio_id=portfolio_id,
            preserve_product_scope_cash=True,
            shared_position_groups_by_strategy=shared_position_groups_by_strategy,
        ),
    )


def _apply_single_asset_legacy_aliases(
    *,
    payload: dict[str, Any],
    normalized_inventory: Mapping[str, Mapping[str, Any]],
) -> None:
    if len(normalized_inventory) != 1:
        payload.pop("base_currency", None)
        payload.pop("inventory", None)
        payload.pop("components", None)
        return
    base_currency, inventory = next(iter(normalized_inventory.items()))
    payload["base_currency"] = base_currency
    payload["inventory"] = inventory
    payload["components"] = list(inventory.get("components") or [])


def build_portfolio_snapshot_v2(
    *,
    portfolio_id: str,
    inventory_by_asset: Mapping[str, Mapping[str, Any]],
    balance_rows: Sequence[Mapping[str, Any]],
    account_rows: Sequence[Mapping[str, Any]],
    account_scope_status: Sequence[Mapping[str, Any]] | None = None,
    account_totals: Mapping[str, Any] | None = None,
    now_ms_value: int,
) -> dict[str, Any]:
    normalized_inventory = normalize_inventory_by_asset(inventory_by_asset)
    coherent_balance_rows = _coherent_balance_rows(balance_rows)
    coherent_account_rows = _coherent_balance_rows(account_rows)
    payload: dict[str, Any] = {
        "portfolio_id": portfolio_id,
        "inventory_by_asset": normalized_inventory,
        "balances": {
            "rows": coherent_balance_rows,
            "totals": _balances_totals(coherent_balance_rows),
        },
        "accounts": {
            "rows": coherent_account_rows,
        },
        "server_ts_ms": int(now_ms_value),
    }
    if account_scope_status:
        payload["accounts"]["scope_status"] = [
            dict(scope)
            for scope in account_scope_status
            if isinstance(scope, Mapping)
        ]
    if isinstance(account_totals, Mapping) and account_totals:
        payload["accounts"]["totals"] = dict(account_totals)
    _apply_single_asset_legacy_aliases(
        payload=payload,
        normalized_inventory=normalized_inventory,
    )
    return payload


def build_portfolio_snapshot(
    *,
    portfolio_id: str,
    base_currency: str,
    inventory_components: Mapping[str, StrategyInventoryComponent | None],
    balance_rows_by_strategy: Mapping[str, Sequence[Mapping[str, Any]]],
    required_strategy_ids: set[str],
    now_ms_value: int,
    stale_after_ms: int = DEFAULT_PORTFOLIO_INVENTORY_STALE_AFTER_MS,
    aggregation_mode: str = "strict",
    inventory_payload: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    inventory = dict(inventory_payload) if inventory_payload is not None else aggregate_components(
        portfolio_id=portfolio_id,
        base_currency=base_currency,
        components=inventory_components,
        required_strategy_ids=required_strategy_ids,
        now_ms_value=now_ms_value,
        stale_after_ms=stale_after_ms,
        aggregation_mode=aggregation_mode,
    )
    balance_rows = build_portfolio_balance_rows(
        portfolio_id=portfolio_id,
        balance_rows_by_strategy=balance_rows_by_strategy,
    )
    return build_portfolio_snapshot_v2(
        portfolio_id=portfolio_id,
        inventory_by_asset={base_currency.upper(): inventory},
        balance_rows=balance_rows,
        account_rows=[],
        account_scope_status=None,
        account_totals=None,
        now_ms_value=now_ms_value,
    )


def encode_portfolio_snapshot(payload: Mapping[str, Any]) -> str:
    return json.dumps(payload, separators=(",", ":"))


def decode_portfolio_snapshot(raw: Any) -> dict[str, Any] | None:
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
    payload = dict(raw)
    raw_inventory = payload.get("inventory_by_asset")
    if isinstance(raw_inventory, Mapping):
        normalized_inventory = normalize_inventory_by_asset(raw_inventory)
        payload["inventory_by_asset"] = normalized_inventory
        _apply_single_asset_legacy_aliases(
            payload=payload,
            normalized_inventory=normalized_inventory,
        )
    return payload
