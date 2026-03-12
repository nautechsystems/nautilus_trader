from __future__ import annotations

"""Balance and portfolio payload normalization helpers."""

from collections.abc import Mapping
from collections.abc import Sequence
from decimal import Decimal
from typing import Any

from ._payloads_common import ContractCatalogEntry
from ._payloads_common import _decimal_text
from ._payloads_common import _is_position_row
from ._payloads_common import _position_signed_qty
from ._payloads_common import _position_venue_signed_qty
from ._payloads_common import _raw_symbol_from_instrument_id
from ._payloads_common import _to_decimal
from ._payloads_common import as_list
from ._payloads_common import canonical_naming_fields
from ._payloads_common import coerce_ts_ms
from ._payloads_common import contract_id_for_leg
from ._payloads_common import decode_text
from ._payloads_common import enrich_row_with_canonical_naming
from ._payloads_common import normalize_symbol_parts
from ._payloads_common import safe_float
from ._payloads_common import strategy_id_from_row


def _strategy_product_type_hint(strategy_id: Any) -> str:
    text = decode_text(strategy_id).strip().lower()
    if not text:
        return ""
    if any(token in text for token in ("_perp_", "-perp-", ":perp:", "/perp/")) or text.endswith(
        ("_perp", "-perp", ":perp", "/perp"),
    ):
        return "perp"
    if any(token in text for token in ("_spot_", "-spot-", ":spot:", "/spot/")) or text.endswith(
        ("_spot", "-spot", ":spot", "/spot"),
    ):
        return "spot"
    return ""


def _row_product_type_hint(row: Mapping[str, Any], strategy_id: str | None = None) -> str:
    explicit = decode_text(row.get("product_type") or row.get("market_type")).strip().lower()
    if explicit in {"spot", "perp"}:
        return explicit
    instrument_id = decode_text(row.get("instrument_id") or row.get("symbol")).strip().upper()
    if instrument_id:
        return decode_text(
            canonical_naming_fields(
                instrument_id=instrument_id,
                exchange=row.get("exchange"),
                venue=row.get("venue"),
                symbol=row.get("symbol"),
                asset=row.get("asset"),
                inventory_asset=row.get("inventory_asset")
                or row.get("coin")
                or row.get("base")
                or row.get("asset"),
                is_position=_is_position_row(dict(row)),
            ).get("product_type"),
        ).strip().lower()
    return _strategy_product_type_hint(strategy_id or row.get("strategy_id"))


def _position_group_key(row: dict[str, Any], strategy_id: str) -> tuple[str, str, str] | None:
    sid = strategy_id_from_row(row, strategy_id)
    exchange = decode_text(row.get("exchange") or row.get("venue")).strip().lower()
    instrument = (
        decode_text(
            row.get("instrument_id")
            or row.get("symbol")
            or row.get("asset")
            or row.get("coin")
            or row.get("base"),
        )
        .strip()
        .upper()
    )
    if not instrument:
        return None
    return (sid, exchange, instrument)


def _position_agg_seed(row: dict[str, Any]) -> dict[str, Any]:
    return {
        "row": dict(row),
        "row_count": 0,
        "qty_base": Decimal(0),
        "has_qty_base": False,
        "qty_venue": Decimal(0),
        "has_qty_venue": False,
        "avg_num": Decimal(0),
        "avg_den": Decimal(0),
        "upnl": Decimal(0),
        "has_upnl": False,
        "rpnl": Decimal(0),
        "has_rpnl": False,
        "qty_conversion_status": decode_text(row.get("qty_conversion_status")).strip() or None,
        "qty_conversion_source": decode_text(row.get("qty_conversion_source")).strip() or None,
        "mark": None,
        "mark_ts_ms": None,
        "valuation_totals": {
            "mv_raw": Decimal(0),
            "notional": Decimal(0),
            "notional_quote": Decimal(0),
            "notional_usd": Decimal(0),
        },
        "valuation_counts": {
            "mv_raw": 0,
            "notional": 0,
            "notional_quote": 0,
            "notional_usd": 0,
        },
    }


def _position_agg_update(agg: dict[str, Any], row: dict[str, Any]) -> None:
    agg["row_count"] += 1
    qty_base = _position_signed_qty(row)
    qty_venue = _position_venue_signed_qty(row)
    if qty_base is not None:
        agg["qty_base"] += qty_base
        agg["has_qty_base"] = True
    if qty_venue is not None:
        agg["qty_venue"] += qty_venue
        agg["has_qty_venue"] = True
    qty_weight = qty_venue if qty_venue is not None else qty_base
    if qty_weight is not None:
        avg_px = _to_decimal(
            row.get("avg_px")
            or row.get("avg_price")
            or row.get("entry_price")
            or row.get("avg_px_open")
            or row.get("avg_px_close"),
        )
        if avg_px is not None and qty_weight != 0:
            agg["avg_num"] += abs(qty_weight) * avg_px
            agg["avg_den"] += abs(qty_weight)

    upnl = _to_decimal(
        row.get("unrealized_pnl")
        or row.get("unrealizedPnl")
    )
    if upnl is not None:
        agg["upnl"] += upnl
        agg["has_upnl"] = True
    rpnl = _to_decimal(row.get("realized_pnl") or row.get("realizedPnl"))
    if rpnl is not None:
        agg["rpnl"] += rpnl
        agg["has_rpnl"] = True
    if agg["qty_conversion_status"] is None:
        agg["qty_conversion_status"] = decode_text(row.get("qty_conversion_status")).strip() or None
    if agg["qty_conversion_source"] is None:
        agg["qty_conversion_source"] = decode_text(row.get("qty_conversion_source")).strip() or None
    mark = _to_decimal(row.get("mark_raw") or row.get("mark"))
    if mark is not None:
        mark_ts_ms = _row_ts_ms(row)
        previous_mark_ts_ms = agg.get("mark_ts_ms")
        if previous_mark_ts_ms is None or mark_ts_ms >= previous_mark_ts_ms:
            agg["mark"] = mark
            agg["mark_ts_ms"] = mark_ts_ms
    valuation_totals = agg["valuation_totals"]
    valuation_counts = agg["valuation_counts"]
    for key in ("mv_raw", "notional", "notional_quote", "notional_usd"):
        valuation = _to_decimal(row.get(key))
        if valuation is None:
            continue
        valuation_totals[key] += valuation
        valuation_counts[key] += 1


