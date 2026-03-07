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
        "qty": Decimal(0),
        "avg_num": Decimal(0),
        "avg_den": Decimal(0),
        "upnl": Decimal(0),
        "has_upnl": False,
    }


def _position_agg_update(agg: dict[str, Any], row: dict[str, Any]) -> None:
    qty = _position_signed_qty(row)
    if qty is not None:
        agg["qty"] += qty
        avg_px = _to_decimal(
            row.get("avg_px")
            or row.get("avg_price")
            or row.get("entry_price")
            or row.get("avg_px_open")
            or row.get("avg_px_close"),
        )
        if avg_px is not None and qty != 0:
            agg["avg_num"] += abs(qty) * avg_px
            agg["avg_den"] += abs(qty)

    upnl = _to_decimal(
        row.get("unrealized_pnl")
        or row.get("unrealizedPnl")
        or row.get("realized_pnl")
        or row.get("realizedPnl"),
    )
    if upnl is not None:
        agg["upnl"] += upnl
        agg["has_upnl"] = True


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


def _position_row_from_agg(key: tuple[str, str, str], agg: dict[str, Any]) -> dict[str, Any] | None:
    sid, exchange, instrument = key
    qty: Decimal = agg["qty"]
    if qty == 0:
        return None
    side = "LONG" if qty > 0 else "SHORT"
    avg_px = agg["avg_num"] / agg["avg_den"] if agg["avg_den"] > 0 else None
    upnl = agg["upnl"] if agg["has_upnl"] else None

    row = dict(agg["row"])
    row["strategy_id"] = sid
    if exchange:
        row["exchange"] = exchange
    row.setdefault("kind", "position")
    row["instrument_id"] = decode_text(row.get("instrument_id") or instrument).strip() or instrument
    if not decode_text(row.get("asset")).strip():
        row["asset"] = instrument
    qty_text = _decimal_text(qty)
    row["signed_qty"] = qty_text
    row["quantity"] = _decimal_text(abs(qty))
    row["free"] = qty_text
    row["total"] = qty_text

    meta_parts = [side]
    if avg_px is not None:
        meta_parts.append(f"avg={_decimal_text(avg_px)}")
    if upnl is not None:
        meta_parts.append(f"uPnL={_decimal_text(upnl)}")
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
            account_id = decode_text(event.get("account_id")).strip()
            venue = account_id.split("-", maxsplit=1)[0].lower() if account_id else ""
            for balance_index, balance in enumerate(event_balances):
                if not isinstance(balance, dict):
                    continue
                asset = decode_text(balance.get("currency")).strip().upper()
                out.append(
                    {
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
                    },
                )
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
                    )
                    if account_flattened:
                        flattened += account_flattened
                        continue
                    flattened_row = {
                        **account,
                        "strategy_id": sid,
                        "row_id": account_row_id,
                    }
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
        )

        if flattened > 0:
            continue

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


def _cash_row_key(row: Mapping[str, Any]) -> tuple[str, str, str] | None:
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


def collapse_balance_display_rows(rows: Sequence[Mapping[str, Any]]) -> list[dict[str, Any]]:
    """
    Prefer spot cash rows over duplicate spot-position rows for the same venue/base asset.

    Some venue snapshots publish spot inventory twice: once as account cash and once as a spot "position".
    Balances should render that inventory once, while Signal and other raw snapshot consumers remain untouched.
    """

    cash_keys: set[tuple[str, str]] = set()
    normalized_rows = [dict(source_row) for source_row in rows if isinstance(source_row, Mapping)]

    for row in normalized_rows:
        if _is_position_row(row):
            continue
        if _balance_product_type(row) != "spot":
            continue
        key = _balance_inventory_key(row)
        if key is not None:
            cash_keys.add(key)

    collapsed: list[dict[str, Any]] = []
    for row in normalized_rows:
        if _is_position_row(row) and _balance_product_type(row) == "spot":
            key = _balance_inventory_key(row)
            if key is not None and key in cash_keys:
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
    qty: Decimal = agg["qty"]
    if qty == 0:
        return None

    side = "LONG" if qty > 0 else "SHORT"
    avg_px = agg["avg_num"] / agg["avg_den"] if agg["avg_den"] > 0 else None
    upnl = agg["upnl"] if agg["has_upnl"] else None

    row = dict(agg["row"])
    row["strategy_id"] = portfolio_id
    if exchange:
        row["exchange"] = exchange
    row.setdefault("kind", "position")
    row["instrument_id"] = decode_text(row.get("instrument_id") or instrument).strip() or instrument
    if not decode_text(row.get("asset")).strip():
        row["asset"] = instrument
    qty_text = _decimal_text(qty)
    row["signed_qty"] = qty_text
    row["quantity"] = _decimal_text(abs(qty))
    row["free"] = qty_text
    row["total"] = qty_text

    meta_parts = [side]
    if avg_px is not None:
        meta_parts.append(f"avg={_decimal_text(avg_px)}")
    if upnl is not None:
        meta_parts.append(f"uPnL={_decimal_text(upnl)}")
    row["locked"] = " ".join(meta_parts)
    row["side"] = side
    row["row_id"] = f"{portfolio_id}:pos:{exchange}:{instrument}"
    return row


