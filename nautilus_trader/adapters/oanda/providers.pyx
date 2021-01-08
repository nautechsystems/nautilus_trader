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

import oandapyV20
from oandapyV20.endpoints.accounts import AccountInstruments

from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_class cimport AssetClassParser
from nautilus_trader.model.c_enums.currency_type cimport CurrencyType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Quantity


cdef class OandaInstrumentProvider:
    """
    Provides a means of loading Oanda `Instrument` objects.
    """

    def __init__(
        self,
        client not None: oandapyV20.API,
        str account_id not None,
        bint load_all=False,
    ):
        """
        Initialize a new instance of the `OandaInstrumentProvider` class.

        Parameters
        ----------
        client : oandapyV20.API
            The Oanda client.
        account_id : str
            The Oanda account identifier.
        load_all : bool, optional
            If all instruments should be loaded at instantiation.

        """
        self.venue = Venue("OANDA")
        self.count = 0
        self._instruments = {}  # type: dict[Symbol: Instrument]
        self._client = client
        self._account_id = account_id

        if load_all:
            self.load_all()

    cpdef void load_all(self) except *:
        """
        Load all instruments for the venue.
        """
        req = AccountInstruments(accountID=self._account_id)
        res = self._client.request(req)

        cdef list instruments = res.get("instruments", {})
        cdef dict values
        cdef Instrument instrument
        for values in instruments:
            instrument = self._parse_instrument(values)
            self._instruments[instrument.symbol] = instrument

        self.count = len(self._instruments)

    cpdef dict get_all(self):
        """
        Return all loaded instruments.

        If no instruments loaded, will return an empty dict.

        Returns
        -------
        dict[Symbol, Instrument]

        """
        return self._instruments.copy()

    cpdef Instrument get(self, Symbol symbol):
        """
        Return the instrument for the given symbol (if found).

        Returns
        -------
        Instrument or None

        """
        return self._instruments.get(symbol)

    cdef Instrument _parse_instrument(self, dict values):
        cdef str oanda_name = values["name"]
        cdef str oanda_type = values["type"]
        cdef list symbol_pieces = values["name"].split('_', maxsplit=1)

        cdef Symbol symbol = Symbol(oanda_name.replace('_', '/', 1), self.venue)
        cdef Currency base_currency = None
        cdef Currency quote_currency = Currency(symbol_pieces[1], 2, CurrencyType.FIAT)

        if oanda_type == "CURRENCY":
            asset_class = AssetClass.FX
            asset_type = AssetType.SPOT
            base_currency = Currency(symbol_pieces[0], 2, CurrencyType.FIAT)
        elif oanda_type == "METAL":
            asset_class = AssetClass.COMMODITY
            asset_type = AssetType.SPOT
        else:
            asset_class = AssetClassParser.from_str(values["tags"][0]["name"])
            asset_type = AssetType.CFD

        cdef int price_precision = int(values["displayPrecision"])
        cdef int size_precision = int(values["tradeUnitsPrecision"])

        tick_size: Decimal = Decimal(f"{1.0 / 10 ** price_precision:.{price_precision}f}")

        # TODO: Depends on account currency (refactor)
        maker_fee: Decimal = Decimal("0.00025")
        taker_fee: Decimal = Decimal("0.00025")

        return Instrument(
            symbol=symbol,
            asset_class=asset_class,
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
            lot_size=Quantity(1),
            max_quantity=Quantity(values["maximumOrderUnits"]),
            min_quantity=Quantity(values["minimumTradeSize"]),
            max_notional=None,
            min_notional=None,
            max_price=None,
            min_price=None,
            margin_init=Decimal(values["marginRate"]),
            margin_maint=Decimal(values["marginRate"]),
            maker_fee=maker_fee,
            taker_fee=taker_fee,
            financing=values.get("financing", {}),
            timestamp=datetime.utcnow(),
            info=values,
        )
