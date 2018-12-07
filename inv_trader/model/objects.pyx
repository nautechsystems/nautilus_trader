#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="objects.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from datetime import datetime
from decimal import Decimal

from inv_trader.core.precondition import Precondition
from inv_trader.model.enums import Venue, Resolution, QuoteType, SecurityType, CurrencyCode


cdef class Symbol:
    """
    Represents the symbol for a financial market tradeable instrument.
    """
    cdef str _code
    cdef object _venue

    def __init__(self,
                 code: str,
                 venue: Venue):
        """
        Initializes a new instance of the Symbol class.

        :param code: The symbols code.
        :param venue: The symbols venue.
        :raises ValueError: If the code is not a valid string.
        """
        Precondition.valid_string(code, 'code')

        self._code = code.upper()
        self._venue = venue

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.code == other.code and self.venue == other.venue
        else:
            return False

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash((self._code, self._venue))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the symbol.
        """
        return str(f"{self._code}.{self._venue.name}")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the symbol.
        """
        return str(f"<{str(self)} object at {id(self)}>")

    @property
    def code(self) -> str:
        """
        :return: The symbols code.
        """
        return self._code

    @property
    def venue(self) -> Venue:
        """
        :return: The symbols venue.
        """
        return self._venue


cdef class Price:
    """
    Provides a factory for creating Decimal objects representing price.
    """

    @staticmethod
    def create(double price,
               int decimals):
        """
        Creates and returns a new price from the given values.
        The price is rounded to the given decimal digits.

        :param price: The price value.
        :param decimals: The decimal precision of the price.
        :return: A Decimal representing the price.
        :raises ValueError: If the price is not positive (> 0).
        :raises ValueError: If the decimals is negative.
        """
        Precondition.positive(price, 'price')
        Precondition.not_negative(decimals, 'decimals')

        return Decimal(f'{round(price, decimals):.{decimals}f}')


cdef class Money:
    """
    Provides a factory for creating Decimal objects representing money.
    """

    @staticmethod
    def zero() -> Decimal:
        """
        Creates and returns a new zero amount of money.

        :return:
        """
        return Decimal('0.00')

    @staticmethod
    def create(amount: float) -> Decimal:
        """
        Creates and returns money from the given values.
        The money is rounded to two decimal digits.

        :param amount: The money amount.
        :return: A Decimal representing the money.
        :raises ValueError: If the amount is not positive (> 0).
        """
        Precondition.positive(amount, 'amount')

        return Decimal(f'{round(amount, 2):.{2}f}')


cdef class Tick:
    """
    Represents a single tick in a financial market.
    """
    cdef object _symbol
    cdef object _bid
    cdef object _ask
    cdef object _timestamp

    def __init__(self,
                 symbol: Symbol,
                 bid: Decimal,
                 ask: Decimal,
                 timestamp: datetime):
        """
        Initializes a new instance of the Tick class.

        :param symbol: The tick symbol.
        :param bid: The tick best bid price.
        :param ask: The tick best ask price.
        :param timestamp: The tick timestamp (UTC).
        :raises ValueError: If the bid is not positive (> 0).
        :raises ValueError: If the ask is not positive (> 0).
        """
        Precondition.positive(bid, 'bid')
        Precondition.positive(ask, 'ask')

        self._symbol = symbol
        self._bid = bid
        self._ask = ask
        self._timestamp = timestamp

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return (self._symbol == other.symbol
                    and self._bid == other.bid
                    and self._ask == other.ask
                    and self._timestamp == other.timestamp)
        else:
            return False

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(self._timestamp)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the tick.
        """
        return str(f"Tick({self._symbol},{self._bid},{self._ask},"
                f"{self._timestamp.isoformat()})")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the tick.
        """
        return str(f"<{str(self)} object at {id(self)}>")

    @property
    def symbol(self) -> Symbol:
        """
        :return: The ticks symbol.
        """
        return self._symbol

    @property
    def bid(self) -> Decimal:
        """
        :return: The ticks bid price.
        """
        return self._bid

    @property
    def ask(self) -> Decimal:
        """
        :return: The ticks ask price.
        """
        return self._ask

    @property
    def timestamp(self) -> datetime:
        """
        :return: The ticks timestamp (UTC).
        """
        return self._timestamp


