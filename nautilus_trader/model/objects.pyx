# -------------------------------------------------------------------------------------------------
# <copyright file="objects.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

"""Define common trading model value objects."""

import re
import pandas as pd
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.c_enums.bar_structure cimport (
    BarStructure,
    bar_structure_to_string,
    bar_structure_from_string
)
from nautilus_trader.model.c_enums.price_type cimport (
    PriceType,
    price_type_to_string,
    price_type_from_string)
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.c_enums.currency cimport Currency, currency_from_string
from nautilus_trader.model.identifiers cimport Venue


cdef Quantity _ZERO_QUANTITY = Quantity()

cdef class Quantity:
    """
    Represents a quantity with non-negative integer value.

    Attributes
    ----------
    value : int
        The absolute value of the quantity.
    """

    def __init__(self, long value=0):
        """
        Initializes a new instance of the Quantity class.

        Parameters
        ----------
        value : long
            The value of the quantity (>= 0).

        Raises
        ------
        ValueError
            If the value is negative (< 0).

        """
        Condition.not_negative_int(value, 'value')

        self.value = value

    @staticmethod
    cdef Quantity zero():
        """
        Returns
        -------
        Quantity
            A quantity of zero.

        """
        return _ZERO_QUANTITY

    cpdef bint equals(self, Quantity other):
        """
        Check if this object is equal to (==) the given object.

        Parameters
        ----------
        other : Quantity
            The other quantity to equal.
        
        Returns
        -------
        bool
            True if the other quantity is equal, else False.
            
        """
        return self.value == other.value

    cpdef str to_string(self, bint format_commas=False):
        """
        Return the formatted string representation of this object.
        
        Parameters
        ----------
        format_commas : bool 
            If the string should be formatted with commas separating thousands.
        
        Returns
        -------
        str
        
        """
        if format_commas:
            return f'{self.value:,}'
        else:
            return str(self.value)

    def __eq__(self, other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.value == <int?>other
        except TypeError:
            return self.value == <int>other.value

    def __ne__(self, other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.value != <int?>other
        except TypeError:
            return self.value != <int>other.value

    def __lt__(self, other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.value < <int?>other
        except TypeError:
            return self.value < <int>other.value

    def __le__(self, other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.value <= <int?>other
        except TypeError:
            return self.value <= <int>other.value

    def __gt__(self, other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.value > <int?>other
        except TypeError:
            return self.value > <int>other.value

    def __ge__(self, other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.value >= <int?>other
        except TypeError:
            return self.value >= <int>other.value

    def __add__(self, other) -> int:
        """
        Return the result of adding the given object to this object.

        :param other: The other object.
        :return int.
        """
        try:
            return self.value + <int?>other
        except TypeError:
            return self.value + <int>other.value

    def __sub__(self, other) -> int:
        """
        Return the result of subtracting the given object from this object.

        :param other: The other object.
        :return int.
        """
        try:
            return self.value - <int?>other
        except TypeError:
            return self.value - <int>other.value

    def __idiv__(self, other) -> int:
        """
        Return the result of dividing this object by the given object.

        :param other: The other object.
        :return int.
        """
        try:
            return self.value // <int?>other
        except TypeError:
            return self.value // <int>other.value

    def __truediv__(self, other) -> int:
        """
        Return the result of dividing this object by the given object.

        :param other: The other object.
        :return int.
        """
        try:
            return self.value // <int?>other
        except TypeError:
            return self.value // <int>other.value

    def __mul__(self, other) -> int:
        """
        Return the result of multiplying this object by the given object.

        :param other: The other object.
        :return int.
        """
        try:
            return self.value * <int?>other
        except TypeError:
            return self.value * <int>other.value

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.value)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.value}) object at {id(self)}>"


cdef class Price(Decimal):
    """
    Represents a price of a financial market instrument.
    """

    def __init__(self, double value, int precision):
        """
        Initializes a new instance of the Price class.

        :param value: The value of the price (>= 0).
        :param precision: The decimal precision of the price (> 0).
        :raises ValueError: If the value is negative (< 0).
        :raises ValueError: If the precision is not positive (> 0).
        """
        Condition.not_negative(value, 'value')

        super().__init__(value, precision)

    @staticmethod
    cdef Price from_string(str value):
        """
        Return a price from the given string. Precision will be inferred from the
        number of digits after the decimal place.

        :param value: The string value to parse.
        :return: Price.
        """
        Condition.valid_string(value, 'value')

        return Price(float(value), precision=Decimal.precision_from_string(value))

    cpdef Price add(self, Decimal other):
        """
        Return a new price by adding the given decimal to this price.

        :param other: The other price to add (precision must be <= this decimals precision).
        :raises ValueError: If the precision of the given decimal is not <= this precision.
        :return Price.
        """
        Condition.true(self.precision >= other.precision, 'self.precision >= price.precision')

        return Price(self._value + other._value, self.precision)

    cpdef Price subtract(self, Decimal other):
        """
        Return a new price by subtracting the decimal price from this price.

        :param other: The other decimal to subtract (precision must be <= this decimals precision).
        :raises ValueError: If price precision is < the other decimal precision.
        :raises ValueError: If value of the other decimal is greater than this price.
        :return Price.
        """
        Condition.true(self.precision >= other.precision, 'self.precision >= price.precision')

        return Price(self._value - other._value, self.precision)

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.to_string()}, precision={self.precision}) object at {id(self)}>"


cdef Money _ZERO_MONEY = Money()

cdef class Money(Decimal):
    """
    Represents the 'concept' of money.
    """

    def __init__(self, double value=0.0):
        """
        Initializes a new instance of the Money class.
        Note: The value is rounded to 2 decimal places of precision.

        :param value: The value of the money.
        """
        super().__init__(value, precision=2)

    @staticmethod
    cdef Money zero():
        """
        Return money with a zero value.
        
        :return Money.
        """
        return _ZERO_MONEY

    @staticmethod
    cdef Money from_string(str value):
        """
        Return money parsed from the given string value.
        
        :param value: The string value to parse.
        :return Money.
        """
        Condition.valid_string(value, 'value')

        return Money(float(value))

    cpdef Money add(self, Money other):
        """
        Return a new price by adding the given price to this price.

        :param other: The other money to add.
        :return Price.
        :raises ValueError: If the precision of the prices are not equal.
        """
        return Money(self._value + other._value)

    cpdef Money subtract(self, Money other):
        """
        Return a new price by subtracting the given price from this price.

        :param other: The other price to subtract.
        :return Price.
        :raises ValueError: If the precision of the prices are not equal.
        """
        return Money(self._value - other._value)

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.to_string()}) object at {id(self)}>"


