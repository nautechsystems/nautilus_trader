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

from libc.stdint cimport int64_t

from decimal import Decimal

from cpython.datetime cimport datetime

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
            margin_init=Decimal(0),
            margin_maint=Decimal(0),
            maker_fee=Decimal(0),
            taker_fee=Decimal(0),
            ts_event_ns=ts_event_ns,
            ts_recv_ns=ts_recv_ns,
            info=dict(),  # TODO - Add raw response?
        )

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "type": type(self).__name__,
            "venue_name": self.id.venue.value,
            "event_type_id": self.event_type_id,
            "event_type_name": self.event_type_name,
            "competition_id": self.competition_id,
            "competition_name": self.competition_name,
            "event_id": self.event_id,
            "event_name": self.event_name,
            "event_country_code": self.event_country_code,
            "event_open_date": self.event_open_date,
            "betting_type": self.betting_type,
            "market_id": self.market_id,
            "market_name": self.market_name,
            "market_start_time": self.market_start_time,
            "market_type": self.market_type,
            "selection_id": self.selection_id,
            "selection_name": self.selection_name,
            "selection_handicap": self.selection_handicap,
            "ts_event_ns": self.ts_event_ns,
            "ts_recv_ns": self.ts_recv_ns,
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
        return BettingInstrument(**values)

    def make_symbol(self):
        cdef tuple keys = (
            "event_type_name",
            "competition_name",
            "event_name",
            "event_open_date",
            "betting_type",
            "market_type",
            "market_name",
            "selection_name",
            "selection_handicap",
        )
        return Symbol(value="|".join([str(getattr(self, k)) for k in keys]))
