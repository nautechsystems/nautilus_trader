# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

"""Defines fundamental value objects for the trading domain."""

import decimal

import cython

from cpython.object cimport Py_EQ
from cpython.object cimport Py_GE
from cpython.object cimport Py_GT
from cpython.object cimport Py_LE
from cpython.object cimport Py_LT
from cpython.object cimport PyObject_RichCompareBool
from libc.math cimport isnan
from libc.stdint cimport int64_t
from libc.stdint cimport uint8_t
from libc.stdint cimport uint16_t
from libc.stdint cimport uint64_t
from libc.string cimport strcmp

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport precision_from_cstr
from nautilus_trader.core.rust.model cimport FIXED_PRECISION as RUST_FIXED_PRECISION
from nautilus_trader.core.rust.model cimport FIXED_SCALAR as RUST_FIXED_SCALAR
from nautilus_trader.core.rust.model cimport HIGH_PRECISION_MODE as RUST_HIGH_PRECISION_MODE
from nautilus_trader.core.rust.model cimport MONEY_MAX as RUST_MONEY_MAX
from nautilus_trader.core.rust.model cimport MONEY_MIN as RUST_MONEY_MIN
from nautilus_trader.core.rust.model cimport MONEY_RAW_MAX
from nautilus_trader.core.rust.model cimport MONEY_RAW_MIN
from nautilus_trader.core.rust.model cimport PRECISION_BYTES
from nautilus_trader.core.rust.model cimport PRECISION_BYTES as RUST_PRECISION_BYTES
from nautilus_trader.core.rust.model cimport PRICE_MAX as RUST_PRICE_MAX
from nautilus_trader.core.rust.model cimport PRICE_MIN as RUST_PRICE_MIN
from nautilus_trader.core.rust.model cimport PRICE_RAW_MAX
from nautilus_trader.core.rust.model cimport PRICE_RAW_MIN
from nautilus_trader.core.rust.model cimport QUANTITY_MAX as RUST_QUANTITY_MAX
from nautilus_trader.core.rust.model cimport QUANTITY_MIN as RUST_QUANTITY_MIN
from nautilus_trader.core.rust.model cimport QUANTITY_RAW_MAX
from nautilus_trader.core.rust.model cimport MoneyRaw
from nautilus_trader.core.rust.model cimport PriceRaw
from nautilus_trader.core.rust.model cimport QuantityRaw
from nautilus_trader.core.rust.model cimport currency_code_to_cstr
from nautilus_trader.core.rust.model cimport currency_exists
from nautilus_trader.core.rust.model cimport currency_from_cstr
from nautilus_trader.core.rust.model cimport currency_from_py
from nautilus_trader.core.rust.model cimport currency_hash
from nautilus_trader.core.rust.model cimport currency_register
from nautilus_trader.core.rust.model cimport currency_to_cstr
from nautilus_trader.core.rust.model cimport money_from_raw
from nautilus_trader.core.rust.model cimport money_new
from nautilus_trader.core.rust.model cimport price_from_raw
from nautilus_trader.core.rust.model cimport price_new
from nautilus_trader.core.rust.model cimport quantity_from_raw
from nautilus_trader.core.rust.model cimport quantity_new
from nautilus_trader.core.rust.model cimport quantity_saturating_sub
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.string cimport ustr_to_pystr


# Value object valid range constants for Python
QUANTITY_MAX = RUST_QUANTITY_MAX
QUANTITY_MIN = RUST_QUANTITY_MIN
PRICE_MAX = RUST_PRICE_MAX
PRICE_MIN = RUST_PRICE_MIN
MONEY_MAX = RUST_MONEY_MAX
MONEY_MIN = RUST_MONEY_MIN

HIGH_PRECISION = bool(RUST_HIGH_PRECISION_MODE)
FIXED_PRECISION = RUST_FIXED_PRECISION
FIXED_SCALAR = RUST_FIXED_SCALAR
FIXED_PRECISION_BYTES = RUST_PRECISION_BYTES
FIXED_DECIMAL_SCALE = decimal.Decimal(10) ** FIXED_PRECISION