cdef class Tick:
    """
    Represents a single tick in a financial market.
    """

    def __init__(self,
                 Symbol symbol not None,
                 Price bid not None,
                 Price ask not None,
                 datetime timestamp not None,
                 TickType tick_type=TickType.TRADE,
                 double bid_size=1.0,
                 double ask_size=1.0):
        """
        Initializes a new instance of the Tick class.

        :param symbol: The tick symbol.
        :param bid: The tick best bid price.
        :param ask: The tick best ask price.
        :param timestamp: The tick timestamp (UTC).
        :param tick_type: The tick type (default=TRADE).
        :param bid_size: The tick bid size (default=1.0).
        :param ask_size: The tick ask size (default=1.0).
        :raises ValueError: If tick_type is UNDEFINED.
        :raises ValueError: If the bid_size is negative (< 0).
        :raises ValueError: If the ask_size is negative (< 0).
        """
        Condition.not_equal(tick_type, TickType.UNDEFINED, 'tick_type', 'UNDEFINED')
        Condition.not_negative(bid_size, 'bid_size')
        Condition.not_negative(ask_size, 'ask_size')

        self.type = TickType.TRADE
        self.symbol = symbol
        self.bid = bid
        self.ask = ask
        self.bid_size = bid_size
        self.ask_size = ask_size
        self.timestamp = timestamp

    @staticmethod
    cdef Tick from_string_with_symbol(Symbol symbol, str values):
        """
        Return a tick parsed from the given symbol and values string.

        :param symbol: The tick symbol.
        :param values: The tick values string.
        :return Tick.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.valid_string(values, 'values')

        cdef list split_values = values.split(',', maxsplit=2)
        return Tick(
            symbol,
            Price.from_string(split_values[0]),
            Price.from_string(split_values[1]),
            pd.to_datetime(split_values[2]))

    @staticmethod
    cdef Tick from_string(str value):
        """
        Return a tick parsed from the given value string.

        :param value: The tick value string to parse.
        :return Tick.
        """
        Condition.valid_string(value, 'value')

        cdef list split_values = value.split(',', maxsplit=3)
        return Tick(
            Symbol.from_string(split_values[0]),
            Price.from_string(split_values[1]),
            Price.from_string(split_values[2]),
            pd.to_datetime(split_values[3]))

    @staticmethod
    def py_from_string_with_symbol(Symbol symbol, str values) -> Tick:
        """
        Python wrapper for the from_string_with_symbol method.

        Return a tick parsed from the given symbol and values string.

        :param symbol: The tick symbol.
        :param values: The tick values string.
        :return Tick.
        """
        return Tick.from_string_with_symbol(symbol, values)

    @staticmethod
    def py_from_string(str values) -> Tick:
        """
        Python wrapper for the from_string method.

        Return a tick parsed from the given values string.

        :param values: The tick values string.
        :return Tick.
        """
        return Tick.from_string(values)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        return f"{self.bid},{self.ask},{self.timestamp.isoformat()}"

    def __eq__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.
        Note: The equality is based on the ticks timestamp only.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp == other.timestamp

    def __ne__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.
        Note: The equality is based on the ticks timestamp only.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp != other.timestamp

    def __lt__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.
        Note: The equality is based on the ticks timestamp only.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp < other.timestamp

    def __le__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.
        Note: The equality is based on the ticks timestamp only.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp <= other.timestamp

    def __gt__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.
        Note: The equality is based on the ticks timestamp only.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp > other.timestamp

    def __ge__(self, Tick other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.
        Note: The equality is based on the ticks timestamp only.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp >= other.timestamp

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.timestamp)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.symbol},{self.to_string()}) object at {id(self)}>"


cdef class BarSpecification:
    """
    Represents the specification of a financial market trade bar.
    """
    def __init__(self,
                 int step,
                 BarStructure structure,
                 PriceType price_type):
        """
        Initializes a new instance of the BarSpecification class.

        :param step: The bar step (> 0).
        :param structure: The bar structure.
        :param price_type: The bar price type.
        :raises ValueError: If the step is not positive (> 0).
        :raises ValueError: If the price type is LAST.
        """
        Condition.positive_int(step, 'step')
        Condition.true(price_type != PriceType.LAST, 'price_type != PriceType.LAST')

        self.step = step
        self.structure = structure
        self.price_type = price_type

    @staticmethod
    cdef BarSpecification from_string(str value):
        """
        Return a bar specification parsed from the given string.
        Note: String format example is '200-TICK-[MID]'.
        
        :param value: The bar specification string to parse.
        :return BarSpecification.
        """
        Condition.valid_string(value, 'value')

        cdef list split1 = value.split('-', maxsplit=2)
        cdef list split2 = split1[1].split('[', maxsplit=1)
        cdef str structure = split2[0]
        cdef str price_type = split2[1].strip(']')

        return BarSpecification(
            int(split1[0]),
            bar_structure_from_string(structure),
            price_type_from_string(price_type))

    @staticmethod
    def py_from_string(str value) -> BarSpecification:
        """
        Python wrapper for the from_string method.

        Return a bar specification parsed from the given string.
        Note: String format example is '1-MINUTE-[BID]'.

        :param value: The bar specification string to parse.
        :return BarSpecification.
        """
        return BarSpecification.from_string(value)

    cdef str structure_string(self):
        """
        Return the bar structure as a string
        
        :return str.
        """
        return bar_structure_to_string(self.structure)

    cdef str price_type_string(self):
        """
        Return the price type as a string.
        
        :return str.
        """
        return price_type_to_string(self.price_type)

    cpdef bint equals(self, BarSpecification other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return (self.step == other.step
                and self.structure == other.structure
                and self.price_type == other.price_type)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        return f"{self.step}-{bar_structure_to_string(self.structure)}[{price_type_to_string(self.price_type)}]"

    def __eq__(self, BarSpecification other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, BarSpecification other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
         Return the hash code of this object.

        :return int.
        """
        return hash((self.step, self.structure, self.price_type))

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.to_string()}) object at {id(self)}>"


