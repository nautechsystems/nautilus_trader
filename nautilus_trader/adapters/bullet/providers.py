# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import json
import time
from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.bullet.constants import BULLET_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


_MAX_PRECISION = 16


def _find_filter(filters: list[dict[str, Any]], filter_type: str) -> dict[str, Any] | None:
    for f in filters:
        if f.get("filterType") == filter_type:
            return f
    return None


def _precision_from_str(s: str) -> int:
    """Derive decimal precision from a decimal string like '0.01', capped at _MAX_PRECISION."""
    if "." not in s:
        return 0
    frac = s.rstrip("0").split(".")[-1]
    return min(len(frac), _MAX_PRECISION)


def _parse_symbol_fields(
    info: dict[str, Any],
) -> tuple[str, str, str, str, int, str, int, str]:
    """Return (raw_symbol, base_code, quote_code, margin_code, price_precision, tick_str, size_precision, step_str)."""
    raw_symbol = info["symbol"]
    base_code = info.get("baseAsset", raw_symbol.split("-")[0])
    quote_code = info.get("quoteAsset", "USD")
    margin_code = info.get("marginAsset", quote_code)

    filters = info.get("filters", [])
    price_filter = _find_filter(filters, "PRICE_FILTER")
    lot_filter = _find_filter(filters, "LOT_SIZE")

    if price_filter and price_filter.get("tickSize"):
        tick_str = price_filter["tickSize"]
        price_precision = _precision_from_str(tick_str)
    else:
        price_precision = min(info.get("pricePrecision", 8), _MAX_PRECISION)
        tick_str = f"1e-{price_precision}"

    if lot_filter and lot_filter.get("stepSize"):
        step_str = lot_filter["stepSize"]
        size_precision = _precision_from_str(step_str)
    else:
        size_precision = min(info.get("quantityPrecision", 8), _MAX_PRECISION)
        step_str = f"1e-{size_precision}"

    return raw_symbol, base_code, quote_code, margin_code, price_precision, tick_str, size_precision, step_str


def _symbol_to_instrument(info: dict[str, Any], ts_event: int, ts_init: int) -> CryptoPerpetual:
    raw_symbol, base_code, quote_code, margin_code, price_precision, tick_str, size_precision, step_str = (
        _parse_symbol_fields(info)
    )

    instrument_id = InstrumentId(symbol=Symbol(f"{raw_symbol}-PERP"), venue=BULLET_VENUE)

    try:
        base_currency = Currency.from_str(base_code)
    except Exception:
        base_currency = USD

    try:
        quote_currency = Currency.from_str(quote_code)
    except Exception:
        quote_currency = USD

    try:
        settlement_currency = Currency.from_str(margin_code)
    except Exception:
        settlement_currency = quote_currency

    price_increment = Price(Decimal(tick_str), precision=price_precision)
    size_increment = Quantity(Decimal(step_str), precision=size_precision)

    maker_fee_bps_list: list[str] = info.get("makerFeeBps", [])
    taker_fee_bps_list: list[str] = info.get("takerFeeBps", [])
    maker_fee = Decimal(maker_fee_bps_list[0]) / 10000 if maker_fee_bps_list else Decimal("0")
    taker_fee = Decimal(taker_fee_bps_list[0]) / 10000 if taker_fee_bps_list else Decimal("0")

    return CryptoPerpetual(
        instrument_id=instrument_id,
        raw_symbol=Symbol(raw_symbol),
        base_currency=base_currency,
        quote_currency=quote_currency,
        settlement_currency=settlement_currency,
        is_inverse=False,
        price_precision=price_precision,
        size_precision=size_precision,
        price_increment=price_increment,
        size_increment=size_increment,
        maker_fee=maker_fee,
        taker_fee=taker_fee,
        ts_event=ts_event,
        ts_init=ts_init,
        info=info,
    )