@cython.auto_pickle(True)
cdef class Quantity:
    """
    Represents a quantity with a non-negative value.

    Capable of storing either a whole number (no decimal places) of 'contracts'
    or 'shares' (instruments denominated in whole units) or a decimal value
    containing decimal places for instruments denominated in fractional units.

    Handles up to 16 decimals of precision (in high-precision mode).

    - ``QUANTITY_MAX`` = 34_028_236_692_093
    - ``QUANTITY_MIN`` = 0

    Parameters
    ----------
    value : integer, float, string, Decimal
        The value of the quantity.
    precision : uint8_t
        The precision for the quantity. Use a precision of 0 for whole numbers
        (no fractional units).

    Raises
    ------
    ValueError
        If `value` is greater than 34_028_236_692_093.
    ValueError
        If `value` is negative (< 0).
    ValueError
        If `precision` is greater than 16.
    OverflowError
        If `precision` is negative (< 0).

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/index.html#Qty
    """

    def __init__(self, double value, uint8_t precision) -> None:
        if precision > FIXED_PRECISION:
            raise ValueError(
                f"invalid `precision` greater than max {FIXED_PRECISION}, was {precision}"
            )
        if isnan(value):
            raise ValueError(
                f"invalid `value`, was {value:_}",
            )
        if value > RUST_QUANTITY_MAX:
            raise ValueError(
                f"invalid `value` greater than `QUANTITY_MAX` {RUST_QUANTITY_MAX:_}, was {value:_}",
            )
        if value < RUST_QUANTITY_MIN:
            raise ValueError(
                f"invalid `value` less than `QUANTITY_MIN` {RUST_QUANTITY_MIN:_}, was {value:_}",
            )

        self._mem = quantity_new(value, precision)

    def __getstate__(self):
        return self._mem.raw, self._mem.precision

    def __setstate__(self, state):
        self._mem = quantity_from_raw(state[0], state[1])

    def __eq__(self, other) -> bool:
        return Quantity._compare(self, other, Py_EQ)

    def __lt__(self, other) -> bool:
        return Quantity._compare(self, other, Py_LT)

    def __le__(self, other) -> bool:
        return Quantity._compare(self, other, Py_LE)

    def __gt__(self, other) -> bool:
        return Quantity._compare(self, other, Py_GT)

    def __ge__(self, other) -> bool:
        return Quantity._compare(self, other, Py_GE)

    def __add__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) + float(b)
        return Quantity._extract_decimal(a) + Quantity._extract_decimal(b)

    def __radd__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) + float(b)
        return Quantity._extract_decimal(a) + Quantity._extract_decimal(b)

    def __sub__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) - float(b)
        return Quantity._extract_decimal(a) - Quantity._extract_decimal(b)

    def __rsub__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) - float(b)
        return Quantity._extract_decimal(a) - Quantity._extract_decimal(b)

    def __mul__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) * float(b)
        return Quantity._extract_decimal(a) * Quantity._extract_decimal(b)

    def __rmul__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) * float(b)
        return Quantity._extract_decimal(a) * Quantity._extract_decimal(b)

    def __truediv__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) / float(b)
        return Quantity._extract_decimal(a) / Quantity._extract_decimal(b)

    def __rtruediv__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) / float(b)
        return Quantity._extract_decimal(a) / Quantity._extract_decimal(b)

    def __floordiv__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) // float(b)
        return Quantity._extract_decimal(a) // Quantity._extract_decimal(b)

    def __rfloordiv__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) // float(b)
        return Quantity._extract_decimal(a) // Quantity._extract_decimal(b)

    def __mod__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) % float(b)
        return Quantity._extract_decimal(a) % Quantity._extract_decimal(b)

    def __rmod__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) % float(b)
        return Quantity._extract_decimal(a) % Quantity._extract_decimal(b)

    def __neg__(self) -> decimal.Decimal:
        return self.as_decimal().__neg__()

    def __pos__(self) -> decimal.Decimal:
        return self.as_decimal().__pos__()

    def __abs__(self) -> decimal.Decimal:
        return abs(self.as_decimal())

    def __round__(self, ndigits = None) -> decimal.Decimal:
        return round(self.as_decimal(), ndigits)

    def __float__(self) -> float:
        return self.as_f64_c()

    def __int__(self) -> int:
        return int(self.as_decimal())

    def __hash__(self) -> int:
        return hash(self._mem.raw)

    def __str__(self) -> str:
        return f"{self.as_decimal():.{self._mem.precision}f}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @property
    def raw(self) -> QuantityRaw:
        """
        Return the raw memory representation of the quantity value.

        Returns
        -------
        int

        """
        return self._mem.raw

    @property
    def precision(self) -> int:
        """
        Return the precision for the quantity.

        Returns
        -------
        uint8_t

        """
        return self._mem.precision

    cdef bint eq(self, Quantity other):
        Condition.not_none(other, "other")
        return self._mem.raw == other._mem.raw

    cdef bint ne(self, Quantity other):
        Condition.not_none(other, "other")
        return self._mem.raw != other._mem.raw

    cdef bint lt(self, Quantity other):
        Condition.not_none(other, "other")
        return self._mem.raw < other._mem.raw

    cdef bint le(self, Quantity other):
        Condition.not_none(other, "other")
        return self._mem.raw <= other._mem.raw

    cdef bint gt(self, Quantity other):
        Condition.not_none(other, "other")
        return self._mem.raw > other._mem.raw

    cdef bint ge(self, Quantity other):
        Condition.not_none(other, "other")
        return self._mem.raw >= other._mem.raw

    cdef bint is_zero(self):
        return self._mem.raw == 0

    cdef bint is_negative(self):
        return self._mem.raw < 0

    cdef bint is_positive(self):
        return self._mem.raw > 0

    cdef Quantity add(self, Quantity other):
        return Quantity.from_raw_c(self._mem.raw + other._mem.raw, self._mem.precision)

    cdef Quantity sub(self, Quantity other):
        return Quantity.from_raw_c(self._mem.raw - other._mem.raw, self._mem.precision)

    cdef Quantity saturating_sub(self, Quantity other):
        return Quantity.from_mem_c(quantity_saturating_sub(self._mem, other._mem))

    cdef void add_assign(self, Quantity other):
        self._mem.raw += other._mem.raw
        if self._mem.precision == 0:
            self._mem.precision = other.precision

    cdef void sub_assign(self, Quantity other):
        self._mem.raw -= other._mem.raw
        if self._mem.precision == 0:
            self._mem.precision = other.precision

    cdef QuantityRaw raw_uint_c(self):
        return self._mem.raw

    cdef double as_f64_c(self):
        return self._mem.raw / RUST_FIXED_SCALAR

    @staticmethod
    cdef double raw_to_f64_c(QuantityRaw raw):
        return raw / RUST_FIXED_SCALAR

    @staticmethod
    def raw_to_f64(raw) -> float:
        return Quantity.raw_to_f64_c(raw)

    @staticmethod
    cdef Quantity from_mem_c(Quantity_t mem):
        cdef Quantity quantity = Quantity.__new__(Quantity)
        quantity._mem = mem
        return quantity

    @staticmethod
    cdef Quantity from_raw_c(QuantityRaw raw, uint8_t precision):
        cdef Quantity quantity = Quantity.__new__(Quantity)
        quantity._mem = quantity_from_raw(raw, precision)
        return quantity

    @staticmethod
    cdef object _extract_decimal(object obj):
        assert not isinstance(obj, float)  # Design-time error
        if hasattr(obj, "as_decimal"):
            return obj.as_decimal()
        else:
            return decimal.Decimal(obj)

    @staticmethod
    cdef bint _compare(a, b, int op):
        if isinstance(a, Quantity):
            a = <Quantity>a.as_decimal()
        elif isinstance(a, Price):
            a = <Price>a.as_decimal()

        if isinstance(b, Quantity):
            b = <Quantity>b.as_decimal()
        elif isinstance(b, Price):
            b = <Price>b.as_decimal()

        return PyObject_RichCompareBool(a, b, op)

    @staticmethod
    cdef Quantity zero_c(uint8_t precision):
        return Quantity(0, precision)

    @staticmethod
    cdef Quantity from_str_c(str value):
        value = value.replace('_', '')

        cdef uint8_t precision = precision_from_cstr(pystr_to_cstr(value))
        if precision > FIXED_PRECISION:
            raise ValueError(
                f"invalid `precision` greater than max {FIXED_PRECISION}, was {precision}"
            )

        decimal_value = decimal.Decimal(value)

        if decimal_value < 0:
            raise ValueError(
                f"invalid negative quantity, was {value}"
            )

        scaled = decimal_value * (10 ** precision)
        integral = scaled.to_integral_value(rounding=decimal.ROUND_HALF_EVEN)

        raw_py = int(integral) * (10 ** (FIXED_PRECISION - precision))
        if raw_py > QUANTITY_RAW_MAX:
            raise ValueError(
                f"invalid raw quantity value exceeds max {QUANTITY_RAW_MAX}, was {raw_py}"
            )

        cdef QuantityRaw raw = <QuantityRaw>(raw_py)
        return Quantity.from_raw_c(raw, precision)

    @staticmethod
    cdef Quantity from_int_c(QuantityRaw value):
        return Quantity(value, precision=0)

    @staticmethod
    def zero(uint8_t precision=0) -> Quantity:
        """
        Return a quantity with a value of zero.

        precision : uint8_t, default 0
            The precision for the quantity.

        Returns
        -------
        Quantity

        Raises
        ------
        ValueError
            If `precision` is greater than 16.
        OverflowError
            If `precision` is negative (< 0).

        Warnings
        --------
        The default precision is zero.

        """
        return Quantity.zero_c(precision)

    @staticmethod
    def from_raw(raw: int, uint8_t precision) -> Quantity:
        """
        Return a quantity from the given `raw` fixed-point integer and `precision`.

        Handles up to 16 decimals of precision (in high-precision mode).

        Parameters
        ----------
        raw : int
            The raw fixed-point quantity value.
        precision : uint8_t
            The precision for the quantity. Use a precision of 0 for whole numbers
            (no fractional units).

        Returns
        -------
        Quantity

        Raises
        ------
        ValueError
            If `precision` is greater than 16.
        OverflowError
            If `precision` is negative (< 0).

        Warnings
        --------
        Small `raw` values can produce a zero quantity depending on the `precision`.

        """
        if precision > FIXED_PRECISION:
            raise ValueError(
                f"invalid `precision` greater than max {FIXED_PRECISION}, was {precision}"
            )
        return Quantity.from_raw_c(raw, precision)

    @staticmethod
    def from_str(str value) -> Quantity:
        """
        Return a quantity parsed from the given string.

        Handles up to 16 decimals of precision (in high-precision mode).

        Parameters
        ----------
        value : str
            The value to parse.

        Returns
        -------
        Quantity

        Raises
        ------
        ValueError
            If inferred precision is greater than 16.
        ValueError
            If raw value is outside the valid representable range [0, `QUANTITY_RAW_MAX`].
        OverflowError
            If inferred precision is negative (< 0).

        Warnings
        --------
        The decimal precision will be inferred from the number of digits
        following the '.' point (if no point then precision zero).

        """
        Condition.not_none(value, "value")

        return Quantity.from_str_c(value)

    @staticmethod
    def from_int(value: int) -> Quantity:
        """
        Return a quantity from the given integer value.

        A precision of zero will be inferred.

        Parameters
        ----------
        value : int
            The value for the quantity.

        Returns
        -------
        Quantity

        """
        Condition.not_none(value, "value")

        return Quantity.from_int_c(value)

    cpdef str to_formatted_str(self):
        """
        Return the formatted string representation of the quantity.

        Returns
        -------
        str

        """
        return f"{self.as_decimal():,.{self._mem.precision}f}".replace(",", "_")

    cpdef object as_decimal(self):
        """
        Return the value as a built-in `Decimal`.

        Returns
        -------
        Decimal

        """
        raw_decimal = decimal.Decimal(self._mem.raw)
        return raw_decimal / FIXED_DECIMAL_SCALE

    cpdef double as_double(self):
        """
        Return the value as a `double`.

        Returns
        -------
        double

        """
        return self.as_f64_c()