def _group_position_rows(
    rows: list[dict[str, Any]],
    *,
    strategy_id: str,
) -> tuple[dict[tuple[str, str, str], dict[str, Any]], list[dict[str, Any]]]:
    non_positions: list[dict[str, Any]] = []
    grouped: dict[tuple[str, str, str], dict[str, Any]] = {}
    for row in rows:
        if not isinstance(row, dict):
            continue
        if not _is_position_row(row):
            non_positions.append(dict(row))
            continue
        key = _position_group_key(row, strategy_id)
        if key is None:
            non_positions.append(dict(row))
            continue
        agg = grouped.get(key)
        if agg is None:
            agg = _position_agg_seed(row)
            grouped[key] = agg
        _position_agg_update(agg, row)
    return grouped, non_positions


def _clear_position_valuation_fields(row: dict[str, Any]) -> None:
    for key in (
        "mark",
        "mark_raw",
        "mv",
        "mv_raw",
        "mv_display",
        "notional",
        "notional_quote",
        "notional_usd",
    ):
        row.pop(key, None)


def _apply_unmarked_position_valuation(row: dict[str, Any], agg: Mapping[str, Any]) -> None:
    row_count = int(agg.get("row_count") or 0)
    if row_count <= 1:
        return

    _clear_position_valuation_fields(row)
    valuation_totals = agg.get("valuation_totals") or {}
    valuation_counts = agg.get("valuation_counts") or {}
    for key in ("mv_raw", "notional", "notional_quote", "notional_usd"):
        if valuation_counts.get(key) != row_count:
            continue
        total = valuation_totals.get(key)
        if total is not None:
            row[key] = float(total)


def _position_row_from_agg(key: tuple[str, str, str], agg: dict[str, Any]) -> dict[str, Any] | None:
    sid, exchange, instrument = key
    has_qty_base = bool(agg["has_qty_base"])
    has_qty_venue = bool(agg["has_qty_venue"])
    qty_base: Decimal | None = agg["qty_base"] if has_qty_base else None
    qty_venue: Decimal | None = agg["qty_venue"] if has_qty_venue else None
    qty_default = qty_base if qty_base is not None else qty_venue
    if qty_default is None or qty_default == 0:
        return None
    side = "LONG" if qty_default > 0 else "SHORT"
    avg_px = agg["avg_num"] / agg["avg_den"] if agg["avg_den"] > 0 else None
    upnl = agg["upnl"] if agg["has_upnl"] else None
    rpnl = agg["rpnl"] if agg["has_rpnl"] else None
    mark = agg.get("mark")

    row = dict(agg["row"])
    row["strategy_id"] = sid
    if exchange:
        row["exchange"] = exchange
    row.setdefault("kind", "position")
    row["instrument_id"] = decode_text(row.get("instrument_id") or instrument).strip() or instrument
    if not decode_text(row.get("asset")).strip():
        row["asset"] = instrument
    qty_text = _decimal_text(qty_default)
    row["signed_qty"] = qty_text
    row["quantity"] = _decimal_text(abs(qty_default))
    row["free"] = qty_text
    row["total"] = qty_text
    if qty_base is not None:
        row["signed_qty_base"] = _decimal_text(qty_base)
        row["quantity_base"] = _decimal_text(abs(qty_base))
    if qty_venue is not None:
        row["signed_qty_venue"] = _decimal_text(qty_venue)
        row["quantity_venue"] = _decimal_text(abs(qty_venue))
    if agg["qty_conversion_status"] is not None:
        row["qty_conversion_status"] = agg["qty_conversion_status"]
    if agg["qty_conversion_source"] is not None:
        row["qty_conversion_source"] = agg["qty_conversion_source"]
    if mark is not None:
        _clear_position_valuation_fields(row)
        row["mark"] = float(mark)
        row["mark_raw"] = float(mark)
        row["mv_raw"] = float(qty_default * mark)
    else:
        _apply_unmarked_position_valuation(row, agg)

    meta_parts = [side]
    if avg_px is not None:
        meta_parts.append(f"avg={_decimal_text(avg_px)}")
    if upnl is not None:
        meta_parts.append(f"uPnL={_decimal_text(upnl)}")
    if rpnl is not None:
        meta_parts.append(f"rPnL={_decimal_text(rpnl)}")
    row["locked"] = " ".join(meta_parts)
    row["side"] = side
    row["row_id"] = f"{sid}:pos:{exchange}:{instrument}"
    return row


def _aggregate_position_rows(rows: list[dict[str, Any]], strategy_id: str) -> list[dict[str, Any]]:
    grouped, non_positions = _group_position_rows(rows, strategy_id=strategy_id)
    merged_positions: list[dict[str, Any]] = []
    for key, agg in grouped.items():
        merged = _position_row_from_agg(key, agg)
        if merged is not None:
            merged_positions.append(merged)
    return merged_positions + non_positions


