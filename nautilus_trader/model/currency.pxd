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

from nautilus_trader.model.c_enums.currency_type cimport CurrencyType


# Crypto currencies
cdef Currency BTC
cdef Currency ETH
cdef Currency USDT
cdef Currency XRP
cdef Currency BCH
cdef Currency BNB
cdef Currency DOT
cdef Currency LINK
cdef Currency LTC

# Fiat currencies
cdef Currency AUD
cdef Currency CAD
cdef Currency CHF
cdef Currency CNY
cdef Currency CNH
cdef Currency CZK
cdef Currency EUR
cdef Currency GBP
cdef Currency HKD
cdef Currency JPY
cdef Currency MXN
cdef Currency NOK
cdef Currency NZD
cdef Currency RUB
cdef Currency SEK
cdef Currency TRY
cdef Currency SGD
cdef Currency USD
cdef Currency ZAR


cdef class Currency:
    cdef readonly str code
    """The identifier code of the currency.\n\n:returns: `str`"""
    cdef readonly int precision
    """The specified precision of the currency.\n\n:returns: `int`"""
    cdef readonly CurrencyType currency_type
    """The general type of the currency.\n\n:returns: `CurrencyType`"""

    @staticmethod
    cdef Currency from_str_c(str code)