@cython.auto_pickle(True)
cdef class Price:
    """
    Represents a price in a market.

    The number of decimal places may vary. For certain asset classes, prices may
    have negative values. For example, prices for options instruments can be
    negative under certain conditions.

    Handles up to 16 decimals of precision (in high-precision mode).

    - ``PRICE_MAX`` = 17_014_118_346_046
    - ``PRICE_MIN`` = -17_014_118_346_046

    Parameters
    ----------
    value : integer, float, string or Decimal
        The value of the price.
    precision : uint8_t
        The precision for the price. Use a precision of 0 for whole numbers
        (no fractional units).

    Raises
    ------
    ValueError
        If `value` is greater than 17_014_118_346_046.
    ValueError
        If `value` is less than -17_014_118_346_046.
    ValueError
        If `precision` is greater than 16.
    OverflowError
        If `precision` is negative (< 0).

    References
    ----------
    https://www.onixs.biz/fix-dictionary/5.0.SP2/index.html#Price
    """

    def __init__(self, double value, uint8_t precision) -> None:
        if precision > FIXED_PRECISION:
            raise ValueError(
                f"invalid `precision` greater than max {FIXED_PRECISION}, was {precision}"
            )
        if isnan(value):
            raise ValueError(
                f"invalid `value`, was {value:_}",
            )
        if value > RUST_PRICE_MAX:
            raise ValueError(
                f"invalid `value` greater than `PRICE_MAX` {RUST_PRICE_MAX:_}, was {value:_}",
            )
        if value < RUST_PRICE_MIN:
            raise ValueError(
                f"invalid `value` less than `PRICE_MIX` {RUST_PRICE_MIN:_}, was {value:_}",
            )

        self._mem = price_new(value, precision)

    def __getstate__(self):
        return self._mem.raw, self._mem.precision

    def __setstate__(self, state):
        self._mem = price_from_raw(state[0], state[1])

    def __eq__(self, other) -> bool:
        return Price._compare(self, other, Py_EQ)

    def __lt__(self, other) -> bool:
        return Price._compare(self, other, Py_LT)

    def __le__(self, other) -> bool:
        return Price._compare(self, other, Py_LE)

    def __gt__(self, other) -> bool:
        return Price._compare(self, other, Py_GT)

    def __ge__(self, other) -> bool:
        return Price._compare(self, other, Py_GE)

    def __add__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) + float(b)
        return Price._extract_decimal(a) + Price._extract_decimal(b)

    def __radd__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) + float(b)
        return Price._extract_decimal(a) + Price._extract_decimal(b)

    def __sub__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) - float(b)
        return Price._extract_decimal(a) - Price._extract_decimal(b)

    def __rsub__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) - float(b)
        return Price._extract_decimal(a) - Price._extract_decimal(b)

    def __mul__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) * float(b)
        return Price._extract_decimal(a) * Price._extract_decimal(b)

    def __rmul__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) * float(b)
        return Price._extract_decimal(a) * Price._extract_decimal(b)

    def __truediv__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) / float(b)
        return Price._extract_decimal(a) / Price._extract_decimal(b)

    def __rtruediv__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) / float(b)
        return Price._extract_decimal(a) / Price._extract_decimal(b)

    def __floordiv__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) // float(b)
        return Price._extract_decimal(a) // Price._extract_decimal(b)

    def __rfloordiv__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) // float(b)
        return Price._extract_decimal(a) // Price._extract_decimal(b)

    def __mod__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) % float(b)
        return Price._extract_decimal(a) % Price._extract_decimal(b)

    def __rmod__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) % float(b)
        return Price._extract_decimal(a) % Price._extract_decimal(b)

    def __neg__(self) -> decimal.Decimal:
        return self.as_decimal().__neg__()

    def __pos__(self) -> decimal.Decimal:
        return self.as_decimal().__pos__()

    def __abs__(self) -> decimal.Decimal:
        return abs(self.as_decimal())

    def __round__(self, ndigits = None) -> decimal.Decimal:
        return round(self.as_decimal(), ndigits)

    def __float__(self) -> float:
        return self.as_f64_c()

    def __int__(self) -> int:
        return int(self.as_decimal())

    def __hash__(self) -> int:
        return hash(self._mem.raw)

    def __str__(self) -> str:
        return f"{self.as_decimal():.{self._mem.precision}f}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self})"

    @property
    def raw(self) -> PriceRaw:
        """
        Return the raw memory representation of the price value.

        Returns
        -------
        int

        """
        return self._mem.raw

    @property
    def precision(self) -> int:
        """
        Return the precision for the price.

        Returns
        -------
        uint8_t

        """
        return self._mem.precision

    @staticmethod
    cdef Price from_mem_c(Price_t mem):
        cdef Price price = Price.__new__(Price)
        price._mem = mem
        return price

    @staticmethod
    cdef Price from_raw_c(PriceRaw raw, uint8_t precision):
        cdef Price price = Price.__new__(Price)
        price._mem = price_from_raw(raw, precision)
        return price

    @staticmethod
    cdef object _extract_decimal(object obj):
        assert not isinstance(obj, float)  # Design-time error
        if hasattr(obj, "as_decimal"):
            return obj.as_decimal()
        else:
            return decimal.Decimal(obj)

    @staticmethod
    cdef bint _compare(a, b, int op):
        if isinstance(a, Quantity):
            a = <Quantity>a.as_decimal()
        elif isinstance(a, Price):
            a = <Price>a.as_decimal()

        if isinstance(b, Quantity):
            b = <Quantity>b.as_decimal()
        elif isinstance(b, Price):
            b = <Price>b.as_decimal()

        return PyObject_RichCompareBool(a, b, op)

    @staticmethod
    cdef double raw_to_f64_c(PriceRaw raw):
        return raw / RUST_FIXED_SCALAR

    @staticmethod
    cdef Price from_str_c(str value):
        value = value.replace('_', '')

        cdef uint8_t precision = precision_from_cstr(pystr_to_cstr(value))
        if precision > FIXED_PRECISION:
            raise ValueError(
                f"invalid `precision` greater than max {FIXED_PRECISION}, was {precision}"
            )

        decimal_value = decimal.Decimal(value)
        scaled = decimal_value * (10 ** precision)
        integral = scaled.to_integral_value(rounding=decimal.ROUND_HALF_EVEN)

        raw_py = int(integral) * (10 ** (FIXED_PRECISION - precision))
        if raw_py < PRICE_RAW_MIN or raw_py > PRICE_RAW_MAX:
            raise ValueError(
                f"invalid raw price value outside range [{PRICE_RAW_MIN}, {PRICE_RAW_MAX}], was {raw_py}"
            )

        cdef PriceRaw raw = <PriceRaw>(raw_py)
        return Price.from_raw_c(raw, precision)

    @staticmethod
    cdef Price from_int_c(PriceRaw value):
        return Price(value, precision=0)

    cdef bint eq(self, Price other):
        Condition.not_none(other, "other")
        return self._mem.raw == other._mem.raw

    cdef bint ne(self, Price other):
        Condition.not_none(other, "other")
        return self._mem.raw != other._mem.raw

    cdef bint lt(self, Price other):
        Condition.not_none(other, "other")
        return self._mem.raw < other._mem.raw

    cdef bint le(self, Price other):
        Condition.not_none(other, "other")
        return self._mem.raw <= other._mem.raw

    cdef bint gt(self, Price other):
        Condition.not_none(other, "other")
        return self._mem.raw > other._mem.raw

    cdef bint ge(self, Price other):
        Condition.not_none(other, "other")
        return self._mem.raw >= other._mem.raw

    cdef bint is_zero(self):
        return self._mem.raw == 0

    cdef bint is_negative(self):
        return self._mem.raw < 0

    cdef bint is_positive(self):
        return self._mem.raw > 0

    cdef Price add(self, Price other):
        return Price.from_raw_c(self._mem.raw + other._mem.raw, self._mem.precision)

    cdef Price sub(self, Price other):
        return Price.from_raw_c(self._mem.raw - other._mem.raw, self._mem.precision)

    cdef void add_assign(self, Price other):
        self._mem.raw += other._mem.raw

    cdef void sub_assign(self, Price other):
        self._mem.raw -= other._mem.raw

    cdef PriceRaw raw_int_c(self):
        return self._mem.raw

    cdef double as_f64_c(self):
        return self._mem.raw / RUST_FIXED_SCALAR

    @staticmethod
    def from_raw(raw: int, uint8_t precision) -> Price:
        """
        Return a price from the given `raw` fixed-point integer and `precision`.

        Handles up to 16 decimals of precision (in high-precision mode).

        Parameters
        ----------
        raw : int
            The raw fixed-point price value.
        precision : uint8_t
            The precision for the price. Use a precision of 0 for whole numbers
            (no fractional units).

        Returns
        -------
        Price

        Raises
        ------
        ValueError
            If `precision` is greater than 16.
        OverflowError
            If `precision` is negative (< 0).

        Warnings
        --------
        Small `raw` values can produce a zero price depending on the `precision`.

        """
        if precision > FIXED_PRECISION:
            raise ValueError(
                f"invalid `precision` greater than max {FIXED_PRECISION}, was {precision}"
            )
        return Price.from_raw_c(raw, precision)

    @staticmethod
    def from_str(str value) -> Price:
        """
        Return a price parsed from the given string.

        Handles up to 16 decimals of precision (in high-precision mode).

        Parameters
        ----------
        value : str
            The value to parse.

        Returns
        -------
        Price

        Warnings
        --------
        The decimal precision will be inferred from the number of digits
        following the '.' point (if no point then precision zero).

        Raises
        ------
        ValueError
            If inferred precision is greater than 16.
        ValueError
            If raw value is outside the valid representable range [`PRICE_RAW_MIN`, `PRICE_RAW_MAX`].
        OverflowError
            If inferred precision is negative (< 0).

        """
        Condition.not_none(value, "value")

        return Price.from_str_c(value)

    @staticmethod
    def from_int(value: int) -> Price:
        """
        Return a price from the given integer value.

        A precision of zero will be inferred.

        Parameters
        ----------
        value : int
            The value for the price.

        Returns
        -------
        Price

        """
        Condition.not_none(value, "value")

        return Price.from_int_c(value)

    cpdef str to_formatted_str(self):
        """
        Return the formatted string representation of the price.

        Returns
        -------
        str

        """
        return f"{self.as_decimal():,.{self._mem.precision}f}".replace(",", "_")

    cpdef object as_decimal(self):
        """
        Return the value as a built-in `Decimal`.

        Returns
        -------
        Decimal

        """
        raw_decimal = decimal.Decimal(self._mem.raw)
        return raw_decimal / FIXED_DECIMAL_SCALE

    cpdef double as_double(self):
        """
        Return the value as a `double`.

        Returns
        -------
        double

        """
        return self.as_f64_c()


