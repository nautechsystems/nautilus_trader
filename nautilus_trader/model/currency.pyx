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
from nautilus_trader.model.c_enums.currency_type cimport CurrencyTypeParser

# Crypto currencies
BTC = Currency('BTC', precision=8, currency_type=CurrencyType.CRYPTO)
ETH = Currency('ETH', precision=8, currency_type=CurrencyType.CRYPTO)
USDT = Currency('USDT', precision=8, currency_type=CurrencyType.CRYPTO)
XRP = Currency('XRP', precision=8, currency_type=CurrencyType.CRYPTO)
BCH = Currency('BCH', precision=2, currency_type=CurrencyType.CRYPTO)
BNB = Currency('BNB', precision=4, currency_type=CurrencyType.CRYPTO)
DOT = Currency('DOT', precision=4, currency_type=CurrencyType.CRYPTO)
LINK = Currency('LINK', precision=4, currency_type=CurrencyType.CRYPTO)
LTC = Currency('LTC', precision=2, currency_type=CurrencyType.CRYPTO)

# Fiat currencies
AUD = Currency('AUD', precision=2, currency_type=CurrencyType.FIAT)
CAD = Currency('CAD', precision=2, currency_type=CurrencyType.FIAT)
CHF = Currency('CHF', precision=2, currency_type=CurrencyType.FIAT)
CNY = Currency('CNY', precision=2, currency_type=CurrencyType.FIAT)
CNH = Currency('CNH', precision=2, currency_type=CurrencyType.FIAT)
CZK = Currency('CZK', precision=2, currency_type=CurrencyType.FIAT)
EUR = Currency('EUR', precision=2, currency_type=CurrencyType.FIAT)
GBP = Currency('GBP', precision=2, currency_type=CurrencyType.FIAT)
HKD = Currency('HKD', precision=2, currency_type=CurrencyType.FIAT)
JPY = Currency('JPY', precision=2, currency_type=CurrencyType.FIAT)
MXN = Currency('MXN', precision=2, currency_type=CurrencyType.FIAT)
NOK = Currency('NOK', precision=2, currency_type=CurrencyType.FIAT)
NZD = Currency('NZD', precision=2, currency_type=CurrencyType.FIAT)
RUB = Currency('RUB', precision=2, currency_type=CurrencyType.FIAT)
SEK = Currency('SEK', precision=2, currency_type=CurrencyType.FIAT)
TRY = Currency('TRY', precision=2, currency_type=CurrencyType.FIAT)
SGD = Currency('SGD', precision=2, currency_type=CurrencyType.FIAT)
USD = Currency('USD', precision=2, currency_type=CurrencyType.FIAT)
ZAR = Currency('ZAR', precision=2, currency_type=CurrencyType.FIAT)


cdef dict _CURRENCY_TABLE = {
    'BTC': BTC,
    'ETH': ETH,
    'XRP': XRP,
    'BCH': BCH,
    'BNB': BNB,
    'DOT': DOT,
    'LINK': LINK,
    'LTC': LTC,
    'USDT': USDT,
    'AUD': AUD,
    'CAD': CAD,
    'CHF': CHF,
    'CNY': CNY,
    'CNH': CNH,
    'CZK': CZK,
    'EUR': EUR,
    'GBP': GBP,
    'HKD': HKD,
    'JPY': JPY,
    'MXN': MXN,
    'NOK': NOK,
    'NZD': NZD,
    'RUB': RUB,
    'SEK': SEK,
    'TRY': TRY,
    'SGD': SGD,
    'USD': USD,
    'ZAR': ZAR,
}


cdef class Currency:
    """
    Represents a medium of exchange in a specified denomination with a fixed
    decimal precision.
    """

    def __init__(
            self,
            str code,
            int precision,
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
        currency_type : CurrencyType
            The currency type.

        Raises
        ------
        ValueError
            If code is not a valid string.
        ValueError
            If precision is negative (< 0).
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
                f"precision={self.precision}, "
                f"type={CurrencyTypeParser.to_str(self.currency_type)})")

    @staticmethod
    cdef Currency from_str_c(str code):
        return _CURRENCY_TABLE.get(code)

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
        Currency or None

        """
        return _CURRENCY_TABLE.get(code)
