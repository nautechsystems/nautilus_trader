"""
Provide inventory extraction and skew computation helpers for MakerV3.
"""

from __future__ import annotations

from collections.abc import Callable
from collections.abc import Iterable
from collections.abc import Mapping
from contextlib import suppress
from decimal import Decimal
from typing import Any

from nautilus_trader.accounting.accounts.base import Account
from flux.strategies.makerv3.pricing import clamp_decimal
from flux.strategies.makerv3.pricing import to_decimal
from flux.strategies.makerv3.pricing import to_decimal_or_none
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.position import Position


INVENTORY_SKEW_RUNTIME_PARAMS: set[str] = {
    "des_qty_global",
    "max_qty_global",
    "max_skew_bps_global",
    "des_qty_local",
    "max_qty_local",
    "max_skew_bps_local",
    "linear_offset_bps",
}


def _stringify_identifier(value: Any) -> str:
    if value is None:
        return ""
    to_str = getattr(value, "to_str", None)
    if callable(to_str):
        with suppress(Exception):
            return str(to_str())
    code = getattr(value, "code", None)
    if code is not None:
        return str(code)
    return str(value)


def normalize_contract_symbol(raw_symbol: str) -> tuple[str, str]:
    """
    Normalize a raw symbol into uppercase base/quote components.
    """
    symbol = str(raw_symbol).strip()
    if not symbol:
        return "", ""

    if "/" in symbol:
        base, quote = symbol.split("/", maxsplit=1)
        return base.strip().upper(), quote.strip().upper()

    cleaned = symbol.replace("-", "_").replace("/", "_")
    if "_" in cleaned:
        base, quote = cleaned.split("_", maxsplit=1)
        if base and quote:
            return base.strip().upper(), quote.strip().upper()

    return symbol.upper(), ""


def maker_base_currency_code(
    *,
    instrument: Instrument | None,
    instrument_id: InstrumentId,
) -> str | None:
    """
    Return the maker instrument base currency code when available.
    """
    if instrument is None:
        return None

    direct_code = _stringify_identifier(getattr(instrument, "base_currency", None)).upper()
    if direct_code:
        return direct_code

    parsed_base, _ = normalize_contract_symbol(str(getattr(instrument, "id", instrument_id)))
    return parsed_base or None


def instrument_base_currency_code(
    *,
    instrument: Instrument | None,
    instrument_id: Any,
) -> str | None:
    """
    Return an instrument base currency code from metadata when available.
    """
    direct_code = _stringify_identifier(getattr(instrument, "base_currency", None)).upper()
    if direct_code:
        return direct_code

    raw_symbol = _stringify_identifier(getattr(instrument, "raw_symbol", ""))
    symbol = _stringify_identifier(getattr(instrument_id, "symbol", None))
    parsed_base, _ = normalize_contract_symbol(raw_symbol or symbol or str(instrument_id))
    return parsed_base or None


def account_venue_code(account: Any) -> str:
    """
    Return an uppercase account venue/issuer code when available.
    """
    account_id = getattr(account, "id", None)
    get_issuer = getattr(account_id, "get_issuer", None)
    if callable(get_issuer):
        with suppress(Exception):
            issuer = get_issuer()
            if issuer:
                return str(issuer).strip().upper()

    issuer = _stringify_identifier(getattr(account_id, "issuer", None)).upper()
    if issuer:
        return issuer

    account_id_text = _stringify_identifier(account_id).strip().upper()
    if "-" in account_id_text:
        return account_id_text.split("-", maxsplit=1)[0]
    if ":" in account_id_text:
        return account_id_text.split(":", maxsplit=1)[0]
    return account_id_text


def _position_signed_qty_value(position: Position) -> Decimal | None:
    signed_qty = to_decimal_or_none(getattr(position, "signed_qty", None))
    if signed_qty is None:
        qty = to_decimal_or_none(getattr(position, "quantity", None))
        side = _stringify_identifier(getattr(position, "side", "")).upper()
        if qty is not None:
            signed_qty = -qty if side == "SHORT" else qty
    return signed_qty


def position_signed_qty(positions: Iterable[Position]) -> Decimal | None:
    """
    Aggregate signed position quantity across open positions.
    """
    total = Decimal(0)
    found = False
    for position in positions:
        signed_qty = _position_signed_qty_value(position)
        if signed_qty is None:
            continue
        total += signed_qty
        found = True
    return total if found else None


