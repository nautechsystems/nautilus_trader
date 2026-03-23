"""Shared types and pure helpers for Flux API payload assembly."""

from __future__ import annotations

import json
import time
from collections.abc import Mapping
from collections.abc import Sequence
from dataclasses import dataclass
from datetime import datetime
from decimal import Decimal
from decimal import InvalidOperation
from typing import Any


_STABLE_CASH_ASSETS = frozenset({"USD", "USDT", "USDC", "DAI", "FDUSD", "USDE"})


@dataclass(frozen=True, slots=True)
class ContractCatalogEntry:
    """Describe a contract that may appear in balances, legs, and signal payloads."""

    exchange: str
    symbol: str
    instrument_id: str = ""


@dataclass(frozen=True, slots=True)
class StrategyMetadata:
    """Strategy metadata exposed on API payloads."""

    strategy_class: str
    strategy_groups: str
    base_asset: str
    quote_asset: str
    param_set: str = ""
    strategy_family: str = ""
    strategy_version: str = ""

    def as_payload(self, *, strategy_id: str) -> dict[str, str]:
        """Serialize metadata into the stable API wire shape."""

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
_JS_SAFE_INTEGER_MAX = 9_007_199_254_740_991
_REDIS_STREAM_ID_SEQ_MULTIPLIER = 4096


def contract_id_for_leg(*, exchange: Any, symbol: Any, instrument_id: Any = None) -> str:
    """Build the stable contract identifier used across payload builders."""

    exchange_text = decode_text(exchange).strip().lower()
    symbol_text = (
        decode_text(instrument_id).strip().upper()
        or decode_text(symbol).strip().upper()
    )
    return f"{exchange_text}:{symbol_text}"


def now_ms() -> int:
    """Return the current UNIX timestamp in milliseconds."""

    return int(time.time() * 1_000)


def decode_text(value: Any) -> str:
    """Convert an arbitrary value to text, normalizing ``None`` to an empty string."""

    if value is None:
        return ""
    if isinstance(value, bytes):
        return value.decode("utf-8", errors="replace")
    return str(value)


def load_json(value: Any) -> Any:
    """Parse JSON-like values while tolerating empty or invalid payload fields."""

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
    """Normalize a scalar, tuple, or ``None`` into a list."""

    if value is None:
        return []
    if isinstance(value, list):
        return list(value)
    if isinstance(value, tuple):
        return list(value)
    return [value]


def decode_text_list(value: Any) -> list[str]:
    """Normalize a scalar or sequence into a de-duplicated list of non-empty text values."""

    out: list[str] = []
    seen: set[str] = set()
    for item in as_list(value):
        text = decode_text(item).strip()
        if not text or text in seen:
            continue
        seen.add(text)
        out.append(text)
    return out


def safe_int(value: Any) -> int | None:
    """Return ``int(value)`` when possible, otherwise ``None``."""

    if value is None:
        return None
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


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
    """Return ``float(value)`` when possible, otherwise ``None``."""

    if value is None:
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def safe_bool(value: Any) -> bool | None:
    """Parse common truthy and falsey payload encodings into a boolean."""

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


def project_trade_quantity_fields(
    row: Mapping[str, Any],
    *,
    base_first: bool = False,
) -> dict[str, Any]:
    """Project explicit trade quantity fields into the operator-facing API contract."""

    out = dict(row)
    qty_base = decode_text(out.get("qty_base")).strip()

    if base_first and qty_base:
        out["qty_base"] = qty_base
        out["qty"] = qty_base

    return out


def coerce_ts_ms(value: Any) -> int | None:
    """Coerce seconds, milliseconds, microseconds, nanoseconds, or ISO text into epoch ms."""

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
    """Split a symbol into base and quote assets using Flux's known exchange formats."""

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
        if venue_text.split("_", maxsplit=1)[0] == "IBKR":
            return "equity"
        if raw_symbol:
            return "spot"
        return "perp"
    if raw_symbol:
        return "spot"
    if is_position:
        return "perp"
    return "cash"


def _derive_pair_parts(*, instrument_id: Any, raw_symbol: str, fallback_symbol: Any) -> tuple[str, str, str]:
    raw_symbol_text = decode_text(raw_symbol).strip().upper()
    if not raw_symbol_text:
        raw_symbol_text = _raw_symbol_from_instrument_id(instrument_id)
    if not raw_symbol_text:
        raw_symbol_text = decode_text(fallback_symbol).strip().upper()

    pair_source, _explicit_contract_type = _strip_contract_suffix(raw_symbol_text)
    base_asset, quote_asset = normalize_symbol_parts(symbol=pair_source or raw_symbol_text)
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
    product_type: Any = None,
    is_position: bool = False,
) -> dict[str, str]:
    """Derive the canonical instrument, venue, and display fields used across payload rows."""

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
    derived_product_type = "perp" if contract_type in _PERP_CONTRACT_TYPES else "spot"
    explicit_product_type = decode_text(product_type).strip().lower()
    if explicit_product_type in {"spot", "perp"}:
        if contract_type == "cash":
            product_type = explicit_product_type
        elif explicit_product_type == derived_product_type:
            product_type = explicit_product_type
        else:
            # Prefer the resolved contract type when upstream product metadata conflicts with the
            # instrument identity. This avoids spot positions inheriting a stale "perp" label.
            product_type = derived_product_type
    else:
        product_type = derived_product_type
    if not base_asset:
        base_asset = inventory_asset_text or decode_text(asset).strip().upper()
    pair = f"{base_asset}/{quote_asset}" if base_asset and quote_asset else (inventory_asset_text or raw_symbol)

    display_asset = inventory_asset_text or base_asset or raw_symbol or decode_text(symbol).strip().upper()
    if contract_type == "cash" and venue_root == "ibkr":
        display_name_short = display_asset
    elif (
        display_asset in _STABLE_CASH_ASSETS
        and not is_position
        and not instrument_text
    ):
        display_name_short = display_asset
    elif contract_type == "equity":
        display_name_short = f"{display_asset} Stock".strip() if display_asset else "Stock"
    elif product_type == "perp":
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


