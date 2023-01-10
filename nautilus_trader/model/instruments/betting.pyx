# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import pandas as pd

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport AssetClass
from nautilus_trader.core.rust.model cimport AssetType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity

from nautilus_trader.adapters.betfair.parsing.common import betfair_instrument_id


cdef class BettingInstrument(Instrument):
    """
    Represents an instrument in a betting market.
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
        str currency not None,
        str selection_handicap,
        uint64_t ts_event,
        uint64_t ts_init,
        str tick_scheme_name="BETFAIR",
        int price_precision=7,  # TODO(bm): pending refactor
        Price min_price = None,
        Price max_price = None,
        dict info = {},
    ):
        assert event_open_date.tzinfo or market_start_time.tzinfo is not None

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
        self.event_open_date = pd.Timestamp(event_open_date).tz_convert("UTC")

        # Market Info e.g. Match odds / Handicap
        self.betting_type = betting_type
        self.market_id = market_id
        self.market_type = market_type
        self.market_name = market_name
        self.market_start_time = pd.Timestamp(market_start_time).tz_convert("UTC")

        # Selection/Runner (individual selection/runner) e.g. (LA Lakers)
        self.selection_id = selection_id
        self.selection_name = selection_name
        self.selection_handicap = selection_handicap
        instrument_id = betfair_instrument_id(
            market_id=market_id, runner_id=selection_id, runner_handicap=selection_handicap
        )

        super().__init__(
            instrument_id=instrument_id,
            native_symbol=instrument_id.symbol,
            asset_class=AssetClass.SPORTS_BETTING,
            asset_type=AssetType.SPOT,
            quote_currency=Currency.from_str_c(currency),
            is_inverse=False,
            size_precision=4,
            price_precision=price_precision,
            price_increment=None,
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
            ts_event=ts_event,
            ts_init=ts_init,
            tick_scheme_name=tick_scheme_name,
            info=info,
        )
        if not min_price and tick_scheme_name:
            self.min_price = self._tick_scheme.min_price
        if not max_price and tick_scheme_name:
            self.max_price = self._tick_scheme.max_price

    @staticmethod
    cdef BettingInstrument from_dict_c(dict values):
        Condition.not_none(values, "values")
        data = values.copy()
        data['event_open_date'] = pd.Timestamp(data['event_open_date'])
        data['market_start_time'] = pd.Timestamp(data['market_start_time'])
        return BettingInstrument(**{k: v for k, v in data.items() if k not in ('id',)})

    @staticmethod
    cdef dict to_dict_c(BettingInstrument obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "BettingInstrument",
            "id": obj.id.to_str(),
            "venue_name": obj.id.venue.to_str(),
            "event_type_id": obj.event_type_id,
            "event_type_name": obj.event_type_name,
            "competition_id": obj.competition_id,
            "competition_name": obj.competition_name,
            "event_id": obj.event_id,
            "event_name": obj.event_name,
            "event_country_code": obj.event_country_code,
            "event_open_date": obj.event_open_date.isoformat(),
            "betting_type": obj.betting_type,
            "market_id": obj.market_id,
            "market_name": obj.market_name,
            "market_start_time": obj.market_start_time.isoformat(),
            "market_type": obj.market_type,
            "selection_id": obj.selection_id,
            "selection_name": obj.selection_name,
            "selection_handicap": obj.selection_handicap,
            "currency": obj.quote_currency.code,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
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

    cpdef Money notional_value(self, Quantity quantity, Price price, bint inverse_as_quote=False):
        Condition.not_none(quantity, "quantity")
        cdef double bet_price = 1.0 / price.as_f64_c()
        return Money(quantity.as_f64_c() * float(self.multiplier) * bet_price, self.quote_currency)
