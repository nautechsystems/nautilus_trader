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

from decimal import Decimal

from libc.stdint cimport int64_t

import ccxt

from nautilus_trader.common.providers cimport InstrumentProvider
from nautilus_trader.core.functions cimport precision_from_str
from nautilus_trader.core.time cimport unix_timestamp_ns
from nautilus_trader.model.c_enums.currency_type cimport CurrencyType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.crypto_swap cimport CryptoSwap
from nautilus_trader.model.instruments.currency cimport CurrencySpot
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CCXTInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects from a unified CCXT exchange.
    """

    def __init__(self, client not None: ccxt.Exchange, bint load_all=False):
        """
        Initialize a new instance of the ``CCXTInstrumentProvider`` class.

        Parameters
        ----------
        client : ccxt.Exchange
            The client for the provider.
        load_all : bool, optional
            If all instruments should be loaded at instantiation.

        """
        super().__init__()

        self._client = client

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

    cdef void _load_instruments(self) except *:
        cdef str k
        cdef dict v
        cdef InstrumentId instrument_id
        cdef Instrument instrument
        for k, v in self._client.markets.items():
            instrument_id = InstrumentId(Symbol(k.replace(".", "")), self.venue)
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
                    precision=self._get_currency_precision(precision, precision_mode),
                    iso4217=0,
                    name=code,
                    currency_type=currency_type,
                )

            self._currencies[code] = currency

    cdef int _get_currency_precision(self, double value, int mode) except *:
        if mode == 2:  # DECIMAL_PLACE
            return int(value)
        elif mode == 4:  # TICK_SIZE
            return precision_from_str(str(value))

    cdef CurrencyType _parse_currency_type(self, str code):
        return CurrencyType.FIAT if Currency.is_fiat_c(code) else CurrencyType.CRYPTO

    cdef Instrument _parse_instrument(self, InstrumentId instrument_id, dict values):
        cdef bint is_spot = values.get("spot", False)
        cdef bint is_swap = values.get("swap", False)
        cdef bint is_future = values.get("future", False)
        cdef bint is_option = values.get("option", False)
        cdef bint is_inverse = values.get("info", {}).get("isInverse", False)

        cdef dict precisions = values["precision"]
        if self._client.precisionMode == 2:  # DECIMAL_PLACES
            price_precision = int(precisions.get("price"))
            price_increment = Price(1.0 / 10 ** price_precision, precision=price_precision)
            size_precision = int(precisions.get("amount", 8))
            size_increment = Quantity(1.0 / 10 ** size_precision, precision=size_precision)
        elif self._client.precisionMode == 4:  # TICK_SIZE
            price_precision = precision_from_str(str(precisions.get("price")))
            price_increment = Price(precisions.get("price"), precision=price_precision)
            size_precision = precision_from_str(str(precisions.get("amount")).rstrip(".0"))
            amount_prec = precisions.get("amount", 1)
            if amount_prec is None:
                amount_prec = 1
            size_increment = Quantity(float(amount_prec) / 10 ** size_precision, size_precision)
        else:
            raise RuntimeError(
                f"The {self._client.name} exchange is using SIGNIFICANT_DIGITS "
                f"precision which is not currently supported in this version."
            )

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

        settlement_currency = values["info"].get("settlCurrency")
        if settlement_currency is not None and settlement_currency != "":
            if settlement_currency.upper() == "XBT":
                settlement_currency = "BTC"
            settlement_currency = self._currencies[settlement_currency]

        lot_size = values["info"].get("lotSize")
        if lot_size is not None and Decimal(lot_size) > 0:
            lot_size = Quantity(lot_size, precision=size_precision)
        else:
            lot_size = None

        max_quantity = values["limits"].get("amount").get("max")
        if max_quantity is not None:
            max_quantity = Quantity(max_quantity, precision=size_precision)

        min_quantity = values["limits"].get("amount").get("min")
        if min_quantity is not None:
            min_quantity = Quantity(min_quantity, precision=size_precision)

        max_notional = values["limits"].get("cost").get("max")
        if max_notional is not None:
            max_notional = Money(max_notional, currency=quote_currency)

        min_notional = values["limits"].get("cost").get("min")
        if min_notional is not None:
            min_notional = Money(min_notional, currency=quote_currency)

        max_price = values["limits"].get("price").get("max")
        if max_price is not None:
            max_price = Price(max_price, precision=price_precision)

        min_price = values["limits"].get("price").get("min")
        if min_price is not None:
            min_price = Price(min_price, precision=price_precision)

        maker_fee = values.get("maker")
        if maker_fee is None:
            maker_fee = Decimal()
        else:
            maker_fee = Decimal(f"{maker_fee:.4f}")

        taker_fee = values.get("taker")
        if taker_fee is None:
            taker_fee = Decimal()
        else:
            taker_fee = Decimal(f"{taker_fee:.4f}")

        cdef int64_t timestamp = unix_timestamp_ns()

        if is_spot or is_future:  # TODO(cs): Use CurrencySpot for futures for now
            return CurrencySpot(
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
                max_notional=max_notional,
                min_notional=min_notional,
                max_price=max_price,
                min_price=min_price,
                margin_init=Decimal(),   # Margin trading not implemented
                margin_maint=Decimal(),  # Margin trading not implemented
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event_ns=timestamp,
                ts_recv_ns=timestamp,
                info=values,
            )
        elif is_swap:
            return CryptoSwap(
                instrument_id=instrument_id,
                base_currency=base_currency,
                quote_currency=quote_currency,
                settlement_currency=settlement_currency,
                is_inverse=is_inverse,
                price_precision=price_precision,
                size_precision=size_precision,
                price_increment=price_increment,
                size_increment=size_increment,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=max_notional,
                min_notional=min_notional,
                max_price=max_price,
                min_price=min_price,
                margin_init=Decimal(),   # Margin trading not implemented
                margin_maint=Decimal(),  # Margin trading not implemented
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event_ns=timestamp,
                ts_recv_ns=timestamp,
                info=values,
            )
        elif is_option:
            raise RuntimeError("crypto options not supported in this version")