cdef class Money:
    """
    Represents an amount of money in a specified currency denomination.

    - ``MONEY_MAX`` = 17_014_118_346_046
    - ``MONEY_MIN`` = -17_014_118_346_046

    Parameters
    ----------
    value : integer, float, string or Decimal
        The amount of money in the currency denomination.
    currency : Currency
        The currency of the money.

    Raises
    ------
    ValueError
        If `value` is greater than 17_014_118_346_046.
    ValueError
        If `value` is less than -17_014_118_346_046.
    """

    def __init__(self, value, Currency currency not None) -> None:
        cdef double value_f64 = 0.0 if value is None else float(value)

        if isnan(value_f64):
            raise ValueError(
                f"invalid `value`, was {value:_}",
            )
        if value_f64 > RUST_MONEY_MAX:
            raise ValueError(
                f"invalid `value` greater than `MONEY_MAX` {RUST_MONEY_MAX:_}, was {value:_}",
            )
        if value_f64 < RUST_MONEY_MIN:
            raise ValueError(
                f"invalid `value` less than `MONEY_MIN` {RUST_MONEY_MIN:_}, was {value:_}",
            )

        self._mem = money_new(value_f64, currency._mem)

    def __getstate__(self):
        return self._mem.raw, self.currency_code_c()

    def __setstate__(self, state):
        cdef Currency currency = Currency.from_str_c(state[1])
        self._mem = money_from_raw(state[0], currency._mem)

    def __eq__(self, Money other) -> bool:
        Condition.not_none(other, "other")
        if self._mem.currency.code != other._mem.currency.code:
            Condition.is_true(self._mem.currency.code == other._mem.currency.code, f"currency {self.currency.code} != other.currency {other.currency.code}")
        return self._mem.raw == other._mem.raw

    def __lt__(self, Money other) -> bool:
        Condition.not_none(other, "other")
        Condition.is_true(self._mem.currency.code == other._mem.currency.code, "currency != other.currency")
        return self._mem.raw < other._mem.raw

    def __le__(self, Money other) -> bool:
        Condition.not_none(other, "other")
        Condition.is_true(self._mem.currency.code == other._mem.currency.code, "currency != other.currency")
        return self._mem.raw <= other._mem.raw

    def __gt__(self, Money other) -> bool:
        Condition.not_none(other, "other")
        Condition.is_true(self._mem.currency.code == other._mem.currency.code, "currency != other.currency")
        return self._mem.raw > other._mem.raw

    def __ge__(self, Money other) -> bool:
        Condition.not_none(other, "other")
        Condition.is_true(self._mem.currency.code == other._mem.currency.code, "currency != other.currency")
        return self._mem.raw >= other._mem.raw

    def __add__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) + float(b)
        return Money._extract_decimal(a) + Money._extract_decimal(b)

    def __radd__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) + float(b)
        return Money._extract_decimal(a) + Money._extract_decimal(b)

    def __sub__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) - float(b)
        return Money._extract_decimal(a) - Money._extract_decimal(b)

    def __rsub__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) - float(b)
        return Money._extract_decimal(a) - Money._extract_decimal(b)

    def __mul__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) * float(b)
        return Money._extract_decimal(a) * Money._extract_decimal(b)

    def __rmul__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) * float(b)
        return Money._extract_decimal(a) * Money._extract_decimal(b)

    def __truediv__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) / float(b)
        return Money._extract_decimal(a) / Money._extract_decimal(b)

    def __rtruediv__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) / float(b)
        return Money._extract_decimal(a) / Money._extract_decimal(b)

    def __floordiv__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) // float(b)
        return Money._extract_decimal(a) // Money._extract_decimal(b)

    def __rfloordiv__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) // float(b)
        return Money._extract_decimal(a) // Money._extract_decimal(b)

    def __mod__(a, b) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) % float(b)
        return Money._extract_decimal(a) % Money._extract_decimal(b)

    def __rmod__(b, a) -> decimal.Decimal | float:
        if isinstance(a, float) or isinstance(b, float):
            return float(a) % float(b)
        return Money._extract_decimal(a) % Money._extract_decimal(b)

    def __neg__(self) -> decimal.Decimal:
        return self.as_decimal().__neg__()

    def __pos__(self) -> decimal.Decimal:
        return self.as_decimal().__pos__()

    def __abs__(self) -> decimal.Decimal:
        return abs(self.as_decimal())

    def __round__(self, ndigits = None) -> decimal.Decimal:
        return round(self.as_decimal(), ndigits)

    def __float__(self) -> float:
        return self.as_f64_c()

    def __int__(self) -> int:
        return int(self.as_decimal())

    def __hash__(self) -> int:
        return hash((self._mem.raw, self.currency_code_c()))

    def __str__(self) -> str:
        return f"{self.as_decimal():.{self._mem.currency.precision}f} {self.currency_code_c()}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.as_decimal():.{self._mem.currency.precision}f}, {self.currency_code_c()})"

    @property
    def raw(self) -> MoneyRaw:
        """
        Return the raw memory representation of the money amount.

        Returns
        -------
        int

        """
        return self._mem.raw

    @property
    def currency(self) -> Currency:
        """
        Return the currency for the money.

        Returns
        -------
        Currency

        """
        return Currency.from_str_c(self.currency_code_c())

    @staticmethod
    cdef double raw_to_f64_c(MoneyRaw raw):
        return raw / RUST_FIXED_SCALAR

    @staticmethod
    cdef Money from_raw_c(MoneyRaw raw, Currency currency):
        cdef Money money = Money.__new__(Money)
        money._mem = money_from_raw(raw, currency._mem)
        return money

    @staticmethod
    cdef object _extract_decimal(object obj):
        assert not isinstance(obj, float)  # Design-time error
        if hasattr(obj, "as_decimal"):
            return obj.as_decimal()
        else:
            return decimal.Decimal(obj)

    @staticmethod
    cdef Money from_str_c(str value):
        cdef list pieces = value.split(' ', maxsplit=1)

        if len(pieces) != 2:
            raise ValueError(f"The `Money` string value was malformed, was {value}")

        amount_str = pieces[0].replace('_', '')

        cdef Currency currency = Currency.from_str_c(pieces[1])
        cdef uint8_t precision = currency._mem.precision
        decimal_value = decimal.Decimal(amount_str)
        scaled = decimal_value * (10 ** precision)
        integral = scaled.to_integral_value(rounding=decimal.ROUND_HALF_EVEN)

        raw_py = int(integral) * (10 ** (FIXED_PRECISION - precision))
        if raw_py < MONEY_RAW_MIN or raw_py > MONEY_RAW_MAX:
            raise ValueError(
                f"invalid raw money value outside range [{MONEY_RAW_MIN}, {MONEY_RAW_MAX}], was {raw_py}"
            )

        cdef MoneyRaw raw = <MoneyRaw>(raw_py)
        return Money.from_raw_c(raw, currency)

    cdef str currency_code_c(self):
        return cstr_to_pystr(currency_code_to_cstr(&self._mem.currency))

    cdef bint is_zero(self):
        return self._mem.raw == 0

    cdef bint is_negative(self):
        return self._mem.raw < 0

    cdef bint is_positive(self):
        return self._mem.raw > 0

    cdef Money add(self, Money other):
        Condition.not_none(other, "other")
        Condition.is_true(self._mem.currency.code == other._mem.currency.code, "currency != other.currency")
        return Money.from_raw_c(self._mem.raw + other._mem.raw, self.currency)

    cdef Money sub(self, Money other):
        Condition.not_none(other, "other")
        Condition.is_true(self._mem.currency.code == other._mem.currency.code, "currency != other.currency")
        return Money.from_raw_c(self._mem.raw - other._mem.raw, self.currency)

    cdef void add_assign(self, Money other):
        Condition.not_none(other, "other")
        Condition.is_true(self._mem.currency.code == other._mem.currency.code, "currency != other.currency")
        self._mem.raw += other._mem.raw

    cdef void sub_assign(self, Money other):
        Condition.not_none(other, "other")
        Condition.is_true(self._mem.currency.code == other._mem.currency.code, "currency != other.currency")
        self._mem.raw -= other._mem.raw

    cdef MoneyRaw raw_int_c(self):
        return self._mem.raw

    cdef double as_f64_c(self):
        return self._mem.raw / RUST_FIXED_SCALAR

    @staticmethod
    def from_raw(raw: int, Currency currency) -> Money:
        """
        Return money from the given `raw` fixed-point integer and `currency`.

        Parameters
        ----------
        raw : int
            The raw fixed-point money amount.
        currency : Currency
            The currency of the money.

        Returns
        -------
        Money

        Warnings
        --------
        Small `raw` values can produce a zero money amount depending on the precision of the currency.

        """
        Condition.not_none(currency, "currency")
        return Money.from_raw_c(raw, currency)

    @staticmethod
    def from_str(str value) -> Money:
        """
        Return money parsed from the given string.

        Must be correctly formatted with a value and currency separated by a
        whitespace delimiter.

        Example: "1000000.00 USD".

        Parameters
        ----------
        value : str
            The value to parse.

        Returns
        -------
        Money

        Raises
        ------
        ValueError
            If inferred currency precision is greater than 16.
        ValueError
            If raw value is outside the valid representable range [`MONEY_RAW_MIN`, `MONEY_RAW_MAX`].
        OverflowError
            If inferred currency precision is negative (< 0).

        """
        Condition.not_none(value, "value")

        cdef tuple pieces = value.partition(' ')

        if len(pieces) != 3:
            raise ValueError(f"The `Money` string value was malformed, was {value}")

        return Money.from_str_c(value)

    cpdef str to_formatted_str(self):
        """
        Return the formatted string representation of the money.

        Returns
        -------
        str

        """
        return f"{self.as_decimal():,.{self._mem.currency.precision}f} {self.currency_code_c()}".replace(",", "_")

    cpdef object as_decimal(self):
        """
        Return the value as a built-in `Decimal`.

        Returns
        -------
        Decimal

        """
        raw_decimal = decimal.Decimal(self._mem.raw)
        return raw_decimal / FIXED_DECIMAL_SCALE

    cpdef double as_double(self):
        """
        Return the value as a `double`.

        Returns
        -------
        double

        """
        return self.as_f64_c()