def position_inventory_total(
    positions: Iterable[Position],
    *,
    base_currency: str,
    instrument_lookup: Callable[[Any], Instrument | None] | None = None,
    venue: Any | None = None,
) -> Decimal | None:
    """
    Aggregate signed position quantity for a base asset, optionally scoped to a venue.
    """
    code = str(base_currency).strip().upper()
    if not code:
        return None

    total = Decimal(0)
    found = False
    for position in positions:
        if not position_matches_base_currency(
            position,
            base_currency=code,
            instrument_lookup=instrument_lookup,
            venue=venue,
        ):
            continue

        signed_qty = _position_signed_qty_value(position)
        if signed_qty is None:
            continue
        total += signed_qty
        found = True
    return total if found else None


def position_matches_base_currency(
    position: Position,
    *,
    base_currency: str,
    instrument_lookup: Callable[[Any], Instrument | None] | None = None,
    venue: Any | None = None,
) -> bool:
    """
    Return whether a position belongs to the requested base asset and optional venue.
    """
    code = str(base_currency).strip().upper()
    if not code:
        return False

    venue_code = _stringify_identifier(venue).upper()
    instrument_id = getattr(position, "instrument_id", None)
    if venue_code and _stringify_identifier(getattr(instrument_id, "venue", None)).upper() != venue_code:
        return False

    instrument: Instrument | None = None
    if callable(instrument_lookup) and instrument_id is not None:
        with suppress(Exception):
            instrument = instrument_lookup(instrument_id)
    position_base_currency = instrument_base_currency_code(
        instrument=instrument,
        instrument_id=instrument_id,
    )
    return str(position_base_currency or "").upper() == code


def _inventory_total(*components: Decimal | None) -> Decimal | None:
    found = False
    total = Decimal(0)
    for component in components:
        if component is None:
            continue
        total += component
        found = True
    return total if found else None


def local_inventory_total(
    *,
    local_position_qty: Decimal | None,
    local_spot_qty: Decimal | None,
) -> Decimal | None:
    return _inventory_total(local_position_qty, local_spot_qty)


def _inventory_source(*components: tuple[str, Decimal | None]) -> str:
    present = [name for name, value in components if value is not None]
    if len(present) == 2:
        if present == ["positions", "spot"]:
            return "positions_plus_spot"
        return "_plus_".join(present)
    if len(present) == 1:
        return "spot_balance" if present[0] == "spot" else present[0]
    return "unavailable"


def _account_venue_code(account: Account) -> str:
    account_id = _stringify_identifier(getattr(account, "id", None))
    if not account_id:
        account_id = _stringify_identifier(
            getattr(getattr(account, "last_event", None), "account_id", None),
        )
    text = account_id.strip()
    if not text:
        return ""
    for separator in ("-", ":", "."):
        if separator in text:
            return text.split(separator, maxsplit=1)[0].upper()
    return text.upper()


def spot_balance_total(
    *,
    accounts: Iterable[Account],
    currency_code: str,
    venue: Any | None = None,
) -> Decimal | None:
    """
    Aggregate total spot balance for a currency across accounts.
    """
    code = str(currency_code).strip().upper()
    if not code:
        return None
    venue_code = _stringify_identifier(venue).upper()
    total = Decimal(0)
    found = False
    for account in accounts:
        if venue_code and _account_venue_code(account) != venue_code:
            continue
        balances_total_fn = getattr(account, "balances_total", None)
        if not callable(balances_total_fn):
            continue
        balances_total: Any = None
        with suppress(Exception):
            balances_total = balances_total_fn()
        if not isinstance(balances_total, dict):
            continue
        for currency, amount in balances_total.items():
            if _stringify_identifier(currency).upper() != code:
                continue
            amount_dec = to_decimal_or_none(amount)
            if amount_dec is None:
                continue
            total += amount_dec
            found = True
    return total if found else None