class BarType:
    """
    Represents a financial market symbol and bar specification.
    """

    def __init__(self,
                 symbol: Symbol,
                 period: int,
                 resolution: Resolution,
                 quote_type: QuoteType):
        """
        Initializes a new instance of the BarType class.

        :param symbol: The bar symbol.
        :param period: The bar period.
        :param resolution: The bar resolution.
        :param quote_type: The bar quote type.
        :raises ValueError: If the period is not positive (> 0).
        """
        Precondition.positive(period, 'period')

        self._symbol = symbol
        self._period = period
        self._resolution = resolution
        self._quote_type = quote_type

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.__dict__ == other.__dict__
        else:
            return False

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash((self._symbol, self._period, self._resolution, self._quote_type))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the bar type.
        """
        return str(f"{str(self._symbol)}"
                f"-{self._period}-{self._resolution.name}[{self._quote_type.name}]")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the bar type.
        """
        return str(f"<{str(self)} object at {id(self)}>")

    @property
    def symbol(self) -> Symbol:
        """
        :return: The bar types symbol.
        """
        return self._symbol

    @property
    def period(self) -> int:
        """
        :return: The bar types period.
        """
        return self._period

    @property
    def resolution(self) -> Resolution:
        """
        :return: The bar types resolution.
        """
        return self._resolution

    @property
    def quote_type(self) -> QuoteType:
        """
        :return: The bar types quote type.
        """
        return self._quote_type


cdef class Bar:
    """
    Represents a financial market trade bar.
    """
    cdef object _open
    cdef object _high
    cdef object _low
    cdef object _close
    cdef int _volume
    cdef object _timestamp

    def __init__(self,
                 open_price: Decimal,
                 high_price: Decimal,
                 low_price: Decimal,
                 close_price: Decimal,
                 int volume,
                 timestamp: datetime):
        """
        Initializes a new instance of the Bar class.

        :param open_price: The bars open price.
        :param high_price: The bars high price.
        :param low_price: The bars low price.
        :param close_price: The bars close price.
        :param volume: The bars volume.
        :param timestamp: The bars timestamp (UTC).
        :raises ValueError: If the open_price is not positive (> 0).
        :raises ValueError: If the high_price is not positive (> 0).
        :raises ValueError: If the low_price is not positive (> 0).
        :raises ValueError: If the close_price is not positive (> 0).
        :raises ValueError: If the volume is negative.
        """
        Precondition.positive(open_price, 'open_price')
        Precondition.positive(high_price, 'high_price')
        Precondition.positive(low_price, 'low_price')
        Precondition.positive(close_price, 'close_price')
        Precondition.not_negative(volume, 'volume')

        self._open = open_price
        self._high = high_price
        self._low = low_price
        self._close = close_price
        self._volume = volume
        self._timestamp = timestamp

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self._timestamp == other.timestamp
        else:
            return False

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(str(self._timestamp))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the bar.
        """
        return str(f"Bar({self._open},{self._high},{self._low},{self._close},"
                f"{self._volume},{self._timestamp.isoformat()})")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the bar.
        """
        return str(f"<{str(self)} object at {id(self)}>")

    @property
    def open(self) -> Decimal:
        """
        :return: The bars open price.
        """
        return self._open

    @property
    def high(self) -> Decimal:
        """
        :return: The bars high price.
        """
        return self._high

    @property
    def low(self) -> Decimal:
        """
        :return: The bars low price.
        """
        return self._low

    @property
    def close(self) -> Decimal:
        """
        :return: The bars close price.
        """
        return self._close

    @property
    def volume(self) -> int:
        """
        :return: The bars volume (tick volume).
        """
        return self._volume

    @property
    def timestamp(self) -> datetime:
        """
        :return: The bars timestamp (UTC).
        """
        return self._timestamp


