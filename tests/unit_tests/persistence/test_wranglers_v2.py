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

import pandas as pd
import polars as pl
from fsspec.utils import pathlib

from nautilus_trader.persistence.loaders import TardisTradeDataLoader
from nautilus_trader.persistence.loaders_v2 import QuoteTickDataFrameLoader
from nautilus_trader.persistence.wranglers_v2 import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers_v2 import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from tests import TESTS_PACKAGE_ROOT


TEST_DATA_DIR = pathlib.Path(TESTS_PACKAGE_ROOT).joinpath("test_data")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


def test_quote_tick_data_wrangler() -> None:
    # Arrange
    path = TEST_DATA_DIR / "truefx-audusd-ticks.csv"
    tick_data: pl.DataFrame = QuoteTickDataFrameLoader.read_csv(path)

    wrangler = QuoteTickDataWrangler(instrument=AUDUSD_SIM)

    # Act
    ticks = wrangler.process(tick_data)

    # Assert
    assert len(ticks) == 100_000
    assert str(ticks[0]) == "AUD/USD.SIM,0.67067,0.67070,1000000,1000000,1580398089820000"
    assert str(ticks[-1]) == "AUD/USD.SIM,0.66934,0.66938,1000000,1000000,1580504394501000"


def test_trade_tick_data_wrangler() -> None:
    # Arrange
    path = TEST_DATA_DIR / "tardis_trades.csv"
    tick_data: pd.DataFrame = TardisTradeDataLoader.load(path)

    wrangler = TradeTickDataWrangler(instrument=AUDUSD_SIM)

    # Act
    ticks = wrangler.process_from_pandas(tick_data)

    # Assert
    assert len(ticks) == 9999
    assert str(ticks[0]) == "AUD/USD.SIM,9682.00000,0,BUYER,42377944,1582329602418379000"
    assert str(ticks[-1]) == "AUD/USD.SIM,9666.84000,0,SELLER,42387942,1582337147852384000"