cdef class Currency:
    """
    Represents a medium of exchange in a specified denomination with a fixed
    decimal precision.

    Handles up to 16 decimals of precision (in high-precision mode).

    Parameters
    ----------
    code : str
        The currency code.
    precision : uint8_t
        The currency decimal precision.
    iso4217 : uint16
        The currency ISO 4217 code.
    name : str
        The currency name.
    currency_type : CurrencyType
        The currency type.

    Raises
    ------
    ValueError
        If `code` is not a valid string.
    OverflowError
        If `precision` is negative (< 0).
    ValueError
        If `precision` greater than 16.
    ValueError
        If `name` is not a valid string.
    """

    def __init__(
        self,
        str code,
        uint8_t precision,
        uint16_t iso4217,
        str name,
        CurrencyType currency_type,
    ) -> None:
        Condition.valid_string(code, "code")
        Condition.valid_string(name, "name")
        if precision > FIXED_PRECISION:
            raise ValueError(
                f"invalid `precision` greater than max {FIXED_PRECISION}, was {precision}"
            )

        self._mem = currency_from_py(
            pystr_to_cstr(code),
            precision,
            iso4217,
            pystr_to_cstr(name),
            currency_type,
        )

    def __getstate__(self):
        return (
            self.code,
            self._mem.precision,
            self._mem.iso4217,
            self.name,
            <CurrencyType>self._mem.currency_type,
        )

    def __setstate__(self, state):
        self._mem = currency_from_py(
            pystr_to_cstr(state[0]),
            state[1],
            state[2],
            pystr_to_cstr(state[3]),
            state[4],
        )

    def __eq__(self, Currency other) -> bool:
        if other is None:
            raise RuntimeError("other was None in __eq__")
        return strcmp(self._mem.code, other._mem.code) == 0

    def __hash__(self) -> int:
        return currency_hash(&self._mem)

    def __str__(self) -> str:
        return ustr_to_pystr(self._mem.code)

    def __repr__(self) -> str:
        return cstr_to_pystr(currency_to_cstr(&self._mem))

    @property
    def code(self) -> str:
        """
        Return the currency code.

        Returns
        -------
        str

        """
        return ustr_to_pystr(self._mem.code)

    @property
    def name(self) -> str:
        """
        Return the currency name.

        Returns
        -------
        str

        """
        return ustr_to_pystr(self._mem.name)

    @property
    def precision(self) -> int:
        """
        Return the currency decimal precision.

        Returns
        -------
        uint8

        """
        return self._mem.precision

    @property
    def iso4217(self) -> int:
        """
        Return the currency ISO 4217 code.

        Returns
        -------
        str

        """
        return self._mem.iso4217

    @property
    def currency_type(self) -> CurrencyType:
        """
        Return the currency type.

        Returns
        -------
        CurrencyType

        """
        return <CurrencyType>self._mem.currency_type

    cdef uint8_t get_precision(self):
        return self._mem.precision

    @staticmethod
    cdef void register_c(Currency currency, bint overwrite=False):
        cdef Currency existing = Currency.from_internal_map_c(currency.code)
        if existing is not None and not overwrite:
            return  # Already exists in internal map
        currency_register(currency._mem)

    @staticmethod
    cdef Currency from_internal_map_c(str code):
        cdef const char* code_ptr = pystr_to_cstr(code)
        if not currency_exists(code_ptr):
            return None
        cdef Currency currency = Currency.__new__(Currency)
        currency._mem = currency_from_cstr(code_ptr)
        return currency

    @staticmethod
    cdef Currency from_str_c(str code, bint strict=False):
        cdef Currency currency = Currency.from_internal_map_c(code)
        if currency is not None:
            return currency
        if strict:
            return None

        # Strict mode false with no currency found (very likely a crypto)
        currency = Currency(
            code=code,
            precision=8,
            iso4217=0,
            name=code,
            currency_type=CurrencyType.CRYPTO,
        )
        currency_register(currency._mem)

        return currency

    @staticmethod
    cdef bint is_fiat_c(str code):
        cdef Currency currency = Currency.from_internal_map_c(code)
        if currency is None:
            return False

        return <CurrencyType>currency._mem.currency_type == CurrencyType.FIAT

    @staticmethod
    cdef bint is_crypto_c(str code):
        cdef Currency currency = Currency.from_internal_map_c(code)
        if currency is None:
            return False

        return <CurrencyType>currency._mem.currency_type == CurrencyType.CRYPTO

    @staticmethod
    def register(Currency currency, bint overwrite=False):
        """
        Register the given `currency`.

        Will override the internal currency map.

        Parameters
        ----------
        currency : Currency
            The currency to register
        overwrite : bool
            If the currency in the internal currency map should be overwritten.

        """
        Condition.not_none(currency, "currency")

        return Currency.register_c(currency, overwrite)

    @staticmethod
    def from_internal_map(str code):
        """
        Return the currency with the given `code` from the built-in internal map (if found).

        Parameters
        ----------
        code : str
            The code of the currency.

        Returns
        -------
        Currency or ``None``

        """
        Condition.not_none(code, "code")

        return Currency.from_internal_map_c(code)

    @staticmethod
    def from_str(str code, bint strict=False):
        """
        Parse a currency from the given string (if found).

        Parameters
        ----------
        code : str
            The code of the currency.
        strict : bool, default False
            If not `strict` mode then an unknown currency will very likely
            be a Cryptocurrency, so for robustness will then return a new
            `Currency` object using the given `code` with a default `precision` of 8.

        Returns
        -------
        Currency or ``None``

        """
        return Currency.from_str_c(code, strict)

    @staticmethod
    def is_fiat(str code):
        """
        Return whether a currency with the given code is ``FIAT``.

        Parameters
        ----------
        code : str
            The code of the currency.

        Returns
        -------
        bool
            True if ``FIAT``, else False.

        Raises
        ------
        ValueError
            If `code` is not a valid string.

        """
        Condition.valid_string(code, "code")

        return Currency.is_fiat_c(code)

    @staticmethod
    def is_crypto(str code):
        """
        Return whether a currency with the given code is ``CRYPTO``.

        Parameters
        ----------
        code : str
            The code of the currency.

        Returns
        -------
        bool
            True if ``CRYPTO``, else False.

        Raises
        ------
        ValueError
            If `code` is not a valid string.

        """
        Condition.valid_string(code, "code")

        return Currency.is_crypto_c(code)