def build_balances_rows(*, raw_snapshot: Any, strategy_id: str) -> list[dict[str, Any]]:
    """Flatten a raw strategy balance snapshot into normalized balance and position rows."""

    def _append_event_balances(
        *,
        events: Any,
        sid: str,
        root_ts_ms: Any,
        row_prefix: str,
        product_type: str,
        account_id: str = "",
        account_venue: str = "",
    ) -> int:
        appended = 0
        if not isinstance(events, list):
            return appended
        for event_index, event in enumerate(events):
            if not isinstance(event, dict):
                continue
            event_balances = event.get("balances")
            if not isinstance(event_balances, list):
                continue
            event_account_id = decode_text(event.get("account_id")).strip() or account_id
            venue = decode_text(event.get("venue") or account_venue).strip().lower()
            if not venue and event_account_id and "-" in event_account_id:
                venue = event_account_id.split("-", maxsplit=1)[0].lower()
            for balance_index, balance in enumerate(event_balances):
                if not isinstance(balance, dict):
                    continue
                asset = decode_text(balance.get("currency")).strip().upper()
                flattened_row = {
                    "strategy_id": sid,
                    "exchange": venue,
                    "asset": asset,
                    "coin": asset,
                    "base": asset,
                    "free": balance.get("free"),
                    "locked": balance.get("locked"),
                    "total": balance.get("total"),
                    "ts_ms": event.get("ts_ms") if event.get("ts_ms") is not None else root_ts_ms,
                    "row_id": f"{row_prefix}:evt:{event_index}:{balance_index}",
                }
                if event_account_id:
                    flattened_row["account_id"] = event_account_id
                    flattened_row["account"] = event_account_id
                if product_type in {"spot", "perp"}:
                    flattened_row["product_type"] = product_type
                    flattened_row["market_type"] = product_type
                out.append(flattened_row)
                appended += 1
        return appended

    rows = as_list(raw_snapshot)
    out: list[dict[str, Any]] = []
    for row in rows:
        if not isinstance(row, dict):
            continue
        current = dict(row)
        sid = strategy_id_from_row(current, strategy_id)
        current["strategy_id"] = sid
        current_product_type = _row_product_type_hint(current, sid)
        flattened = 0
        root_ts_ms = current.get("ts_ms")

        accounts = current.get("accounts")
        if isinstance(accounts, list) and accounts:
            for index, account in enumerate(accounts):
                if isinstance(account, dict):
                    account_row_id = f"{sid}:acc:{index}"
                    account_flattened = _append_event_balances(
                        events=account.get("events"),
                        sid=sid,
                        root_ts_ms=root_ts_ms,
                        row_prefix=account_row_id,
                        product_type=current_product_type,
                        account_id=decode_text(account.get("account_id")).strip(),
                        account_venue=decode_text(account.get("venue")).strip(),
                    )
                    if account_flattened:
                        flattened += account_flattened
                        continue
                    flattened_row = {
                        **account,
                        "strategy_id": sid,
                        "row_id": account_row_id,
                    }
                    if current_product_type in {"spot", "perp"}:
                        flattened_row["product_type"] = current_product_type
                        flattened_row["market_type"] = current_product_type
                    if root_ts_ms is not None and flattened_row.get("ts_ms") is None:
                        flattened_row["ts_ms"] = root_ts_ms
                    out.append(flattened_row)
                    flattened += 1

        positions = current.get("positions")
        if isinstance(positions, list) and positions:
            for index, position in enumerate(positions):
                if not isinstance(position, dict):
                    continue
                flattened_row = {
                    **position,
                    "strategy_id": sid,
                    "row_id": f"{sid}:posraw:{index}",
                }
                flattened_row.setdefault("kind", "position")
                if root_ts_ms is not None and flattened_row.get("ts_ms") is None:
                    flattened_row["ts_ms"] = root_ts_ms
                out.append(flattened_row)
                flattened += 1

        flattened += _append_event_balances(
            events=current.get("events"),
            sid=sid,
            root_ts_ms=root_ts_ms,
            row_prefix=sid,
            product_type=current_product_type,
        )

        if flattened > 0:
            continue

        if current_product_type in {"spot", "perp"}:
            current.setdefault("product_type", current_product_type)
            current.setdefault("market_type", current_product_type)
        out.append(current)

    filtered = [row for row in out if strategy_id_from_row(row, strategy_id) == strategy_id]
    return [
        enrich_row_with_canonical_naming(row)
        for row in _aggregate_position_rows(filtered, strategy_id)
    ]


def _row_ts_ms(row: Mapping[str, Any]) -> int:
    ts_ms = coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
    return ts_ms if ts_ms is not None else 0


def _balance_row_qty(row: Mapping[str, Any]) -> float | None:
    return safe_float(
        row.get("total")
        or row.get("quantity")
        or row.get("signed_qty")
        or row.get("qty")
        or row.get("free"),
    )


def _carry_forward_cash_mark(
    row: dict[str, Any],
    previous: tuple[int, dict[str, Any]] | None,
) -> dict[str, Any]:
    if previous is None or row.get("mark_raw") is not None:
        return row

    previous_row = previous[1]
    previous_mark = safe_float(previous_row.get("mark_raw") or previous_row.get("mark"))
    if previous_mark is None:
        return row

    row["mark_raw"] = previous_mark
    qty = _balance_row_qty(row)
    if qty is not None:
        row["mv_raw"] = qty * previous_mark
    elif previous_row.get("mv_raw") is not None:
        row["mv_raw"] = previous_row.get("mv_raw")
    return row


def _cash_quantity_component_signature(value: Any) -> str:
    parsed = _to_decimal(value)
    if parsed is not None:
        return _decimal_text(parsed)
    return decode_text(value).strip()


def _cash_quantity_signature(row: Mapping[str, Any]) -> tuple[str, str, str]:
    return (
        _cash_quantity_component_signature(row.get("free")),
        _cash_quantity_component_signature(row.get("locked")),
        _cash_quantity_component_signature(
            row.get("total") or row.get("quantity") or row.get("signed_qty"),
        ),
    )


def _cash_row_key(
    row: Mapping[str, Any],
    *,
    preserve_product_scope_cash: bool = False,
) -> tuple[str, str, str, str] | None:
    base_key = _cash_row_identity(row)
    if base_key is None:
        return None
    exchange, account, asset = base_key
    product_type = _row_product_type_hint(row)
    merge_scope = ""
    if (
        preserve_product_scope_cash
        and product_type in {"spot", "perp"}
        and asset not in _STABLE_BALANCE_ASSETS
    ):
        merge_scope = product_type
    elif product_type == "perp" and asset not in _STABLE_BALANCE_ASSETS:
        merge_scope = product_type
    return (exchange, account, asset, merge_scope)


def _cash_row_identity(row: Mapping[str, Any]) -> tuple[str, str, str] | None:
    exchange = decode_text(row.get("exchange") or row.get("venue")).strip().lower()
    account = decode_text(
        row.get("account")
        or row.get("account_id")
        or row.get("wallet")
        or row.get("subaccount"),
    ).strip()
    asset = decode_text(row.get("asset") or row.get("coin") or row.get("base")).strip().upper()
    if not asset:
        return None
    return (exchange, account, asset)


def _portfolio_cash_row_id(
    portfolio_id: str,
    cash_key: tuple[str, str, str, str],
) -> str:
    exchange, account, asset, merge_scope = cash_key
    if merge_scope:
        return f"{portfolio_id}:cash:{exchange}:{account}:{merge_scope}:{asset}"
    return f"{portfolio_id}:cash:{exchange}:{account}:{asset}"


def _is_zero_balance_qty(qty: float | None) -> bool:
    return qty is not None and qty == 0.0


def _should_replace_cash_row(
    *,
    cash_key: tuple[str, str, str, str],
    previous: tuple[int, dict[str, Any]] | None,
    candidate_row: Mapping[str, Any],
    candidate_ts_ms: int,
) -> bool:
    if previous is None:
        return True

    previous_ts_ms, previous_row = previous
    merge_scope = cash_key[3]
    if not merge_scope and cash_key[2] in _STABLE_BALANCE_ASSETS:
        previous_qty = _balance_row_qty(previous_row)
        candidate_qty = _balance_row_qty(candidate_row)
        previous_is_zero = _is_zero_balance_qty(previous_qty)
        candidate_is_zero = _is_zero_balance_qty(candidate_qty)

        if previous_is_zero and not candidate_is_zero:
            return True
        if not previous_is_zero and candidate_is_zero:
            return False

        if candidate_ts_ms == previous_ts_ms:
            previous_product_type = _row_product_type_hint(previous_row)
            candidate_product_type = _row_product_type_hint(candidate_row)
            if previous_product_type != candidate_product_type:
                return candidate_product_type == "spot"

    return candidate_ts_ms >= previous_ts_ms


