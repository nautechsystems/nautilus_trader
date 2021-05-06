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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.currency_type cimport CurrencyType
from nautilus_trader.model.c_enums.currency_type cimport CurrencyTypeParser
from nautilus_trader.model.currencies cimport _CURRENCY_MAP


cdef class Currency:
    """
    Represents a medium of exchange in a specified denomination with a fixed
    decimal precision.
    """

    def __init__(
        self,
        str code,
        int precision,
        int iso4217,
        str name,
        CurrencyType currency_type,
    ):
        """
        Initialize a new instance of the `Currency` class.

        Parameters
        ----------
        code : str
            The currency code.
        precision : int
            The currency decimal precision.
        iso4217 : int
            The currency ISO 4217 code.
        name : str
            The currency name.
        currency_type : CurrencyType
            The currency type.

        Raises
        ------
        ValueError
            If code is not a valid string.
        ValueError
            If precision is negative (< 0).
        ValueError
            If name is not a valid string.

        """
        Condition.valid_string(code, "code")
        Condition.valid_string(name, "name")
        Condition.not_negative_int(precision, "precision")

        self.code = code
        self.name = name
        self.precision = precision
        self.iso4217 = iso4217
        self.currency_type = currency_type

    def __eq__(self, Currency other) -> bool:
        return self.code == other.code and self.precision == other.precision

    def __ne__(self, Currency other) -> bool:
        return self.code != other.code or self.precision != other.precision

    def __hash__(self) -> int:
        return hash((self.code, self.precision))

    def __str__(self) -> str:
        return self.code

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"code={self.code}, "
                f"name={self.name}, "
                f"precision={self.precision}, "
                f"iso4217={self.iso4217}, "
                f"type={CurrencyTypeParser.to_str(self.currency_type)})")

    @staticmethod
    cdef Currency from_str_c(str code, bint force_crypto=False):
        cdef Currency currency = _CURRENCY_MAP.get(code)
        if currency is None and force_crypto:
            currency = Currency(
                code=code,
                precision=8,
                iso4217=0,
                name=code,
                currency_type=CurrencyType.CRYPTO,
            )
        return currency

    @staticmethod
    def from_str(str code, bint force_crypto=False):
        """
        Parse a currency from the given string (if found).

        If not found and `force_crypto` is set `True`, then will return a crypto
        currency with precision 8 and name equal to the given code.

        In normal trading operations it should not be necessary to use
        `force_crypto`. Instead, the `InstrumentProvider` should handle the
        proper instantiation of available currencies upon connection

        Parameters
        ----------
        code : str
            The code of the currency to get.
        force_crypto : bool
            If an unknown crypto should be returned if code is not found in the
            internal currency map.

        Returns
        -------
        Currency or None

        Warnings
        --------
        If `force_crypto` is set to `True` then a `Currency` will always be
        returned - which may not be what you expect depending on the `code`
        input.

        """
        return Currency.from_str_c(code, force_crypto=force_crypto)

    @staticmethod
    cdef bint is_fiat_c(str code):
        cdef Currency currency = _CURRENCY_MAP.get(code)
        if currency is None:
            return False

        return currency.currency_type == CurrencyType.FIAT

    @staticmethod
    cdef bint is_crypto_c(str code):
        cdef Currency currency = _CURRENCY_MAP.get(code)
        if currency is None:
            return False

        return currency.currency_type == CurrencyType.CRYPTO

    @staticmethod
    def is_fiat(str code):
        """
        Return a value indicating whether a currency with the given code is Fiat.

        Parameters
        ----------
        code : str
            The code of the currency.

        Returns
        -------
        bool
            True if Fiat, else False.

        Raises
        ------
        ValueError
            If code is not a valid string.

        """
        Condition.valid_string(code, "code")

        return Currency.is_fiat_c(code)

    @staticmethod
    def is_crypto(str code):
        """
        Return a value indicating whether a currency with the given code is Crypto.

        Parameters
        ----------
        code : str
            The code of the currency.

        Returns
        -------
        bool
            True if Crypto, else False.

        Raises
        ------
        ValueError
            If code is not a valid string.

        """
        Condition.valid_string(code, "code")

        return Currency.is_crypto_c(code)
