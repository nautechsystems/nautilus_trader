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

from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.asset_type cimport AssetTypeParser
from nautilus_trader.model.c_enums.currency_type cimport CurrencyType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CCXTInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects from a unified CCXT exchange.
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
        super().__init__()

        self._client = client
        self._currencies = {}  # type: dict[str, Currency]

        self.venue = Venue(client.name.upper())

        if load_all:
            self.load_all()

    async def load_all_async(self):
        """
        Load all instruments for the venue asynchronously.
        """
        await self._client.load_markets(reload=True)
        self._load_currencies()
        self._load_instruments()

    cpdef void load_all(self) except *:
        """
        Load all instruments for the venue.
        """
        self._client.load_markets(reload=True)
        self._load_currencies()
        self._load_instruments()

    cpdef Currency currency(self, str code):
        """
        Return the currency with the given code (if found).

        Parameters
        ----------
        code : str
            The currency code.

        Returns
        -------
        Currency or None

        """
        return self._currencies.get(code)

    cdef void _load_instruments(self) except *:
        cdef str k
        cdef dict v
        cdef InstrumentId instrument_id
        cdef Instrument instrument
        for k, v in self._client.markets.items():
            instrument_id = InstrumentId(Symbol(k), self.venue)
            instrument = self._parse_instrument(instrument_id, v)
            if instrument is None:
                continue  # Something went wrong in parsing

            self._instruments[instrument_id] = instrument

    cdef void _load_currencies(self) except *:
        cdef int precision_mode = self._client.precisionMode

        cdef str code
        cdef dict values
        cdef Currency currency
        for code, values in self._client.currencies.items():
            currency_type = self._parse_currency_type(code)
            currency = Currency.from_str_c(code)
            if currency is None:
                precision = values.get("precision")
                if precision is None:
                    continue
                currency = Currency(
                    code=code,
                    precision=self._get_precision(precision, precision_mode),
                    iso4217=0,
                    name=code,
                    currency_type=currency_type,
                )

            self._currencies[code] = currency

    cdef inline int _tick_size_to_precision(self, double tick_size) except *:
        cdef tick_size_str = f"{tick_size:f}"
        return len(tick_size_str.partition('.')[2].rstrip('0'))

    cdef inline int _get_precision(self, double value, int mode) except *:
        if mode == 2:  # DECIMAL_PLACE
            return int(value)
        elif mode == 4:  # TICK_SIZE
            return self._tick_size_to_precision(value)

    cdef inline CurrencyType _parse_currency_type(self, str code):
        return CurrencyType.FIAT if Currency.is_fiat_c(code) else CurrencyType.CRYPTO

    cdef Instrument _parse_instrument(self, InstrumentId instrument_id, dict values):
        # Precisions
        cdef dict precisions = values["precision"]
        if self._client.precisionMode == 2:  # DECIMAL_PLACES
            price_precision = precisions.get("price")
            size_precision = precisions.get("amount", 8)
            tick_size = Decimal(f"{1.0 / 10 ** price_precision:.{price_precision}f}")
        elif self._client.precisionMode == 4:  # TICK_SIZE
            tick_size = Decimal(precisions.get("price"))
            price_precision = self._tick_size_to_precision(tick_size)
            size_precision = precisions.get("amount")
            if size_precision is None:
                size_precision = 0
            size_precision = self._tick_size_to_precision(size_precision)
        else:
            raise RuntimeError(f"The {self._client.name} exchange is using "
                               f"SIGNIFICANT_DIGITS precision which is not "
                               f"currently supported in this version.")

        cdef str asset_type_str = values.get("type")
        if asset_type_str is not None:
            asset_type = AssetTypeParser.from_str(asset_type_str.upper())
        else:
            asset_type = AssetType.UNDEFINED

        base_currency = values.get("base")
        if base_currency is not None:
            base_currency = Currency.from_str_c(values["base"])
            if base_currency is None:
                base_currency = self._currencies.get(values["base"])
                if base_currency is None:
                    return None

        quote_currency = Currency.from_str_c(values["quote"])
        if quote_currency is None:
            quote_currency = self._currencies[values["quote"]]

        max_quantity = values["limits"].get("amount").get("max")
        if max_quantity is not None:
            max_quantity = Quantity(max_quantity, precision=size_precision)

        min_quantity = values["limits"].get("amount").get("min")
        if min_quantity is not None:
            min_quantity = Quantity(min_quantity, precision=size_precision)

        lot_size = values["info"].get("lotSize")
        if lot_size is not None:
            lot_size = Quantity(lot_size)
        elif min_quantity is not None:
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

        maker_fee = values.get("maker")
        if maker_fee is None:
            maker_fee = Decimal()
        else:
            maker_fee = Decimal(maker_fee)

        taker_fee = values.get("taker")
        if taker_fee is None:
            taker_fee = Decimal()
        else:
            taker_fee = Decimal(taker_fee)

        cdef bint is_inverse = values.get("info", {}).get("isInverse", False)

        return Instrument(
            instrument_id=instrument_id,
            asset_class=AssetClass.CRYPTO,
            asset_type=asset_type,
            base_currency=base_currency,
            quote_currency=quote_currency,
            settlement_currency=quote_currency,
            is_inverse=is_inverse,
            price_precision=price_precision,
            size_precision=size_precision,
            tick_size=tick_size,
            multiplier=Decimal(1),
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