cdef class AccountBalance:
    """
    Represents an account balance denominated in a particular currency.

    Parameters
    ----------
    total : Money
        The total account balance.
    locked : Money
        The account balance locked (assigned to pending orders).
    free : Money
        The account balance free for trading.

    Raises
    ------
    ValueError
        If money currencies are not equal.
    ValueError
        If `total` - `locked` != `free`.
    """

    def __init__(
        self,
        Money total not None,
        Money locked not None,
        Money free not None,
    ) -> None:
        Condition.equal(total.currency, locked.currency, "total.currency", "locked.currency")
        Condition.equal(total.currency, free.currency, "total.currency", "free.currency")
        Condition.is_true(total.raw_int_c() - locked.raw_int_c() == free.raw_int_c(), f"`total` ({total}) - `locked` ({locked}) != `free` ({free})")

        self.total = total
        self.locked = locked
        self.free = free
        self.currency = total.currency

    def __eq__(self, AccountBalance other) -> bool:
        return (
            self.total == other.total
            and self.locked == other.locked
            and self.free == other.free
        )

    def __hash__(self) -> int:
        return hash((self.total, self.locked, self.free))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"total={self.total.to_formatted_str()}, "
            f"locked={self.locked.to_formatted_str()}, "
            f"free={self.free.to_formatted_str()})"
        )

    @staticmethod
    cdef AccountBalance from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef Currency currency = Currency.from_str_c(values["currency"])
        return AccountBalance(
            total=Money(values["total"], currency),
            locked=Money(values["locked"], currency),
            free=Money(values["free"], currency),
        )

    @staticmethod
    def from_dict(dict values) -> AccountBalance:
        """
        Return an account balance from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        AccountBalance

        """
        return AccountBalance.from_dict_c(values)

    cpdef AccountBalance copy(self):
        """
        Return a copy of this account balance.

        Returns
        -------
        AccountBalance

        """
        return AccountBalance(
            total=self.total,
            locked=self.locked,
            free=self.free,
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
            "total": f"{self.total.as_decimal():.{self.currency.precision}f}",
            "locked": f"{self.locked.as_decimal():.{self.currency.precision}f}",
            "free": f"{self.free.as_decimal():.{self.currency.precision}f}",
            "currency": self.currency.code,
        }