cdef class BarType:
    """
    Represents a financial market symbol and bar specification.
    """

    def __init__(self,
                 Symbol symbol not None,
                 BarSpecification bar_spec not None):
        """
        Initializes a new instance of the BarType class.

        :param symbol: The bar symbol.
        :param bar_spec: The bar specification.
        """
        self.symbol = symbol
        self.specification = bar_spec

    @staticmethod
    cdef BarType from_string(str value):
        """
        Return a bar type parsed from the given string.

        :param value: The bar type string to parse.
        :return BarType.
        """
        Condition.valid_string(value, 'value')

        cdef list split_string = re.split(r'[.-]+', value)
        cdef str structure = split_string[3].split('[', maxsplit=1)[0]
        cdef str price_type = split_string[3].split('[', maxsplit=1)[1].strip(']')
        cdef Symbol symbol = Symbol(split_string[0], Venue(split_string[1]))
        cdef BarSpecification bar_spec = BarSpecification(
            int(split_string[2]),
            bar_structure_from_string(structure.upper()),
            price_type_from_string(price_type.upper()))

        return BarType(symbol, bar_spec)

    @staticmethod
    def py_from_string(str value) -> BarType:
        """
        Python wrapper for the from_string method.

        Return a bar type parsed from the given string.

        :param value: The bar type string to parse.
        :return BarType.
        """
        return BarType.from_string(value)

    cdef str structure_string(self):
        """
        Return the bar structure as a string
        
        :return str.
        """
        return self.specification.structure_string()

    cdef str price_type_string(self):
        """
        Return the price type as a string.
        
        :return str.
        """
        return self.specification.price_type_string()

    cpdef bint equals(self, BarType other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.symbol.equals(other.symbol) and self.specification.equals(other.specification)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        return f"{self.symbol.to_string()}-{self.specification}"

    def __eq__(self, BarType other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, BarType other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash((self.symbol, self.specification))

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.to_string()}) object at {id(self)}>"