cdef class DataBar:
    """
    Represents a financial market trade bar.
    """
    cdef readonly double open
    cdef readonly double high
    cdef readonly double low
    cdef readonly double close
    cdef readonly double volume
    cdef readonly object timestamp

    def __cinit__(self,
                 double open_price,
                 double high_price,
                 double low_price,
                 double close_price,
                 double volume,
                 object timestamp):
        """
        Initializes a new instance of the Bar class.

        :param open_price: The bars open price.
        :param high_price: The bars high price.
        :param low_price: The bars low price.
        :param close_price: The bars close price.
        :param volume: The bars volume.
        :param timestamp: The bars timestamp (UTC).
        :raises ValueError: If the open_price is not positive (> 0).
        :raises ValueError: If the high_price is not positive (> 0).
        :raises ValueError: If the low_price is not positive (> 0).
        :raises ValueError: If the close_price is not positive (> 0).
        :raises ValueError: If the volume is negative.
        """
        self.open = open_price
        self.high = high_price
        self.low = low_price
        self.close = close_price
        self.volume = volume
        self.timestamp = timestamp

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.open == other.open
        else:
            return False

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(str(self.timestamp))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the bar.
        """
        return str(f"Bar({self.open},{self.high},{self.low},{self.close},"
                f"{self.volume},{self.timestamp.isoformat()})")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the bar.
        """
        return str(f"<{str(self)} object at {id(self)}>")