def _collapse_duplicate_cash_scope_rows(
    cash_latest: dict[tuple[str, str, str, str], tuple[int, dict[str, Any]]],
    cash_source_strategy_ids: dict[tuple[str, str, str, str], set[str]],
) -> None:
    grouped: dict[tuple[str, str, str, str, str, str], list[tuple[str, str, str, str]]] = {}
    for cash_key, (_ts_ms, row) in cash_latest.items():
        exchange, account, asset, _merge_scope = cash_key
        if asset in _STABLE_BALANCE_ASSETS:
            continue
        grouped.setdefault(
            (exchange, account, asset, *_cash_quantity_signature(row)),
            [],
        ).append(cash_key)

    for duplicate_keys in grouped.values():
        if len(duplicate_keys) < 2:
            continue

        keep_key = max(
            duplicate_keys,
            key=lambda key: (
                _row_product_type_hint(cash_latest[key][1]) == "spot",
                safe_float(cash_latest[key][1].get("mark_raw") or cash_latest[key][1].get("mark"))
                is not None,
                cash_latest[key][0],
                decode_text(cash_latest[key][1].get("row_id")).strip(),
            ),
        )
        latest_ts_ms = max(cash_latest[key][0] for key in duplicate_keys)
        merged_strategy_ids: set[str] = set()
        for key in duplicate_keys:
            merged_strategy_ids.update(cash_source_strategy_ids.get(key, set()))

        kept_row = dict(cash_latest[keep_key][1])
        if latest_ts_ms > 0:
            kept_row["ts_ms"] = latest_ts_ms
        if kept_row.get("account") and len(merged_strategy_ids) > 1:
            kept_row["scope"] = "shared_account"

        cash_latest[keep_key] = (latest_ts_ms, kept_row)
        cash_source_strategy_ids[keep_key] = merged_strategy_ids

        for key in duplicate_keys:
            if key == keep_key:
                continue
            cash_latest.pop(key, None)
            cash_source_strategy_ids.pop(key, None)


def _annotate_shared_scope_stable_cash_row(row: Mapping[str, Any]) -> dict[str, Any]:
    return dict(row)


def _row_exchange_hint(row: Mapping[str, Any]) -> str:
    exchange = decode_text(row.get("exchange") or row.get("venue")).strip().lower()
    if exchange:
        return exchange
    instrument_id = decode_text(row.get("instrument_id") or row.get("symbol")).strip().upper()
    if "." not in instrument_id:
        return ""
    suffix = instrument_id.split(".", maxsplit=1)[1]
    return suffix.lower()


def _balance_inventory_key(row: Mapping[str, Any]) -> tuple[str, str] | None:
    exchange = _row_exchange_hint(row)
    if not exchange:
        return None

    inventory_asset = decode_text(
        row.get("inventory_asset")
        or row.get("asset")
        or row.get("coin")
        or row.get("base"),
    ).strip().upper()
    if not inventory_asset:
        naming = canonical_naming_fields(
            instrument_id=row.get("instrument_id"),
            exchange=row.get("exchange"),
            venue=row.get("venue"),
            symbol=row.get("symbol"),
            asset=row.get("asset"),
            inventory_asset=row.get("coin") or row.get("asset") or row.get("base"),
            is_position=_is_position_row(dict(row)),
        )
        inventory_asset = decode_text(naming.get("inventory_asset")).strip().upper()
    if not inventory_asset:
        return None
    return (exchange, inventory_asset)


def _balance_account_hint(row: Mapping[str, Any]) -> str:
    return decode_text(
        row.get("account")
        or row.get("account_id")
        or row.get("subaccount"),
    ).strip()


def _balance_duplicate_display_key(row: Mapping[str, Any]) -> tuple[str, str, str] | None:
    inventory_key = _balance_inventory_key(row)
    if inventory_key is None:
        return None
    exchange, inventory_asset = inventory_key
    return (exchange, _balance_account_hint(row), inventory_asset)


def _balance_product_type(row: Mapping[str, Any]) -> str:
    product_type = decode_text(row.get("product_type")).strip().lower()
    if product_type in {"spot", "perp"}:
        return product_type
    naming = canonical_naming_fields(
        instrument_id=row.get("instrument_id"),
        exchange=row.get("exchange"),
        venue=row.get("venue"),
        symbol=row.get("symbol"),
        asset=row.get("asset"),
        inventory_asset=row.get("coin") or row.get("asset") or row.get("base"),
        is_position=_is_position_row(dict(row)),
    )
    return decode_text(naming.get("product_type")).strip().lower()


def _is_shared_scope_stable_cash_row(row: Mapping[str, Any]) -> bool:
    if _is_position_row(dict(row)):
        return False
    if row.get("scope") != "shared_account":
        return False
    asset = decode_text(row.get("asset") or row.get("coin") or row.get("base")).strip().upper()
    if asset not in _STABLE_BALANCE_ASSETS:
        return False
    return _balance_product_type(row) in {"spot", "perp"}


def _canonical_shared_stable_cash_row_id(row: Mapping[str, Any]) -> str:
    row_id = decode_text(row.get("row_id")).strip()
    product_type = _balance_product_type(row)
    asset = decode_text(row.get("asset") or row.get("coin") or row.get("base")).strip().upper()
    if row_id and product_type in {"spot", "perp"} and asset and row_id.endswith(f":{product_type}:{asset}"):
        prefix, _scope, asset = row_id.rsplit(":", maxsplit=2)
        return f"{prefix}:{asset}"
    exchange = decode_text(row.get("exchange") or row.get("venue")).strip().lower()
    account = decode_text(row.get("account") or row.get("account_id")).strip()
    strategy_id = decode_text(row.get("strategy_id")).strip()
    if strategy_id and exchange and asset:
        return f"{strategy_id}:cash:{exchange}:{account}:{asset}" if account else f"{strategy_id}:cash:{exchange}:{asset}"
    return row_id


def _should_prefer_shared_stable_cash_row(
    previous_row: Mapping[str, Any],
    candidate_row: Mapping[str, Any],
) -> bool:
    previous_qty = _balance_row_qty(previous_row)
    candidate_qty = _balance_row_qty(candidate_row)
    previous_is_zero = _is_zero_balance_qty(previous_qty)
    candidate_is_zero = _is_zero_balance_qty(candidate_qty)
    if previous_is_zero and not candidate_is_zero:
        return True
    if not previous_is_zero and candidate_is_zero:
        return False

    previous_ts_ms = _row_ts_ms(previous_row)
    candidate_ts_ms = _row_ts_ms(candidate_row)
    if candidate_ts_ms != previous_ts_ms:
        return candidate_ts_ms > previous_ts_ms

    previous_product_type = _balance_product_type(previous_row)
    candidate_product_type = _balance_product_type(candidate_row)
    if previous_product_type != candidate_product_type:
        return candidate_product_type == "spot"

    return decode_text(candidate_row.get("row_id")).strip() >= decode_text(previous_row.get("row_id")).strip()


