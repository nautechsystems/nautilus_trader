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

import datetime
import gzip
import pathlib
import pickle

import msgspec
import pandas as pd
from ib_insync import AccountValue
from ib_insync import BarData
from ib_insync import Contract
from ib_insync import Execution
from ib_insync import HistoricalTickBidAsk
from ib_insync import HistoricalTickLast
from ib_insync import LimitOrder as IBLimitOrder
from ib_insync import Order as IBOrder
from ib_insync import OrderStatus
from ib_insync import Trade
from ib_insync import TradeLogEntry

from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.model.instruments.equity import Equity
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = pathlib.Path(TESTS_PACKAGE_ROOT + "/integration_tests/adapters/interactive_brokers/")
RESPONSES_PATH = pathlib.Path(TEST_PATH / "responses")
STREAMING_PATH = pathlib.Path(TEST_PATH / "streaming")
CONTRACT_PATH = pathlib.Path(RESPONSES_PATH / "contracts")


class IBTestDataStubs:
    @staticmethod
    def contract_details(symbol: str):
        return pickle.load(  # noqa: S301
            open(RESPONSES_PATH / f"contracts/{symbol.upper()}.pkl", "rb"),
        )

    @staticmethod
    def contract(secType="STK", symbol="AAPL", exchange="NASDAQ", **kwargs):
        return Contract(secType=secType, symbol=symbol, exchange=exchange, **kwargs)

    @staticmethod
    def instrument(symbol: str) -> Equity:
        contract_details = IBTestDataStubs.contract_details(symbol)
        return parse_instrument(contract_details=contract_details)

    @staticmethod
    def account_values() -> list[AccountValue]:
        with open(RESPONSES_PATH / "account_values.json", "rb") as f:
            raw = msgspec.json.decode(f.read())
            return [AccountValue(**acc) for acc in raw]

    @staticmethod
    def market_depth(name: str = "eurusd"):
        with open(STREAMING_PATH / f"{name}_depth.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301

    @staticmethod
    def tickers(name: str = "eurusd"):
        with open(STREAMING_PATH / f"{name}_ticker.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301

    @staticmethod
    def historic_trades():
        trades = []
        with gzip.open(RESPONSES_PATH / "historic/trade_ticks.json.gz", "rb") as f:
            for line in f:
                data = msgspec.json.decode(line)
                tick = HistoricalTickLast(**data)
                trades.append(tick)
        return trades

    @staticmethod
    def historic_bid_ask():
        trades = []
        with gzip.open(RESPONSES_PATH / "historic/bid_ask_ticks.json.gz", "rb") as f:
            for line in f:
                data = msgspec.json.decode(line)
                tick = HistoricalTickBidAsk(**data)
                trades.append(tick)
        return trades

    @staticmethod
    def historic_bars():
        trades = []
        with gzip.open(RESPONSES_PATH / "historic/bars.json.gz", "rb") as f:
            for line in f:
                data = msgspec.json.decode(line)
                data["date"] = pd.Timestamp(data["date"]).to_pydatetime()
                tick = BarData(**data)
                trades.append(tick)
        return trades


class IBExecTestStubs:
    @staticmethod
    def create_order(
        order_id: int = 1,
        client_id: int = 1,
        permId: int = 0,
        kind: str = "LIMIT",
        action: str = "BUY",
        quantity: int = 100000,
        limit_price: float = 105.0,
    ):
        if kind == "LIMIT":
            return IBLimitOrder(
                orderId=order_id,
                clientId=client_id,
                action=action,
                totalQuantity=quantity,
                lmtPrice=limit_price,
                permId=permId,
            )
        else:
            raise RuntimeError

    @staticmethod
    def trade_pending_submit(contract=None, order: IBOrder = None) -> Trade:
        contract = contract or IBTestDataStubs.contract_details("AAPL").contract
        order = order or IBExecTestStubs.create_order()
        return Trade(
            contract=contract,
            order=order,
            orderStatus=OrderStatus(
                orderId=41,
                status="PendingSubmit",
                filled=0.0,
                remaining=0.0,
                avgFillPrice=0.0,
                permId=0,
                parentId=0,
                lastFillPrice=0.0,
                clientId=0,
                whyHeld="",
                mktCapPrice=0.0,
            ),
            fills=[],
            log=[
                TradeLogEntry(
                    time=datetime.datetime(
                        2022,
                        3,
                        5,
                        3,
                        6,
                        23,
                        492613,
                        tzinfo=datetime.timezone.utc,
                    ),
                    status="PendingSubmit",
                    message="",
                    errorCode=0,
                ),
            ],
        )

    @staticmethod
    def trade_pre_submit(contract=None, order: IBOrder = None) -> Trade:
        contract = contract or IBTestDataStubs.contract_details("AAPL").contract
        order = order or IBExecTestStubs.create_order()
        return Trade(
            contract=contract,
            order=order,
            orderStatus=OrderStatus(
                orderId=41,
                status="PreSubmitted",
                filled=0.0,
                remaining=1.0,
                avgFillPrice=0.0,
                permId=189868420,
                parentId=0,
                lastFillPrice=0.0,
                clientId=1,
                whyHeld="",
                mktCapPrice=0.0,
            ),
            fills=[],
            log=[
                TradeLogEntry(
                    time=datetime.datetime(
                        2022,
                        3,
                        5,
                        3,
                        6,
                        23,
                        492613,
                        tzinfo=datetime.timezone.utc,
                    ),
                    status="PendingSubmit",
                    message="",
                    errorCode=0,
                ),
                TradeLogEntry(
                    time=datetime.datetime(
                        2022,
                        3,
                        5,
                        3,
                        6,
                        26,
                        871811,
                        tzinfo=datetime.timezone.utc,
                    ),
                    status="PreSubmitted",
                    message="",
                    errorCode=0,
                ),
            ],
        )

    @staticmethod
    def trade_submitted(contract=None, order: IBOrder = None) -> Trade:
        contract = contract or IBTestDataStubs.contract_details("AAPL").contract
        order = order or IBExecTestStubs.create_order()
        return Trade(
            contract=contract,
            order=order,
            orderStatus=OrderStatus(
                orderId=41,
                status="Submitted",
                filled=0.0,
                remaining=1.0,
                avgFillPrice=0.0,
                permId=order.permId,
                parentId=0,
                lastFillPrice=0.0,
                clientId=1,
                whyHeld="",
                mktCapPrice=0.0,
            ),
            fills=[],
            log=[
                TradeLogEntry(
                    time=datetime.datetime(
                        2022,
                        3,
                        5,
                        3,
                        6,
                        23,
                        492613,
                        tzinfo=datetime.timezone.utc,
                    ),
                    status="PendingSubmit",
                    message="",
                    errorCode=0,
                ),
                TradeLogEntry(
                    time=datetime.datetime(
                        2022,
                        3,
                        5,
                        3,
                        6,
                        26,
                        871811,
                        tzinfo=datetime.timezone.utc,
                    ),
                    status="PreSubmitted",
                    message="",
                    errorCode=0,
                ),
                TradeLogEntry(
                    time=datetime.datetime(
                        2022,
                        3,
                        5,
                        3,
                        6,
                        28,
                        378175,
                        tzinfo=datetime.timezone.utc,
                    ),
                    status="Submitted",
                    message="",
                    errorCode=0,
                ),
            ],
        )

    @staticmethod
    def trade_pre_cancel(contract=None, order: IBOrder = None) -> Trade:
        contract = contract or IBTestDataStubs.contract_details("AAPL").contract
        order = order or IBExecTestStubs.create_order()
        return Trade(
            contract=contract,
            order=order,
            orderStatus=OrderStatus(
                orderId=41,
                status="PendingCancel",
                filled=0.0,
                remaining=1.0,
                avgFillPrice=0.0,
                permId=189868420,
                parentId=0,
                lastFillPrice=0.0,
                clientId=1,
                whyHeld="",
                mktCapPrice=0.0,
            ),
            fills=[],
            log=[
                TradeLogEntry(
                    time=datetime.datetime(
                        2022,
                        3,
                        6,
                        2,
                        17,
                        18,
                        455087,
                        tzinfo=datetime.timezone.utc,
                    ),
                    status="PendingCancel",
                    message="",
                    errorCode=0,
                ),
            ],
        )

    @staticmethod
    def trade_canceled(contract=None, order: IBOrder = None) -> Trade:
        contract = contract or IBTestDataStubs.contract_details("AAPL").contract
        order = order or IBExecTestStubs.create_order()
        return Trade(
            contract=contract,
            order=order,
            orderStatus=OrderStatus(
                orderId=41,
                status="Cancelled",
                filled=0.0,
                remaining=1.0,
                avgFillPrice=0.0,
                permId=order.permId,
                parentId=0,
                lastFillPrice=0.0,
                clientId=1,
                whyHeld="",
                mktCapPrice=0.0,
            ),
            fills=[],
            log=[
                TradeLogEntry(
                    time=datetime.datetime(
                        2022,
                        3,
                        6,
                        2,
                        17,
                        18,
                        455087,
                        tzinfo=datetime.timezone.utc,
                    ),
                    status="PendingCancel",
                    message="",
                    errorCode=0,
                ),
                TradeLogEntry(
                    time=datetime.datetime(2022, 3, 6, 2, 23, 2, 847, tzinfo=datetime.timezone.utc),
                    status="Cancelled",
                    message="Error 10148, reqId 45: OrderId 45 that needs to be cancelled cannot be cancelled, state: PendingCancel.",
                    errorCode=10148,
                ),
            ],
        )

    @staticmethod
    def execution() -> Execution:
        return Execution(
            execId="1",
            time=datetime.datetime(1970, 1, 1, tzinfo=datetime.timezone.utc),
            acctNumber="111",
            exchange="NYSE",
            side="BUY",
            shares=100,
            price=50.0,
            permId=0,
            clientId=0,
            orderId=0,
            liquidation=0,
            cumQty=100,
            avgPrice=50.0,
            orderRef="",
            evRule="",
            evMultiplier=0.0,
            modelCode="",
            lastLiquidity=0,
        )