class Instrument:
    """
    Represents a tradeable financial market instrument.
    """

    def __init__(self,
                 symbol: Symbol,
                 broker_symbol: str,
                 quote_currency: CurrencyCode,
                 security_type: SecurityType,
                 tick_decimals: int,
                 tick_size: Decimal,
                 tick_value: Decimal,
                 target_direct_spread: Decimal,
                 round_lot_size: int,
                 contract_size: int,
                 min_stop_distance_entry: int,
                 min_limit_distance_entry: int,
                 min_stop_distance: int,
                 min_limit_distance: int,
                 min_trade_size: int,
                 max_trade_size: int,
                 margin_requirement: Decimal,
                 rollover_interest_buy: Decimal,
                 rollover_interest_sell: Decimal,
                 timestamp: datetime):
        """
        Initializes a new instance of the Instrument class.

        :param symbol: The instruments symbol.
        :param broker_symbol: The instruments broker symbol.
        :param quote_currency: The instruments quote currency.
        :param security_type: The instruments security type.
        :param tick_decimals: The instruments tick decimal digits precision.
        :param tick_size: The instruments tick size.
        :param tick_value: The instruments tick value.
        :param target_direct_spread: The instruments target direct spread (set by broker).
        :param round_lot_size: The instruments rounded lot size.
        :param contract_size: The instruments contract size if applicable.
        :param min_stop_distance_entry: The instruments minimum distance for stop entry orders.
        :param min_limit_distance_entry: The instruments minimum distance for limit entry orders.
        :param min_stop_distance: The instruments minimum tick distance for stop orders.
        :param min_limit_distance: The instruments minimum tick distance for limit orders.
        :param min_trade_size: The instruments minimum trade size.
        :param max_trade_size: The instruments maximum trade size.
        :param margin_requirement: The instruments margin requirement per unit.
        :param rollover_interest_buy: The instruments rollover interest for long positions.
        :param rollover_interest_sell: The instruments rollover interest for short positions.
        :param timestamp: The timestamp the instrument was created/updated at.
        """
        Precondition.valid_string(broker_symbol, 'broker_symbol')
        Precondition.not_negative(tick_decimals, 'tick_decimals')
        Precondition.positive(tick_size, 'tick_size')
        Precondition.positive(tick_value, 'tick_value')
        Precondition.not_negative(target_direct_spread, 'target_direct_spread')
        Precondition.positive(contract_size, 'contract_size')
        Precondition.not_negative(min_stop_distance_entry, 'min_stop_distance_entry')
        Precondition.not_negative(min_limit_distance_entry, 'min_limit_distance_entry')
        Precondition.not_negative(min_stop_distance, 'min_stop_distance')
        Precondition.not_negative(min_limit_distance, 'min_limit_distance')
        Precondition.not_negative(min_limit_distance, 'min_limit_distance')
        Precondition.positive(min_trade_size, 'min_trade_size')
        Precondition.positive(max_trade_size, 'max_trade_size')
        Precondition.not_negative(margin_requirement, 'margin_requirement')

        self._symbol = symbol
        self._broker_symbol = broker_symbol
        self._quote_currency = quote_currency
        self._security_type = security_type
        self._tick_decimals = tick_decimals
        self._tick_size = tick_size
        self._tick_value = tick_value
        self._target_direct_spread = target_direct_spread
        self._round_lot_size = round_lot_size
        self._contract_size = contract_size
        self._min_stop_distance_entry = min_stop_distance_entry
        self._min_limit_distance_entry = min_limit_distance_entry
        self._min_stop_distance = min_stop_distance
        self._min_limit_distance = min_limit_distance
        self._min_trade_size = min_trade_size
        self._max_trade_size = max_trade_size
        self._margin_requirement = margin_requirement
        self._rollover_interest_buy = rollover_interest_buy
        self._rollover_interest_sell = rollover_interest_sell
        self._timestamp = timestamp

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.__dict__ == other.__dict__
        else:
            return False

    def __ne__(self, other) -> bool:
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Override the default hash implementation.
        """
        return hash(str(self._symbol))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the instrument.
        """
        return str(f"Instrument({self._symbol.code}.{self._symbol.venue})")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the instrument.
        """
        return str(f"<{str(self)} object at {id(self)}>")

    @property
    def symbol(self) -> Symbol:
        """
        :return: The instruments symbol.
        """
        return self._symbol

    @property
    def broker_symbol(self) -> str:
        """
        :return: The instruments broker symbol.
        """
        return self._broker_symbol

    @property
    def quote_currency(self) -> str:
        """
        :return: The instruments quote currency.
        """
        return self._quote_currency.name

    @property
    def security_type(self) -> str:
        """
        :return: The instruments security type.
        """
        return self._security_type.name

    @property
    def tick_decimals(self) -> int:
        """
        :return: The instruments tick decimal digits precision.
        """
        return self._tick_decimals

    @property
    def tick_size(self) -> Decimal:
        """
        :return: The instruments tick size.
        """
        return self._tick_size

    @property
    def tick_value(self) -> Decimal:
        """
        :return: The instruments tick value.
        """
        return self._tick_value

    @property
    def target_direct_spread(self) -> Decimal:
        """
        :return: The instruments target direct spread (set by broker).
        """
        return self._target_direct_spread

    @property
    def round_lot_size(self) -> int:
        """
        :return: The instruments rounded lot size.
        """
        return self._round_lot_size

    @property
    def contract_size(self) -> int:
        """
        :return: The instruments contract size.
        """
        return self._contract_size

    @property
    def min_stop_distance_entry(self) -> int:
        """
        :return: The instruments minimum tick distance for stop entry orders.
        """
        return self._min_stop_distance_entry

    @property
    def min_limit_distance_entry(self) -> int:
        """
        :return: The instruments minimum tick distance for limit entry orders.
        """
        return self._min_limit_distance_entry

    @property
    def min_stop_distance(self) -> int:
        """
        :return: The instruments minimum tick distance for stop orders.
        """
        return self._min_stop_distance

    @property
    def min_limit_distance(self) -> int:
        """
        :return: The instruments minimum tick distance for limit orders.
        """
        return self._min_limit_distance

    @property
    def min_trade_size(self) -> int:
        """
        :return: The instruments minimum trade size.
        """
        return self._min_trade_size

    @property
    def max_trade_size(self) -> int:
        """
        :return: The instruments maximum trade size.
        """
        return self._max_trade_size

    @property
    def margin_requirement(self) -> Decimal:
        """
        :return: The instruments margin requirement.
        """
        return self._margin_requirement

    @property
    def rollover_interest_buy(self) -> Decimal:
        """
        :return: The instruments rollover interest for long positions.
        """
        return self._rollover_interest_buy

    @property
    def rollover_interest_sell(self) -> Decimal:
        """
        :return: The instruments rollover interest for short positions.
        """
        return self._rollover_interest_sell

    @property
    def timestamp(self) -> datetime:
        """
        :return: The timestamp the instrument was created/updated at.
        """
        return self._timestamp
