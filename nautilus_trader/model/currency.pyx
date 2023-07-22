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

from libc.stdint cimport uint8_t
from libc.stdint cimport uint16_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport currency_exists
from nautilus_trader.core.rust.model cimport currency_from_cstr
from nautilus_trader.core.rust.model cimport currency_from_py
from nautilus_trader.core.rust.model cimport currency_hash
from nautilus_trader.core.rust.model cimport currency_register
from nautilus_trader.core.rust.model cimport currency_to_cstr
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.core.string cimport ustr_to_pystr
from nautilus_trader.model.enums_c cimport CurrencyType


cdef class Currency:
    """
    Represents a medium of exchange in a specified denomination with a fixed
    decimal precision.

    Handles up to 9 decimals of precision.

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
        If `precision` greater than 9.
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
    ):
        Condition.valid_string(code, "code")
        Condition.valid_string(name, "name")
        Condition.true(precision <= 9, f"invalid `precision` greater than max 9, was {precision}")

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
        return self._mem.code == other._mem.code

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
        print(f"Currency '{code}' not found, created {repr(currency)}")
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
