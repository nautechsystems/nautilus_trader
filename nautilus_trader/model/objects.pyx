# -------------------------------------------------------------------------------------------------
# <copyright file="objects.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal
from cpython.datetime cimport datetime, timedelta

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.types cimport StringValue
from nautilus_trader.model.objects cimport Venue
from nautilus_trader.model.c_enums.resolution cimport Resolution, resolution_string
from nautilus_trader.model.c_enums.quote_type cimport QuoteType, quote_type_string
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.c_enums.currency cimport Currency


cdef Quantity ZERO_QUANTITY = Quantity(0)


cdef class Quantity:
    """
    Represents a non-negative integer quantity.
    """

    def __init__(self, long value):
        """
        Initializes a new instance of the Quantity class.

        :param value: The value of the quantity (>= 0).
        :raises ConditionFailed: If the value is negative (< 0).
        """
        Condition.not_negative(value, 'value')

        self.value = value

    @staticmethod
    def zero() -> Quantity:
        """
        Return a quantity of zero.
        
        :return: Quantity.
        """
        return ZERO_QUANTITY

    cdef bint equals(self, Quantity other):
        """
        Return a value indicating whether the object equals the given object.
        
        :param other: The other string to compare
        :return: True if the objects are equal, otherwise False.
        """
        return self.value == other.value

    def __eq__(self, Quantity other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.

        :return: bool.

        :return: bool.
        """
        return self.equals(other)

    def __ne__(self, Quantity other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.

        :return: bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash representation of this object.

        :return: int.
        """
        return hash(self.value)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the quantity.
        """
        return str(self.value)

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the quantity.
        """
        return f"<{self.__class__.__name__}({self.value}) object at {id(self)}>"

    def __lt__(self, Quantity other) -> bool:
        return self.value < other.value

    def __le__(self, Quantity other) -> bool:
        return self.value <= other.value

    def __eq__(self, Quantity other) -> bool:
        return self.value == other.value

    def __ne__(self, Quantity other) -> bool:
        return self.value != other.value

    def __gt__(self, Quantity other) -> bool:
        return self.value > other.value

    def __ge__(self, Quantity other) -> bool:
        return self.value >= other.value

    def __add__(self, other) -> int:
        if isinstance(other, Quantity):
            return self.value + other.value
        elif isinstance(other, long):
            return self.value + other
        else:
            raise NotImplementedError(f"Cannot add {type(other)} to a quantity.")

    def __sub__(self, other) -> int:
        if isinstance(other, Quantity):
            return self.value - other.value
        elif isinstance(other, long):
            return self.value - other
        else:
            raise NotImplementedError(f"Cannot subtract {type(other)} from a quantity.")


cdef class Brokerage(StringValue):
    """
    Represents a brokerage.
    """

    def __init__(self, str name):
        """
        Initializes a new instance of the Brokerage class.

        :param name: The brokerages name.
        :raises ConditionFailed: If the name is not a valid string.
        """
        super().__init__(name.upper())


cdef class Venue(StringValue):
    """
    Represents a trading venue for a financial market tradeable instrument.
    """

    def __init__(self, str name):
        """
        Initializes a new instance of the Venue class.

        :param name: The venues name.
        :raises ConditionFailed: If the name is not a valid string.
        """
        super().__init__(name.upper())


cdef class Exchange(Venue):
    """
    Represents an exchange that financial market instruments are traded on.
    """

    def __init__(self, str name):
        """
        Initializes a new instance of the Exchange class.

        :param name: The exchanges name.
        :raises ConditionFailed: If the name is not a valid string.
        """
        super().__init__(name.upper())


cdef class Symbol:
    """
    Represents the symbol for a financial market tradeable instrument.
    """

    def __init__(self,
                 str code,
                 Venue venue):
        """
        Initializes a new instance of the Symbol class.

        :param code: The symbols code.
        :param venue: The symbols venue.
        :raises ConditionFailed: If the code is not a valid string.
        """
        Condition.valid_string(code, 'code')

        self.code = code.upper()
        self.venue = venue
        self.value = f'{self.code}.{self.venue.value}'

    cdef bint equals(self, Symbol other):
        """
        Return a value indicating whether the object equals the given object.
        
        :param other: The other object to compare
        :return: True if the objects are equal, otherwise False.
        """
        return self.value == other.value

    def __eq__(self, Symbol other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.

        :return: bool.
        """
        return self.equals(other)

    def __ne__(self, Symbol other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.

        :return: bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash representation of this object.

        :return: int.
        """
        return hash((self.code, self.venue))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the symbol.
        """
        return self.value

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the symbol.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"


cdef inline str _get_decimal_str(float value, int precision):
    return f'{value:.{precision}f}'


cdef inline int _get_precision(str value):
    cdef tuple partitioned
    if value.__contains__('.'):
        partitioned = value.rpartition('.')
        return len(partitioned[2])
    else:
        return 0


cdef class Price:
    """
    Represents a financial market price.
    """

    def __init__(self, object value, int precision=0):
        """
        Initializes a new instance of the Price class.

        :param value: The value of the price (> 0).
        Note: Can be str, float, int or Decimal only.
        :raises TypeError: If the value is not a str, float, int or Decimal.
        :raises InvalidOperation: If the value str is malformed.
        :raises ConditionFailed: If the value is not positive (> 0).
        :raises ConditionFailed: If the value is int or float and the precision is not positive (> 0).
        """
        if isinstance(value, str):
            self.value = Decimal(value)
            self.precision = _get_precision(value)
        elif isinstance(value, float):
            Condition.positive(precision, 'precision')
            self.value = Decimal(_get_decimal_str(value, precision))
            self.precision = precision
        elif isinstance(value, int):
            Condition.positive(precision, 'precision')
            self.value = Decimal(_get_decimal_str(float(value), precision))
            self.precision = precision
        elif isinstance(value, Decimal):
            self.value = value
            self.precision = _get_precision(str(value))
        else:
            raise TypeError(f'Cannot initialize a Price with a {type(value)}.')

        if self.value <= 0:
            raise ValueError('the value of the price was not positive')

    def __eq__(self, Price other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.

        :return: bool.
        """
        return self.equals(other)

    def __ne__(self, Price other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.

        :return: bool.
        """
        return not self.equals(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the price.
        """
        return str(self.value)

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the symbol.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"

    def __lt__(self, Price other) -> bool:
        return self.value < other.value

    def __le__(self, Price other) -> bool:
        return self.value <= other.value

    def __eq__(self, Price other) -> bool:
        return self.value == other.value

    def __ne__(self, Price other) -> bool:
        return self.value != other.value

    def __gt__(self, Price other) -> bool:
        return self.value > other.value

    def __ge__(self, Price other) -> bool:
        return self.value >= other.value

    def __add__(self, other) -> Decimal:
        if isinstance(other, float):
            return Decimal(_get_decimal_str(float(self.value) + other, self.precision))
        elif isinstance(other, Decimal):
            return Decimal(_get_decimal_str(float(self.value) + float(other), self.precision))
        elif isinstance(other, Price):
            return Decimal(_get_decimal_str(float(self.value) + other.as_float(), self.precision))
        else:
            raise NotImplementedError(f"Cannot add {type(other)} to a price.")

    def __sub__(self, other) -> Decimal:
        if isinstance(other, float):
            return Decimal(_get_decimal_str(float(self.value) - other, self.precision))
        elif isinstance(other, Decimal):
            return Decimal(_get_decimal_str(float(self.value) - float(other), self.precision))
        elif isinstance(other, Price):
            return Decimal(_get_decimal_str(float(self.value) - other.as_float(), self.precision))
        else:
            raise NotImplementedError(f"Cannot subtract {type(other)} from a price.")

    def __truediv__(self, other) -> Decimal:
        if isinstance(other, float):
            return Decimal(_get_decimal_str(float(self.value) / other, self.precision))
        elif isinstance(other, Decimal):
            return Decimal(_get_decimal_str(float(self.value) / float(other), self.precision))
        elif isinstance(other, Price):
            return Decimal(_get_decimal_str(float(self.value) / other.as_float(), self.precision))
        else:
            raise NotImplementedError(f"Cannot divide price by {type(other)}.")

    def __mul__(self, other) -> Decimal:
        if isinstance(other, float):
            return Decimal(_get_decimal_str(float(self.value) * other, self.precision))
        elif isinstance(other, Decimal):
            return Decimal(_get_decimal_str(float(self.value) * float(other), self.precision))
        elif isinstance(other, Price):
            return Decimal(_get_decimal_str(float(self.value) * other.as_float(), self.precision))
        else:
            raise NotImplementedError(f"Cannot multiply price with {type(other)}.")

    cdef bint equals(self, Price other):
        """
        Return a value indicating whether the object equals the given object.
        
        :param other: The other string to compare
        :return: True if the objects are equal, otherwise False.
        """
        return self.value == other.value and self.precision == other.precision

    cpdef Price add(self, Price price):
        """
        Return a new price by adding the given price to this price.

        :param price: The other price to add.
        :return: Price.
        :raises ConditionFailed: If the precision of the prices are not equal.
        """
        Condition.true(self.precision == price.precision, 'self.precision == price.precision')

        return Price(self.value + price.value)

    cpdef Price subtract(self, Price price):
        """
        Return a new price by subtracting the given price from this price.

        :param price: The other price to subtract.
        :return: Price.
        :raises ConditionFailed: If the precision of the prices are not equal.
        """
        Condition.true(self.precision == price.precision, 'self.precision == price.precision')

        return Price(self.value - price.value)

    cpdef float as_float(self):
        """
        Return a float representation of the price.
        
        :return: float.
        """
        return float(self.value)


cdef Money ZERO_MONEY = Money(Decimal('0.00'))


cdef class Money:
    """
    Represents money.
    """

    def __init__(self, object value):
        """
        Initializes a new instance of the Money class.

        :param value: The value of the money.
        Note: Value is rounded to 2 decimal places of precision.
        """
        if isinstance(value, str):
            self.value = Decimal(f'{float(value):.2f}')
        else:
            self.value = Decimal(f'{value:.2f}')

    @staticmethod
    def zero() -> Money:
        """
        Return money with a zero amount.
        
        :return: Money.
        """
        return ZERO_MONEY

    def __eq__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.

        :return: bool.
        """
        return self.equals(other)

    def __ne__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.

        :return: bool.
        """
        return not self.equals(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the price.
        """
        return f'{self.value:,.2f}'

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the symbol.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"

    def __lt__(self, Money other) -> bool:
        return self.value < other.value

    def __le__(self, Money other) -> bool:
        return self.value <= other.value

    def __eq__(self, Money other) -> bool:
        return self.value == other.value

    def __ne__(self, Money other) -> bool:
        return self.value != other.value

    def __gt__(self, Money other) -> bool:
        return self.value > other.value

    def __ge__(self, Money other) -> bool:
        return self.value >= other.value

    def __add__(self, other) -> Money:
        if isinstance(other, Money):
            return Money(self.value + other.value)
        elif isinstance(other, Decimal):
            return Money(self.value + other)
        elif isinstance(other, int):
            return self + Money(other)
        else:
            raise NotImplementedError(f"Cannot add {type(other)} to money.")

    def __sub__(self, other) -> Money:
        if isinstance(other, Money):
            return Money(self.value - other.value)
        elif isinstance(other, Decimal):
            return Money(self.value - other)
        elif isinstance(other, int):
            return self - Money(other)
        else:
            raise NotImplementedError(f"Cannot subtract {type(other)} from money.")

    def __truediv__(self, other) -> Money:
        if isinstance(other, Money):
            return Money(self.value / other.value)
        elif isinstance(other, Decimal):
            return Money(self.value / other)
        elif isinstance(other, int):
            return self / Money(other)
        else:
            raise NotImplementedError(f"Cannot divide money by {type(other)}.")

    def __mul__(self, other) -> Money:
        if isinstance(other, Money):
            return Money(self.value * other.value)
        elif isinstance(other, Decimal):
            return Money(self.value * other)
        elif isinstance(other, int):
            return self * Money(other)
        else:
            raise NotImplementedError(f"Cannot multiply money with {type(other)}.")

    cdef bint equals(self, Money other):
        """
        Return a value indicating whether the object equals the given object.
        
        :param other: The other string to compare
        :return: True if the objects are equal, otherwise False.
        """
        return self.value == other.value

    cpdef float as_float(self):
        """
        Return a float representation of the money.
        
        :return: float.
        """
        return float(self.value)


cdef class Tick:
    """
    Represents a single tick in a financial market.
    """

    def __init__(self,
                 Symbol symbol,
                 Price bid,
                 Price ask,
                 datetime timestamp):
        """
        Initializes a new instance of the Tick class.

        :param symbol: The tick symbol.
        :param bid: The tick best bid price.
        :param ask: The tick best ask price.
        :param timestamp: The tick timestamp (UTC).
        :raises ConditionFailed: If the bid price is not positive (> 0).
        :raises ConditionFailed: If the ask price is not positive (> 0).
        """
        self.symbol = symbol
        self.bid = bid
        self.ask = ask
        self.timestamp = timestamp

    def __lt__(self, Tick other) -> bool:
        return self.timestamp < other.timestamp

    def __le__(self, Tick other) -> bool:
        return self.timestamp <= other.timestamp

    def __eq__(self, Tick other) -> bool:
        return self.timestamp == other.timestamp

    def __ne__(self, Tick other) -> bool:
        return self.timestamp != other.timestamp

    def __gt__(self, Tick other) -> bool:
        return self.timestamp > other.timestamp

    def __ge__(self, Tick other) -> bool:
        return self.timestamp >= other.timestamp

    def __cmp__(self, Tick other) -> int:
        """
        Override the default comparison.
        """
        if self.timestamp < other.timestamp:
            return -1
        elif self.timestamp == other.timestamp:
            return 0
        else:
            return 1

    def __hash__(self) -> int:
        """"
        Return the hash representation of this object.

        :return: int.
        """
        return hash(self.timestamp)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the tick.
        """
        return f"{self.bid},{self.ask},{self.timestamp.isoformat()}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the tick.
        """
        return f"<{self.__class__.__name__}({self.symbol},{str(self)}) object at {id(self)}>"


cdef class BarSpecification:
    """
    Represents the specification of a financial market trade bar.
    """
    def __init__(self,
                 int period,
                 Resolution resolution,
                 QuoteType quote_type):
        """
        Initializes a new instance of the BarSpecification class.

        :param period: The bar period.
        :param resolution: The bar resolution.
        :param quote_type: The bar quote type.
        :raises ConditionFailed: If the period is not positive (> 0).
        """
        Condition.positive(period, 'period')

        self.period = period
        self.resolution = resolution
        self.quote_type = quote_type

    cpdef timedelta timedelta(self):
        """
        Return the time bar timedelta.
        :return: timedelta.
        """
        if self.resolution == Resolution.TICK:
            return timedelta(0)
        if self.resolution == Resolution.SECOND:
            return timedelta(seconds=self.period)
        if self.resolution == Resolution.MINUTE:
            return timedelta(minutes=self.period)
        if self.resolution == Resolution.HOUR:
            return timedelta(hours=self.period)
        if self.resolution == Resolution.DAY:
            return timedelta(days=self.period)
        else:
            raise RuntimeError(f"Cannot calculate timedelta for {resolution_string(self.resolution)}")

    cdef bint equals(self, BarSpecification other):
        """
        Return a value indicating whether the object equals the given object.
        
        :param other: The other object to compare
        :return: True if the objects are equal, otherwise False.
        """
        return (self.period == other.period
                and self.resolution == other.resolution
                and self.quote_type == other.quote_type)

    cdef str resolution_string(self):
        """
        Return the resolution as a string
        
        :return: str.
        """
        return resolution_string(self.resolution)

    cdef str quote_type_string(self):
        """
        Return the quote type as a string.
        
        :return: str.
        """
        return quote_type_string(self.quote_type)

    def __eq__(self, BarSpecification other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.

        :return: bool.
        """
        return self.equals(other)

    def __ne__(self, BarSpecification other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.

        :return: bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash representation of this object.

        :return: int.
        """
        return hash((self.period, self.resolution, self.quote_type))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the bar type.
        """
        return f"{self.period}-{resolution_string(self.resolution)}[{quote_type_string(self.quote_type)}]"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the bar type.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"


cdef class BarType:
    """
    Represents a financial market symbol and bar specification.
    """

    def __init__(self,
                 Symbol symbol,
                 BarSpecification bar_spec):
        """
        Initializes a new instance of the BarType class.

        :param symbol: The bar symbol.
        :param bar_spec: The bar specification.
        """
        self.symbol = symbol
        self.specification = bar_spec

    cdef str resolution_string(self):
        """
        Return the resolution as a string
        
        :return: str.
        """
        return self.specification.resolution_string()

    cdef str quote_type_string(self):
        """
        Return the quote type as a string.
        
        :return: str.
        """
        return self.specification.quote_type_string()

    cdef bint equals(self, BarType other):
        """
        Return a value indicating whether the object equals the given object.
        
        :param other: The other object to compare
        :return: True if the objects are equal, otherwise False.
        """
        return self.symbol.equals(other.symbol) and self.specification.equals(other.specification)

    def __eq__(self, BarType other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.

        :return: bool.
        """
        return self.equals(other)

    def __ne__(self, BarType other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.

        :return: bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash representation of this object.

        :return: int.
        """
        return hash((self.symbol, self.specification))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the bar type.
        """
        return f"{str(self.symbol)}-{self.specification}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the bar type.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"


cdef class Bar:
    """
    Represents a financial market trade bar.
    """

    def __init__(self,
                 Price open_price,
                 Price high_price,
                 Price low_price,
                 Price close_price,
                 long volume,
                 datetime timestamp,
                 bint checked=False):
        """
        Initializes a new instance of the Bar class.

        :param open_price: The bars open price.
        :param high_price: The bars high price.
        :param low_price: The bars low price.
        :param close_price: The bars close price.
        :param volume: The bars volume.
        :param timestamp: The bars timestamp (UTC).
        :param checked: A value indicating whether the bar was checked valid.
        :raises ConditionFailed: If checked is true and the volume is negative.
        :raises ConditionFailed: If checked is true and the high_price is not >= low_price.
        :raises ConditionFailed: If checked is true and the high_price is not >= close_price.
        :raises ConditionFailed: If checked is true and the low_price is not <= close_price.
        """
        if checked:
            Condition.not_negative(volume, 'volume')
            Condition.true(high_price >= low_price, 'high_price >= low_price')
            Condition.true(high_price >= close_price, 'high_price >= close_price')
            Condition.true(low_price <= close_price, 'low_price <= close_price')

        self.open = open_price
        self.high = high_price
        self.low = low_price
        self.close = close_price
        self.volume = volume
        self.timestamp = timestamp
        self.checked = checked

    def __eq__(self, Bar other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.

        :return: bool.
        """
        return self.timestamp == other.timestamp

    def __ne__(self, Bar other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.

        :return: bool.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Return the hash representation of this object.

        :return: int.
        """
        return hash(str(self.timestamp))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the bar.
        """
        return (f"{self.open},{self.high},{self.low},{self.close},"
                f"{self.volume},{self.timestamp.isoformat()}")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the bar.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"


cdef class DataBar:
    """
    Represents a financial market trade bar.
    """

    def __init__(self,
                 float open_price,
                 float high_price,
                 float low_price,
                 float close_price,
                 float volume,
                 datetime timestamp):
        """
        Initializes a new instance of the DataBar class.

        :param open_price: The bars open price.
        :param high_price: The bars high price.
        :param low_price: The bars low price.
        :param close_price: The bars close price.
        :param volume: The bars volume.
        :param timestamp: The bars timestamp (UTC).
        :raises ConditionFailed: If the open_price is not positive (> 0).
        :raises ConditionFailed: If the high_price is not positive (> 0).
        :raises ConditionFailed: If the low_price is not positive (> 0).
        :raises ConditionFailed: If the close_price is not positive (> 0).
        :raises ConditionFailed: If the volume is negative.
        """
        self.open = open_price
        self.high = high_price
        self.low = low_price
        self.close = close_price
        self.volume = volume
        self.timestamp = timestamp

    def __eq__(self, DataBar other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.
        """
        return self.open == other.open

    def __ne__(self, DataBar other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.:return: bool.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Return the hash representation of this object.

        :return: int.
        """
        return hash(str(self.timestamp))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the bar.
        """
        return (f"{self.open},{self.high},{self.low},{self.close},"
                f"{self.volume},{self.timestamp.isoformat()}")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the bar.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"


cdef class Instrument:
    """
    Represents a tradeable financial market instrument.
    """

    def __init__(self,
                 InstrumentId instrument_id,
                 Symbol symbol,
                 str broker_symbol,
                 Currency quote_currency,
                 SecurityType security_type,
                 int tick_precision,
                 object tick_size,
                 Quantity round_lot_size,
                 int min_stop_distance_entry,
                 int min_stop_distance,
                 int min_limit_distance_entry,
                 int min_limit_distance,
                 Quantity min_trade_size,
                 Quantity max_trade_size,
                 object rollover_interest_buy,
                 object rollover_interest_sell,
                 datetime timestamp):
        """
        Initializes a new instance of the Instrument class.

        :param symbol: The instruments identifier.
        :param symbol: The instruments symbol.
        :param broker_symbol: The instruments broker symbol.
        :param quote_currency: The instruments quote currency.
        :param security_type: The instruments security type.
        :param tick_precision: The instruments tick decimal digits precision.
        :param tick_size: The instruments tick size.
        :param round_lot_size: The instruments rounded lot size.
        :param min_stop_distance_entry: The instruments minimum distance for stop entry orders.
        :param min_stop_distance: The instruments minimum tick distance for stop orders.
        :param min_limit_distance_entry: The instruments minimum distance for limit entry orders.
        :param min_limit_distance: The instruments minimum tick distance for limit orders.
        :param min_trade_size: The instruments minimum trade size.
        :param max_trade_size: The instruments maximum trade size.
        :param rollover_interest_buy: The instruments rollover interest for long positions.
        :param rollover_interest_sell: The instruments rollover interest for short positions.
        :param timestamp: The timestamp the instrument was created/updated at.
        """
        Condition.valid_string(broker_symbol, 'broker_symbol')
        Condition.not_negative(tick_precision, 'tick_precision')
        Condition.positive(tick_size, 'tick_size')
        Condition.not_negative(min_stop_distance_entry, 'min_stop_distance_entry')
        Condition.not_negative(min_limit_distance_entry, 'min_limit_distance_entry')
        Condition.not_negative(min_stop_distance, 'min_stop_distance')
        Condition.not_negative(min_limit_distance, 'min_limit_distance')
        Condition.not_negative(min_limit_distance, 'min_limit_distance')
        Condition.positive(min_trade_size.value, 'min_trade_size')
        Condition.positive(max_trade_size.value, 'max_trade_size')

        self.id = instrument_id
        self.symbol = symbol
        self.broker_symbol = broker_symbol
        self.quote_currency = quote_currency
        self.security_type = security_type
        self.tick_precision = tick_precision
        self.tick_size = tick_size
        self.round_lot_size = round_lot_size
        self.min_stop_distance_entry = min_stop_distance_entry
        self.min_stop_distance = min_stop_distance
        self.min_limit_distance_entry = min_limit_distance_entry
        self.min_limit_distance = min_limit_distance
        self.min_trade_size = min_trade_size
        self.max_trade_size = max_trade_size
        self.rollover_interest_buy = rollover_interest_buy
        self.rollover_interest_sell = rollover_interest_sell
        self.timestamp = timestamp

    def __eq__(self, Instrument other) -> bool:
        """
        Return a value indicating whether this object is equal to the given object.
        """
        return self.id == other.id

    def __ne__(self, Instrument other) -> bool:
        """
        Return a value indicating whether this object is not equal to the given object.:return: bool.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Return the hash representation of this object.

        :return: int.
        """
        return hash(str(self.symbol))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the instrument.
        """
        return f"{self.__class__.__name__}({self.symbol})"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the instrument.
        """
        return f"<{str(self)} object at {id(self)}>"