def _symbol_to_pyo3_instrument(
    info: dict[str, Any],
    ts_event: int,
    ts_init: int,
) -> nautilus_pyo3.CryptoPerpetual:
    raw_symbol, base_code, quote_code, margin_code, price_precision, tick_str, size_precision, step_str = (
        _parse_symbol_fields(info)
    )

    instrument_id = nautilus_pyo3.InstrumentId.from_str(f"{raw_symbol}-PERP.BULLET")

    try:
        base_currency = nautilus_pyo3.Currency.from_str(base_code)
    except Exception:
        base_currency = nautilus_pyo3.Currency.from_str("USD")

    try:
        quote_currency = nautilus_pyo3.Currency.from_str(quote_code)
    except Exception:
        quote_currency = nautilus_pyo3.Currency.from_str("USD")

    try:
        settlement_currency = nautilus_pyo3.Currency.from_str(margin_code)
    except Exception:
        settlement_currency = quote_currency

    price_increment = nautilus_pyo3.Price(float(Decimal(tick_str)), price_precision)
    size_increment = nautilus_pyo3.Quantity(float(Decimal(step_str)), size_precision)

    maker_fee_bps_list: list[str] = info.get("makerFeeBps", [])
    taker_fee_bps_list: list[str] = info.get("takerFeeBps", [])
    maker_fee = float(Decimal(maker_fee_bps_list[0]) / 10000) if maker_fee_bps_list else 0.0
    taker_fee = float(Decimal(taker_fee_bps_list[0]) / 10000) if taker_fee_bps_list else 0.0

    return nautilus_pyo3.CryptoPerpetual(
        instrument_id=instrument_id,
        raw_symbol=nautilus_pyo3.Symbol.from_str(raw_symbol),
        base_currency=base_currency,
        quote_currency=quote_currency,
        settlement_currency=settlement_currency,
        is_inverse=False,
        price_precision=price_precision,
        size_precision=size_precision,
        price_increment=price_increment,
        size_increment=size_increment,
        maker_fee=maker_fee,
        taker_fee=taker_fee,
        ts_event=ts_event,
        ts_init=ts_init,
    )


class BulletInstrumentProvider(InstrumentProvider):
    """
    Provides instruments from the Bullet.xyz perpetuals exchange.

    Fetches symbol definitions from ``GET /fapi/v1/exchangeInfo`` and converts
    each entry into a ``CryptoPerpetual`` instrument.

    Parameters
    ----------
    client : nautilus_pyo3.BulletHttpClient
        The Bullet HTTP client used to fetch exchange info.
    client_id : ClientId
        The client ID used for logging.
    config : InstrumentProviderConfig, optional
        Instrument provider configuration.

    """

    def __init__(
        self,
        client: nautilus_pyo3.BulletHttpClient,
        client_id: ClientId,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        PyCondition.not_none(client, "client")
        super().__init__(config=config or InstrumentProviderConfig())

        self._client: nautilus_pyo3.BulletHttpClient = client
        self._client_id = client_id
        self._instruments_pyo3: list[nautilus_pyo3.CryptoPerpetual] = []

    def instruments_pyo3(self) -> list[nautilus_pyo3.CryptoPerpetual]:
        """Return PyO3 instruments (needed by the WebSocket client for precision metadata)."""
        return list(self._instruments_pyo3)

    async def load_all_async(self, filters: dict | None = None) -> None:
        self._log.info("Loading Bullet instruments...")

        raw_json = await self._client.exchange_info_json()
        exchange_info: dict[str, Any] = json.loads(raw_json)
        symbols: list[dict[str, Any]] = exchange_info.get("symbols", [])

        ts_init = time.time_ns()

        self._instruments.clear()
        self._currencies.clear()
        self._instruments_pyo3.clear()

        loaded = 0
        for sym_info in symbols:
            if sym_info.get("status", "TRADING") != "TRADING":
                continue
            try:
                instrument = _symbol_to_instrument(sym_info, ts_event=ts_init, ts_init=ts_init)
                instrument_pyo3 = _symbol_to_pyo3_instrument(sym_info, ts_event=ts_init, ts_init=ts_init)
            except Exception as e:
                self._log.warning(f"Skipping symbol {sym_info.get('symbol')}: {e}")
                continue

            if not self._accept_instrument(instrument, filters or self._filters):
                continue

            self.add(instrument)
            self._instruments_pyo3.append(instrument_pyo3)
            loaded += 1

        if loaded:
            self._log.info(f"Loaded {loaded} Bullet instruments")
        else:
            self._log.warning("No Bullet instruments matched the requested filters")

    def _accept_instrument(self, instrument: CryptoPerpetual, filters: dict | None) -> bool:
        if not filters:
            return True

        symbols = filters.get("symbols")
        if symbols and instrument.id.symbol.value.upper() not in {s.upper() for s in symbols}:
            return False

        bases = filters.get("bases")
        base_code = instrument.base_currency.code if instrument.base_currency else None
        if bases and (not base_code or base_code.upper() not in {b.upper() for b in bases}):
            return False

        return True
