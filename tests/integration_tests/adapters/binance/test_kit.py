# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import gzip
import pathlib
import pickle

import orjson
import pandas as pd

from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.model.instruments.equity import Equity
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = pathlib.Path(TESTS_PACKAGE_ROOT + "/integration_tests/adapters/binance/resources")
RESPONSES_PATH = pathlib.Path(TEST_PATH / "http_responses")
STREAMING_PATH = pathlib.Path(TEST_PATH / "streaming_responses")


class BinanceTestStubs:
    @staticmethod
    def contract_details(symbol: str):
        with open(RESPONSES_PATH / "/http_spot_market_exchange_info.json", "rb") as f:
            info = orjson.loads(f.read())
        return list(filter(lambda x: x["symbol"] == symbol, info["symbols"]))

    @staticmethod
    def instrument(symbol: str) -> Equity:
        contract_details = BinanceTestStubs.contract_details(symbol)
        return parse_instrument(contract_details=contract_details)

    @staticmethod
    def market_depth():
        with open(STREAMING_PATH / "http_spot_market_depth.json", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301

    @staticmethod
    def tickers(name: str = "eurusd"):
        with open(STREAMING_PATH / "http_spot_market_klines.json", "rb") as f:
            return orjson.loads(f.read())  # noqa: S301

    @staticmethod
    def historic_trades():
        trades = []
        with gzip.open(RESPONSES_PATH / "historic/trade_ticks.json.gz", "rb") as f:
            for line in f:
                data = orjson.loads(line)
                trades.append(data)
        return trades

    @staticmethod
    def historic_bid_ask():
        trades = []
        with gzip.open(RESPONSES_PATH / "historic/bid_ask_ticks.json.gz", "rb") as f:
            for line in f:
                data = orjson.loads(line)
                trades.append(data)
        return trades

    @staticmethod
    def historic_bars():
        trades = []
        with gzip.open(RESPONSES_PATH / "http_spot_market_klines.json", "rb") as f:
            for line in f:
                data = orjson.loads(line)
                data["date"] = pd.Timestamp(data["date"]).to_pydatetime()
                trades.append(data)
        return trades
