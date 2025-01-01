# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.persistence.wranglers_v2 import QuoteTickDataWranglerV2
from nautilus_trader.persistence.wranglers_v2 import TradeTickDataWranglerV2
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def test_quote_tick_data_wrangler() -> None:
    # Arrange
    path = TEST_DATA_DIR / "truefx" / "audusd-ticks.csv"
    df = pd.read_csv(path)
    instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")

    # Act
    wrangler = QuoteTickDataWranglerV2.from_instrument(instrument)
    pyo3_quotes = wrangler.from_pandas(df)

    quotes = QuoteTick.from_pyo3_list(pyo3_quotes)

    # Assert
    assert len(pyo3_quotes) == 100_000
    assert len(quotes) == 100_000
    assert isinstance(quotes[0], QuoteTick)
    assert str(pyo3_quotes[0]) == "AUD/USD.SIM,0.67067,0.67070,1000000,1000000,1580398089820000000"
    assert str(pyo3_quotes[-1]) == "AUD/USD.SIM,0.66934,0.66938,1000000,1000000,1580504394501000000"


def test_trade_tick_data_wrangler() -> None:
    # Arrange
    path = TEST_DATA_DIR / "binance" / "ethusdt-trades.csv"
    df = pd.read_csv(path)
    instrument = TestInstrumentProvider.ethusdt_binance()

    # Act
    wrangler = TradeTickDataWranglerV2.from_instrument(instrument)
    pyo3_trades = wrangler.from_pandas(df)

    trades = TradeTick.from_pyo3_list(pyo3_trades)

    # Assert
    assert len(pyo3_trades) == 69806
    assert len(trades) == 69806
    assert isinstance(trades[0], TradeTick)
    assert (
        str(pyo3_trades[0]) == "ETHUSDT.BINANCE,423.76,2.67900,BUYER,148568980,1597399200223000000"
    )
    assert (
        str(pyo3_trades[-1]) == "ETHUSDT.BINANCE,426.89,0.16100,BUYER,148638715,1597417198693000000"
    )
