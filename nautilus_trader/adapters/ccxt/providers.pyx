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

from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.asset_type cimport AssetTypeParser
from nautilus_trader.model.c_enums.currency_type cimport CurrencyType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CCXTInstrumentProvider:
    """
    Provides a means of loading `Instrument` from a unified CCXT exchange.
    """

    def __init__(self, client not None: ccxt.Exchange, bint load_all=False):
        """
        Initialize a new instance of the `CCXTInstrumentProvider` class.

        Parameters
        ----------
        client : ccxt.Exchange
            The client for the provider.
        load_all : bool, optional
            If all instruments should be loaded at instantiation.

        """
        self.venue = Venue(client.name.upper())
        self.count = 0
        self._instruments = {}  # type: dict[Symbol: Instrument]
        self._client = client

        if load_all:
            self.load_all()

    cpdef void load_all(self) except *:
        """
        Pre-load all instruments.
        """
        self._client.load_markets(reload=True)

        if self._client.markets is None:
            return  # No markets

        cdef str k
        cdef dict v
        cdef Symbol symbol
        cdef Instrument instrument
        for k, v in self._client.markets.items():
            symbol = Symbol(k, self.venue)
            try:
                instrument = self._parse_instrument(symbol, v)
            except Exception as ex:
                print(f"Exception on parsing {symbol.code} instrument: {ex}")
                continue

            self._instruments[symbol] = instrument

        self.count = len(self._instruments)

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

    cdef Instrument _parse_instrument(self, Symbol symbol, dict values):
        # Precisions
        base_precision = values["precision"].get("base", 8)
        quote_precision = values["precision"].get("quote", 8)
        price_precision = values["precision"].get("price")
        size_precision = values["precision"].get("amount", 8)

        base_currency = Currency.from_str_c(values["base"])
        if base_currency is None:
            base_currency = Currency(values["base"], base_precision, CurrencyType.CRYPTO)

        quote_currency = Currency.from_str_c(values["quote"])
        if quote_currency is None:
            quote_currency = Currency(values["quote"], quote_precision, CurrencyType.CRYPTO)

        tick_size = Decimal(f"{values['limits']['amount']['min']:{price_precision}f}")

        max_quantity = values["limits"].get("amount").get("max")
        if max_quantity is not None:
            max_quantity = Quantity(max_quantity, precision=size_precision)

        min_quantity = values["limits"].get("amount").get("min")
        if min_quantity is not None:
            min_quantity = Quantity(min_quantity, precision=size_precision)
            lot_size = Quantity(min_quantity, precision=size_precision)
        else:
            lot_size = Quantity(1)

        max_notional = values["limits"].get("cost").get("max")
        if max_notional is not None:
            max_notional = Money(max_notional, currency=quote_currency)

        min_notional = values["limits"].get("cost").get("min")
        if min_notional is not None:
            min_notional = Money(min_notional, currency=quote_currency)

        max_price = values["limits"].get("cost").get("max")
        if max_price is not None:
            max_price = Price(max_price, precision=price_precision)

        min_price = values["limits"].get("cost").get("min")
        if min_price is not None:
            min_price = Price(min_price, precision=price_precision)

        if values.get("type") is not None:
            asset_type = AssetTypeParser.from_str(values["type"].upper())
        else:
            asset_type = AssetType.UNDEFINED

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
            leverage=Decimal(1),
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
