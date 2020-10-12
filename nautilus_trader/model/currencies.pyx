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
from nautilus_trader.model.currency cimport Currency

BTC = Currency('BTC', precision=8, currency_type=CurrencyType.CRYPTO)
XBT = Currency('XBT', precision=8, currency_type=CurrencyType.CRYPTO)
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
