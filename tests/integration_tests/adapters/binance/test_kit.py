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

import pathlib
import time
from typing import List

import msgspec
import orjson

from nautilus_trader.adapters.binance.common.schemas import BinanceQuote
from nautilus_trader.adapters.binance.common.schemas import BinanceTrade
from nautilus_trader.adapters.binance.spot.parsing.data import parse_spot_instrument_http
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotExchangeInfo
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotSymbolInfo
from nautilus_trader.adapters.binance.spot.schemas.wallet import BinanceSpotTradeFees
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.instruments.equity import Equity
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = pathlib.Path(TESTS_PACKAGE_ROOT + "/integration_tests/adapters/binance/resources")
RESPONSES_PATH = pathlib.Path(TEST_PATH / "http_responses")
STREAMING_PATH = pathlib.Path(TEST_PATH / "streaming_responses")


class BinanceTestStubs:
    @staticmethod
    def excahnge_info():
        with open(RESPONSES_PATH / "http_spot_market_exchange_info.json", "rb") as f:
            info: BinanceSpotExchangeInfo = msgspec.json.decode(
                f.read(),
                type=BinanceSpotExchangeInfo,
            )
        return info

    @staticmethod
    def symbol_info(symbol: str):
        info: BinanceSpotExchangeInfo = BinanceTestStubs.excahnge_info()
        symbol_info: BinanceSpotSymbolInfo = list(
            filter(lambda x: x.symbol == symbol, info.symbols)
        )[0]
        return symbol_info

    @staticmethod
    def fees_info(symbol: str):
        with open(RESPONSES_PATH / "http_wallet_trading_fees.json", "rb") as f:
            info: List[BinanceSpotTradeFees] = msgspec.json.decode(
                f.read(),
                type=List[BinanceSpotTradeFees],
            )
        return list(filter(lambda x: x.symbol == symbol, info))[0]

    @staticmethod
    def instrument(symbol: str) -> Equity:
        info = BinanceTestStubs.excahnge_info()
        return parse_spot_instrument_http(
            symbol_info=BinanceTestStubs.symbol_info(symbol),
            fees=BinanceTestStubs.fees_info(symbol),
            ts_event=millis_to_nanos(info.serverTime),
            ts_init=time.time_ns(),
        )

    @staticmethod
    def market_depth():
        with open(STREAMING_PATH / "http_spot_market_depth.json", "rb") as f:
            return orjson.loads(f.read())  # noqa: S301

    @staticmethod
    def ticker():
        with open(RESPONSES_PATH / "http_spot_market_book_ticker.json", "rb") as f:
            return msgspec.json.decode(f.read(), type=BinanceQuote)

    @staticmethod
    def historic_trades():
        with open(RESPONSES_PATH / "http_spot_market_historical_trades.json", "rb") as f:
            trades = msgspec.json.decode(f.read(), type=List[BinanceTrade])
        return trades

    @staticmethod
    def historic_bars():
        with open(RESPONSES_PATH / "http_spot_market_klines.json", "rb") as f:
            data = msgspec.json.decode(
                f.read(),
            )
        return data