def _collapse_shared_scope_stable_cash_rows(
    rows: Sequence[Mapping[str, Any]],
) -> list[dict[str, Any]]:
    normalized_rows = [dict(source_row) for source_row in rows if isinstance(source_row, Mapping)]
    grouped: dict[tuple[str, str, str], list[int]] = {}
    for index, row in enumerate(normalized_rows):
        if not _is_shared_scope_stable_cash_row(row):
            continue
        key = _cash_row_identity(row)
        if key is None:
            continue
        grouped.setdefault(key, []).append(index)

    if not grouped:
        return normalized_rows

    drop_indexes: set[int] = set()
    for indexes in grouped.values():
        if len(indexes) < 2:
            continue
        keep_index = indexes[0]
        keep_row = normalized_rows[keep_index]
        for index in indexes[1:]:
            row = normalized_rows[index]
            if _should_prefer_shared_stable_cash_row(keep_row, row):
                keep_index = index
                keep_row = row

        canonical = dict(keep_row)
        canonical["row_id"] = _canonical_shared_stable_cash_row_id(canonical)
        canonical.pop("display_name_short", None)
        canonical.pop("display_name_long", None)
        normalized_rows[keep_index] = canonical
        for index in indexes:
            if index != keep_index:
                drop_indexes.add(index)

    return [
        row
        for index, row in enumerate(normalized_rows)
        if index not in drop_indexes
    ]


def collapse_balance_display_rows(rows: Sequence[Mapping[str, Any]]) -> list[dict[str, Any]]:
    """
    Prefer spot cash rows over duplicate spot-position rows for the same venue/base asset.

    Some venue snapshots publish spot inventory twice: once as account cash and once as a spot "position".
    Balances should render that inventory once, while Signal and other raw snapshot consumers remain untouched.
    """

    cash_keys: dict[tuple[str, str], set[str]] = {}
    normalized_rows = _collapse_shared_scope_stable_cash_rows(rows)

    for row in normalized_rows:
        if _is_position_row(row):
            continue
        if _balance_product_type(row) != "spot":
            continue
        key = _balance_duplicate_display_key(row)
        if key is not None:
            exchange, account, inventory_asset = key
            cash_keys.setdefault((exchange, inventory_asset), set()).add(account)

    collapsed: list[dict[str, Any]] = []
    for row in normalized_rows:
        if _is_position_row(row) and _balance_product_type(row) == "spot":
            key = _balance_duplicate_display_key(row)
            if key is not None:
                exchange, account, inventory_asset = key
                cash_accounts = cash_keys.get((exchange, inventory_asset), set())
                if cash_accounts and (not account or account in cash_accounts or "" in cash_accounts):
                    continue
        collapsed.append(row)
    return collapsed


def _position_portfolio_key(row: Mapping[str, Any]) -> tuple[str, str] | None:
    exchange = decode_text(row.get("exchange") or row.get("venue")).strip().lower()
    instrument = decode_text(
        row.get("instrument_id")
        or row.get("symbol")
        or row.get("asset")
        or row.get("coin")
        or row.get("base"),
    ).strip().upper()
    if not instrument:
        return None
    return (exchange, instrument)


def _position_portfolio_row_from_agg(
    key: tuple[str, str],
    agg: Mapping[str, Any],
    *,
    portfolio_id: str,
) -> dict[str, Any] | None:
    exchange, instrument = key
    has_qty_base = bool(agg["has_qty_base"])
    has_qty_venue = bool(agg["has_qty_venue"])
    qty_base: Decimal | None = agg["qty_base"] if has_qty_base else None
    qty_venue: Decimal | None = agg["qty_venue"] if has_qty_venue else None
    qty_default = qty_base if qty_base is not None else qty_venue
    if qty_default is None or qty_default == 0:
        return None

    side = "LONG" if qty_default > 0 else "SHORT"
    avg_px = agg["avg_num"] / agg["avg_den"] if agg["avg_den"] > 0 else None
    upnl = agg["upnl"] if agg["has_upnl"] else None
    rpnl = agg["rpnl"] if agg["has_rpnl"] else None
    mark = agg.get("mark")

    row = dict(agg["row"])
    row["strategy_id"] = portfolio_id
    if exchange:
        row["exchange"] = exchange
    row.setdefault("kind", "position")
    row["instrument_id"] = decode_text(row.get("instrument_id") or instrument).strip() or instrument
    if not decode_text(row.get("asset")).strip():
        row["asset"] = instrument
    qty_text = _decimal_text(qty_default)
    row["signed_qty"] = qty_text
    row["quantity"] = _decimal_text(abs(qty_default))
    row["free"] = qty_text
    row["total"] = qty_text
    if qty_base is not None:
        row["signed_qty_base"] = _decimal_text(qty_base)
        row["quantity_base"] = _decimal_text(abs(qty_base))
    if qty_venue is not None:
        row["signed_qty_venue"] = _decimal_text(qty_venue)
        row["quantity_venue"] = _decimal_text(abs(qty_venue))
    qty_conversion_status = decode_text(agg.get("qty_conversion_status")).strip()
    qty_conversion_source = decode_text(agg.get("qty_conversion_source")).strip()
    if qty_conversion_status:
        row["qty_conversion_status"] = qty_conversion_status
    if qty_conversion_source:
        row["qty_conversion_source"] = qty_conversion_source
    if mark is not None:
        _clear_position_valuation_fields(row)
        row["mark"] = float(mark)
        row["mark_raw"] = float(mark)
        row["mv_raw"] = float(qty_default * mark)
    else:
        _apply_unmarked_position_valuation(row, agg)

    meta_parts = [side]
    if avg_px is not None:
        meta_parts.append(f"avg={_decimal_text(avg_px)}")
    if upnl is not None:
        meta_parts.append(f"uPnL={_decimal_text(upnl)}")
    if rpnl is not None:
        meta_parts.append(f"rPnL={_decimal_text(rpnl)}")
    row["locked"] = " ".join(meta_parts)
    row["side"] = side
    row["row_id"] = f"{portfolio_id}:pos:{exchange}:{instrument}"
    return row