def _is_position_row(row: Mapping[str, Any]) -> bool:
    kind = decode_text(row.get("kind")).strip().lower()
    if kind == "position":
        return True
    asset = decode_text(row.get("asset") or row.get("coin") or row.get("base")).strip().upper()
    instrument_text = decode_text(row.get("instrument_id") or row.get("symbol")).strip().upper()
    return "PERP" in asset or "PERP" in instrument_text or "LINEAR" in instrument_text


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
    """Return a row annotated with canonical naming fields used by Flux clients."""

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
        product_type=out.get("product_type") or out.get("market_type"),
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
    """Extract a strategy ID from a row, defaulting to the caller-supplied fallback."""

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
    """Flatten Redis stream entries into row dictionaries with stable cursor metadata."""

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
    """Return the first row for ``strategy_id`` or fall back to the first row in the sequence."""

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


def _position_qty_from_keys(
    row: Mapping[str, Any],
    *,
    signed_keys: Sequence[str],
    magnitude_keys: Sequence[str],
) -> Decimal | None:
    signed_key_set = set(signed_keys)
    source_key = ""
    value: Any = None
    for key in signed_keys:
        candidate = row.get(key)
        if candidate is None:
            continue
        value = candidate
        source_key = key
        break
    if value is None:
        for key in magnitude_keys:
            candidate = row.get(key)
            if candidate is None:
                continue
            value = candidate
            source_key = key
            break
    qty = _to_decimal(value)
    if qty is None:
        return None
    if source_key not in signed_key_set:
        side = decode_text(row.get("side") or row.get("position_side")).strip().upper()
        if (side == "SHORT" and qty > 0) or (side == "LONG" and qty < 0):
            qty = -qty
    return qty


def _position_signed_qty(row: dict[str, Any]) -> Decimal | None:
    return _position_qty_from_keys(
        row,
        signed_keys=("signed_qty_base", "signed_qty"),
        magnitude_keys=("quantity_base", "total", "free", "quantity", "qty", "size"),
    )


def _position_venue_signed_qty(row: dict[str, Any]) -> Decimal | None:
    return _position_qty_from_keys(
        row,
        signed_keys=("signed_qty_venue", "signed_qty"),
        magnitude_keys=("quantity_venue", "quantity", "qty", "size", "total", "free"),
    )


def build_params_payload(
    *,
    strategy_id: str,
    params: dict[str, Any],
    schema: Mapping[str, Mapping[str, Any]],
    running: bool | None = None,
    metadata: StrategyMetadata | None = None,
) -> dict[str, Any]:
    """Build the params response payload for a strategy."""

    payload: dict[str, Any] = {
        "strategy_id": strategy_id,
        "params": params,
        "schema": {str(name): dict(spec) for name, spec in schema.items()},
    }
    payload["running"] = running
    if metadata is not None:
        payload["meta"] = metadata.as_payload(strategy_id=strategy_id)
    return payload


def build_trades_rows(
    *,
    rows: Sequence[dict[str, Any]],
    strategy_id: str,
    limit: int,
    since_ms: int | None,
    since_seq: int | None = None,
    base_first_qty: bool = False,
) -> list[dict[str, Any]]:
    """Normalize trade rows and apply time or sequence-based pagination."""

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
        filtered.append(
            enrich_row_with_canonical_naming(
                project_trade_quantity_fields(out, base_first=base_first_qty),
            ),
        )
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
    """Return the newest alerts for a strategy."""

    filtered: list[dict[str, Any]] = []
    for index, row in enumerate(rows):
        if strategy_id_from_row(row, strategy_id) != strategy_id:
            continue
        out = dict(row)
        row_id = decode_text(out.get("row_id")).strip()
        if not row_id:
            row_id = decode_text(out.get("id")).strip()
        if not row_id:
            row_id = decode_text(out.get("entry_id")).strip()
        if not row_id:
            ts_ms = coerce_ts_ms(out.get("ts_ms") or out.get("ts") or out.get("timestamp")) or 0
            row_id = f"{strategy_id}:alert:{ts_ms}:{index}"
        out["row_id"] = row_id
        out["id"] = decode_text(out.get("id")).strip() or row_id
        filtered.append(out)
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
    """Build the standard error payload returned by the Flux API."""

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
    """Wrap a response payload in the standard Flux API envelope."""

    return {
        "ok": bool(ok),
        "api_version": api_version,
        "request_id": request_id,
        "timestamp_ms": int(timestamp_ms),
        "data": data,
        "error": dict(error) if error else None,
    }
