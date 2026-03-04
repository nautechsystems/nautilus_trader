"""Provide inventory extraction and skew computation helpers for MakerV3."""

from __future__ import annotations

from collections.abc import Callable
from collections.abc import Iterable
from collections.abc import Mapping
from decimal import Decimal
from typing import Any

from nautilus_trader.accounting.accounts.base import Account
from nautilus_trader.flux.strategies.makerv3.pricing import clamp_decimal
from nautilus_trader.flux.strategies.makerv3.pricing import to_decimal
from nautilus_trader.flux.strategies.makerv3.pricing import to_decimal_or_none
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
        try:
            return str(to_str())
        except Exception:
            pass
    code = getattr(value, "code", None)
    if code is not None:
        return str(code)
    return str(value)


def normalize_contract_symbol(raw_symbol: str) -> tuple[str, str]:
    """Normalize a raw symbol into uppercase base/quote components."""
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


def maker_base_currency_code(*, instrument: Instrument | None, instrument_id: InstrumentId) -> str | None:
    """Return the maker instrument base currency code when available."""
    if instrument is None:
        return None

    direct_code = _stringify_identifier(getattr(instrument, "base_currency", None)).upper()
    if direct_code:
        return direct_code

    parsed_base, _ = normalize_contract_symbol(str(getattr(instrument, "id", instrument_id)))
    return parsed_base or None


def position_signed_qty(positions: Iterable[Position]) -> Decimal | None:
    """Aggregate signed position quantity across open positions."""
    total = Decimal("0")
    found = False
    for position in positions:
        signed_qty = to_decimal_or_none(getattr(position, "signed_qty", None))
        if signed_qty is None:
            qty = to_decimal_or_none(getattr(position, "quantity", None))
            side = _stringify_identifier(getattr(position, "side", "")).upper()
            if qty is not None:
                signed_qty = -qty if side == "SHORT" else qty
        if signed_qty is None:
            continue
        total += signed_qty
        found = True
    return total if found else None


def spot_balance_total(*, accounts: Iterable[Account], currency_code: str) -> Decimal | None:
    """Aggregate total spot balance for a currency across accounts."""
    code = str(currency_code).strip().upper()
    if not code:
        return None
    total = Decimal("0")
    found = False
    for account in accounts:
        balances_total_fn = getattr(account, "balances_total", None)
        if not callable(balances_total_fn):
            continue
        try:
            balances_total = balances_total_fn()
        except Exception:
            continue
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
    position_qty: Decimal | None,
    spot_qty: Decimal | None,
    base_currency: str | None,
    runtime_params: Mapping[str, Any],
) -> dict[str, Any]:
    """Compute inventory skew context from balances and runtime parameters."""
    if position_qty is not None:
        inventory_qty = position_qty
        inventory_source = "maker_position"
    elif spot_qty is not None:
        inventory_qty = spot_qty
        inventory_source = "maker_spot_balance"
    else:
        inventory_qty = None
        inventory_source = "unavailable"

    des_qty_global = to_decimal(runtime_params["des_qty_global"])
    max_qty_global = to_decimal(runtime_params["max_qty_global"])
    max_skew_bps_global = to_decimal(runtime_params["max_skew_bps_global"])
    des_qty_local = to_decimal(runtime_params["des_qty_local"])
    max_qty_local = to_decimal(runtime_params["max_qty_local"])
    max_skew_bps_local = to_decimal(runtime_params["max_skew_bps_local"])
    linear_offset_bps = to_decimal(runtime_params["linear_offset_bps"])

    global_ratio: Decimal | None = None
    global_skew_bps: Decimal | None = None
    if inventory_qty is not None and max_qty_global > 0:
        global_ratio = clamp_decimal(
            (inventory_qty - des_qty_global) / max_qty_global,
            Decimal("-1"),
            Decimal("1"),
        )
        global_skew_bps = global_ratio * max(Decimal("0"), max_skew_bps_global)

    local_ratio: Decimal | None = None
    local_skew_bps: Decimal | None = None
    if inventory_qty is not None and max_qty_local > 0:
        local_ratio = clamp_decimal(
            (inventory_qty - des_qty_local) / max_qty_local,
            Decimal("-1"),
            Decimal("1"),
        )
        local_skew_bps = local_ratio * max(Decimal("0"), max_skew_bps_local)

    total_skew_bps = linear_offset_bps
    if global_skew_bps is not None:
        total_skew_bps += global_skew_bps
    if local_skew_bps is not None:
        total_skew_bps += local_skew_bps

    return {
        "inventory_qty": inventory_qty,
        "inventory_source": inventory_source,
        "base_currency": base_currency,
        "position_qty": position_qty,
        "spot_qty": spot_qty,
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
    """Cache computed inventory skew for a short TTL."""

    def __init__(self, ttl_ms: int) -> None:
        """Initialize the cache with a TTL in milliseconds."""
        self._ttl_ms = max(0, int(ttl_ms))
        self._cache: dict[str, Any] | None = None
        self._cache_ts_ns = 0

    def set_ttl_ms(self, ttl_ms: int) -> None:
        """Update the cache TTL in milliseconds."""
        self._ttl_ms = max(0, int(ttl_ms))

    def invalidate(self) -> None:
        """Drop any cached skew snapshot immediately."""
        self._cache = None
        self._cache_ts_ns = 0

    def get(
        self,
        *,
        now_ns: int,
        runtime_params: Mapping[str, Any],
        compute: Callable[[Mapping[str, Any]], dict[str, Any]],
    ) -> dict[str, Any]:
        """Return cached skew when fresh, otherwise recompute and cache it."""
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
    "compute_inventory_skew",
    "maker_base_currency_code",
    "normalize_contract_symbol",
    "position_signed_qty",
    "spot_balance_total",
]
