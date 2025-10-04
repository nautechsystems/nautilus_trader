# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
"""Instrument provider for Coinbase."""

import asyncio
import json
from decimal import Decimal

import nautilus_pyo3
from nautilus_trader.adapters.coinbase.constants import COINBASE_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencySpot
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class CoinbaseInstrumentProvider(InstrumentProvider):
    """
    Provides instruments for the Coinbase Advanced Trade API.

    Parameters
    ----------
    client : nautilus_pyo3.CoinbaseHttpClient
        The Coinbase HTTP client.
    """

    def __init__(
        self,
        client: nautilus_pyo3.CoinbaseHttpClient,
    ) -> None:
        super().__init__()
        self._client = client
        self._log_warnings = True

    async def load_all_async(self, filters: dict | None = None) -> None:
        """
        Load all instruments from Coinbase.

        Parameters
        ----------
        filters : dict, optional
            Not applicable for this provider.

        """
        products_json = await self._client.list_products()
        products_data = json.loads(products_json)

        for product in products_data.get("products", []):
            try:
                instrument = self._parse_instrument(product)
                if instrument:
                    self.add(instrument)
            except Exception as e:
                if self._log_warnings:
                    self._log.warning(f"Failed to parse instrument {product.get('product_id')}: {e}")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load specific instruments by ID.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : dict, optional
            Not applicable for this provider.

        """
        for instrument_id in instrument_ids:
            try:
                product_id = instrument_id.symbol.value.replace("/", "-")
                product_json = await self._client.get_product(product_id)
                product = json.loads(product_json)
                
                instrument = self._parse_instrument(product)
                if instrument:
                    self.add(instrument)
            except Exception as e:
                if self._log_warnings:
                    self._log.warning(f"Failed to load instrument {instrument_id}: {e}")

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        """
        Load a single instrument.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.
        filters : dict, optional
            Not applicable for this provider.

        """
        await self.load_ids_async([instrument_id], filters)

    def _parse_instrument(self, product: dict) -> CurrencySpot | None:
        """Parse a Coinbase product into a NautilusTrader instrument."""
        product_id = product.get("product_id")
        if not product_id:
            return None

        # Skip if trading is disabled
        if product.get("trading_disabled", False) or product.get("is_disabled", False):
            return None

        # Parse product ID (e.g., "BTC-USD" -> base="BTC", quote="USD")
        parts = product_id.split("-")
        if len(parts) != 2:
            return None

        base_currency_str = parts[0]
        quote_currency_str = parts[1]

        # Create currencies
        base_currency = Currency.from_str(base_currency_str)
        quote_currency = Currency.from_str(quote_currency_str)

        # Parse price and size increments
        price_increment = Decimal(product.get("price_increment", "0.01"))
        base_increment = Decimal(product.get("base_increment", "0.00000001"))
        
        # Parse min/max sizes
        base_min_size = Decimal(product.get("base_min_size", "0"))
        base_max_size = Decimal(product.get("base_max_size", "1000000"))
        quote_min_size = Decimal(product.get("quote_min_size", "0"))
        quote_max_size = Decimal(product.get("quote_max_size", "1000000000"))

        # Create instrument ID
        symbol = Symbol(product_id.replace("-", "/"))
        instrument_id = InstrumentId(symbol=symbol, venue=COINBASE_VENUE)

        # Determine precision from increments
        price_precision = abs(price_increment.as_tuple().exponent)
        size_precision = abs(base_increment.as_tuple().exponent)

        # Create instrument
        instrument = CurrencySpot(
            instrument_id=instrument_id,
            raw_symbol=Symbol(product_id),
            base_currency=base_currency,
            quote_currency=quote_currency,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=Price.from_str(str(price_increment)),
            size_increment=Quantity.from_str(str(base_increment)),
            lot_size=None,
            max_quantity=Quantity.from_str(str(base_max_size)),
            min_quantity=Quantity.from_str(str(base_min_size)),
            max_notional=None,
            min_notional=Money(quote_min_size, quote_currency),
            max_price=None,
            min_price=None,
            margin_init=Decimal(0),
            margin_maint=Decimal(0),
            maker_fee=Decimal("0.006"),  # Default Coinbase maker fee (0.6%)
            taker_fee=Decimal("0.006"),  # Default Coinbase taker fee (0.6%)
            ts_event=0,
            ts_init=0,
        )

        return instrument

