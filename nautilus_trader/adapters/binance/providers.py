# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
import time
from decimal import Decimal
from typing import Dict

import orjson

from nautilus_trader.adapters.binance.common import BINANCE_VENUE
from nautilus_trader.adapters.binance.http.api.spot_market import BinanceSpotMarketHttpAPI
from nautilus_trader.adapters.binance.http.api.wallet import BinanceWalletHttpAPI
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.text import precision_from_str
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments.currency import CurrencySpot
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BinanceInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` from the Binance API.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        logger: Logger,
    ):
        """
        Initialize a new instance of the ``BinanceInstrumentProvider`` class.

        Parameters
        ----------
        client : APIClient
            The client for the provider.
        logger : Logger
            The logger for the provider.

        """
        super().__init__()

        self.venue = BINANCE_VENUE
        self._client = client
        self._log = LoggerAdapter(type(self).__name__, logger)

        self._wallet = BinanceWalletHttpAPI(self._client)
        self._spot_market = BinanceSpotMarketHttpAPI(self._client)

        # Async loading flags
        self._loaded = False
        self._loading = False

    async def load_all_or_wait_async(self) -> None:
        """
        Load the latest Binance instruments into the provider asynchronously, or
        await loading.

        If `load_async` has been previously called then will immediately return.
        """
        if self._loaded:
            return  # Already loaded

        if not self._loading:
            self._log.debug("Loading instruments...")
            await self.load_all_async()
            self._log.info(f"Loaded {self.count} instruments.")
        else:
            self._log.debug("Awaiting loading...")
            while self._loading:
                # Wait 100ms
                await asyncio.sleep(0.1)

    async def load_all_async(self) -> None:
        """
        Load the latest Binance instruments into the provider asynchronously.

        """
        # Set async loading flag
        self._loading = True

        # Get current commission rates
        raw: bytes = await self._wallet.trade_fee()
        fees: Dict[str, Dict[str, str]] = {s["symbol"]: s for s in orjson.loads(raw)}

        # Get exchange info for all assets
        raw = await self._spot_market.exchange_info()
        response = orjson.loads(raw)
        server_time_ns: int = millis_to_nanos(response["serverTime"])

        for info in response["symbols"]:
            native_symbol: str = info["symbol"]

            # Create base asset
            base_asset: str = info["baseAsset"]
            base_currency = Currency(
                code=base_asset,
                precision=info["baseAssetPrecision"],
                iso4217=0,  # Currently undetermined for crypto assets
                name=base_asset,
                currency_type=CurrencyType.CRYPTO,
            )

            # Create quote asset
            quote_asset: str = info["quoteAsset"]
            quote_currency = Currency(
                code=quote_asset,
                precision=info["quoteAssetPrecision"],
                iso4217=0,  # Currently undetermined for crypto assets
                name=quote_asset,
                currency_type=CurrencyType.CRYPTO,
            )

            symbol = Symbol(base_currency.code + "/" + quote_currency.code)
            instrument_id = InstrumentId(symbol=symbol, venue=BINANCE_VENUE)

            # Parse instrument filters
            symbol_filters = {f["filterType"]: f for f in info["filters"]}
            price_filter = symbol_filters.get("PRICE_FILTER")
            lot_size_filter = symbol_filters.get("LOT_SIZE")
            min_notional_filter = symbol_filters.get("MIN_NOTIONAL")
            # market_lot_size_filter = symbol_filters.get("MARKET_LOT_SIZE")

            tick_size = price_filter["tickSize"].rstrip("0")
            step_size = lot_size_filter["stepSize"].rstrip("0")
            price_precision = precision_from_str(tick_size)
            size_precision = precision_from_str(step_size)
            price_increment = Price.from_str(tick_size)
            size_increment = Quantity.from_str(step_size)
            lot_size = Quantity.from_str(step_size)
            max_quantity = Quantity(float(lot_size_filter["maxQty"]), precision=size_precision)
            min_quantity = Quantity(float(lot_size_filter["minQty"]), precision=size_precision)
            min_notional = None
            if min_notional_filter is not None:
                min_notional = Money(min_notional_filter["minNotional"], currency=quote_currency)
            max_price = Price(float(price_filter["maxPrice"]), precision=price_precision)
            min_price = Price(float(price_filter["minPrice"]), precision=price_precision)
            pair_fees = fees.get(native_symbol)
            maker_fee: Decimal = Decimal(0)
            taker_fee: Decimal = Decimal(0)
            if pair_fees:
                maker_fee = Decimal(pair_fees["makerCommission"])
                taker_fee = Decimal(pair_fees["takerCommission"])

            # Create instrument
            instrument = CurrencySpot(
                instrument_id=instrument_id,
                base_currency=base_currency,
                quote_currency=quote_currency,
                price_precision=price_precision,
                size_precision=size_precision,
                price_increment=price_increment,
                size_increment=size_increment,
                lot_size=lot_size,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=None,
                min_notional=min_notional,
                max_price=max_price,
                min_price=min_price,
                margin_init=Decimal(0),
                margin_maint=Decimal(0),
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event=server_time_ns,
                ts_init=time.time_ns(),
                info=info,
            )

            self.add_currency(currency=base_currency)
            self.add_currency(currency=quote_currency)
            self.add(instrument=instrument)

        # Set async loading flags
        self._loading = False
        self._loaded = True
