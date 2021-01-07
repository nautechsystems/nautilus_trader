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

from datetime import datetime
from decimal import Decimal

import ccxt

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetTypeParser
from nautilus_trader.model.c_enums.currency_type cimport CurrencyType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class BinanceInstrumentProvider:
    """
    Provides a means of loading Binance `Instrument` objects.
    """

    def __init__(self, client not None: ccxt.binance, bint load_all=False):
        """
        Initialize a new instance of the `BinanceInstrumentProvider` class.

        Parameters
        ----------
        client : ccxt.Exchange
            The client for the provider.
        load_all : bool, optional
            If all instruments should be loaded at instantiation.

        Raises
        ------
        ValueError
            If client.name != 'Binance'.

        """
        Condition.true(client.name == "Binance", "client.name == `Binance`")

        self.venue = Venue("BINANCE")
        self.count = 0
        self._instruments = {}  # type: dict[Symbol: Instrument]
        self._client = client

        if load_all:
            self.load_all()

    async def load_all_async(self):
        await self._client.load_markets(reload=True)
        self._load_instruments()

    cpdef void load_all(self) except *:
        """
        Pre-load all instruments.
        """
        self._client.load_markets(reload=True)
        self._load_instruments()

    cpdef dict get_all(self):
        """
        Get all loaded instruments.

        If no instruments loaded will return the empty dict.

        Returns
        -------
        dict[Symbol, Instrument]

        """
        return self._instruments.copy()

    cpdef Instrument get(self, Symbol symbol):
        """
        Get the instrument for the given symbol (if found).

        Returns
        -------
        Instrument or None

        """
        return self._instruments.get(symbol)

    cdef void _load_instruments(self) except *:
        if self._client.markets is None:
            return  # No markets

        cdef str k
        cdef dict v
        cdef Symbol symbol
        cdef Instrument instrument
        for k, v in self._client.markets.items():
            symbol = Symbol(k, self.venue)
            instrument = self._parse_instrument(symbol, v)

            self._instruments[symbol] = instrument

        self.count = len(self._instruments)

    cdef Instrument _parse_instrument(self, Symbol symbol, dict values):
        # Precisions
        base_precision = values["precision"]["base"]
        quote_precision = values["precision"]["quote"]
        price_precision = values["precision"]["price"]
        size_precision = values["precision"]["amount"]

        base_currency = Currency.from_str_c(values["base"])
        if base_currency is None:
            base_currency = Currency(values["base"], base_precision, CurrencyType.CRYPTO)

        quote_currency = Currency.from_str_c(values["quote"])
        if quote_currency is None:
            quote_currency = Currency(values["quote"], quote_precision, CurrencyType.CRYPTO)

        tick_size = Decimal(f"{values['limits']['amount']['min']:{price_precision}f}")

        lot_size_filter = values["info"]["filters"][2]
        assert lot_size_filter["filterType"] == "LOT_SIZE"
        lot_size = Quantity(lot_size_filter["stepSize"])

        max_quantity = values["limits"]["amount"]["max"]
        if max_quantity is not None:
            max_quantity = Quantity(max_quantity, precision=size_precision)

        min_quantity = values["limits"]["amount"]["min"]
        if min_quantity is not None:
            min_quantity = Quantity(min_quantity, precision=size_precision)

        max_notional = values["limits"]["cost"]["max"]
        if max_notional is not None:
            max_notional = Money(max_notional, currency=quote_currency)

        min_notional = values["limits"]["cost"]["min"]
        if min_notional is not None:
            min_notional = Money(min_notional, currency=quote_currency)

        max_price = values["limits"]["cost"]["max"]
        if max_price is not None:
            max_price = Price(max_price, precision=price_precision)

        min_price = values["limits"]["cost"]["min"]
        if min_price is not None:
            min_price = Price(min_price, precision=price_precision)

        asset_type = AssetTypeParser.from_str(values["type"].upper())

        maker_fee = Decimal(values["maker"])
        taker_fee = Decimal(values["taker"])

        return Instrument(
            symbol=symbol,
            asset_class=AssetClass.CRYPTO,
            asset_type=asset_type,
            base_currency=base_currency,
            quote_currency=quote_currency,
            settlement_currency=quote_currency,
            is_inverse=False,
            price_precision=price_precision,
            size_precision=size_precision,
            tick_size=tick_size,
            multiplier=Decimal(1),
            leverage=Decimal(1),  # TODO: Refactor this out of instrument
            lot_size=lot_size,
            max_quantity=max_quantity,
            min_quantity=min_quantity,
            max_notional=max_notional,
            min_notional=min_notional,
            max_price=max_price,
            min_price=min_price,
            margin_init=Decimal(),         # Margin trading not implemented
            margin_maint=Decimal(),        # Margin trading not implemented
            maker_fee=maker_fee,
            taker_fee=taker_fee,
            financing={},
            timestamp=datetime.utcnow(),
            info=values,
        )