def compute_inventory_skew(
    *,
    global_position_qty: Decimal | None,
    global_spot_qty: Decimal | None,
    local_position_qty: Decimal | None,
    local_spot_qty: Decimal | None,
    global_inventory_qty_override: Decimal | None = None,
    global_inventory_source_override: str | None = None,
    base_currency: str | None,
    runtime_params: Mapping[str, Any],
) -> dict[str, Any]:
    """
    Compute inventory skew context from balances and runtime parameters.
    """
    global_inventory_qty = _inventory_total(global_position_qty, global_spot_qty)
    global_inventory_source = _inventory_source(
        ("positions", global_position_qty),
        ("spot", global_spot_qty),
    )
    if global_inventory_qty_override is not None:
        global_inventory_qty = global_inventory_qty_override
        global_inventory_source = global_inventory_source_override or "override"
    local_inventory_qty = _inventory_total(local_position_qty, local_spot_qty)
    local_inventory_source = _inventory_source(
        ("positions", local_position_qty),
        ("spot", local_spot_qty),
    )

    des_qty_global = to_decimal(runtime_params["des_qty_global"])
    max_qty_global = to_decimal(runtime_params["max_qty_global"])
    max_skew_bps_global = to_decimal(runtime_params["max_skew_bps_global"])
    des_qty_local = to_decimal(runtime_params["des_qty_local"])
    max_qty_local = to_decimal(runtime_params["max_qty_local"])
    max_skew_bps_local = to_decimal(runtime_params["max_skew_bps_local"])
    linear_offset_bps = to_decimal(runtime_params["linear_offset_bps"])

    global_ratio: Decimal | None = None
    global_skew_bps: Decimal | None = None
    if global_inventory_qty is not None and max_qty_global > 0:
        global_ratio = clamp_decimal(
            (global_inventory_qty - des_qty_global) / max_qty_global,
            Decimal(-1),
            Decimal(1),
        )
        global_skew_bps = global_ratio * max(Decimal(0), max_skew_bps_global)

    local_ratio: Decimal | None = None
    local_skew_bps: Decimal | None = None
    if local_inventory_qty is not None and max_qty_local > 0:
        local_ratio = clamp_decimal(
            (local_inventory_qty - des_qty_local) / max_qty_local,
            Decimal(-1),
            Decimal(1),
        )
        local_skew_bps = local_ratio * max(Decimal(0), max_skew_bps_local)

    total_skew_bps = linear_offset_bps
    if global_skew_bps is not None:
        total_skew_bps += global_skew_bps
    if local_skew_bps is not None:
        total_skew_bps += local_skew_bps

    return {
        "inventory_qty": global_inventory_qty,
        "inventory_source": global_inventory_source,
        "base_currency": base_currency,
        "position_qty": global_position_qty,
        "spot_qty": global_spot_qty,
        "global_position_qty": global_position_qty,
        "global_spot_qty": global_spot_qty,
        "global_inventory_qty": global_inventory_qty,
        "global_inventory_source": global_inventory_source,
        "local_position_qty": local_position_qty,
        "local_spot_qty": local_spot_qty,
        "local_inventory_qty": local_inventory_qty,
        "local_inventory_source": local_inventory_source,
        "des_qty_global": des_qty_global,
        "max_qty_global": max_qty_global,
        "max_skew_bps_global": max_skew_bps_global,
        "des_qty_local": des_qty_local,
        "max_qty_local": max_qty_local,
        "max_skew_bps_local": max_skew_bps_local,
        "linear_offset_bps": linear_offset_bps,
        "global_ratio": global_ratio,
        "global_skew_bps": global_skew_bps,
        "local_ratio": local_ratio,
        "local_skew_bps": local_skew_bps,
        "total_skew_bps": total_skew_bps,
    }


class InventorySkewCache:
    """
    Cache computed inventory skew for a short TTL.
    """

    def __init__(self, ttl_ms: int) -> None:
        """
        Initialize the cache with a TTL in milliseconds.
        """
        self._ttl_ms = max(0, int(ttl_ms))
        self._cache: dict[str, Any] | None = None
        self._cache_ts_ns = 0

    def set_ttl_ms(self, ttl_ms: int) -> None:
        """
        Update the cache TTL in milliseconds.
        """
        self._ttl_ms = max(0, int(ttl_ms))

    def invalidate(self) -> None:
        """
        Drop any cached skew snapshot immediately.
        """
        self._cache = None
        self._cache_ts_ns = 0

    def get(
        self,
        *,
        now_ns: int,
        runtime_params: Mapping[str, Any],
        compute: Callable[[Mapping[str, Any]], dict[str, Any]],
    ) -> dict[str, Any]:
        """
        Return cached skew when fresh, otherwise recompute and cache it.
        """
        ttl_ns = self._ttl_ms * 1_000_000
        if self._cache is not None and ttl_ns > 0 and now_ns - self._cache_ts_ns < ttl_ns:
            return self._cache

        skew_ctx = compute(runtime_params)
        self._cache = skew_ctx
        self._cache_ts_ns = now_ns
        return skew_ctx


__all__ = [
    "INVENTORY_SKEW_RUNTIME_PARAMS",
    "InventorySkewCache",
    "account_venue_code",
    "compute_inventory_skew",
    "instrument_base_currency_code",
    "position_inventory_total",
    "position_matches_base_currency",
    "maker_base_currency_code",
    "normalize_contract_symbol",
    "position_signed_qty",
    "spot_balance_total",
]