cdef class MarginBalance:
    """
    Represents a margin balance optionally associated with a particular instrument.

    Parameters
    ----------
    initial : Money
        The initial (order) margin requirement for the instrument.
    maintenance : Money
        The maintenance (position) margin requirement for the instrument.
    instrument_id : InstrumentId, optional
        The instrument ID associated with the margin.

    Raises
    ------
    ValueError
        If `margin_init` currency does not equal `currency`.
    ValueError
        If `margin_maint` currency does not equal `currency`.
    ValueError
        If any margin is negative (< 0).
    """

    def __init__(
        self,
        Money initial not None,
        Money maintenance not None,
        InstrumentId instrument_id = None,
    ) -> None:
        Condition.equal(initial.currency, maintenance.currency, "initial.currency", "maintenance.currency")
        Condition.is_true(initial.raw_int_c() >= 0, f"initial margin was negative ({initial})")
        Condition.is_true(maintenance.raw_int_c() >= 0, f"maintenance margin was negative ({maintenance})")

        self.initial = initial
        self.maintenance = maintenance
        self.currency = initial.currency
        self.instrument_id = instrument_id

    def __eq__(self, MarginBalance other) -> bool:
        return (
            self.initial == other.initial
            and self.maintenance == other.maintenance
            and self.instrument_id == other.instrument_id
        )

    def __hash__(self) -> int:
        return hash((self.initial, self.maintenance, self.instrument_id))

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"initial={self.initial.to_formatted_str()}, "
            f"maintenance={self.maintenance.to_formatted_str()}, "
            f"instrument_id={self.instrument_id.to_str() if self.instrument_id is not None else None})"
        )

    @staticmethod
    cdef MarginBalance from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef Currency currency = Currency.from_str_c(values["currency"])
        cdef str instrument_id_str = values.get("instrument_id")
        return MarginBalance(
            initial=Money(values["initial"], currency),
            maintenance=Money(values["maintenance"], currency),
            instrument_id=InstrumentId.from_str_c(instrument_id_str) if instrument_id_str is not None else None,
        )

    @staticmethod
    def from_dict(dict values) -> MarginBalance:
        """
        Return a margin balance from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        MarginAccountBalance

        """
        return MarginBalance.from_dict_c(values)

    cpdef MarginBalance copy(self):
        """
        Return a copy of this margin balance.

        Returns
        -------
        MarginBalance

        """
        return MarginBalance(
            initial=self.initial,
            maintenance=self.maintenance,
            instrument_id=self.instrument_id,
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
            "initial": f"{self.initial.as_decimal():.{self.currency.precision}f}",
            "maintenance": f"{self.maintenance.as_decimal():.{self.currency.precision}f}",
            "currency": self.currency.code,
            "instrument_id": self.instrument_id.to_str() if self.instrument_id is not None else None,
        }
