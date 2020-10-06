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


# TODO: Add all currencies
cdef Currency _BTC = Currency('BTC', precision=8, currency_type=CurrencyType.CRYPTO)
cdef Currency _ETH = Currency('ETH', precision=8, currency_type=CurrencyType.CRYPTO)
cdef Currency _XRP = Currency('XRP', precision=8, currency_type=CurrencyType.CRYPTO)
cdef Currency _USDT = Currency('USDT', precision=8, currency_type=CurrencyType.CRYPTO)
cdef Currency _AUD = Currency('AUD', precision=2, currency_type=CurrencyType.FIAT)
cdef Currency _USD = Currency('USD', precision=2, currency_type=CurrencyType.FIAT)
cdef Currency _CAD = Currency('CAD', precision=2, currency_type=CurrencyType.FIAT)
cdef Currency _EUR = Currency('EUR', precision=2, currency_type=CurrencyType.FIAT)
cdef Currency _GBP = Currency('GBP', precision=2, currency_type=CurrencyType.FIAT)
cdef Currency _CHF = Currency('CHF', precision=2, currency_type=CurrencyType.FIAT)
cdef Currency _HKD = Currency('HKD', precision=2, currency_type=CurrencyType.FIAT)
cdef Currency _NZD = Currency('NZD', precision=2, currency_type=CurrencyType.FIAT)
cdef Currency _SGD = Currency('SGD', precision=2, currency_type=CurrencyType.FIAT)
cdef Currency _JPY = Currency('JPY', precision=2, currency_type=CurrencyType.FIAT)

cdef dict _CURRENCY_TABLE = {
    'BTC': _BTC,
    'ETH': _ETH,
    'XRP': _XRP,
    'USDT': _USDT,
    'AUD': _AUD,
    'USD': _USD,
    'CAD': _CAD,
    'EUR': _EUR,
    'GBP': _GBP,
    'CHF': _CHF,
    'HKD': _HKD,
    'NZD': _NZD,
    'SGD': _SGD,
    'JPY': _JPY,
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

    @staticmethod
    def BTC() -> Currency:
        """
        The Bitcoin crypto currency.

        Returns
        -------
        Currency

        """
        return _BTC

    @staticmethod
    def ETH() -> Currency:
        """
        The Ether crypto currency.

        Returns
        -------
        Currency

        """
        return _ETH

    @staticmethod
    def XRP() -> Currency:
        """
        The Ripple crypto currency.

        Returns
        -------
        Currency

        """
        return _XRP

    @staticmethod
    def USDT() -> Currency:
        """
        The Tether crypto currency.

        Returns
        -------
        Currency

        """
        return _USDT

    @staticmethod
    def AUD() -> Currency:
        """
        The Australian Dollar fiat currency.

        Returns
        -------
        Currency

        """
        return _AUD

    @staticmethod
    def USD() -> Currency:
        """
        The United States Dollar fiat currency.

        Returns
        -------
        Currency

        """
        return _USD

    @staticmethod
    def CAD() -> Currency:
        """
        The Canadian Dollar fiat currency.

        Returns
        -------
        Currency

        """
        return _CAD

    @staticmethod
    def CHF() -> Currency:
        """
        The Swiss Frank fiat currency.

        Returns
        -------
        Currency

        """
        return _CHF

    @staticmethod
    def EUR() -> Currency:
        """
        The Euro fiat currency.

        Returns
        -------
        Currency

        """
        return _EUR

    @staticmethod
    def HKD() -> Currency:
        """
        The Hong Kong Dollar currency.

        Returns
        -------
        Currency

        """
        return _HKD

    @staticmethod
    def SGD() -> Currency:
        """
        The Singapore Dollar currency.

        Returns
        -------
        Currency

        """
        return _SGD

    @staticmethod
    def NZD() -> Currency:
        """
        The New Zealand Dollar currency.

        Returns
        -------
        Currency

        """
        return _NZD

    @staticmethod
    def JPY() -> Currency:
        """
        The Japanese Yen currency.

        Returns
        -------
        Currency

        """
        return _JPY