def merge_portfolio_balances_rows(
    *,
    rows_by_strategy: Mapping[str, Sequence[Mapping[str, Any]]],
    portfolio_id: str = "tokenmm",
    preserve_product_scope_cash: bool = False,
) -> list[dict[str, Any]]:
    """Merge per-strategy balance rows into a single portfolio-level balance view."""

    cash_latest: dict[tuple[str, str, str, str], tuple[int, dict[str, Any]]] = {}
    cash_latest_marked: dict[tuple[str, str, str, str], tuple[int, dict[str, Any]]] = {}
    cash_source_strategy_ids: dict[tuple[str, str, str, str], set[str]] = {}
    position_grouped: dict[tuple[str, str], dict[str, Any]] = {}
    passthrough_rows: list[dict[str, Any]] = []
    scoped_cash_conflicts: set[tuple[str, str, str]] = set()

    if preserve_product_scope_cash:
        product_types_by_cash: dict[tuple[str, str, str], set[str]] = {}
        for rows in rows_by_strategy.values():
            for source_row in rows:
                if not isinstance(source_row, Mapping):
                    continue
                row = dict(source_row)
                if _is_position_row(row):
                    continue
                cash_identity = _cash_row_identity(row)
                if cash_identity is None:
                    continue
                product_type = _row_product_type_hint(row)
                if product_type not in {"spot", "perp"}:
                    continue
                product_types = product_types_by_cash.setdefault(cash_identity, set())
                product_types.add(product_type)
                if len(product_types) > 1:
                    scoped_cash_conflicts.add(cash_identity)

    for rows in rows_by_strategy.values():
        for source_row in rows:
            if not isinstance(source_row, Mapping):
                continue
            row = dict(source_row)

            if _is_position_row(row):
                position_key = _position_portfolio_key(row)
                if position_key is None:
                    continue
                agg = position_grouped.get(position_key)
                if agg is None:
                    agg = _position_agg_seed(row)
                    position_grouped[position_key] = agg
                _position_agg_update(agg, row)
                continue

            cash_identity = _cash_row_identity(row)
            cash_key = _cash_row_key(
                row,
                preserve_product_scope_cash=bool(
                    preserve_product_scope_cash
                    and cash_identity is not None
                    and cash_identity in scoped_cash_conflicts
                ),
            )
            if cash_key is None:
                passthrough_rows.append(row)
                continue
            source_strategy_id = strategy_id_from_row(row, portfolio_id)
            if source_strategy_id:
                cash_source_strategy_ids.setdefault(cash_key, set()).add(source_strategy_id)

            row_ts_ms = _row_ts_ms(row)
            row_mark = safe_float(row.get("mark_raw") or row.get("mark"))
            marked_previous = cash_latest_marked.get(cash_key)
            if row_mark is not None and (marked_previous is None or row_ts_ms >= marked_previous[0]):
                cash_latest_marked[cash_key] = (row_ts_ms, dict(row))
            previous = cash_latest.get(cash_key)
            if _should_replace_cash_row(
                cash_key=cash_key,
                previous=previous,
                candidate_row=row,
                candidate_ts_ms=row_ts_ms,
            ):
                merged = dict(row)
                product_type = _row_product_type_hint(row)
                merged["strategy_id"] = portfolio_id
                merged["row_id"] = _portfolio_cash_row_id(portfolio_id, cash_key)
                merged["exchange"] = cash_key[0]
                if cash_key[1]:
                    merged["account"] = cash_key[1]
                merged["asset"] = cash_key[2]
                merged["coin"] = cash_key[2]
                merged["base"] = cash_key[2]
                if product_type in {"spot", "perp"}:
                    merged["product_type"] = product_type
                    merged["market_type"] = product_type
                cash_latest[cash_key] = (row_ts_ms, merged)

    for cash_key, latest in list(cash_latest.items()):
        latest_ts_ms, latest_row = latest
        marked_previous = cash_latest_marked.get(cash_key)
        if marked_previous is None:
            carried = dict(latest_row)
        else:
            carried = _carry_forward_cash_mark(dict(latest_row), marked_previous)
        if cash_key[1] and (
            len(cash_source_strategy_ids.get(cash_key, set())) > 1
            or cash_key[:3] in scoped_cash_conflicts
        ):
            carried["scope"] = "shared_account"
        cash_latest[cash_key] = (latest_ts_ms, carried)

    _collapse_duplicate_cash_scope_rows(cash_latest, cash_source_strategy_ids)

    merged_positions: list[dict[str, Any]] = []
    for key, agg in position_grouped.items():
        position_row = _position_portfolio_row_from_agg(key, agg, portfolio_id=portfolio_id)
        if position_row is not None:
            merged_positions.append(position_row)

    merged_cash = [item[1] for item in cash_latest.values()]
    merged_rows = [*merged_positions, *merged_cash, *passthrough_rows]
    merged_rows.sort(key=_portfolio_balance_sort_key)
    return collapse_balance_display_rows(
        [
            _annotate_shared_scope_stable_cash_row(enrich_row_with_canonical_naming(row))
            for row in merged_rows
        ],
    )


_STABLE_BALANCE_ASSETS = frozenset({"USD", "USDT", "USDC", "DAI", "FDUSD", "USDE"})
_PREFERRED_SPOT_QUOTES = ("USDT", "USDC", "USD", "FDUSD", "DAI", "USDE")


def _normalized_symbol_signature(symbol: Any) -> str:
    text = decode_text(symbol).strip().upper()
    if not text:
        return ""
    return "".join(ch for ch in text if ch.isalnum())


def _contract_market_mid(row: Mapping[str, Any]) -> float | None:
    mid = safe_float(row.get("mid"))
    if mid is not None:
        return mid
    bid = safe_float(row.get("bid"))
    ask = safe_float(row.get("ask"))
    if bid is not None and ask is not None:
        return (bid + ask) / 2.0
    return bid if bid is not None else ask


def _row_asset_hint(row: Mapping[str, Any]) -> str:
    for key in ("asset", "coin", "base"):
        asset = decode_text(row.get(key)).strip().upper()
        if asset and all(token not in asset for token in ("PERP", "LINEAR")):
            return asset
    return ""