cdef class Bar:
    """
    Represents a financial market trade bar.
    """

    def __init__(self,
                 Price open_price not None,
                 Price high_price not None,
                 Price low_price not None,
                 Price close_price not None,
                 double volume,
                 datetime timestamp not None,
                 bint check=False):
        """
        Initializes a new instance of the Bar class.

        :param open_price: The bars open price.
        :param high_price: The bars high price.
        :param low_price: The bars low price.
        :param close_price: The bars close price.
        :param volume: The bars volume (>= 0).
        :param timestamp: The bars timestamp (UTC).
        :param check: If the bar parameters should be checked valid.
        :raises ValueError: If check and the volume is negative (< 0).
        :raises ValueError: If check and the high_price is not >= low_price.
        :raises ValueError: If check and the high_price is not >= close_price.
        :raises ValueError: If check and the low_price is not <= close_price.
        """
        Condition.not_negative(volume, 'volume')

        if check:
            Condition.true(high_price.ge(low_price), 'high_price >= low_price')
            Condition.true(high_price.ge(close_price), 'high_price >= close_price')
            Condition.true(low_price.le(close_price), 'low_price <= close_price')

        self.open = open_price
        self.high = high_price
        self.low = low_price
        self.close = close_price
        self.volume = volume
        self.timestamp = timestamp
        self.checked = check

    @staticmethod
    cdef Bar from_string(str value):
        """
        Return a bar parsed from the given string.

        :param value: The bar string to parse.
        :return Bar.
        """
        Condition.valid_string(value, 'value')

        cdef list split_bar = value.split(',', maxsplit=5)

        return Bar(Price.from_string(split_bar[0]),
                   Price.from_string(split_bar[1]),
                   Price.from_string(split_bar[2]),
                   Price.from_string(split_bar[3]),
                   float(split_bar[4]),
                   pd.to_datetime(split_bar[5]))

    @staticmethod
    def py_from_string(str value) -> Bar:
        """
        Python wrapper for the from_string method.

        Return a bar parsed from the given string.

        :param value: The bar string to parse.
        :return Bar.
        """
        return Bar.from_string(value)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        return (f"{self.open},"
                f"{self.high},"
                f"{self.low},"
                f"{self.close},"
                f"{self.volume},"
                f"{self.timestamp}")

    def __eq__(self, Bar other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.
        Note: The equality is based on the bars timestamp only.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp == other.timestamp

    def __ne__(self, Bar other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.
        Note: The equality is based on the bars timestamp only.

        :param other: The other object.
        :return bool.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.
        Note: The hash is based on the bars timestamp only.

        :return int.
        """
        return hash(str(self.timestamp))

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.to_string()}) object at {id(self)}>"


cdef class DataBar:
    """
    Represents a financial market trade bar.
    """

    def __init__(self,
                 double open_price,
                 double high_price,
                 double low_price,
                 double close_price,
                 double volume,
                 datetime timestamp not None):
        """
        Initializes a new instance of the DataBar class.

        :param open_price: The bars open price.
        :param high_price: The bars high price.
        :param low_price: The bars low price.
        :param close_price: The bars close price.
        :param volume: The bars volume.
        :param timestamp: The bars timestamp (UTC).
        """
        self.open = open_price
        self.high = high_price
        self.low = low_price
        self.close = close_price
        self.volume = volume
        self.timestamp = timestamp

    @staticmethod
    def py_from_string(str value) -> Bar:
        """
        Python wrapper for the from_string method.

        Return a bar parsed from the given string.

        :param value: The bar string to parse.
        :return Bar.
        """
        return Bar.from_string(value)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        return (f"{self.open},"
                f"{self.high},"
                f"{self.low},"
                f"{self.close},"
                f"{self.volume},"
                f"{self.timestamp}")

    def __eq__(self, DataBar other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.timestamp == other.timestamp

    def __ne__(self, DataBar other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(str(self.timestamp))

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return self.to_string()

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.to_string()}) object at {id(self)}>"


cdef class Instrument:
    """
    Represents a tradeable financial market instrument.
    """

    def __init__(self,
                 Symbol symbol not None,
                 str broker_symbol not None,
                 Currency quote_currency,
                 SecurityType security_type,
                 int tick_precision,
                 Decimal tick_size not None,
                 Quantity round_lot_size not None,
                 int min_stop_distance_entry,
                 int min_stop_distance,
                 int min_limit_distance_entry,
                 int min_limit_distance,
                 Quantity min_trade_size not None,
                 Quantity max_trade_size not None,
                 Decimal rollover_interest_buy not None,
                 Decimal rollover_interest_sell not None,
                 datetime timestamp not None):
        """
        Initializes a new instance of the Instrument class.

        :param symbol: The symbol.
        :param broker_symbol: The broker symbol.
        :param quote_currency: The base currency.
        :param security_type: The security type.
        :param tick_precision: The tick decimal digits precision.
        :param tick_size: The tick size.
        :param round_lot_size: The rounded lot size.
        :param min_stop_distance_entry: The minimum distance for stop entry orders.
        :param min_stop_distance: The minimum tick distance for stop orders.
        :param min_limit_distance_entry: The minimum distance for limit entry orders.
        :param min_limit_distance: The minimum tick distance for limit orders.
        :param min_trade_size: The minimum trade size.
        :param max_trade_size: The maximum trade size.
        :param rollover_interest_buy: The rollover interest for long positions.
        :param rollover_interest_sell: The rollover interest for short positions.
        :param timestamp: The timestamp the instrument was created/updated at.
        """
        Condition.valid_string(broker_symbol, 'broker_symbol')
        Condition.not_equal(quote_currency, Currency.UNDEFINED, 'quote_currency', 'UNDEFINED')
        Condition.not_equal(security_type, SecurityType.UNDEFINED, 'security_type', 'UNDEFINED')
        Condition.not_negative_int(tick_precision, 'tick_precision')
        Condition.positive(tick_size.as_double(), 'tick_size.value')
        Condition.not_negative_int(min_stop_distance_entry, 'min_stop_distance_entry')
        Condition.not_negative_int(min_limit_distance_entry, 'min_limit_distance_entry')
        Condition.not_negative_int(min_stop_distance, 'min_stop_distance')
        Condition.not_negative_int(min_limit_distance, 'min_limit_distance')
        Condition.not_negative_int(min_limit_distance, 'min_limit_distance')
        Condition.positive_int(min_trade_size.value, 'min_trade_size')
        Condition.positive_int(max_trade_size.value, 'max_trade_size')

        self.id = InstrumentId(symbol.value)
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
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.id == other.id

    def __ne__(self, Instrument other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Return the hash code of this object.

        :return int.
        """
        return hash(self.symbol.to_string())

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        :return str.
        """
        return f"{self.__class__.__name__}({self.symbol})"

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"


cdef class ForexInstrument(Instrument):
    """
    Represents a tradeable FOREX currency pair.
    """

    def __init__(self,
                 Symbol symbol not None,
                 str broker_symbol not None,
                 int tick_precision,
                 Decimal tick_size not None,
                 Quantity round_lot_size not None,
                 int min_stop_distance_entry,
                 int min_stop_distance,
                 int min_limit_distance_entry,
                 int min_limit_distance,
                 Quantity min_trade_size not None,
                 Quantity max_trade_size not None,
                 Decimal rollover_interest_buy not None,
                 Decimal rollover_interest_sell not None,
                 datetime timestamp not None):
        """
        Initializes a new instance of the Instrument class.

        :param symbol: The symbol.
        :param broker_symbol: The broker symbol.
        :param tick_precision: The tick decimal digits precision.
        :param tick_size: The tick size.
        :param round_lot_size: The rounded lot size.
        :param min_stop_distance_entry: The minimum distance for stop entry orders.
        :param min_stop_distance: The minimum tick distance for stop orders.
        :param min_limit_distance_entry: The minimum distance for limit entry orders.
        :param min_limit_distance: The minimum tick distance for limit orders.
        :param min_trade_size: The minimum trade size.
        :param max_trade_size: The maximum trade size.
        :param rollover_interest_buy: The rollover interest for long positions.
        :param rollover_interest_sell: The rollover interest for short positions.
        :param timestamp: The timestamp the instrument was created/updated at.
        """
        Condition.equal(len(symbol.code), 6, 'len(symbol.code)', '6')

        super().__init__(
            symbol,
            broker_symbol,
            currency_from_string(symbol.code[3:]),
            SecurityType.FOREX,
            tick_precision,
            tick_size,
            round_lot_size,
            min_stop_distance_entry,
            min_stop_distance,
            min_limit_distance_entry,
            min_limit_distance,
            min_trade_size,
            max_trade_size,
            rollover_interest_buy,
            rollover_interest_sell,
            timestamp)

        self.base_currency = currency_from_string(symbol.code[:3])
