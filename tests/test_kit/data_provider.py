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

from pandas import DataFrame

from nautilus_trader.backtest.loaders import CSVBarDataLoader
from nautilus_trader.backtest.loaders import CSVTickDataLoader
from tests.test_kit import PACKAGE_ROOT


class TestDataProvider:

    @staticmethod
    def audusd_ticks() -> DataFrame:
        print("Extracting truefx-audusd-ticks-2020-01.csv.zip...")
        return CSVTickDataLoader.load(PACKAGE_ROOT + "/data/truefx-audusd-ticks-2020-01.csv.zip")

    @staticmethod
    def usdjpy_ticks() -> DataFrame:
        return CSVTickDataLoader.load(PACKAGE_ROOT + "/data/truefx-usdjpy-ticks.csv.zip")

    @staticmethod
    def gbpusd_1min_bid() -> DataFrame:
        print("Extracting fxcm-gbpusd-m1-bid-2012.csv.zip...")
        return CSVBarDataLoader.load(PACKAGE_ROOT + "/data/fxcm-gbpusd-m1-bid-2012.csv.zip")

    @staticmethod
    def gbpusd_1min_ask() -> DataFrame:
        print("Extracting fxcm-gbpusd-m1-ask-2012.csv.zip...")
        return CSVBarDataLoader.load(PACKAGE_ROOT + "/data/fxcm-gbpusd-m1-ask-2012.csv.zip")

    @staticmethod
    def usdjpy_1min_bid() -> DataFrame:
        print("Extracting fxcm-usdjpy-m1-bid-2013.csv.zip...")
        return CSVBarDataLoader.load(PACKAGE_ROOT + "/data/fxcm-usdjpy-m1-bid-2013.csv.zip")

    @staticmethod
    def usdjpy_1min_ask() -> DataFrame:
        print("Extracting fxcm-usdjpy-m1-ask-2013.csv.zip...")
        return CSVBarDataLoader.load(PACKAGE_ROOT + "/data/fxcm-usdjpy-m1-ask-2013.csv.zip")
