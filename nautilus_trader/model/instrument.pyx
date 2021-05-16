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

from cpython.datetime cimport datetime
from libc.stdint cimport int64_t

from decimal import Decimal

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_class cimport AssetClassParser
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.asset_type cimport AssetTypeParser
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Quantity


cdef class Instrument(Data):
    """
    Represents a tradeable financial market instrument.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        AssetClass asset_class,
        AssetType asset_type,
        Currency base_currency,  # Can be None
        Currency quote_currency not None,
        Currency settlement_currency not None,
        bint is_inverse,
        int price_precision,
        int size_precision,
        tick_size not None: Decimal,
        multiplier not None: Decimal,
        Quantity lot_size not None,
        Quantity max_quantity,  # Can be None
        Quantity min_quantity,  # Can be None
        Money max_notional,     # Can be None
        Money min_notional,     # Can be None
        Price max_price,        # Can be None
        Price min_price,        # Can be None
        margin_init not None: Decimal,
        margin_maint not None: Decimal,
        maker_fee not None: Decimal,
        taker_fee not None: Decimal,
        int64_t timestamp_ns,
        dict info=None,
    ):
        """
        Initialize a new instance of the `Instrument` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the instrument.
        asset_class : AssetClass
            The instrument asset class.
        asset_type : AssetType
            The instrument asset type.
        base_currency : Currency, optional
            The base currency. Not applicable for all asset classes.
        quote_currency : Currency
            The quote currency.
        settlement_currency : Currency
            The settlement currency.
        is_inverse : Currency
            If the instrument costing is inverse (quantity expressed in quote currency units).
        price_precision : int
            The price decimal precision.
        size_precision : int
            The trading size decimal precision.
        tick_size : Decimal
            The tick size.
        multiplier : Decimal
            The contract value multiplier.
        lot_size : Quantity
            The rounded lot unit size.
        max_quantity : Quantity
            The maximum possible order quantity.
        min_quantity : Quantity
            The minimum possible order quantity.
        max_notional : Money
            The maximum possible order notional value.
        min_notional : Money
            The minimum possible order notional value.
        max_price : Price
            The maximum possible printed price.
        min_price : Price
            The minimum possible printed price.
        margin_init : Decimal
            The initial margin requirement in percentage of order value.
        margin_maint : Decimal
            The maintenance margin in percentage of position value.
        maker_fee : Decimal
            The fee rate for liquidity makers as a percentage of order value.
        taker_fee : Decimal
            The fee rate for liquidity takers as a percentage of order value.
        timestamp_ns : int64
            The Unix timestamp (nanos) the instrument was created/updated at.
        info : dict[str, object], optional
            The additional instrument information.

        Raises
        ------
        ValueError
            If price_precision is negative (< 0).
        ValueError
            If size_precision is negative (< 0).
        ValueError
            If tick_size is not positive (> 0).
        ValueError
            If multiplier is not positive (> 0).
        ValueError
            If lot size is not positive (> 0).
        ValueError
            If max_quantity is not positive (> 0).
        ValueError
            If min_quantity is negative (< 0).
        ValueError
            If max_notional is not positive (> 0).
        ValueError
            If min_notional is negative (< 0).
        ValueError
            If max_price is not positive (> 0).
        ValueError
            If min_price is negative (< 0).

        """
        Condition.not_negative_int(price_precision, "price_precision")
        Condition.not_negative_int(size_precision, "volume_precision")
        Condition.type(tick_size, Decimal, "tick_size")
        Condition.positive(tick_size, "tick_size")
        Condition.type(multiplier, Decimal, "multiplier")
        Condition.positive(multiplier, "multiplier")
        Condition.positive(lot_size, "lot_size")
        if max_quantity is not None:
            Condition.positive(max_quantity, "max_quantity")
        if min_quantity is not None:
            Condition.not_negative(min_quantity, "min_quantity")
        if max_notional is not None:
            Condition.positive(max_notional, "max_notional")
        if min_notional is not None:
            Condition.not_negative(min_notional, "min_notional")
        if max_price is not None:
            Condition.positive(max_price, "max_price")
        if min_price is not None:
            Condition.not_negative(min_price, "min_price")
        Condition.type(margin_init, Decimal, "margin_init")
        Condition.not_negative(margin_init, "margin_init")
        Condition.type(margin_maint, Decimal, "margin_maint")
        Condition.not_negative(margin_maint, "margin_maint")
        Condition.type(maker_fee, Decimal, "maker_fee")
        Condition.type(taker_fee, Decimal, "taker_fee")
        super().__init__(timestamp_ns)

        self.id = instrument_id
        self.asset_class = asset_class
        self.asset_type = asset_type
        self.base_currency = base_currency  # Can be None
        self.quote_currency = quote_currency
        # Currently not handling quanto settlement
        self.settlement_currency = quote_currency if not is_inverse else base_currency
        self.is_inverse = is_inverse
        self.is_quanto = self._is_quanto(base_currency, quote_currency, settlement_currency)
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.tick_size = tick_size
        self.multiplier = multiplier
        self.lot_size = lot_size
        self.max_quantity = max_quantity
        self.min_quantity = min_quantity
        self.max_notional = max_notional
        self.min_notional = min_notional
        self.max_price = max_price
        self.min_price = min_price
        self.margin_init = margin_init
        self.margin_maint = margin_maint
        self.maker_fee = maker_fee
        self.taker_fee = taker_fee

    @property
    def symbol(self):
        """
        The instruments ticker symbol.

        Returns
        -------
        Symbol

        """
        return self.id.symbol

    @property
    def venue(self):
        """
        The instruments trading venue.

        Returns
        -------
        Venue

        """
        return self.id.venue

    cpdef Price make_price(self, value):
        """
        Create a new price from the given value using the instruments price
        precision.

        Parameters
        ----------
        value : integer, float, str or Decimal
            The value of the price.

        Returns
        -------
        Price

        """
        return Price(float(value), precision=self.price_precision)

    cpdef Quantity make_qty(self, value):
        """
        Create a new quantity from the given value using the instruments size
        precision.

        Parameters
        ----------
        value : integer, float, str or Decimal
            The value of the quantity.

        Returns
        -------
        Quantity

        """
        return Quantity(float(value), precision=self.size_precision)

    cdef bint _is_quanto(
        self,
        Currency base_currency,
        Currency quote_currency,
        Currency settlement_currency,
    ) except *:
        if base_currency is None:
            return False

        return settlement_currency != base_currency and settlement_currency != quote_currency

    def __eq__(self, Instrument other) -> bool:
        return self.id.value == other.id.value

    def __ne__(self, Instrument other) -> bool:
        return self.id.value != other.id.value

    def __hash__(self) -> int:
        return hash(self.id.value)

    def __repr__(self) -> str:
        return (f"{type(self).__name__}"
                f"(id={self.id.value}, "
                f"symbol={self.id.symbol}, "
                f"asset_class={AssetClassParser.to_str(self.asset_class)}, "
                f"asset_type={AssetTypeParser.to_str(self.asset_type)}, "
                f"base_currency={self.base_currency}, "
                f"quote_currency={self.quote_currency}, "
                f"settlement_currency={self.settlement_currency}, "
                f"tick_size={self.tick_size}, "
                f"price_precision={self.price_precision}, "
                f"lot_size={self.lot_size}, "
                f"size_precision={self.size_precision})")


# # TODO: Finish docs
cdef class Future(Instrument):
    """
    Represents a futures contract.
    """

    def __init__(
        self,
        InstrumentId instrument_id not None,
        AssetClass asset_class,
        Currency currency not None,
        str expiry not None,
        int contract_id,
        str local_symbol not None,
        str trading_class not None,
        str market_name not None,
        str long_name not None,
        str contract_month not None,
        str time_zone_id not None,
        str trading_hours not None,
        str liquid_hours not None,
        str last_trade_time not None,
        int multiplier,
        int price_precision,
        tick_size not None: Decimal,
        Quantity lot_size not None,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the `Future` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier.
        asset_class : AssetClass
            The futures contract asset class.
        currency : Currency
            The futures contract currency.
        price_precision : int
            The price decimal precision.
        tick_size : Decimal
            The tick size.
        timestamp_ns : int64
            The timestamp the instrument was created/updated at.

        Raises
        ------
        ValueError
            If multiplier is not positive (> 0).
        ValueError
            If price_precision is negative (< 0).
        ValueError
            If tick_size is not positive (> 0).
        ValueError
            If lot size is not positive (> 0).

        """
        Condition.positive_int(multiplier, "multiplier")
        super().__init__(
            instrument_id=instrument_id,
            asset_class=asset_class,
            asset_type=AssetType.FUTURE,
            base_currency=None,  # N/A
            quote_currency=currency,
            settlement_currency=currency,
            is_inverse=False,
            price_precision=price_precision,
            size_precision=0,  # No fractional contracts
            tick_size=tick_size,
            multiplier=Decimal(multiplier),
            lot_size=lot_size,
            max_quantity=None,
            min_quantity=Quantity(1, precision=0),
            max_notional=None,
            min_notional=None,
            max_price=None,
            min_price=None,
            margin_init=Decimal(),
            margin_maint=Decimal(),
            maker_fee=Decimal(),
            taker_fee=Decimal(),
            timestamp_ns=timestamp_ns,
            info={},
        )

        self.contract_id = contract_id
        self.last_trade_date_or_contract_month = expiry
        self.local_symbol = local_symbol
        self.trading_class = trading_class
        self.market_name = market_name
        self.long_name = long_name
        self.contract_month = contract_month
        self.time_zone_id = time_zone_id
        self.trading_hours = trading_hours
        self.liquid_hours = liquid_hours
        self.last_trade_time = last_trade_time


cdef class BettingInstrument(Instrument):
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
        int64_t timestamp_ns,
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
            base_currency=Currency.from_str_c(currency),
            quote_currency=Currency.from_str_c(currency),
            settlement_currency=Currency.from_str_c(currency),
            is_inverse=False,
            price_precision=5,
            size_precision=4,
            tick_size=Decimal(1),
            multiplier=Decimal(1),
            lot_size=Quantity(1, precision=0),
            max_quantity=None,  # Can be None
            min_quantity=None,  # Can be None
            max_notional=None,     # Can be None
            min_notional=None,     # Can be None
            max_price=None,        # Can be None
            min_price=None,        # Can be None
            margin_init=Decimal(0),
            margin_maint=Decimal(0),
            maker_fee=Decimal(0),
            taker_fee=Decimal(0),
            timestamp_ns=timestamp_ns,
            info=dict(),  # TODO - Add raw response?
        )

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
