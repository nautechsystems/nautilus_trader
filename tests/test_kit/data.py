# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from pandas import DataFrame

from nautilus_trader.backtest.loaders import CSVTickDataLoader, CSVBarDataLoader

from tests.test_kit import PACKAGE_ROOT


class TestDataProvider:

    @staticmethod
    def usdjpy_test_ticks() -> DataFrame:
        return CSVTickDataLoader.load(PACKAGE_ROOT + '/data/USDJPY_ticks.csv')

    @staticmethod
    def gbpusd_1min_bid() -> DataFrame:
        return CSVBarDataLoader.load(PACKAGE_ROOT + '/data/GBPUSD_1 Min_Bid.csv')

    @staticmethod
    def usdjpy_1min_bid() -> DataFrame:
        return CSVBarDataLoader.load(PACKAGE_ROOT + '/data/USDJPY_1 Min_Bid.csv')

    @staticmethod
    def usdjpy_1min_ask() -> DataFrame:
        return CSVBarDataLoader.load(PACKAGE_ROOT + '/data/USDJPY_1 Min_Ask.csv')
