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

import pandas as pd

from libc.stdint cimport int64_t

from decimal import Decimal

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class BettingInstrument(Instrument):
    """
    Represents an instrument in the betting market.
    """

    def __init__(
        self,
        str venue_name not None,
        str event_type_id not None,
        str event_type_name not None,
        str competition_id not None,
        str competition_name not None,
        str event_id not None,
        str event_name not None,
        str event_country_code not None,
        datetime event_open_date not None,
        str betting_type not None,
        str market_id not None,
        str market_name not None,
        datetime market_start_time not None,
        str market_type not None,
        str selection_id not None,
        str selection_name not None,
        str selection_handicap not None,
        str currency not None,
        int64_t ts_event_ns,
        int64_t ts_recv_ns,
    ):
        assert event_open_date.tzinfo is not None
        assert market_start_time.tzinfo is not None

        # Event type (Sport) info e.g. Basketball
        self.event_type_id = event_type_id
        self.event_type_name = event_type_name

        # Competition e.g. NBA
        self.competition_id = competition_id
        self.competition_name = competition_name

        # Event info e.g. Utah Jazz @ Boston Celtics Wed 17 Mar, 10:40
        self.event_id = event_id
        self.event_name = event_name
        self.event_country_code = event_country_code
        self.event_open_date = event_open_date

        # Market Info e.g. Match odds / Handicap
        self.betting_type = betting_type
        self.market_id = market_id
        self.market_type = market_type
        self.market_name = market_name
        self.market_start_time = market_start_time

        # Selection/Runner (individual selection/runner) e.g. (LA Lakers)
        self.selection_id = selection_id
        self.selection_name = selection_name
        self.selection_handicap = selection_handicap

        super().__init__(
            instrument_id=InstrumentId(symbol=self.make_symbol(), venue=Venue(venue_name)),
            asset_class=AssetClass.BETTING,
            asset_type=AssetType.SPOT,
            quote_currency=Currency.from_str_c(currency),
            is_inverse=False,
            price_precision=5,
            size_precision=4,
            price_increment=Price(1e-5, precision=5),
            size_increment=Quantity(1e-4, precision=4),
            multiplier=Quantity.from_int_c(1),
            lot_size=Quantity.from_int_c(1),
            max_quantity=None,   # Can be None
            min_quantity=None,   # Can be None
            max_notional=None,   # Can be None
            min_notional=Money(5, Currency.from_str_c(currency)),
            max_price=None,      # Can be None
            min_price=None,      # Can be None
            margin_init=Decimal(1),
            margin_maint=Decimal(1),
            maker_fee=Decimal(0),
            taker_fee=Decimal(0),
            ts_event_ns=ts_event_ns,
            ts_recv_ns=ts_recv_ns,
            info=dict(),  # TODO - Add raw response?
        )

    @staticmethod
    cdef BettingInstrument from_dict_c(dict values):
        Condition.not_none(values, "values")
        return BettingInstrument(**values)

    @staticmethod
    cdef dict to_dict_c(BettingInstrument obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "BettingInstrument",
            "venue_name": obj.id.venue.value,
            "event_type_id": obj.event_type_id,
            "event_type_name": obj.event_type_name,
            "competition_id": obj.competition_id,
            "competition_name": obj.competition_name,
            "event_id": obj.event_id,
            "event_name": obj.event_name,
            "event_country_code": obj.event_country_code,
            "event_open_date": obj.event_open_date,
            "betting_type": obj.betting_type,
            "market_id": obj.market_id,
            "market_name": obj.market_name,
            "market_start_time": obj.market_start_time,
            "market_type": obj.market_type,
            "selection_id": obj.selection_id,
            "selection_name": obj.selection_name,
            "selection_handicap": obj.selection_handicap,
            "currency": obj.quote_currency.code,
            "ts_event_ns": obj.ts_event_ns,
            "ts_recv_ns": obj.ts_recv_ns,
        }

    @staticmethod
    def from_dict(dict values) -> BettingInstrument:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        BettingInstrument

        """
        return BettingInstrument.from_dict_c(values)

    @staticmethod
    def to_dict(BettingInstrument obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return BettingInstrument.to_dict_c(obj)

    def make_symbol(self):
        cdef tuple keys = (
            "event_type_name",
            "competition_name",
            "event_id",
            "market_start_time",
            "betting_type",
            "market_type",
            "market_id",
            "selection_id",
            "selection_handicap",
        )

        def _clean(s):
            if isinstance(s, (datetime, pd.Timestamp)):
                return pd.Timestamp(s).tz_convert("UTC").strftime("%Y%m%d-%H%M%S")
            return str(s).replace(' ', '').replace(':', '')

        return Symbol(value=",".join([_clean(getattr(self, k)) for k in keys]))

    cpdef Money notional_value(self, Quantity quantity, price: Decimal, bint inverse_as_quote=False):
        Condition.not_none(quantity, "quantity")
        Condition.type(price, (Decimal, Price), "price")
        bet_price: Decimal = Decimal("1.0") / price
        notional_value: Decimal = quantity * self.multiplier * bet_price
        return Money(notional_value, self.quote_currency)
