# -------------------------------------------------------------------------------------------------
# <copyright file="objects.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import iso8601
import re

from decimal import Decimal
from cpython.datetime cimport datetime, timedelta

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.resolution cimport Resolution, resolution_to_string, resolution_from_string
from nautilus_trader.model.c_enums.quote_type cimport QuoteType, quote_type_to_string, quote_type_from_string
from nautilus_trader.model.c_enums.security_type cimport SecurityType
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.identifiers cimport Venue


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
        
        :return Quantity.
        """
        return ZERO_QUANTITY

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
        return format(self.value, ',')

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

    cdef bint equals(self, Price other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value == other.value

    def __eq__(self, Price other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, Price other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __lt__(self, Price other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value < other.value

    def __le__(self, Price other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value <= other.value

    def __gt__(self, Price other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value > other.value

    def __ge__(self, Price other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value >= other.value

    def __add__(self, other) -> Decimal:
        """
        Return the result of adding the given object to this object.

        :param other: The other object.
        :return Decimal.
        """
        if isinstance(other, float):
            return Decimal(_get_decimal_str(float(self.value) + other, self.precision))
        elif isinstance(other, Decimal):
            return Decimal(_get_decimal_str(float(self.value) + float(other), self.precision))
        elif isinstance(other, Price):
            return Decimal(_get_decimal_str(float(self.value) + other.as_float(), self.precision))
        else:
            raise NotImplementedError(f"Cannot add {type(other)} to a price.")

    def __sub__(self, other) -> Decimal:
        """
        Return the result of subtracting the given object from this object.

        :param other: The other object.
        :return Decimal.
        """
        if isinstance(other, float):
            return Decimal(_get_decimal_str(float(self.value) - other, self.precision))
        elif isinstance(other, Decimal):
            return Decimal(_get_decimal_str(float(self.value) - float(other), self.precision))
        elif isinstance(other, Price):
            return Decimal(_get_decimal_str(float(self.value) - other.as_float(), self.precision))
        else:
            raise NotImplementedError(f"Cannot subtract {type(other)} from a price.")

    def __truediv__(self, other) -> Decimal:
        """
        Return the result of dividing this object by the given object.

        :param other: The other object.
        :return Decimal.
        """
        if isinstance(other, float):
            return Decimal(_get_decimal_str(float(self.value) / other, self.precision))
        elif isinstance(other, Decimal):
            return Decimal(_get_decimal_str(float(self.value) / float(other), self.precision))
        elif isinstance(other, Price):
            return Decimal(_get_decimal_str(float(self.value) / other.as_float(), self.precision))
        else:
            raise NotImplementedError(f"Cannot divide price by {type(other)}.")

    def __mul__(self, other) -> Decimal:
        """
        Return the result of multiplying this object by the given object.

        :param other: The other object.
        :return Decimal.
        """
        if isinstance(other, float):
            return Decimal(_get_decimal_str(float(self.value) * other, self.precision))
        elif isinstance(other, Decimal):
            return Decimal(_get_decimal_str(float(self.value) * float(other), self.precision))
        elif isinstance(other, Price):
            return Decimal(_get_decimal_str(float(self.value) * other.as_float(), self.precision))
        else:
            raise NotImplementedError(f"Cannot multiply price with {type(other)}.")

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
        return f'{self.value:,.{self.precision}f}'

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"

    cpdef Price add(self, Price price):
        """
        Return a new price by adding the given price to this price.

        :param price: The other price to add.
        :return Price.
        :raises ConditionFailed: If the precision of the prices are not equal.
        """
        Condition.true(self.precision == price.precision, 'self.precision == price.precision')

        return Price(self.value + price.value)

    cpdef Price subtract(self, Price price):
        """
        Return a new price by subtracting the given price from this price.

        :param price: The other price to subtract.
        :return Price.
        :raises ConditionFailed: If the precision of the prices are not equal.
        """
        Condition.true(self.precision == price.precision, 'self.precision == price.precision')

        return Price(self.value - price.value)

    cpdef float as_float(self):
        """
        Return a float representation of the price.
        
        :return float.
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
        cdef str value_str
        if isinstance(value, str):
            value_str = value.replace(',', '')
            self.value = Decimal(f'{float(value_str):.2f}')
        else:
            self.value = Decimal(f'{value:.2f}')

    @staticmethod
    def zero() -> Money:
        """
        Return money with a zero amount.
        
        :return Money.
        """
        return ZERO_MONEY

    cdef bint equals(self, Money other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value == other.value

    def __eq__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

    def __lt__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is less than (<) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value < other.value

    def __le__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is less than or equal to (<=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value <= other.value

    def __gt__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is greater than (>) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value > other.value

    def __ge__(self, Money other) -> bool:
        """
        Return a value indicating whether this object is greater than or equal to (>=) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.value >= other.value

    def __add__(self, other) -> Money:
        """
        Return the result of adding the given object to this object.

        :param other: The other object.
        :return Money.
        """
        if isinstance(other, Money):
            return Money(self.value + other.value)
        elif isinstance(other, Decimal):
            return Money(self.value + other)
        elif isinstance(other, int):
            return self + Money(other)
        else:
            raise NotImplementedError(f"Cannot add {type(other)} to money.")

    def __sub__(self, other) -> Money:
        """
        Return the result of subtracting the given object from this object.

        :param other: The other object.
        :return Money.
        """
        if isinstance(other, Money):
            return Money(self.value - other.value)
        elif isinstance(other, Decimal):
            return Money(self.value - other)
        elif isinstance(other, int):
            return self - Money(other)
        else:
            raise NotImplementedError(f"Cannot subtract {type(other)} from money.")

    def __truediv__(self, other) -> Money:
        """
        Return the result of dividing this object by the given object.

        :param other: The other object.
        :return Money.
        """
        if isinstance(other, Money):
            return Money(self.value / other.value)
        elif isinstance(other, Decimal):
            return Money(self.value / other)
        elif isinstance(other, int):
            return self / Money(other)
        else:
            raise NotImplementedError(f"Cannot divide money by {type(other)}.")

    def __mul__(self, other) -> Money:
        """
        Return the result of multiplying the given object by this object.

        :param other: The other object.
        :return Money.
        """
        if isinstance(other, Money):
            return Money(self.value * other.value)
        elif isinstance(other, Decimal):
            return Money(self.value * other)
        elif isinstance(other, int):
            return self * Money(other)
        else:
            raise NotImplementedError(f"Cannot multiply money with {type(other)}.")

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
        return f'{self.value:,.2f}'

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"

    cpdef float as_float(self):
        """
        Return a float representation of this object.
        
        :return float.
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
        cdef list split_values = values.split(',', maxsplit=3)

        return Tick(
            symbol,
            Price(split_values[0]),
            Price(split_values[1]),
            iso8601.parse_date(split_values[2]))

    @staticmethod
    cdef Tick from_string(str value):
        """
        Return a tick parsed from the given value string.

        :param value: The tick value string to parse.
        :return Tick.
        """
        cdef list split_values = value.split(',', maxsplit=4)

        return Tick(
            Symbol.from_string(split_values[0]),
            Price(split_values[1]),
            Price(split_values[2]),
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

    cdef bint equals(self, BarSpecification other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return (self.period == other.period
                and self.resolution == other.resolution
                and self.quote_type == other.quote_type)

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
        return hash((self.period, self.resolution, self.quote_type))

    def __str__(self) -> str:
        """
        Return a string representation of this object.

        :return str.
        """
        return f"{self.period}-{resolution_to_string(self.resolution)}[{quote_type_to_string(self.quote_type)}]"

    def __repr__(self) -> str:
        """
        Return a string representation of this object which includes the objects
        location in memory.

        :return str.
        """
        return f"<{self.__class__.__name__}({str(self)}) object at {id(self)}>"

    cpdef timedelta timedelta(self):
        """
        Return the time bar timedelta.
        :return timedelta.
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
            raise RuntimeError(f"Cannot calculate timedelta for {resolution_to_string(self.resolution)}")

    cdef str resolution_string(self):
        """
        Return the resolution as a string
        
        :return str.
        """
        return resolution_to_string(self.resolution)

    cdef str quote_type_string(self):
        """
        Return the quote type as a string.
        
        :return str.
        """
        return quote_type_to_string(self.quote_type)

    @staticmethod
    cdef BarSpecification from_string(str value):
        """
        Return a bar specification parsed from the given string.
    
        Note: String format example is '1-MINUTE-[BID]'.
        :param value: The bar specification string to parse.
        :return BarSpecification.
        """
        cdef list split1 = value.split('-')
        cdef list split2 = split1[1].split('[')
        cdef str resolution = split2[0]
        cdef str quote_type = split2[1].strip(']')

        return BarSpecification(
            int(split1[0]),
            resolution_from_string(resolution),
            quote_type_from_string(quote_type))

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

    cdef str resolution_string(self):
        """
        Return the resolution as a string
        
        :return str.
        """
        return self.specification.resolution_string()

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
        cdef str resolution = split_string[3].split('[')[0]
        cdef str quote_type = split_string[3].split('[')[1].strip(']')
        cdef Symbol symbol = Symbol(split_string[0], Venue(split_string[1]))
        cdef BarSpecification bar_spec = BarSpecification(int(split_string[2]),
                                                          resolution_from_string(resolution.upper()),
                                                          quote_type_from_string(quote_type.upper()))
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
        cdef list split_bar = value.split(',')

        return Bar(Price(split_bar[0]),
                   Price(split_bar[1]),
                   Price(split_bar[2]),
                   Price(split_bar[3]),
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