def _row_contract_key(
    row: Mapping[str, Any],
    *,
    contracts: Sequence[ContractCatalogEntry],
    market_rows: Mapping[str, Mapping[str, Any]] | None = None,
) -> str | None:
    exchange = _row_exchange_hint(row)
    if not exchange:
        return None

    instrument_text = decode_text(row.get("instrument_id") or row.get("symbol")).strip().upper()
    instrument_signature = _normalized_symbol_signature(
        instrument_text.split(".", maxsplit=1)[0] if instrument_text else "",
    )
    asset_hint = _row_asset_hint(row)
    instrument_matches: list[ContractCatalogEntry] = []
    asset_matches: list[ContractCatalogEntry] = []

    for contract in contracts:
        contract_exchange = decode_text(contract.exchange).strip().lower()
        if contract_exchange != exchange:
            continue
        base_asset, _quote_asset = normalize_symbol_parts(symbol=contract.symbol)
        contract_id = contract_id_for_leg(
            exchange=contract.exchange,
            symbol=contract.symbol,
            instrument_id=contract.instrument_id,
        )
        if instrument_signature:
            contract_signature = _normalized_symbol_signature(
                _raw_symbol_from_instrument_id(contract.instrument_id) or contract.symbol,
            )
            if contract_signature and instrument_signature.startswith(contract_signature):
                instrument_matches.append(contract)
        if asset_hint and base_asset == asset_hint:
            asset_matches.append(contract)

    candidates = instrument_matches or asset_matches
    if not candidates:
        return None

    instrument_hint = decode_text(row.get("instrument_id") or row.get("symbol")).strip().upper()
    want_product_type = "spot"
    if _is_position_row(dict(row)) and any(token in instrument_hint for token in ("PERP", "LINEAR", "SWAP")):
        want_product_type = "perp"
    def _contract_sort_key(contract: ContractCatalogEntry) -> tuple[int, int, int, int, str]:
        naming = canonical_naming_fields(
            instrument_id=contract.instrument_id,
            exchange=contract.exchange,
            symbol=contract.symbol,
            is_position=False,
        )
        product_type = decode_text(naming.get("product_type")).strip().lower()
        contract_id = contract_id_for_leg(
            exchange=contract.exchange,
            symbol=contract.symbol,
            instrument_id=contract.instrument_id,
        )
        market_row = (market_rows or {}).get(contract_id) or {}
        market_mid = _contract_market_mid(market_row)
        market_ts_ms = _row_ts_ms(market_row)
        _base_asset, quote_asset = normalize_symbol_parts(symbol=contract.symbol)
        try:
            quote_rank = _PREFERRED_SPOT_QUOTES.index(quote_asset)
        except ValueError:
            quote_rank = len(_PREFERRED_SPOT_QUOTES)
        return (
            0 if product_type == want_product_type else 1,
            0 if market_mid is not None else 1,
            quote_rank if want_product_type == "spot" else 0,
            -market_ts_ms,
            contract_id,
        )

    first = min(candidates, key=_contract_sort_key)
    return contract_id_for_leg(
        exchange=first.exchange,
        symbol=first.symbol,
        instrument_id=first.instrument_id,
    )


def enrich_balances_rows(
    rows: Sequence[Mapping[str, Any]],
    *,
    contracts: Sequence[ContractCatalogEntry],
    market_rows: Mapping[str, Mapping[str, Any]],
) -> list[dict[str, Any]]:
    """Attach marks, market values, and canonical naming to balance rows."""

    enriched: list[dict[str, Any]] = []
    for source_row in rows:
        row = dict(source_row)
        if not _is_position_row(row) and row.get("mark_raw") is not None and row.get("mv_raw") is not None:
            enriched.append(enrich_row_with_canonical_naming(row))
            continue

        qty = safe_float(
            row.get("signed_qty")
            if _is_position_row(row)
            else row.get("total") or row.get("quantity") or row.get("signed_qty") or row.get("free"),
        )
        asset_hint = _row_asset_hint(row)
        contract_key = _row_contract_key(row, contracts=contracts, market_rows=market_rows)
        matched_contract: ContractCatalogEntry | None = None
        if contract_key:
            for contract in contracts:
                candidate_key = contract_id_for_leg(
                    exchange=contract.exchange,
                    symbol=contract.symbol,
                    instrument_id=contract.instrument_id,
                )
                if candidate_key != contract_key:
                    continue
                matched_contract = contract
                base_asset, _quote_asset = normalize_symbol_parts(symbol=contract.symbol)
                current_asset = decode_text(row.get("asset") or row.get("coin") or row.get("base")).strip().upper()
                if base_asset and (
                    current_asset in {"", "UNKNOWN"}
                    or "PERP" in current_asset
                    or "LINEAR" in current_asset
                    or current_asset == decode_text(row.get("instrument_id")).strip().upper()
                ):
                    row["asset"] = base_asset
                    row["coin"] = base_asset
                    row["base"] = base_asset
                break
        mark = safe_float(row.get("mark_raw") or row.get("mark") or row.get("avg_px_open") or row.get("price"))

        if mark is not None and mark <= 0:
            mark = None
        if mark is None and asset_hint in _STABLE_BALANCE_ASSETS:
            mark = 1.0
        if mark is None:
            market_row = market_rows.get(contract_key or "") or {}
            mark = _contract_market_mid(market_row)

        if mark is not None:
            row["mark_raw"] = mark
        if qty is not None and mark is not None:
            row["mv_raw"] = qty * mark

        naming_instrument_id: Any = None
        naming_exchange: Any = None
        naming_symbol: Any = None
        if matched_contract is not None:
            matched_product_type = canonical_naming_fields(
                instrument_id=matched_contract.instrument_id,
                exchange=matched_contract.exchange,
                symbol=matched_contract.symbol,
                is_position=False,
            ).get("product_type")
            if _is_position_row(row):
                naming_exchange = matched_contract.exchange
                naming_symbol = matched_contract.symbol
                naming_instrument_id = matched_contract.instrument_id or None
            elif matched_product_type == "spot":
                naming_exchange = matched_contract.exchange
                naming_symbol = matched_contract.symbol
                naming_instrument_id = matched_contract.instrument_id or None

        enriched.append(
            enrich_row_with_canonical_naming(
                row,
                instrument_id=naming_instrument_id,
                exchange=naming_exchange,
                symbol=naming_symbol,
                asset=row.get("asset"),
                inventory_asset=row.get("coin") or row.get("asset") or row.get("base"),
            ),
        )
    return enriched


