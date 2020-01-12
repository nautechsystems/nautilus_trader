# -------------------------------------------------------------------------------------------------
# <copyright file="objects.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

"""Define common trading model value objects."""

import decimal
import iso8601
import re

from libc.math cimport round
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure, bar_structure_to_string, bar_structure_from_string
from nautilus_trader.model.c_enums.price_type cimport PriceType, price_type_to_string, price_type_from_string
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.identifiers cimport Venue


cdef Quantity _ZERO_QUANTITY = Quantity()

cdef class Quantity:
    """
    Represents a quantity with non-negative integer value.
    """

    def __init__(self, long value=0):
        """
        Initializes a new instance of the Quantity class.

        :param value: The value of the quantity (>= 0).
        :raises ConditionFailed: If the value is negative (< 0).
        """
        Condition.not_negative_int(value, 'value')

        self.value = value

    @staticmethod
    cdef Quantity zero():
        """
        Return a quantity of zero.
        
        :return Quantity.
        """
        return _ZERO_QUANTITY

    cdef bint equals(self, Quantity other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value == other.value

    cdef str to_string_formatted(self):
        """
        Return the formatted string representation of this object.
        
        :return: str.
        """
        return f'{self.value:,}'

    def __eq__(self, Quantity other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, Quantity other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __lt__(self, Quantity other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value < other.value

    def __le__(self, Quantity other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value <= other.value

    def __gt__(self, Quantity other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value > other.value

    def __ge__(self, Quantity other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value >= other.value

    def __add__(self, other) -> int:
        """
        Return the result of adding the given object to this object.

        :param other: The other object.
        :return int.
        """
        if isinstance(other, Quantity):
            return self.value + other.value
        elif isinstance(other, long):
            return self.value + other
        else:
            raise NotImplementedError(f"Cannot add {type(other)} to a quantity.")

    def __sub__(self, other) -> int:
        """
        Return the result of subtracting the given object from this object.

        :param other: The other object.
        :return int.
        """
        if isinstance(other, Quantity):
            return self.value - other.value
        elif isinstance(other, long):
            return self.value - other
        else:
            raise NotImplementedError(f"Cannot subtract {type(other)} from a quantity.")

    def __hash__(self) -> int:
        """"
        Return a hash representation of this object.

        :return int.
        """
        return hash(self.value)

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return str(self.value)

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.value}) object at {id(self)}>"


cdef Decimal _ZERO_DECIMAL = Decimal()

cdef class Decimal:
    """
    Represents a decimal floating point value type.
    """

    def __init__(self, float value=0.0, int precision=1):
        """
        Initializes a new instance of the Decimal class.

        :param value: The value of the decimal.
        :param precision: The precision of the decimal (> 0).
        :raises ConditionFailed: If the precision is not positive (> 0).
        """
        Condition.positive_int(precision, 'precision')

        self.value = decimal.Decimal(f'{value:.{precision}f}')
        self.precision = precision

    cpdef float as_float(self):
        """
        Return the internal decimal value as a floating point number.

        :return float.
        """
        return float(self.value)

    cpdef str to_string(self, bint format_commas=True):
        """
        Return the formatted string representation of this object.
        
        :param format_commas: If the string should be formatted with commas separating thousands.
        :return: str.
        """
        if format_commas:
            return f'{self.value:,.{self.precision}f}'
        else:
            return f'{self.value:.{self.precision}f}'

    @staticmethod
    cdef Decimal zero():
        """
        Return a zero valued decimal.
        
        :return Decimal.
        """
        return _ZERO_DECIMAL

    @staticmethod
    cdef Decimal from_string_to_decimal(str value):
        """
        Return a decimal from the given string. Precision will be inferred from the
        number of digits after the decimal place.

        :param value: The string value to parse.
        :return: Decimal.
        """
        return Decimal(float(value), precision=Decimal.precision_from_string(value))

    @staticmethod
    cdef int precision_from_string(str value):
        """
        Return the decimal precision inferred from the number of digits after the decimal place.

        :param value: The string value to parse (must contain a decimal '.').
        :return: int.
        """
        if value.__contains__('.'):
            return len(value.partition('.')[2])
        else:
            return 1

    cdef bint equals(self, Decimal other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value == other.value

    cpdef bint eq(self, Decimal other):
        """
        Return a value indicating whether this decimal is equal to (==) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        return self.value == other.value

    cpdef bint ne(self, Decimal other):
        """
        Return a value indicating whether this decimal is not equal to (!=) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        return self.value != other.value

    cpdef bint lt(self, Decimal other):
        """
        Return a value indicating whether this decimal is less than (<) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        return self.value < other.value

    cpdef bint le(self, Decimal other):
        """
        Return a value indicating whether this decimal is less than or equal to (<=) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        return self.value <= other.value

    cpdef bint gt(self, Decimal other):
        """
        Return a value indicating whether this decimal is greater than (>) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        return self.value > other.value

    cpdef bint ge(self, Decimal other):
        """
        Return a value indicating whether this decimal is greater than or equal to (>=) the given decimal.

        :param other: The other decimal.
        :return bool.
        """
        return self.value >= other.value

    cpdef Decimal add(self, Decimal other):
        """
        Return a new decimal by adding the given decimal to this decimal.

        :param other: The other decimal to add.
        :raises ConditionFailed: If the precision is not >= the other decimal precision.
        :return Decimal.
        """
        Condition.true(self.precision >= other.precision, 'self.precision >= other.precision')

        self.value += other.value
        return self

    cpdef Decimal subtract(self, Decimal other):
        """
        Return a new decimal by subtracting the given decimal from this decimal.
        Note: This operation can result in negative decimals.

        :param other: The other decimal to subtract.
        :raises ConditionFailed: If the precision is not >= the other decimal precision.
        :return Decimal.
        """
        Condition.true(self.precision >= other.precision, 'self.precision >= other.precision')

        self.value -= other.value
        return self

    def __eq__(self, other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.value == <float?>other
        except TypeError:
            return self.value == <Decimal>other.value

    def __ne__(self, other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return self.value != <float?>other
        except TypeError:
            return self.value != <Decimal>other.value

    def __lt__(self, other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return float(self.value) < <float?>other
        except TypeError:
            return self.value < <Decimal>other.value

    def __le__(self, other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return float(self.value) <= <float?>other
        except TypeError:
            return self.value <= <Decimal>other.value

    def __gt__(self, other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return float(self.value) > <float?>other
        except TypeError:
            return self.value > <Decimal>other.value

    def __ge__(self, other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        :param other: The other object.
        :return bool.
        """
        try:
            return float(self.value) >= <float?>other
        except TypeError:
            return self.value >= <Decimal>other.value

    def __add__(self, other) -> float:
        """
        Return the result of adding the given object to this object.

        :param other: The other object.
        :return float.
        """
        try:
            return float(self.value) + <float?>other
        except TypeError:
            return float(self.value) + float(other.value)

    def __sub__(self, other) -> float:
        """
        Return the result of subtracting the given object from this object.

        :param other: The other object.
        :return float.
        """
        try:
            return float(self.value) - <float?>other
        except TypeError:
            return float(self.value) - float(other.value)

    def __truediv__(self, other) -> float:
        """
        Return the result of dividing this object by the given object.

        :param other: The other object.
        :return float.
        """
        try:
            return float(self.value) / <float?>other
        except TypeError:
            return float(self.value) / float(<Decimal>other.value)

    def __mul__(self, other) -> float:
        """
        Return the result of multiplying this object by the given object.

        :param other: The other object.
        :return float.
        """
        try:
            return float(self.value) * <float?>other
        except TypeError:
            return float(self.value) * float(<Decimal>other.value)

    def __hash__(self) -> int:
        """"
         Return a hash representation of this object.

        :return int.
        """
        return hash(self.value)

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return self.to_string(format_commas=False)

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({str(self)}, precision={self.precision}) object at {id(self)}>"


cdef class Price(Decimal):
    """
    Represents a price of a financial market instrument.
    """

    def __init__(self, float value, int precision):
        """
        Initializes a new instance of the Price class.

        :param value: The value of the price (>= 0).
        :param precision: The decimal precision of the price (> 0).
        :raises ConditionFailed: If the value is negative (< 0).
        :raises ConditionFailed: If the precision is not positive (> 0).
        """
        Condition.not_negative(value, 'value')
        # Condition: precision checked in base class
        super().__init__(value, precision)

    @staticmethod
    cdef Price from_string(str value):
        """
        Return a price from the given string. Precision will be inferred from the
        number of digits after the decimal place.

        :param value: The string value to parse.
        :return: Price.
        """
        return Price(float(value), precision=Decimal.precision_from_string(value))

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.to_string()}, precision={self.precision}) object at {id(self)}>"


cdef Money _ZERO_MONEY = Money()

cdef class Money(Decimal):
    """
    Represents the 'concept' of money.
    """

    def __init__(self, float value=0):
        """
        Initializes a new instance of the Money class.

        :param value: The value of the money.
        Note: The value is rounded to 2 decimal places of precision.
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
        return Money(float(value))

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.to_string()}) object at {id(self)}>"


cdef class Tick:
    """
    Represents a single tick in a financial market.
    """

    def __init__(self,
                 Symbol symbol,
                 Price bid,
                 Price ask,
                 datetime timestamp,
                 TickType tick_type=TickType.TRADE,
                 int bid_size=1,
                 int ask_size=1):
        """
        Initializes a new instance of the Tick class.

        :param symbol: The tick symbol.
        :param bid: The tick best bid price.
        :param ask: The tick best ask price.
        :param timestamp: The tick timestamp (UTC).
        :param tick_type: The optional tick type (default=TRADE).
        :param tick_type: The optional tick type (default=TRADE).
        :raises ConditionFailed: If the bid_size is negative (< 0).
        :raises ConditionFailed: If the ask_size is negative (< 0).
        """
        Condition.not_negative_int(bid_size, 'bid_size')
        Condition.not_negative_int(ask_size, 'ask_size')

        self.type = TickType.TRADE
        self.symbol = symbol
        self.bid = bid
        self.ask = ask
        self.bid_size = bid_size
        self.ask_size = ask_size
        self.timestamp = timestamp

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
        Return a hash representation of this object.

        :return int.
        """
        return hash(self.timestamp)

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return f"{self.bid},{self.ask},{self.timestamp.isoformat()}"

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({self.symbol},{str(self)}) object at {id(self)}>"

    @staticmethod
    cdef Tick from_string_with_symbol(Symbol symbol, str values):
        """
        Return a tick parsed from the given symbol and values string.

        :param symbol: The tick symbol.
        :param values: The tick values string.
        :return Tick.
        """
        cdef list split_values = values.split(',', maxsplit=2)

        return Tick(
            symbol,
            Price.from_string(split_values[0]),
            Price.from_string(split_values[1]),
            iso8601.parse_date(split_values[2]))

    @staticmethod
    cdef Tick from_string(str value):
        """
        Return a tick parsed from the given value string.

        :param value: The tick value string to parse.
        :return Tick.
        """
        cdef list split_values = value.split(',', maxsplit=3)

        return Tick(
            Symbol.from_string(split_values[0]),
            Price.from_string(split_values[1]),
            Price.from_string(split_values[2]),
            iso8601.parse_date(split_values[3]))

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
        :param price_type: The bar quote type.
        :raises ConditionFailed: If the step is not positive (> 0).
        :raises ConditionFailed: If the quote type is LAST.
        """
        Condition.positive_int(step, 'step')
        Condition.true(price_type != PriceType.LAST, 'price_type != PriceType.LAST')

        self.step = step
        self.structure = structure
        self.price_type = price_type

    cdef bint equals(self, BarSpecification other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return (self.step == other.step
                and self.structure == other.structure
                and self.price_type == other.price_type)

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
         Return a hash representation of this object.

        :return int.
        """
        return hash((self.step, self.structure, self.price_type))

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return f"{self.step}-{bar_structure_to_string(self.structure)}[{price_type_to_string(self.price_type)}]"

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"

    cdef str structure_string(self):
        """
        Return the bar structure as a string
        
        :return str.
        """
        return bar_structure_to_string(self.structure)

    cdef str quote_type_string(self):
        """
        Return the quote type as a string.
        
        :return str.
        """
        return price_type_to_string(self.price_type)

    @staticmethod
    cdef BarSpecification from_string(str value):
        """
        Return a bar specification parsed from the given string.

        Note: String format example is '200-TICK-[MID]'.
        :param value: The bar specification string to parse.
        :return BarSpecification.
        """
        cdef list split1 = value.split('-', maxsplit=2)
        cdef list split2 = split1[1].split('[', maxsplit=1)
        cdef str structure = split2[0]
        cdef str price_type = split2[1].strip(']')

        return BarSpecification(
            int(split1[0]),
            bar_structure_from_string(structure),
            price_type_from_string(price_type))

    @staticmethod
    def py_from_string(value: str) -> BarSpecification:
        """
        Python wrapper for the from_string method.

        Return a bar specification parsed from the given string.

        Note: String format example is '1-MINUTE-[BID]'.
        :param value: The bar specification string to parse.
        :return BarSpecification.
        """
        return BarSpecification.from_string(value)


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

    cdef bint equals(self, BarType other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.symbol.equals(other.symbol) and self.specification.equals(other.specification)

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
        Return a hash representation of this object.

        :return int.
        """
        return hash((self.symbol, self.specification))

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return f"{str(self.symbol)}-{self.specification}"

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"

    cdef str structure_string(self):
        """
        Return the bar structure as a string
        
        :return str.
        """
        return self.specification.structure_string()

    cdef str quote_type_string(self):
        """
        Return the quote type as a string.
        
        :return str.
        """
        return self.specification.quote_type_string()

    @staticmethod
    cdef BarType from_string(str value):
        """
        Return a bar type parsed from the given string.

        :param value: The bar type string to parse.
        :return BarType.
        """
        cdef list split_string = re.split(r'[.-]+', value)
        cdef str structure = split_string[3].split('[', maxsplit=1)[0]
        cdef str price_type = split_string[3].split('[', maxsplit=1)[1].strip(']')
        cdef Symbol symbol = Symbol(split_string[0], Venue(split_string[1]))
        cdef BarSpecification bar_spec = BarSpecification(int(split_string[2]),
                                                          bar_structure_from_string(structure.upper()),
                                                          price_type_from_string(price_type.upper()))
        return BarType(symbol, bar_spec)

    @staticmethod
    def py_from_string(value: str) -> BarType:
        """
        Python wrapper for the from_string method.

        Return a bar type parsed from the given string.

        :param value: The bar type string to parse.
        :return BarType.
        """
        return BarType.from_string(value)


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
        :param volume: The bars volume (>= 0).
        :param timestamp: The bars timestamp (UTC).
        :param checked: A value indicating whether the bar was checked valid.
        :raises ConditionFailed: If checked is true and the volume is negative.
        :raises ConditionFailed: If checked is true and the high_price is not >= low_price.
        :raises ConditionFailed: If checked is true and the high_price is not >= close_price.
        :raises ConditionFailed: If checked is true and the low_price is not <= close_price.
        """
        if checked:
            Condition.not_negative_int(volume, 'volume')
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
        Return a hash representation of this object.

        Note: The hash is based on the bars timestamp only.
        :return int.
        """
        return hash(str(self.timestamp))

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return (f"{self.open},"
                f"{self.high},"
                f"{self.low},"
                f"{self.close},"
                f"{self.volume},"
                f"{self.timestamp.isoformat()}")

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"

    @staticmethod
    cdef Bar from_string(str value):
        """
        Return a bar parsed from the given string.

        :param value: The bar string to parse.
        :return Bar.
        """
        cdef list split_bar = value.split(',', maxsplit=5)

        return Bar(Price.from_string(split_bar[0]),
                   Price.from_string(split_bar[1]),
                   Price.from_string(split_bar[2]),
                   Price.from_string(split_bar[3]),
                   long(split_bar[4]),
                   iso8601.parse_date(split_bar[5]))

    @staticmethod
    def py_from_string(value: str) -> Bar:
        """
        Python wrapper for the from_string method.

        Return a bar parsed from the given string.

        :param value: The bar string to parse.
        :return Bar.
        """
        return Bar.from_string(value)


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
        """
        self.open = open_price
        self.high = high_price
        self.low = low_price
        self.close = close_price
        self.volume = volume
        self.timestamp = timestamp

    def __eq__(self, DataBar other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.open == other.open

    def __ne__(self, DataBar other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.__eq__(other)

    def __hash__(self) -> int:
        """"
        Return a hash representation of this object.

        :return int.
        """
        return hash(str(self.timestamp))

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return (f"{self.open},{self.high},{self.low},{self.close},"
                f"{self.volume},{self.timestamp.isoformat()}")

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"


cdef class Instrument:
    """
    Represents a tradeable financial market instrument.
    """

    def __init__(self,
                 Symbol symbol,
                 str broker_symbol,
                 Currency base_currency,
                 SecurityType security_type,
                 int tick_precision,
                 Decimal tick_size,
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

        :param symbol: The instruments symbol.
        :param broker_symbol: The instruments broker symbol.
        :param base_currency: The instruments base currency.
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
        Condition.not_negative_int(tick_precision, 'tick_precision')
        Condition.positive(tick_size.value, 'tick_size.value')
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
        self.base_currency = base_currency
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
        Return a hash representation of this object.

        :return int.
        """
        return hash(str(self.symbol))

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return f"{self.__class__.__name__}({self.symbol})"

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{str(self)} object at {id(self)}>"
