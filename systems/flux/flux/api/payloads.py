from __future__ import annotations

import json
import sys
import time
from collections.abc import Mapping
from collections.abc import Sequence
from dataclasses import dataclass
from datetime import datetime
from decimal import Decimal
from decimal import InvalidOperation
from typing import Any

if __name__ == "flux.api.payloads":
    sys.modules.setdefault("nautilus_trader.flux.api.payloads", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.api.payloads":
    sys.modules.setdefault("flux.api.payloads", sys.modules[__name__])


@dataclass(frozen=True, slots=True)
class ContractCatalogEntry:
    exchange: str
    symbol: str
    instrument_id: str = ""


@dataclass(frozen=True, slots=True)
class StrategyMetadata:
    strategy_class: str
    strategy_groups: str
    base_asset: str
    quote_asset: str
    param_set: str = ""
    strategy_family: str = ""
    strategy_version: str = ""

    def as_payload(self, *, strategy_id: str) -> dict[str, str]:
        payload = {
            "strategy_id": strategy_id,
            "class": self.strategy_class,
            "strategy_groups": self.strategy_groups,
            "base_asset": self.base_asset,
            "quote_asset": self.quote_asset,
        }
        if self.param_set:
            payload["param_set"] = self.param_set
        if self.strategy_family:
            payload["strategy_family"] = self.strategy_family
        if self.strategy_version:
            payload["strategy_version"] = self.strategy_version
        return payload


_KNOWN_QUOTES = (
    "USDT",
    "USDC",
    "PUSD",
    "USDE",
    "USD",
    "BTC",
    "ETH",
    "EUR",
    "GBP",
    "JPY",
    "BNB",
)
_PERP_CONTRACT_TYPES = frozenset({"perp", "linear", "swap", "inverse"})
_CONTRACT_SUFFIXES = (
    ("-LINEAR", "linear"),
    ("-SWAP", "swap"),
    ("-INVERSE", "inverse"),
    ("-PERP", "perp"),
    ("-SPOT", "spot"),
)


def contract_id_for_leg(*, exchange: Any, symbol: Any, instrument_id: Any = None) -> str:
    exchange_text = decode_text(exchange).strip().lower()
    symbol_text = (
        decode_text(instrument_id).strip().upper()
        or decode_text(symbol).strip().upper()
    )
    return f"{exchange_text}:{symbol_text}"


def now_ms() -> int:
    return int(time.time() * 1_000)


def decode_text(value: Any) -> str:
    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return str(value)


def load_json(value: Any) -> Any:
    if value is None:
        return None
    if isinstance(value, dict | list | int | float | bool):
        return value
    text = decode_text(value).strip()
    if not text:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return None


def as_list(value: Any) -> list[Any]:
    if value is None:
        return []
    if isinstance(value, list):
        return list(value)
    if isinstance(value, tuple):
        return list(value)
    return [value]


def safe_int(value: Any) -> int | None:
    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


_JS_SAFE_INTEGER_MAX = 9_007_199_254_740_991
_REDIS_STREAM_ID_SEQ_MULTIPLIER = 4096


def _stream_seq_from_entry_id(entry_id: Any) -> int | None:
    """
    Convert a Redis stream entry ID (``ms-seq``) into a monotonic integer cursor.

    TokenMM clients treat ``seq`` as an integer cursor and run in JavaScript where integers must fit within
    ``Number.MAX_SAFE_INTEGER``. We therefore pack the suffix into the low bits using a bounded multiplier.

    """
    entry_id_text = decode_text(entry_id).strip()
    if not entry_id_text:
        return None
    if "-" not in entry_id_text:
        return safe_int(entry_id_text)

    ms_text, sub_text = entry_id_text.split("-", maxsplit=1)
    ms = safe_int(ms_text)
    if ms is None:
        return None
    sub = safe_int(sub_text)
    if sub is None or sub < 0:
        return ms
    if sub >= _REDIS_STREAM_ID_SEQ_MULTIPLIER:
        # Defensive: keep cursor monotonic and JS-safe, at the expense of intra-ms uniqueness.
        return ms
    packed = (ms * _REDIS_STREAM_ID_SEQ_MULTIPLIER) + sub
    if packed > _JS_SAFE_INTEGER_MAX:
        return ms
    return packed


def safe_float(value: Any) -> float | None:
    if value is None:
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def safe_bool(value: Any) -> bool | None:
    if value is None:
        return None
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        if value in (0, 0.0):
            return False
        if value in (1, 1.0):
            return True
    text = decode_text(value).strip().lower()
    if text in {"1", "true", "t", "yes", "y", "on", "enabled"}:
        return True
    if text in {"0", "false", "f", "no", "n", "off", "disabled"}:
        return False
    return None


def coerce_ts_ms(value: Any) -> int | None:
    if value is None:
        return None

    text = decode_text(value).strip()
    try:
        ts = float(text)
    except (TypeError, ValueError):
        try:
            iso = text
            if iso.endswith("Z"):
                iso = f"{iso[:-1]}+00:00"
            ts = datetime.fromisoformat(iso).timestamp()
        except (TypeError, ValueError):
            return None

    if ts <= 0:
        return None
    if ts < 1_000_000_000_000:
        return int(ts * 1_000)
    if ts >= 1_000_000_000_000_000_000:
        return int(ts / 1_000_000)
    if ts >= 1_000_000_000_000_000:
        return int(ts / 1_000)
    return int(ts)


def normalize_symbol_parts(*, symbol: str) -> tuple[str, str]:
    text = decode_text(symbol).strip().upper()
    if not text:
        return "", ""

    if "/" in text:
        base, quote = text.split("/", maxsplit=1)
        return base, quote

    cleaned = text.replace("-", "_")
    if "_" in cleaned:
        base, quote = cleaned.split("_", maxsplit=1)
        return base, quote

    for quote in _KNOWN_QUOTES:
        if cleaned.endswith(quote) and len(cleaned) > len(quote):
            return cleaned[: -len(quote)], quote
    return cleaned, ""


def _raw_symbol_from_instrument_id(instrument_id: Any) -> str:
    text = decode_text(instrument_id).strip().upper()
    if not text:
        return ""
    symbol_text = text.split(".", maxsplit=1)[0].strip().upper()
    stripped_symbol, _contract_type = _strip_contract_suffix(symbol_text)
    return stripped_symbol or symbol_text


def _contract_type_from_instrument_id(instrument_id: Any) -> str:
    text = decode_text(instrument_id).strip().upper()
    if not text:
        return ""
    symbol_text = text.split(".", maxsplit=1)[0].strip().upper()
    _stripped_symbol, contract_type = _strip_contract_suffix(symbol_text)
    return contract_type


def _strip_contract_suffix(raw_symbol: str) -> tuple[str, str]:
    symbol_text = decode_text(raw_symbol).strip().upper()
    if not symbol_text:
        return "", ""
    for suffix, contract_type in _CONTRACT_SUFFIXES:
        if symbol_text.endswith(suffix) and len(symbol_text) > len(suffix):
            return symbol_text[: -len(suffix)], contract_type
    return symbol_text, ""


def _derive_contract_type(
    *,
    explicit_contract_type: str = "",
    raw_symbol: str,
    venue: str,
    is_position: bool,
) -> str:
    if explicit_contract_type:
        return explicit_contract_type

    stripped_symbol, explicit = _strip_contract_suffix(raw_symbol)
    _ = stripped_symbol
    if explicit:
        return explicit

    venue_text = decode_text(venue).strip().upper()
    if venue_text.endswith("_SPOT"):
        return "spot"
    if venue_text.endswith("_PERP"):
        return "perp"
    if venue_text.endswith("_SWAP"):
        return "swap"
    if venue_text.endswith("_LINEAR"):
        return "linear"
    if is_position:
        return "perp"
    if raw_symbol:
        return "spot"
    return "cash"


def _derive_pair_parts(*, instrument_id: Any, raw_symbol: str, fallback_symbol: Any) -> tuple[str, str, str]:
    raw_symbol_text = decode_text(raw_symbol).strip().upper()
    if not raw_symbol_text:
        raw_symbol_text = _raw_symbol_from_instrument_id(instrument_id)
    if not raw_symbol_text:
        raw_symbol_text = decode_text(fallback_symbol).strip().upper()

    pair_source, _explicit_contract_type = _strip_contract_suffix(raw_symbol_text)
    base_asset, quote_asset = normalize_symbol_parts(symbol=pair_source or raw_symbol_text)
    pair = f"{base_asset}/{quote_asset}" if base_asset and quote_asset else (pair_source or raw_symbol_text)
    return raw_symbol_text, base_asset, quote_asset or ""


def _derive_venue(exchange: Any, venue: Any, instrument_id: Any) -> tuple[str, str]:
    venue_text = decode_text(venue).strip().upper()
    if venue_text:
        venue_root = venue_text.split("_", maxsplit=1)[0].lower()
        return venue_text, venue_root

    exchange_text = decode_text(exchange).strip().upper()
    if exchange_text:
        venue_root = exchange_text.split("_", maxsplit=1)[0].lower()
        return exchange_text, venue_root

    instrument_text = decode_text(instrument_id).strip().upper()
    if "." in instrument_text:
        venue_text = instrument_text.split(".", maxsplit=1)[1].strip().upper()
        venue_root = venue_text.split("_", maxsplit=1)[0].lower()
        return venue_text, venue_root

    return "", ""


def canonical_naming_fields(
    *,
    instrument_id: Any = None,
    exchange: Any = None,
    venue: Any = None,
    symbol: Any = None,
    asset: Any = None,
    inventory_asset: Any = None,
    is_position: bool = False,
) -> dict[str, str]:
    instrument_text = decode_text(instrument_id).strip().upper()
    venue_text, venue_root = _derive_venue(exchange, venue, instrument_text)
    explicit_contract_type = _contract_type_from_instrument_id(instrument_text)
    raw_symbol, base_asset, quote_asset = _derive_pair_parts(
        instrument_id=instrument_text,
        raw_symbol=_raw_symbol_from_instrument_id(instrument_text),
        fallback_symbol=symbol,
    )
    inventory_asset_text = (
        decode_text(inventory_asset).strip().upper()
        or decode_text(asset).strip().upper()
        or base_asset
    )
    contract_type = _derive_contract_type(
        explicit_contract_type=explicit_contract_type,
        raw_symbol=raw_symbol,
        venue=venue_text,
        is_position=is_position,
    )
    product_type = "perp" if contract_type in _PERP_CONTRACT_TYPES else "spot"
    if contract_type == "cash" and product_type != "spot":
        product_type = "spot"
    if not base_asset:
        base_asset = inventory_asset_text or decode_text(asset).strip().upper()
    pair = f"{base_asset}/{quote_asset}" if base_asset and quote_asset else (inventory_asset_text or raw_symbol)

    display_asset = inventory_asset_text or base_asset or raw_symbol or decode_text(symbol).strip().upper()
    if product_type == "perp":
        display_name_short = f"{display_asset} Perp".strip()
    else:
        display_name_short = f"{display_asset} Spot".strip() if display_asset else "Spot"
    if venue_root:
        display_name_long = f"{venue_root.title()} {display_name_short}".strip()
    else:
        display_name_long = display_name_short

    instrument_uid_source = instrument_text or raw_symbol or inventory_asset_text
    instrument_uid = f"{venue_root}:{contract_type}:{instrument_uid_source}".strip(":")

    return {
        "instrument_uid": instrument_uid,
        "instrument_id": instrument_text,
        "venue": venue_text,
        "venue_root": venue_root,
        "product_type": product_type,
        "market_type": product_type,
        "contract_type": contract_type,
        "raw_symbol": raw_symbol,
        "base_asset": base_asset,
        "quote_asset": quote_asset,
        "pair": pair,
        "inventory_asset": inventory_asset_text or base_asset,
        "display_name_short": display_name_short,
        "display_name_long": display_name_long,
    }


def enrich_row_with_canonical_naming(
    row: Mapping[str, Any],
    *,
    instrument_id: Any = None,
    exchange: Any = None,
    venue: Any = None,
    symbol: Any = None,
    asset: Any = None,
    inventory_asset: Any = None,
    is_position: bool | None = None,
) -> dict[str, Any]:
    out = dict(row)
    position = _is_position_row(out) if is_position is None else bool(is_position)
    naming = canonical_naming_fields(
        instrument_id=instrument_id if instrument_id is not None else out.get("instrument_id"),
        exchange=exchange if exchange is not None else out.get("exchange"),
        venue=venue if venue is not None else out.get("venue"),
        symbol=symbol if symbol is not None else out.get("symbol"),
        asset=asset if asset is not None else out.get("asset"),
        inventory_asset=inventory_asset if inventory_asset is not None else (
            out.get("inventory_asset")
            or out.get("coin")
            or out.get("base")
            or out.get("asset")
        ),
        is_position=position,
    )
    out.update(naming)
    if not decode_text(out.get("coin")).strip() and naming["inventory_asset"]:
        out["coin"] = naming["inventory_asset"]
    if not decode_text(out.get("asset")).strip() and naming["inventory_asset"]:
        out["asset"] = naming["inventory_asset"]
    if not decode_text(out.get("exchange")).strip():
        if naming["venue"]:
            out["exchange"] = naming["venue"].lower()
        elif naming["venue_root"]:
            out["exchange"] = naming["venue_root"]
    return out


def strategy_id_from_row(row: Any, fallback: str) -> str:
    if not isinstance(row, dict):
        return fallback
    strategy_id = decode_text(row.get("strategy_id")).strip()
    return strategy_id or fallback


def _with_stream_meta(
    row: dict[str, Any],
    *,
    entry_id: str,
    stream_seq: int | None,
) -> dict[str, Any]:
    out = dict(row)
    if entry_id and not decode_text(out.get("entry_id")).strip():
        out["entry_id"] = entry_id
    if (
        stream_seq is not None
        and safe_int(out.get("seq")) is None
        and safe_int(out.get("_stream_seq")) is None
    ):
        out["_stream_seq"] = stream_seq
    return out


def _flat_row_from_stream_fields(fields: Mapping[Any, Any]) -> dict[str, Any]:
    flat_row: dict[str, Any] = {}
    for raw_key, raw_value in fields.items():
        key = decode_text(raw_key).strip()
        if not key or key == "payload":
            continue
        parsed_value = load_json(raw_value)
        if parsed_value is None:
            text_value = decode_text(raw_value)
            if text_value == "":
                continue
            flat_row[key] = text_value
        else:
            flat_row[key] = parsed_value
    return flat_row


def _stream_rows_from_fields(
    fields: Mapping[Any, Any],
    *,
    entry_id: str,
    stream_seq: int | None,
) -> list[dict[str, Any]]:
    payload = fields.get("payload") or fields.get(b"payload")
    parsed = load_json(payload)
    if isinstance(parsed, dict):
        return [_with_stream_meta(parsed, entry_id=entry_id, stream_seq=stream_seq)]
    if isinstance(parsed, list):
        return [
            _with_stream_meta(item, entry_id=entry_id, stream_seq=stream_seq)
            for item in parsed
            if isinstance(item, dict)
        ]
    flat_row = _flat_row_from_stream_fields(fields)
    if not flat_row:
        return []
    return [_with_stream_meta(flat_row, entry_id=entry_id, stream_seq=stream_seq)]


def extract_stream_rows(stream_entries: Any) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for entry in as_list(stream_entries):
        if not isinstance(entry, (list, tuple)) or len(entry) != 2:
            continue
        stream_id, fields = entry
        if not isinstance(fields, Mapping):
            continue
        entry_id = decode_text(stream_id).strip()
        if not entry_id:
            continue
        stream_seq = _stream_seq_from_entry_id(entry_id)
        rows.extend(
            _stream_rows_from_fields(fields, entry_id=entry_id, stream_seq=stream_seq),
        )
    return rows


def select_latest_strategy_row(rows: Sequence[dict[str, Any]], strategy_id: str) -> dict[str, Any]:
    for row in rows:
        if strategy_id_from_row(row, strategy_id) == strategy_id:
            return dict(row)
    return dict(rows[0]) if rows else {}


def _to_decimal(value: Any) -> Decimal | None:
    if value is None:
        return None
    try:
        return Decimal(str(value))
    except (InvalidOperation, ValueError, TypeError):
        text = decode_text(value).strip()
        if not text:
            return None
        try:
            return Decimal(text)
        except (InvalidOperation, ValueError):
            return None


def _decimal_text(value: Decimal) -> str:
    text = format(value, "f")
    if "." in text:
        text = text.rstrip("0").rstrip(".")
    if text == "-0":
        return "0"
    return text or "0"


def _is_position_row(row: Mapping[str, Any]) -> bool:
    kind = decode_text(row.get("kind")).strip().lower()
    if kind == "position":
        return True
    asset = decode_text(row.get("asset") or row.get("coin") or row.get("base")).strip().upper()
    instrument_text = decode_text(row.get("instrument_id") or row.get("symbol")).strip().upper()
    return "PERP" in asset or "PERP" in instrument_text or "LINEAR" in instrument_text


def _position_signed_qty(row: dict[str, Any]) -> Decimal | None:
    source_key = ""
    value: Any = None
    for key in ("signed_qty", "total", "free", "quantity", "qty", "size"):
        candidate = row.get(key)
        if candidate is None:
            continue
        value = candidate
        source_key = key
        break
    qty = _to_decimal(value)
    if qty is None:
        return None
    if source_key != "signed_qty":
        side = decode_text(row.get("side") or row.get("position_side")).strip().upper()
        if (side == "SHORT" and qty > 0) or (side == "LONG" and qty < 0):
            qty = -qty
    return qty


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


def build_balances_rows(*, raw_snapshot: Any, strategy_id: str) -> list[dict[str, Any]]:  # noqa: C901
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
    return [enrich_row_with_canonical_naming(row) for row in merged_rows]


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


def _row_exchange_hint(row: Mapping[str, Any]) -> str:
    exchange = decode_text(row.get("exchange") or row.get("venue")).strip().lower()
    if exchange:
        return exchange
    instrument_id = decode_text(row.get("instrument_id") or row.get("symbol")).strip().upper()
    if "." not in instrument_id:
        return ""
    suffix = instrument_id.split(".", maxsplit=1)[1]
    return suffix.lower()


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
    allowed_assets: set[str] = set()
    allowed_contracts: set[str] = set()
    for contract in contracts:
        base_asset, quote_asset = normalize_symbol_parts(symbol=contract.symbol)
        if base_asset:
            allowed_assets.add(base_asset)
        if quote_asset:
            allowed_assets.add(quote_asset)
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


def build_legs_payload(
    *,
    contracts: Sequence[ContractCatalogEntry],
    market_rows: Mapping[str, dict[str, Any]],
    now_ms_value: int | None = None,
) -> dict[str, Any]:
    current_ts_ms = now_ms() if now_ms_value is None else int(now_ms_value)
    out: dict[str, Any] = {}
    for contract in contracts:
        contract_id = contract_id_for_leg(
            exchange=contract.exchange,
            symbol=contract.symbol,
            instrument_id=contract.instrument_id,
        )
        row = market_rows.get(contract_id) or {}
        bid = safe_float(row.get("bid"))
        ask = safe_float(row.get("ask"))
        ts_ms = coerce_ts_ms(row.get("ts_ms") or row.get("ts") or row.get("timestamp"))
        age_ms = (current_ts_ms - ts_ms) if ts_ms else None
        leg = {
            "contract_id": contract_id,
            "exchange": contract.exchange,
            "symbol": contract.symbol,
            "bid": bid,
            "ask": ask,
            "mid": (bid + ask) / 2.0 if bid is not None and ask is not None else None,
            "ts_ms": ts_ms,
            "age_ms": age_ms,
            "state": row.get("state") or "",
        }
        leg.update(
            canonical_naming_fields(
                instrument_id=row.get("instrument_id") or contract.instrument_id,
                exchange=contract.exchange,
                symbol=contract.symbol,
                asset=None,
                inventory_asset=None,
                is_position=False,
            ),
        )
        leg.setdefault("coin", leg.get("base_asset") or leg.get("inventory_asset") or "")
        out[contract_id] = leg
    return out


def _normalize_best_case(value: Any) -> str | None:
    text = decode_text(value).strip().lower()
    return text if text in {"case1", "case2"} else None


def _first_valid_float(*values: Any) -> float | None:
    for value in values:
        parsed = safe_float(value)
        if parsed is not None:
            return parsed
    return None


def _resolve_role_leg_id(
    *,
    role_value: Any,
    legs_order: list[str],
    legs: Mapping[str, Any],
) -> str | None:
    text = decode_text(role_value).strip()
    if not text:
        return None
    if text in legs:
        return text

    raw_instrument_id = text.split(":", maxsplit=1)[1] if ":" in text else text
    raw_instrument_id = raw_instrument_id.strip().upper()
    if raw_instrument_id:
        for leg_id, leg in legs.items():
            if not isinstance(leg, Mapping):
                continue
            leg_instrument_id = decode_text(leg.get("instrument_id")).strip().upper()
            if leg_instrument_id and leg_instrument_id == raw_instrument_id:
                return leg_id

    upper = text.upper()
    if upper == "A" and legs_order:
        return legs_order[0]
    if upper == "B" and len(legs_order) > 1:
        return legs_order[1]
    return None


def _role_map_for_signal(
    *,
    state: Mapping[str, Any],
    legs_order: list[str],
    legs: Mapping[str, Any],
) -> dict[str, str]:
    out: dict[str, str] = {}
    raw_role_map = state.get("maker_role_map")
    if isinstance(raw_role_map, Mapping):
        maker_leg = _resolve_role_leg_id(
            role_value=raw_role_map.get("maker_leg"),
            legs_order=legs_order,
            legs=legs,
        )
        ref_leg = _resolve_role_leg_id(
            role_value=raw_role_map.get("ref_leg"),
            legs_order=legs_order,
            legs=legs,
        )
        hedge_leg = _resolve_role_leg_id(
            role_value=raw_role_map.get("hedge_leg"),
            legs_order=legs_order,
            legs=legs,
        )
        if maker_leg:
            out["maker_leg"] = maker_leg
        if ref_leg:
            out["ref_leg"] = ref_leg
        if hedge_leg:
            out["hedge_leg"] = hedge_leg

    if "maker_leg" not in out and legs_order:
        out["maker_leg"] = legs_order[0]
    if "ref_leg" not in out and len(legs_order) > 1:
        out["ref_leg"] = legs_order[1]
    return out


def _spread_bps(*, buy_ask: float | None, sell_bid: float | None) -> float | None:
    if buy_ask is None or sell_bid is None or buy_ask <= 0:
        return None
    return ((sell_bid - buy_ask) / buy_ask) * 10_000.0


def _required_edges_by_case(params: Mapping[str, Any]) -> tuple[float | None, float | None]:
    pool_edge = _first_valid_float(params.get("pool_edge"), params.get("place_edge1"), 0.0)
    bid_edge = _first_valid_float(
        params.get("cex_bid_edge"),
        params.get("bid_edge1"),
        params.get("bid_edge"),
    )
    ask_edge = _first_valid_float(
        params.get("cex_ask_edge"),
        params.get("ask_edge1"),
        params.get("ask_edge"),
    )
    if bid_edge is None and ask_edge is None:
        return None, None

    if bid_edge is None:
        bid_edge = ask_edge
    if ask_edge is None:
        ask_edge = bid_edge
    if bid_edge is None or ask_edge is None:
        return None, None

    pool = pool_edge if pool_edge is not None else 0.0
    return ask_edge + pool, bid_edge + pool


def _risk_delta_from_balances(balances: Sequence[dict[str, Any]]) -> float | None:
    total = Decimal(0)
    found = False
    for row in balances:
        if not isinstance(row, dict):
            continue
        if not _is_position_row(row):
            continue
        qty = _position_signed_qty(row)
        if qty is None:
            continue
        total += qty
        found = True
    return float(total) if found else None


def _state_pricing_debug(state: Mapping[str, Any]) -> tuple[dict[str, Any], dict[str, Any]]:
    pricing_debug = state.get("pricing_debug")
    if not isinstance(pricing_debug, Mapping):
        return {}, {}
    pricing = pricing_debug.get("pricing")
    skew = pricing_debug.get("skew")
    return (
        dict(pricing) if isinstance(pricing, Mapping) else {},
        dict(skew) if isinstance(skew, Mapping) else {},
    )


def _build_fallback_inventory_skew_adjustments(
    *,
    state: Mapping[str, Any],
    params: Mapping[str, Any],
    ts_ms: int | None,
    risk_delta: float | None,
) -> list[dict[str, Any]]:
    pricing, skew = _state_pricing_debug(state)
    total_skew_bps = _first_valid_float(
        skew.get("total_skew_bps"),
        pricing.get("effective_skew_bps"),
    )
    if total_skew_bps is None and not skew:
        inventory_qty = risk_delta
        if inventory_qty is None:
            return []
        des_qty_global = _first_valid_float(params.get("des_qty_global"), 0.0) or 0.0
        max_qty_global = _first_valid_float(params.get("max_qty_global"), 0.0) or 0.0
        max_skew_global = _first_valid_float(params.get("max_skew_bps_global"), 0.0) or 0.0
        des_qty_local = _first_valid_float(params.get("des_qty_local"), 0.0) or 0.0
        max_qty_local = _first_valid_float(params.get("max_qty_local"), 0.0) or 0.0
        max_skew_local = _first_valid_float(params.get("max_skew_bps_local"), 0.0) or 0.0
        linear_offset = _first_valid_float(params.get("linear_offset_bps"), 0.0) or 0.0

        global_ratio = None
        global_skew = None
        if max_qty_global > 0:
            global_ratio = max(-1.0, min(1.0, (inventory_qty - des_qty_global) / max_qty_global))
            global_skew = global_ratio * max(0.0, max_skew_global)
        local_ratio = None
        local_skew = None
        if max_qty_local > 0:
            local_ratio = max(-1.0, min(1.0, (inventory_qty - des_qty_local) / max_qty_local))
            local_skew = local_ratio * max(0.0, max_skew_local)
        total_skew = linear_offset
        if global_skew is not None:
            total_skew += global_skew
        if local_skew is not None:
            total_skew += local_skew

        skew = {
            "inventory_qty": inventory_qty,
            "des_qty_global": des_qty_global,
            "max_qty_global": max_qty_global,
            "max_skew_bps_global": max_skew_global,
            "des_qty_local": des_qty_local,
            "max_qty_local": max_qty_local,
            "max_skew_bps_local": max_skew_local,
            "global_ratio": global_ratio,
            "global_skew_bps": global_skew,
            "local_ratio": local_ratio,
            "local_skew_bps": local_skew,
            "total_skew_bps": total_skew,
        }
        total_skew_bps = total_skew

    base_bid = _first_valid_float(pricing.get("bid_edge1_cfg_bps"), params.get("bid_edge1"))
    base_ask = _first_valid_float(pricing.get("ask_edge1_cfg_bps"), params.get("ask_edge1"))
    eff_bid = _first_valid_float(pricing.get("bid_edge1_eff_bps"))
    eff_ask = _first_valid_float(pricing.get("ask_edge1_eff_bps"))

    adjustment: dict[str, Any] = {
        "type": "inventory_skew",
    }
    if total_skew_bps is not None:
        adjustment["skew_bps_signed"] = total_skew_bps
        adjustment["inv_skew"] = total_skew_bps

    global_ratio = _first_valid_float(skew.get("global_ratio"))
    local_ratio = _first_valid_float(skew.get("local_ratio"))
    if global_ratio is not None:
        adjustment["inv_ratio_global"] = global_ratio
    if local_ratio is not None:
        adjustment["inv_ratio_local"] = local_ratio
    if global_ratio is not None and local_ratio is not None:
        adjustment["inv_ratio"] = max(-1.0, min(1.0, global_ratio + local_ratio))
    elif global_ratio is not None:
        adjustment["inv_ratio"] = global_ratio
    elif local_ratio is not None:
        adjustment["inv_ratio"] = local_ratio

    global_skew = _first_valid_float(skew.get("global_skew_bps"))
    local_skew = _first_valid_float(skew.get("local_skew_bps"))
    if global_skew is not None:
        adjustment["inv_skew_global"] = global_skew
    if local_skew is not None:
        adjustment["inv_skew_local"] = local_skew

    curr_qty = _first_valid_float(skew.get("inventory_qty"))
    if curr_qty is not None:
        adjustment["curr_qty"] = curr_qty
    local_qty = _first_valid_float(
        skew.get("local_inventory_qty"),
        skew.get("local_qty"),
    )
    if local_qty is not None:
        adjustment["local_qty"] = local_qty

    des_qty = _first_valid_float(
        skew.get("des_qty_global"),
        params.get("des_qty_global"),
        params.get("des_qty"),
    )
    max_qty = _first_valid_float(
        skew.get("max_qty_global"),
        params.get("max_qty_global"),
        params.get("max_qty"),
    )
    max_skew = _first_valid_float(
        skew.get("max_skew_bps_global"),
        params.get("max_skew_bps_global"),
        params.get("max_skew_bps"),
    )
    if des_qty is not None:
        adjustment["des_qty"] = des_qty
    if max_qty is not None:
        adjustment["max_qty"] = max_qty
    if max_skew is not None:
        adjustment["max_skew_bps"] = max_skew

    if base_bid is not None:
        adjustment["base_bid_edge_bps"] = base_bid
    if base_ask is not None:
        adjustment["base_ask_edge_bps"] = base_ask
    if eff_bid is not None:
        adjustment["eff_bid_edge_bps"] = eff_bid
    if eff_ask is not None:
        adjustment["eff_ask_edge_bps"] = eff_ask
    if base_bid is not None and eff_bid is not None:
        adjustment["delta_bid_edge_bps"] = eff_bid - base_bid
    if base_ask is not None and eff_ask is not None:
        adjustment["delta_ask_edge_bps"] = eff_ask - base_ask

    if ts_ms is not None:
        adjustment["updated_ts_ms"] = ts_ms

    return [adjustment]


def _merge_inventory_skew_adjustments(
    *,
    current: list[dict[str, Any]],
    fallback: list[dict[str, Any]],
) -> list[dict[str, Any]]:
    if not current or not fallback:
        return current

    fallback_inventory = next(
        (dict(item) for item in fallback if item.get("type") == "inventory_skew"),
        None,
    )
    if fallback_inventory is None:
        return current

    merged: list[dict[str, Any]] = []
    merged_inventory = False
    for item in current:
        if item.get("type") != "inventory_skew" or merged_inventory:
            merged.append(item)
            continue
        merged_item = dict(fallback_inventory)
        for key, value in item.items():
            if value is not None:
                merged_item[key] = value
        merged.append(merged_item)
        merged_inventory = True

    if not merged_inventory:
        merged.append(fallback_inventory)
    return merged


def _derive_pricing_adjustments(  # noqa: C901
    *,
    state: Mapping[str, Any],
    params: Mapping[str, Any],
    ts_ms: int | None,
    risk_delta: float | None,
) -> list[dict[str, Any]]:
    state_adjustments = state.get("pricing_adjustments")
    if isinstance(state_adjustments, list):
        normalized = [dict(item) for item in state_adjustments if isinstance(item, Mapping)]
        fallback = _build_fallback_inventory_skew_adjustments(
            state=state,
            params=params,
            ts_ms=ts_ms,
            risk_delta=risk_delta,
        )
        return _merge_inventory_skew_adjustments(current=normalized, fallback=fallback)

    return _build_fallback_inventory_skew_adjustments(
        state=state,
        params=params,
        ts_ms=ts_ms,
        risk_delta=risk_delta,
    )


def _derive_strategy_family(strategy_class: str) -> str:
    normalized = strategy_class.strip().lower()
    if "maker_v3" in normalized:
        return "maker_v3"
    if "maker_v2" in normalized or normalized.startswith("maker"):
        return "maker_v2"
    return "taker"


def _derive_quote_snapshot(  # noqa: C901
    *,
    state: Mapping[str, Any],
    params: Mapping[str, Any],
    bot_on: bool,
    ts_ms: int | None,
    maker_leg: Mapping[str, Any] | None,
    ref_leg: Mapping[str, Any] | None,
) -> dict[str, Any]:
    state_maker_v3 = state.get("maker_v3")
    quote_snapshot = {}
    if isinstance(state_maker_v3, Mapping):
        raw_quote_snapshot = state_maker_v3.get("quote_snapshot")
        if isinstance(raw_quote_snapshot, Mapping):
            quote_snapshot = dict(raw_quote_snapshot)

    mode = "ON" if bot_on else "OFF"
    reason = decode_text(state.get("state"))
    quote_snapshot["mode"] = decode_text(quote_snapshot.get("mode")).strip() or mode
    quote_snapshot["reason"] = decode_text(quote_snapshot.get("reason")).strip() or reason
    quote_snapshot["ts_ms"] = coerce_ts_ms(quote_snapshot.get("ts_ms") or ts_ms)

    pricing, _ = _state_pricing_debug(state)
    maker_bid = _first_valid_float(
        pricing.get("maker_top_bid"),
        maker_leg.get("bid") if isinstance(maker_leg, Mapping) else None,
    )
    maker_ask = _first_valid_float(
        pricing.get("maker_top_ask"),
        maker_leg.get("ask") if isinstance(maker_leg, Mapping) else None,
    )
    ref_bid = _first_valid_float(
        pricing.get("ref_bid"),
        ref_leg.get("bid") if isinstance(ref_leg, Mapping) else None,
    )
    ref_ask = _first_valid_float(
        pricing.get("ref_ask"),
        ref_leg.get("ask") if isinstance(ref_leg, Mapping) else None,
    )
    place_bid = _first_valid_float(pricing.get("place_bid"), maker_bid)
    place_ask = _first_valid_float(pricing.get("place_ask"), maker_ask)
    cancel_bid = _first_valid_float(pricing.get("cancel_bid"))
    cancel_ask = _first_valid_float(pricing.get("cancel_ask"))
    eff_bid = _first_valid_float(pricing.get("bid_edge1_eff_bps"))
    eff_ask = _first_valid_float(pricing.get("ask_edge1_eff_bps"))
    place_edge = _first_valid_float(pricing.get("place_edge_bps"), params.get("place_edge1"))

    if isinstance(maker_leg, Mapping):
        quote_snapshot["maker_exchange"] = (
            decode_text(maker_leg.get("exchange")).strip().lower() or None
        )
        quote_snapshot["maker_symbol"] = decode_text(maker_leg.get("symbol")).strip() or None
        quote_snapshot["maker_top_ts_ms"] = coerce_ts_ms(maker_leg.get("ts_ms"))
    if isinstance(ref_leg, Mapping):
        quote_snapshot["ref_exchange"] = (
            decode_text(ref_leg.get("exchange")).strip().lower() or None
        )
        quote_snapshot["ref_symbol"] = decode_text(ref_leg.get("symbol")).strip() or None
        quote_snapshot["ref_ts_ms"] = coerce_ts_ms(ref_leg.get("ts_ms"))
    quote_snapshot["ref_source"] = (
        decode_text(pricing.get("anchor_source")).strip() or "reference_leg"
    )

    if maker_bid is not None:
        quote_snapshot["maker_top_bid"] = maker_bid
    if maker_ask is not None:
        quote_snapshot["maker_top_ask"] = maker_ask
    if ref_bid is not None:
        quote_snapshot["ref_bid"] = ref_bid
    if ref_ask is not None:
        quote_snapshot["ref_ask"] = ref_ask
    if place_bid is not None:
        quote_snapshot["place_bid"] = place_bid
    if place_ask is not None:
        quote_snapshot["place_ask"] = place_ask
    if cancel_bid is not None:
        quote_snapshot["cancel_bid"] = cancel_bid
    if cancel_ask is not None:
        quote_snapshot["cancel_ask"] = cancel_ask
    if eff_bid is not None:
        quote_snapshot["eff_bid_edge_bps"] = eff_bid
    if eff_ask is not None:
        quote_snapshot["eff_ask_edge_bps"] = eff_ask
    if place_edge is not None:
        quote_snapshot["place_edge_bps"] = place_edge

    return quote_snapshot


def _derive_maker_quote_status_from_managed_orders(
    *,
    managed_orders: int,
    params: Mapping[str, Any],
) -> dict[str, int] | None:
    managed = max(0, int(managed_orders))
    if managed <= 0:
        return None

    target_depth = (
        max(0, safe_int(params.get("n_orders1")) or 0)
        + max(0, safe_int(params.get("n_orders2")) or 0)
        + max(0, safe_int(params.get("n_orders3")) or 0)
    )
    if target_depth <= 0:
        bid_depth = (managed + 1) // 2
        ask_depth = managed // 2
    else:
        bid_depth = target_depth
        ask_depth = target_depth

    maker_capacity = max(0, bid_depth + ask_depth)
    maker_open_total = managed if maker_capacity <= 0 else min(managed, maker_capacity)
    bid_open = (
        min(bid_depth, (maker_open_total + 1) // 2)
        if bid_depth > 0
        else (maker_open_total + 1) // 2
    )
    ask_open = min(ask_depth, maker_open_total // 2) if ask_depth > 0 else maker_open_total // 2

    return {
        "bid_open": int(max(0, bid_open)),
        "ask_open": int(max(0, ask_open)),
        "bid_depth": int(max(0, bid_depth)),
        "ask_depth": int(max(0, ask_depth)),
        "bid_blocked": int(max(0, bid_depth - bid_open)),
        "ask_blocked": int(max(0, ask_depth - ask_open)),
    }


def build_signals_payload(  # noqa: C901
    *,
    strategy_id: str,
    metadata: StrategyMetadata,
    state: dict[str, Any],
    fv_row: dict[str, Any],
    params: dict[str, Any],
    balances: list[dict[str, Any]],
    legs: dict[str, Any],
) -> dict[str, Any]:
    parsed_bot_on = safe_bool(state.get("bot_on"))
    bot_on = bool(parsed_bot_on if parsed_bot_on is not None else state.get("bot_on", False))
    managed = safe_int(state.get("managed_orders")) or 0
    ts_ms = coerce_ts_ms(state.get("ts_ms") or state.get("ts_event"))
    legs_order = list(legs.keys())
    role_map = _role_map_for_signal(state=state, legs_order=legs_order, legs=legs)

    maker_leg = legs.get(role_map.get("maker_leg") or "") if role_map.get("maker_leg") else None
    ref_leg = legs.get(role_map.get("ref_leg") or "") if role_map.get("ref_leg") else None
    if not isinstance(maker_leg, Mapping):
        maker_leg = None
    if not isinstance(ref_leg, Mapping):
        ref_leg = None

    leg_a = (
        maker_leg if maker_leg is not None else (legs.get(legs_order[0]) if legs_order else None)
    )
    leg_b = (
        ref_leg
        if ref_leg is not None
        else (legs.get(legs_order[1]) if len(legs_order) > 1 else None)
    )
    if not isinstance(leg_a, Mapping):
        leg_a = None
    if not isinstance(leg_b, Mapping):
        leg_b = None

    spread_case1_derived = _spread_bps(
        buy_ask=_first_valid_float(leg_a.get("ask") if leg_a is not None else None),
        sell_bid=_first_valid_float(leg_b.get("bid") if leg_b is not None else None),
    )
    spread_case2_derived = _spread_bps(
        buy_ask=_first_valid_float(leg_b.get("ask") if leg_b is not None else None),
        sell_bid=_first_valid_float(leg_a.get("bid") if leg_a is not None else None),
    )

    spread_case1 = _first_valid_float(
        state.get("spread_net_case1_bps"),
        fv_row.get("spread_net_case1_bps"),
        spread_case1_derived,
    )
    spread_case2 = _first_valid_float(
        state.get("spread_net_case2_bps"),
        fv_row.get("spread_net_case2_bps"),
        spread_case2_derived,
    )

    best_case = _normalize_best_case(
        state.get("spread_net_best_case") or fv_row.get("spread_net_best_case"),
    )
    if best_case is None:
        if spread_case1 is not None and spread_case2 is not None:
            best_case = "case1" if spread_case1 >= spread_case2 else "case2"
        elif spread_case1 is not None:
            best_case = "case1"
        elif spread_case2 is not None:
            best_case = "case2"

    spread_net_bps = _first_valid_float(state.get("spread_net_bps"), fv_row.get("spread_net_bps"))
    if spread_net_bps is None:
        if best_case == "case1":
            spread_net_bps = spread_case1
        elif best_case == "case2":
            spread_net_bps = spread_case2

    decision_edge_bps = _first_valid_float(
        state.get("decision_edge_bps"),
        fv_row.get("decision_edge_bps"),
        spread_net_bps,
    )

    required_case1, required_case2 = _required_edges_by_case(params)
    required_edge_bps = _first_valid_float(
        state.get("required_edge_bps"),
        fv_row.get("required_edge_bps"),
    )
    if required_edge_bps is None:
        if best_case == "case1":
            required_edge_bps = required_case1
        elif best_case == "case2":
            required_edge_bps = required_case2

    edge2_bps = _first_valid_float(state.get("edge2_bps"), fv_row.get("edge2_bps"))
    if edge2_bps is None and decision_edge_bps is not None and required_edge_bps is not None:
        edge2_bps = decision_edge_bps - required_edge_bps

    risk_delta = _first_valid_float(state.get("risk_delta"), fv_row.get("risk_delta"))
    if risk_delta is None:
        risk_delta = _risk_delta_from_balances(balances)
    risk_delta_ts_ms = coerce_ts_ms(
        state.get("risk_delta_ts_ms") or fv_row.get("risk_delta_ts_ms") or ts_ms,
    )

    pricing_adjustments = _derive_pricing_adjustments(
        state=state,
        params=params,
        ts_ms=ts_ms,
        risk_delta=risk_delta,
    )
    quote_snapshot = _derive_quote_snapshot(
        state=state,
        params=params,
        bot_on=bot_on,
        ts_ms=ts_ms,
        maker_leg=maker_leg,
        ref_leg=ref_leg,
    )

    state_quote_status = state.get("maker_quote_status")
    maker_quote_status = (
        dict(state_quote_status) if isinstance(state_quote_status, Mapping) else None
    )
    state_quote_stacks = state.get("quote_stacks")
    quote_stacks = dict(state_quote_stacks) if isinstance(state_quote_stacks, Mapping) else None
    state_balance_readiness = state.get("balance_readiness")
    balance_readiness = (
        dict(state_balance_readiness) if isinstance(state_balance_readiness, Mapping) else None
    )

    tradeable = bot_on
    near_tradeable = False
    blocked = not bot_on

    md_health: dict[str, Any] = {
        "legs_count": len(legs),
        "stale_legs": sorted(
            contract_id for contract_id, row in legs.items() if safe_int(row.get("age_ms")) is None
        ),
    }
    if ts_ms is not None:
        state_age_ms = max(0, now_ms() - ts_ms)
        md_health["strategy_state_age_ms"] = state_age_ms
        live_legs = any(safe_int(row.get("age_ms")) is not None for row in legs.values())
        state_stale = state_age_ms >= 30_000 and not live_legs
        if state_stale:
            managed = 0
            tradeable = False
            blocked = True
            maker_quote_status = {
                "bid_open": 0,
                "ask_open": 0,
                "bid_depth": 0,
                "ask_depth": 0,
                "bid_blocked": 0,
                "ask_blocked": 0,
            }
        md_health["state_stale"] = state_stale

    return {
        "id": strategy_id,
        "meta": metadata.as_payload(strategy_id=strategy_id),
        "strategy_family": metadata.strategy_family or _derive_strategy_family(metadata.strategy_class),
        "tradeable": tradeable,
        "blocked": blocked,
        "near_tradeable": near_tradeable,
        "managed_orders": managed,
        "params": dict(params),
        "balances_ok": bool(balances),
        "risk_delta": risk_delta,
        "risk_delta_ts_ms": risk_delta_ts_ms,
        "decision_edge_bps": decision_edge_bps,
        "edge2_bps": edge2_bps,
        "required_edge_bps": required_edge_bps,
        "spread_net_bps": spread_net_bps,
        "spread_net_case1_bps": spread_case1,
        "spread_net_case2_bps": spread_case2,
        "spread_net_best_case": best_case,
        "maker_role_map": role_map,
        "maker_quote_status": maker_quote_status,
        "quote_stacks": quote_stacks,
        "pricing_adjustments": pricing_adjustments,
        "balance_readiness": balance_readiness,
        "maker_v3": {
            "quote_snapshot": quote_snapshot,
        },
        "state": state,
        "legs": legs,
        "legs_order": legs_order,
        "fv_row": fv_row,
        "balances_count": len(balances),
        "debug": {
            "md_health": md_health,
            "params_loaded": bool(params),
        },
    }


def build_params_payload(
    *,
    strategy_id: str,
    params: dict[str, Any],
    schema: Mapping[str, Mapping[str, Any]],
    running: bool | None = None,
) -> dict[str, Any]:
    payload: dict[str, Any] = {
        "strategy_id": strategy_id,
        "params": params,
        "schema": {str(name): dict(spec) for name, spec in schema.items()},
    }
    payload["running"] = running
    return payload


def build_trades_rows(  # noqa: C901
    *,
    rows: Sequence[dict[str, Any]],
    strategy_id: str,
    limit: int,
    since_ms: int | None,
    since_seq: int | None = None,
) -> list[dict[str, Any]]:
    filtered: list[dict[str, Any]] = []
    for index, row in enumerate(rows):
        if strategy_id_from_row(row, strategy_id) != strategy_id:
            continue
        out = dict(row)
        seq = safe_int(out.get("seq"))
        if seq is None:
            seq = safe_int(out.get("_stream_seq"))
        if seq is None:
            entry_id_text = decode_text(out.get("entry_id")).strip()
            if entry_id_text:
                seq = _stream_seq_from_entry_id(entry_id_text)
        if seq is not None and seq <= 0:
            seq = None
        if seq is None:
            seq = coerce_ts_ms(out.get("ts_ms") or out.get("ts") or out.get("timestamp"))
        if seq is not None:
            out["seq"] = seq
        out.pop("_stream_seq", None)
        if since_seq is not None and (seq is None or seq <= since_seq):
            continue
        ts_ms = coerce_ts_ms(out.get("ts_ms") or out.get("ts") or out.get("timestamp"))
        if since_ms is not None and ts_ms is not None and ts_ms <= since_ms:
            continue
        out["ts_ms"] = int(ts_ms if ts_ms is not None else 0)

        version = safe_int(out.get("version"))
        out["version"] = int(version if version is not None and version > 0 else 1)

        row_id = decode_text(out.get("row_id")).strip()
        if not row_id:
            entry_id = decode_text(out.get("entry_id")).strip()
            if entry_id:
                row_id = f"{strategy_id}:trade:entry:{entry_id}"
            elif seq is not None:
                row_id = f"{strategy_id}:trade:{seq}:{out['ts_ms']}:{out['version']}"
            else:
                row_id = row_id or f"{strategy_id}:trade:{out['ts_ms']}:{index}"
        out["row_id"] = row_id
        filtered.append(enrich_row_with_canonical_naming(out))
    if since_seq is not None:
        # Delta mode must stream oldest unseen seq first to avoid skipping rows.
        filtered.sort(key=lambda item: safe_int(item.get("seq")) or 0)
    else:
        filtered.sort(key=lambda item: coerce_ts_ms(item.get("ts_ms")) or 0, reverse=True)
    return filtered[: max(1, limit)]


def build_alerts_rows(
    *,
    rows: Sequence[dict[str, Any]],
    strategy_id: str,
    limit: int,
) -> list[dict[str, Any]]:
    filtered = [dict(row) for row in rows if strategy_id_from_row(row, strategy_id) == strategy_id]
    filtered.sort(
        key=lambda item: (
            coerce_ts_ms(item.get("ts_ms") or item.get("ts") or item.get("timestamp")) or 0
        ),
        reverse=True,
    )
    return filtered[: max(1, limit)]


def build_error(
    *,
    code: str,
    message: str,
    details: Mapping[str, Any] | None = None,
) -> dict[str, Any]:
    payload: dict[str, Any] = {"code": code, "message": message}
    if details:
        payload["details"] = dict(details)
    return payload


def build_envelope(
    *,
    ok: bool,
    request_id: str,
    timestamp_ms: int,
    api_version: str,
    data: Any,
    error: Mapping[str, Any] | None,
) -> dict[str, Any]:
    return {
        "ok": bool(ok),
        "api_version": api_version,
        "request_id": request_id,
        "timestamp_ms": int(timestamp_ms),
        "data": data,
        "error": dict(error) if error is not None else None,
    }
