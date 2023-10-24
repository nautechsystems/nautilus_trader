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

from nautilus_trader.core.nautilus_pyo3 import Currency


class TestTypesProviderPyo3:
    @staticmethod
    def currency_btc() -> Currency:
        return Currency.from_str("BTC")

    @staticmethod
    def currency_usdt() -> Currency:
        return Currency.from_str("USDT")

    @staticmethod
    def currency_aud() -> Currency:
        return Currency.from_str("AUD")

    @staticmethod
    def currency_gbp() -> Currency:
        return Currency.from_str("GBP")

    @staticmethod
    def currency_eth() -> Currency:
        return Currency.from_str("ETH")