def filter_balance_rows_for_contract_scope(
    rows: Sequence[Mapping[str, Any]],
    *,
    contracts: Sequence[ContractCatalogEntry],
) -> list[dict[str, Any]]:
    """Keep only the balance rows relevant to the contract catalog in scope."""

    allowed_assets: set[str] = set()
    allowed_assets_by_exchange: dict[str, set[str]] = {}
    allowed_contracts: set[str] = set()
    for contract in contracts:
        exchange = decode_text(contract.exchange).strip().lower()
        base_asset, quote_asset = normalize_symbol_parts(symbol=contract.symbol)
        if base_asset:
            allowed_assets.add(base_asset)
            allowed_assets_by_exchange.setdefault(exchange, set()).add(base_asset)
        if quote_asset:
            allowed_assets.add(quote_asset)
            allowed_assets_by_exchange.setdefault(exchange, set()).add(quote_asset)
        naming = canonical_naming_fields(
            instrument_id=contract.instrument_id,
            exchange=contract.exchange,
            symbol=contract.symbol,
            is_position=False,
        )
        if naming.get("product_type") == "perp" and quote_asset in (_STABLE_BALANCE_ASSETS | {"USD"}):
            # Stable-quoted perp books commonly collateralize in venue-specific stable assets, not just the literal quote.
            allowed_assets.update(_STABLE_BALANCE_ASSETS)
            allowed_assets_by_exchange.setdefault(exchange, set()).update(_STABLE_BALANCE_ASSETS)
        allowed_contracts.add(
            contract_id_for_leg(
                exchange=contract.exchange,
                symbol=contract.symbol,
                instrument_id=contract.instrument_id,
            ),
        )

    filtered: list[dict[str, Any]] = []
    for source_row in rows:
        row = dict(source_row)
        if _is_position_row(row):
            contract_key = _row_contract_key(row, contracts=contracts)
            if contract_key in allowed_contracts:
                filtered.append(row)
            continue

        asset = decode_text(row.get("asset") or row.get("coin") or row.get("base")).strip().upper()
        exchange = _row_exchange_hint(row)
        exchange_allowed_assets = allowed_assets_by_exchange.get(exchange) if exchange else None
        if exchange_allowed_assets is not None:
            if asset in exchange_allowed_assets:
                filtered.append(row)
            continue
        if asset in allowed_assets:
            filtered.append(row)
    return filtered


def _clean_float_zero(value: float) -> float:
    return 0.0 if abs(value) < 1e-12 else value


def _balance_risk_semantics(row: Mapping[str, Any]) -> tuple[str, str] | None:
    explicit_key = decode_text(row.get("risk_key")).strip().upper()
    explicit_label = decode_text(row.get("risk_label")).strip()
    if explicit_key:
        return explicit_key, (explicit_label or explicit_key)

    naming = canonical_naming_fields(
        instrument_id=row.get("instrument_id"),
        exchange=row.get("exchange"),
        venue=row.get("venue"),
        symbol=row.get("symbol"),
        asset=row.get("asset"),
        inventory_asset=row.get("inventory_asset") or row.get("coin") or row.get("asset") or row.get("base"),
        is_position=_is_position_row(dict(row)),
    )
    inventory_asset = decode_text(
        row.get("inventory_asset")
        or naming.get("inventory_asset")
        or row.get("coin")
        or row.get("asset")
        or row.get("base"),
    ).strip().upper()
    if not inventory_asset:
        inventory_asset = _row_asset_hint(row)
    if not inventory_asset:
        return None
    if inventory_asset in _STABLE_BALANCE_ASSETS:
        return "USD_CASH", "USD Cash"
    if inventory_asset.endswith("_PERP"):
        inventory_asset = inventory_asset[:-5]
    return inventory_asset, inventory_asset


def build_balance_risk_groups(
    rows: Sequence[Mapping[str, Any]],
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    annotated_rows: list[dict[str, Any]] = []
    grouped: dict[str, dict[str, Any]] = {}

    for source_row in rows:
        row = dict(source_row)
        semantics = _balance_risk_semantics(row)
        if semantics is None:
            annotated_rows.append(row)
            continue

        risk_key, risk_label = semantics
        row["risk_key"] = risk_key
        row["risk_label"] = risk_label
        annotated_rows.append(row)

        qty = (
            safe_float(row.get("signed_qty"))
            if _is_position_row(row)
            else _balance_row_qty(row)
        )
        if qty is None:
            qty = safe_float(row.get("qty")) or 0.0
        mv_raw = safe_float(row.get("mv_raw") or row.get("mv")) or 0.0
        venue = decode_text(row.get("exchange") or row.get("venue")).strip().lower()
        coin = decode_text(
            row.get("inventory_asset")
            or row.get("coin")
            or row.get("asset")
            or row.get("base")
            or risk_key,
        ).strip().upper() or risk_key
        group = grouped.setdefault(
            risk_key,
            {
                "risk_key": risk_key,
                "label": risk_label,
                "net_qty": 0.0,
                "net_mv": 0.0,
                "long_mv": 0.0,
                "short_mv": 0.0,
                "gross_mv": 0.0,
                "sources": [],
                "rows": [],
            },
        )
        group["net_qty"] += qty
        group["net_mv"] += mv_raw
        if mv_raw >= 0:
            group["long_mv"] += mv_raw
        else:
            group["short_mv"] += mv_raw
        group["gross_mv"] += abs(mv_raw)
        if venue and venue not in group["sources"]:
            group["sources"].append(venue)
        group["rows"].append(
            {
                "row_id": decode_text(row.get("row_id")).strip() or None,
                "venue": venue,
                "coin": coin,
                "qty_raw": qty,
                "mv_raw": mv_raw,
                "mark_raw": safe_float(row.get("mark_raw") or row.get("mark")),
                "time_display": decode_text(row.get("time_display")).strip() or None,
                "label": decode_text(row.get("label")).strip() or None,
                "wallet": decode_text(
                    row.get("account")
                    or row.get("account_id")
                    or row.get("wallet")
                    or row.get("subaccount"),
                ).strip()
                or None,
                "address": decode_text(row.get("address")).strip() or None,
            },
        )

    risk_groups: list[dict[str, Any]] = []
    for group in grouped.values():
        gross_mv = _clean_float_zero(float(group["gross_mv"]))
        net_mv = _clean_float_zero(float(group["net_mv"]))
        hedge_ratio = 0.0 if gross_mv <= 0 else max(0.0, min(1.0, 1.0 - (abs(net_mv) / gross_mv)))
        risk_groups.append(
            {
                **group,
                "net_qty": _clean_float_zero(float(group["net_qty"])),
                "net_mv": net_mv,
                "long_mv": _clean_float_zero(float(group["long_mv"])),
                "short_mv": _clean_float_zero(float(group["short_mv"])),
                "gross_mv": gross_mv,
                "abs_net_mv": abs(net_mv),
                "hedge_ratio": _clean_float_zero(hedge_ratio),
            },
        )

    risk_groups.sort(
        key=lambda group: (
            -abs(safe_float(group.get("gross_mv")) or 0.0),
            decode_text(group.get("risk_key")).strip(),
        ),
    )
    return annotated_rows, risk_groups


def _portfolio_balance_sort_key(row: Mapping[str, Any]) -> tuple[int, int, float, int, str]:
    is_position = 0 if _is_position_row(row) else 1
    total_value = abs(safe_float(row.get("total")) or 0.0)
    qty_value = abs(
        safe_float(row.get("signed_qty"))
        or safe_float(row.get("quantity"))
        or 0.0
    )
    is_zero = 1 if total_value == 0.0 and qty_value == 0.0 else 0
    ts_value = -_row_ts_ms(row)
    row_id = decode_text(row.get("row_id")).strip()
    return (is_position, is_zero, -(max(total_value, qty_value)), ts_value, row_id)
