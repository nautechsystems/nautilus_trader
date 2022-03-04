# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.core.rust.model cimport currency_free
from nautilus_trader.core.rust.model cimport currency_new
from nautilus_trader.core.string cimport pystr_to_cstring
from nautilus_trader.model.c_enums.currency_type cimport CurrencyType
from nautilus_trader.model.c_enums.currency_type cimport CurrencyTypeParser
from nautilus_trader.model.currencies cimport _CURRENCY_MAP


cdef class Currency:
    """
    Represents a medium of exchange in a specified denomination with a fixed
    decimal precision.

    Parameters
    ----------
    code : str
        The currency code.
    precision : uint8
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
        Condition.not_negative_int(precision, "precision")

        self._currency = currency_new(
            pystr_to_cstring(code),
            precision,
            iso4217,
            pystr_to_cstring(name),
            currency_type,
        )
        self.code = code
        self.name = name

    def __getstate__(self):
        return (
            self.code,
            self._currency.precision,
            self._currency.iso4217,
            self.name,
            <CurrencyType>self._currency.currency_type,
        )

    def __setstate__(self, values):
        self._currency = currency_new(
            pystr_to_cstring(values[0]),
            values[1],
            values[2],
            pystr_to_cstring(values[3]),
            values[4],
        )
        self.code = values[0]
        self.name = values[3]

    def __eq__(self, Currency other) -> bool:
        return self.code == other.code and self._currency.precision == other._currency.precision

    def __hash__(self) -> int:
        return hash((self.code, self.precision))

    def __str__(self) -> str:
        return self.code

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"code={self.code}, "
            f"name={self.name}, "
            f"precision={self._currency.precision}, "
            f"iso4217={self._currency.iso4217}, "
            f"type={CurrencyTypeParser.to_str(<CurrencyType>self._currency.currency_type)})"
        )

    def __del__(self) -> None:
        currency_free(self._currency)  # `self._currency` moved to Rust (then dropped)

    @property
    def precision(self) -> int:
        """The currency decimal precision."""
        return self._currency.precision

    @property
    def iso4217(self) -> int:
        """The currency ISO 4217 code."""
        return self._currency.iso4217

    @property
    def currency_type(self) -> CurrencyType:
        """The currency type."""
        return <CurrencyType>self._currency.currency_type

    @staticmethod
    cdef void register_c(Currency currency, bint overwrite=False) except *:
        if not overwrite and currency.code in _CURRENCY_MAP:
            return
        _CURRENCY_MAP[currency.code] = currency

    @staticmethod
    cdef Currency from_str_c(str code):
        return _CURRENCY_MAP.get(code)

    @staticmethod
    def register(Currency currency, bint overwrite=False):
        """
        Register the given currency.

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
    def from_str(str code):
        """
        Parse a currency from the given string (if found).

        Parameters
        ----------
        code : str
            The code of the currency to get.

        Returns
        -------
        Currency or ``None``

        """
        return Currency.from_str_c(code)

    @staticmethod
    cdef bint is_fiat_c(str code):
        cdef Currency currency = _CURRENCY_MAP.get(code)
        if currency is None:
            return False

        return <CurrencyType>currency._currency.currency_type == CurrencyType.FIAT

    @staticmethod
    cdef bint is_crypto_c(str code):
        cdef Currency currency = _CURRENCY_MAP.get(code)
        if currency is None:
            return False

        return <CurrencyType>currency._currency.currency_type == CurrencyType.CRYPTO

    @staticmethod
    def is_fiat(str code):
        """
        Return a value indicating whether a currency with the given code is ``FIAT``.

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
        Return a value indicating whether a currency with the given code is ``CRYPTO``.

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
