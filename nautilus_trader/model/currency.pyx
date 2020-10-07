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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.currency_type cimport CurrencyType
from nautilus_trader.model.c_enums.currency_type cimport currency_type_to_string

BTC = Currency('BTC', precision=8, currency_type=CurrencyType.CRYPTO)
ETH = Currency('ETH', precision=8, currency_type=CurrencyType.CRYPTO)
XRP = Currency('XRP', precision=8, currency_type=CurrencyType.CRYPTO)
USDT = Currency('USDT', precision=8, currency_type=CurrencyType.CRYPTO)
AUD = Currency('AUD', precision=2, currency_type=CurrencyType.FIAT)
USD = Currency('USD', precision=2, currency_type=CurrencyType.FIAT)
CAD = Currency('CAD', precision=2, currency_type=CurrencyType.FIAT)
EUR = Currency('EUR', precision=2, currency_type=CurrencyType.FIAT)
GBP = Currency('GBP', precision=2, currency_type=CurrencyType.FIAT)
CHF = Currency('CHF', precision=2, currency_type=CurrencyType.FIAT)
HKD = Currency('HKD', precision=2, currency_type=CurrencyType.FIAT)
NZD = Currency('NZD', precision=2, currency_type=CurrencyType.FIAT)
SGD = Currency('SGD', precision=2, currency_type=CurrencyType.FIAT)
JPY = Currency('JPY', precision=2, currency_type=CurrencyType.FIAT)


cdef dict _CURRENCY_TABLE = {
    'BTC': BTC,
    'ETH': ETH,
    'XRP': XRP,
    'USDT': USDT,
    'AUD': AUD,
    'USD': USD,
    'CAD': CAD,
    'EUR': EUR,
    'GBP': GBP,
    'CHF': CHF,
    'HKD': HKD,
    'NZD': NZD,
    'SGD': SGD,
    'JPY': JPY,
}


# noinspection PyPep8Naming
# (currency naming correct)
cdef class Currency:
    """
    Represents a medium of exchange in a specified denomination.
    """

    def __init__(
            self,
            str code,
            int precision,
            CurrencyType currency_type,
    ):
        """
        Initialize a new instance of the OrderInitialized class.

        Parameters
        ----------
        code : str
            The client order identifier.
        precision : int
            The order symbol.
        currency_type : CurrencyType
            The currency type.

        Raises
        ------
        ValueError
            If code is not a valid string.
        ValueError
            If precision is not position (> 0).
        ValueError
            If currency_type is UNDEFINED.

        """
        Condition.valid_string(code, "code")
        Condition.not_negative_int(precision, "precision")
        Condition.not_equal(currency_type, CurrencyType.UNDEFINED, "currency_type", "UNDEFINED")

        self.code = code
        self.precision = precision
        self.currency_type = currency_type

    def __eq__(self, Currency other) -> bool:
        """
        Return a value indicating whether this object is equal to (==) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return self.code == other.code \
            and self.precision == other.precision \
            and self.currency_type == other.currency_type

    def __ne__(self, Currency other) -> bool:
        """
        Return a value indicating whether this object is not equal to (!=) the given object.

        Parameters
        ----------
        other : object
            The other object to equate.

        Returns
        -------
        bool

        """
        return not self == other

    def __hash__(self) -> int:
        """
        Return the hash code of this object.

        Returns
        -------
        int

        """
        return hash(self.code)

    def __str__(self) -> str:
        """
        Return the string representation of this object.

        Returns
        -------
        str

        """
        return self.code

    def __repr__(self) -> str:
        """
        Return the string representation of this object which includes the objects
        location in memory.

        Returns
        -------
        str

        """
        return (f"{self.__class__.__name__}("
                f"code={self.code}, "
                f"precision={self.precision}, "
                f"type={currency_type_to_string(self.currency_type)})")

    @staticmethod
    def from_string(str code) -> Currency:
        """
        Return a currency from the given string.

        Parameters
        ----------
        code : str
            The code of the currency to get.

        Returns
        -------
        Currency

        """
        return _CURRENCY_TABLE.get(code)

    @staticmethod
    cdef from_string_c(str code):
        """
        Return a currency from the given string.

        Parameters
        ----------
        code : str
            The code of the currency to get.

        Returns
        -------
        Currency

        """
        return _CURRENCY_TABLE.get(code)