def merge_portfolio_balances_rows(
    *,
    rows_by_strategy: Mapping[str, Sequence[Mapping[str, Any]]],
    portfolio_id: str = "tokenmm",
) -> list[dict[str, Any]]:
    """Merge per-strategy balance rows into a single portfolio-level balance view."""

    cash_latest: dict[tuple[str, str, str], tuple[int, dict[str, Any]]] = {}
    cash_latest_marked: dict[tuple[str, str, str], tuple[int, dict[str, Any]]] = {}
    position_grouped: dict[tuple[str, str], dict[str, Any]] = {}
    passthrough_rows: list[dict[str, Any]] = []

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
                    agg = {
                        "row": dict(row),
                        "qty": Decimal(0),
                        "avg_num": Decimal(0),
                        "avg_den": Decimal(0),
                        "upnl": Decimal(0),
                        "has_upnl": False,
                    }
                    position_grouped[position_key] = agg
                _position_agg_update(agg, row)
                continue

            cash_key = _cash_row_key(row)
            if cash_key is None:
                passthrough_rows.append(row)
                continue

            row_ts_ms = _row_ts_ms(row)
            row_mark = safe_float(row.get("mark_raw") or row.get("mark"))
            marked_previous = cash_latest_marked.get(cash_key)
            if row_mark is not None and (marked_previous is None or row_ts_ms >= marked_previous[0]):
                cash_latest_marked[cash_key] = (row_ts_ms, dict(row))
            previous = cash_latest.get(cash_key)
            if previous is None or row_ts_ms >= previous[0]:
                merged = dict(row)
                merged["strategy_id"] = portfolio_id
                merged["row_id"] = f"{portfolio_id}:cash:{cash_key[0]}:{cash_key[1]}:{cash_key[2]}"
                merged["exchange"] = cash_key[0]
                if cash_key[1]:
                    merged["account"] = cash_key[1]
                merged["asset"] = cash_key[2]
                merged["coin"] = cash_key[2]
                merged["base"] = cash_key[2]
                cash_latest[cash_key] = (row_ts_ms, merged)

    for cash_key, latest in list(cash_latest.items()):
        latest_ts_ms, latest_row = latest
        marked_previous = cash_latest_marked.get(cash_key)
        if marked_previous is None:
            continue
        carried = _carry_forward_cash_mark(dict(latest_row), marked_previous)
        cash_latest[cash_key] = (latest_ts_ms, carried)

    merged_positions: list[dict[str, Any]] = []
    for key, agg in position_grouped.items():
        position_row = _position_portfolio_row_from_agg(key, agg, portfolio_id=portfolio_id)
        if position_row is not None:
            merged_positions.append(position_row)

    merged_cash = [item[1] for item in cash_latest.values()]
    merged_rows = [*merged_positions, *merged_cash, *passthrough_rows]
    merged_rows.sort(key=_portfolio_balance_sort_key)
    return collapse_balance_display_rows(
        [enrich_row_with_canonical_naming(row) for row in merged_rows],
    )


_STABLE_BALANCE_ASSETS = frozenset({"USD", "USDT", "USDC", "DAI", "FDUSD", "USDE"})


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
    for contract in candidates:
        naming = canonical_naming_fields(
            instrument_id=contract.instrument_id,
            exchange=contract.exchange,
            symbol=contract.symbol,
            is_position=False,
        )
        if naming.get("product_type") == want_product_type:
            return contract_id_for_leg(
                exchange=contract.exchange,
                symbol=contract.symbol,
                instrument_id=contract.instrument_id,
            )
    first = candidates[0]
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
        contract_key = _row_contract_key(row, contracts=contracts)
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
    allowed_contracts: set[str] = set()
    for contract in contracts:
        base_asset, quote_asset = normalize_symbol_parts(symbol=contract.symbol)
        if base_asset:
            allowed_assets.add(base_asset)
        if quote_asset:
            allowed_assets.add(quote_asset)
        naming = canonical_naming_fields(
            instrument_id=contract.instrument_id,
            exchange=contract.exchange,
            symbol=contract.symbol,
            is_position=False,
        )
        if naming.get("product_type") == "perp" and quote_asset == "USD":
            # Hyperliquid and similar USD-quoted perps commonly settle/collateralize in USDC.
            allowed_assets.add("USDC")
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
        if asset in allowed_assets:
            filtered.append(row)
    return filtered


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
