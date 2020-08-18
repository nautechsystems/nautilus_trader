# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import pytz
from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.datetime cimport format_iso8601
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.c_enums.bar_structure cimport bar_structure_to_string, bar_structure_from_string
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport price_type_to_string, price_type_from_string
from nautilus_trader.model.objects cimport Price, Quantity
from nautilus_trader.model.identifiers cimport Symbol, Venue


cdef class BarSpecification:
    """
    Represents the specification of a financial market trade bar.
    """
    def __init__(self,
                 int step,
                 BarStructure structure,
                 PriceType price_type):
        """
        Initialize a new instance of the BarSpecification class.

        :param step: The bar step (> 0).
        :param structure: The bar structure.
        :param price_type: The bar price type.
        :raises ValueError: If the step is not positive (> 0).
        :raises ValueError: If the price type is LAST.
        """
        Condition.positive_int(step, 'step')
        Condition.true(price_type != PriceType.LAST, 'price_type != PriceType.LAST')
        Condition.not_equal(structure, BarStructure.UNDEFINED, 'structure', 'UNDEFINED')
        Condition.not_equal(price_type, PriceType.UNDEFINED, 'price_type', 'UNDEFINED')

        self.step = step
        self.structure = structure
        self.price_type = price_type

    @staticmethod
    cdef BarSpecification from_string(str value):
        """
        Return a bar specification parsed from the given string.
        Note: String format example is '200-TICK-MID'.

        :param value: The bar specification string to parse.
        :return BarSpecification.
        """
        Condition.valid_string(value, 'value')

        cdef list split = value.split('-', maxsplit=3)

        return BarSpecification(
            int(split[0]),
            bar_structure_from_string(split[1]),
            price_type_from_string(split[2]))

    @staticmethod
    def py_from_string(str value) -> BarSpecification:
        """
        Python wrapper for the from_string method.

        Return a bar specification parsed from the given string.
        Note: String format example is '1-MINUTE-BID'.

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
        return (self.step == other.step and            # noqa (W504 - easier to read)
                self.structure == other.structure and  # noqa (W504 - easier to read)
                self.price_type == other.price_type)   # noqa (W504 - easier to read)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        return f"{self.step}-{bar_structure_to_string(self.structure)}-{price_type_to_string(self.price_type)}"

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
        Initialize a new instance of the BarType class.

        :param symbol: The bar symbol.
        :param bar_spec: The bar specification.
        """
        self.symbol = symbol
        self.spec = bar_spec

    @staticmethod
    cdef BarType from_string(str value):
        """
        Return a bar type parsed from the given string.

        :param value: The bar type string to parse.
        :return BarType.
        """
        Condition.valid_string(value, 'value')

        cdef list split = value.split('-', maxsplit=3)
        cdef list symbol_split = split[0].split('.', maxsplit=1)

        cdef Symbol symbol = Symbol(symbol_split[0], Venue(symbol_split[1]))
        cdef BarSpecification bar_spec = BarSpecification(
            int(split[1]),
            bar_structure_from_string(split[2]),
            price_type_from_string(split[3]))

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
        return self.spec.structure_string()

    cdef str price_type_string(self):
        """
        Return the price type as a string.

        :return str.
        """
        return self.spec.price_type_string()

    cpdef bint equals(self, BarType other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return self.symbol.equals(other.symbol) and self.spec.equals(other.spec)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        return f"{self.symbol.to_string()}-{self.spec}"

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
        return hash((self.symbol, self.spec))

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
                 Quantity volume not None,
                 datetime timestamp not None,
                 bint check=False):
        """
        Initialize a new instance of the Bar class.

        :param open_price: The bars open price.
        :param high_price: The bars high price.
        :param low_price: The bars low price.
        :param close_price: The bars close price.
        :param volume: The bars volume.
        :param timestamp: The bars timestamp (UTC).
        :param check: If the bar parameters should be checked valid.
        :raises ValueError: If check and the high_price is not >= low_price.
        :raises ValueError: If check and the high_price is not >= close_price.
        :raises ValueError: If check and the low_price is not <= close_price.
        """
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
    cdef Bar from_serializable_string(str value):
        """
        Return a bar parsed from the given string.

        :param value: The bar string to parse.
        :return Bar.
        """
        Condition.valid_string(value, 'value')

        cdef list pieces = value.split(',', maxsplit=5)

        return Bar(Price.from_string(pieces[0]),
                   Price.from_string(pieces[1]),
                   Price.from_string(pieces[2]),
                   Price.from_string(pieces[3]),
                   Quantity.from_string(pieces[4]),
                   datetime.fromtimestamp(long(pieces[5]) / 1000, pytz.utc))

    @staticmethod
    def py_from_serializable_string(str value) -> Bar:
        """
        Python wrapper for the from_string method.

        Return a bar parsed from the given string.

        :param value: The bar string to parse.
        :return Bar.
        """
        return Bar.from_serializable_string(value)

    cpdef bint equals(self, Bar other):
        """
        Return a value indicating whether this object is equal to (==) the given object.

        :param other: The other object.
        :return bool.
        """
        return (self.open.equals(other.open) and      # noqa (W504)
                self.high.equals(other.high) and      # noqa (W504)
                self.low.equals(other.low) and        # noqa (W504)
                self.close.equals(other.close) and    # noqa (W504)
                self.volume.equals(other.volume) and  # noqa (W504)
                self.timestamp == other.timestamp)    # noqa (W504)

    cpdef str to_string(self):
        """
        Return the string representation of this object.

        :return: str.
        """
        return (f"{self.open.to_string()},"
                f"{self.high.to_string()},"
                f"{self.low.to_string()},"
                f"{self.close.to_string()},"
                f"{self.volume.to_string()},"
                f"{format_iso8601(self.timestamp)}")

    cpdef str to_serializable_string(self):
        """
        Return the serializable string representation of this object.

        :return: str.
        """
        return (f"{self.open.to_string()},"
                f"{self.high.to_string()},"
                f"{self.low.to_string()},"
                f"{self.close.to_string()},"
                f"{self.volume.to_string()},"
                f"{long(self.timestamp.timestamp())}")

    def __eq__(self, Bar other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.
        Note: The equality is based on the bars timestamp only.

        :param other: The other object.
        :return bool.
        """
        return self.equals(other)

    def __ne__(self, Bar other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.
        Note: The equality is based on the bars timestamp only.

        :param other: The other object.
        :return bool.
        """
        return not self.equals(other)

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
