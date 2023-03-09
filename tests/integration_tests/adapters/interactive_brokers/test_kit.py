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

import datetime
import gzip
import pathlib
import pickle
from typing import Optional

import msgspec
import pandas as pd
from ib_insync import AccountValue
from ib_insync import BarData
from ib_insync import Contract
from ib_insync import ContractDetails
from ib_insync import Execution
from ib_insync import HistoricalTickBidAsk
from ib_insync import HistoricalTickLast
from ib_insync import LimitOrder as IBLimitOrder
from ib_insync import Order as IBOrder
from ib_insync import OrderStatus
from ib_insync import TagValue
from ib_insync import Trade
from ib_insync import TradeLogEntry

from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.instruments.equity import Equity
from nautilus_trader.model.instruments.option import Option
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = pathlib.Path(TESTS_PACKAGE_ROOT + "/integration_tests/adapters/interactive_brokers/")
RESPONSES_PATH = pathlib.Path(TEST_PATH / "responses")
STREAMING_PATH = pathlib.Path(TEST_PATH / "streaming")
CONTRACT_PATH = pathlib.Path(RESPONSES_PATH / "contracts")


class IBTestProviderStubs:
    @staticmethod
    def aapl_equity_contract_details() -> ContractDetails:
        return ContractDetails(
            contract=Contract(
                secType="STK",
                conId=265598,
                symbol="AAPL",
                exchange="AMEX",
                primaryExchange="NASDAQ",
                currency="USD",
                localSymbol="AAPL",
                tradingClass="NMS",
            ),
            marketName="NMS",
            minTick=0.01,
            orderTypes="ACTIVETIM,AD,ADJUST,ALERT,ALLOC,AVGCOST,BASKET,BENCHPX,CASHQTY,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,IOC,LIT,LMT,MIT,MKT,MTL,NGCOMB,NONALGO,OCA,PEGBENCH,SCALE,SCALERST,SNAPMID,SNAPMKT,SNAPREL,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF",  # noqa
            validExchanges="SMART,AMEX,NYSE,CBOE,PHLX,ISE,CHX,ARCA,ISLAND,DRCTEDGE,BEX,BATS,EDGEA,CSFBALGO,JEFFALGO,BYX,IEX,EDGX,FOXRIVER,PEARL,NYSENAT,LTSE,MEMX,TPLUS1,PSX",  # noqa
            priceMagnifier=1,
            underConId=0,
            longName="APPLE INC",
            contractMonth="",
            industry="Technology",
            category="Computers",
            subcategory="Computers",
            timeZoneId="US/Eastern",
            tradingHours="20221207:0700-20221207:2000;20221208:0700-20221208:2000;20221209:0700-20221209:2000;20221210:CLOSED;20221211:CLOSED;20221212:0700-20221212:2000",  # noqa
            liquidHours="20221207:0700-20221207:2000;20221208:0700-20221208:2000;20221209:0700-20221209:2000;20221210:CLOSED;20221211:CLOSED;20221212:0700-20221212:2000",  # noqa
            evRule="",
            evMultiplier=0,
            mdSizeMultiplier=1,
            aggGroup=1,
            underSymbol="",
            underSecType="",
            marketRuleIds="26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26",
            secIdList=[TagValue(tag="ISIN", value="US0378331005")],
            realExpirationDate="",
            lastTradeTime="",
            stockType="COMMON",
            minSize=1.0,
            sizeIncrement=1.0,
            suggestedSizeIncrement=100.0,
            cusip="",
            ratings="",
            descAppend="",
            bondType="",
            couponType="",
            callable=False,
            putable=False,
            coupon=0,
            convertible=False,
            maturity="",
            issueDate="",
            nextOptionDate="",
            nextOptionType="",
            nextOptionPartial=False,
            notes="",
        )

    @staticmethod
    def cl_future_contract_details() -> ContractDetails:
        return ContractDetails(
            contract=Contract(
                secType="FUT",
                conId=174230596,
                symbol="CL",
                lastTradeDateOrContractMonth="20231120",
                multiplier="1000",
                exchange="NYMEX",
                currency="USD",
                localSymbol="CLZ3",
                tradingClass="CL",
            ),
            marketName="CL",
            minTick=0.01,
            orderTypes="ACTIVETIM,AD,ADJUST,ALERT,ALGO,ALLOC,AVGCOST,BASKET,BENCHPX,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,ICE,IOC,LIT,LMT,LTH,MIT,MKT,MTL,NGCOMB,NONALGO,OCA,PEGBENCH,SCALE,SCALERST,SIZECHK,SNAPMID,SNAPMKT,SNAPREL,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF",  # noqa
            validExchanges="NYMEX,QBALGO",
            priceMagnifier=1,
            underConId=17340715,
            longName="Light Sweet Crude Oil",
            contractMonth="202312",
            industry="",
            category="",
            subcategory="",
            timeZoneId="US/Eastern",
            tradingHours="20221206:1800-20221207:1700;20221207:1800-20221208:1700;20221208:1800-20221209:1700;20221210:CLOSED;20221211:1800-20221212:1700;20221212:1800-20221213:1700",  # noqa
            liquidHours="20221207:0930-20221207:1700;20221208:0930-20221208:1700;20221209:0930-20221209:1700;20221210:CLOSED;20221211:CLOSED;20221212:0930-20221212:1700;20221212:1800-20221213:1700",  # noqa
            evRule="",
            evMultiplier=0,
            mdSizeMultiplier=1,
            aggGroup=2147483647,
            underSymbol="CL",
            underSecType="IND",
            marketRuleIds="32,32",
            secIdList=[],
            realExpirationDate="20231120",
            lastTradeTime="14:30:00",
            stockType="",
            minSize=1.0,
            sizeIncrement=1.0,
            suggestedSizeIncrement=1.0,
            cusip="",
            ratings="",
            descAppend="",
            bondType="",
            couponType="",
            callable=False,
            putable=False,
            coupon=0,
            convertible=False,
            maturity="",
            issueDate="",
            nextOptionDate="",
            nextOptionType="",
            nextOptionPartial=False,
            notes="",
        )

    @staticmethod
    def eurusd_forex_contract_details() -> ContractDetails:
        return ContractDetails(
            contract=Contract(
                secType="CASH",
                conId=12087792,
                symbol="EUR",
                exchange="IDEALPRO",
                currency="USD",
                localSymbol="EUR.USD",
                tradingClass="EUR.USD",
            ),
            marketName="EUR.USD",
            minTick=5e-05,
            orderTypes="ACTIVETIM,AD,ADJUST,ALERT,ALGO,ALLOC,AVGCOST,BASKET,CASHQTY,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,IOC,LIT,LMT,MIT,MKT,NONALGO,OCA,REL,RELPCTOFS,SCALE,SCALERST,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF",  # noqa
            validExchanges="IDEALPRO",
            priceMagnifier=1,
            underConId=0,
            longName="European Monetary Union Euro",
            contractMonth="",
            industry="",
            category="",
            subcategory="",
            timeZoneId="US/Eastern",
            tradingHours="20221205:1715-20221206:1700;20221206:1715-20221207:1700;20221207:1715-20221208:1700;20221208:1715-20221209:1700;20221210:CLOSED;20221211:1715-20221212:1700",  # noqa
            liquidHours="20221205:1715-20221206:1700;20221206:1715-20221207:1700;20221207:1715-20221208:1700;20221208:1715-20221209:1700;20221210:CLOSED;20221211:1715-20221212:1700",  # noqa
            evRule="",
            evMultiplier=0,
            mdSizeMultiplier=1,
            aggGroup=4,
            underSymbol="",
            underSecType="",
            marketRuleIds="239",
            secIdList=[],
            realExpirationDate="",
            lastTradeTime="",
            stockType="",
            minSize=1.0,
            sizeIncrement=1.0,
            suggestedSizeIncrement=1.0,
            cusip="",
            ratings="",
            descAppend="",
            bondType="",
            couponType="",
            callable=False,
            putable=False,
            coupon=0,
            convertible=False,
            maturity="",
            issueDate="",
            nextOptionDate="",
            nextOptionType="",
            nextOptionPartial=False,
            notes="",
        )

    @staticmethod
    def tsla_option_contract_details() -> ContractDetails:
        return ContractDetails(
            contract=Contract(
                secType="OPT",
                conId=445067953,
                symbol="TSLA",
                lastTradeDateOrContractMonth="20230120",
                strike=100.0,
                right="C",
                multiplier="100",
                exchange="MIAX",
                currency="USD",
                localSymbol="TSLA  230120C00100000",
                tradingClass="TSLA",
            ),
            marketName="TSLA",
            minTick=0.01,
            orderTypes="ACTIVETIM,AD,ADJUST,ALERT,ALLOC,AVGCOST,BASKET,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,IOC,LIT,LMT,MIT,MKT,MTL,NGCOMB,NONALGO,OCA,OPENCLOSE,SCALE,SCALERST,SNAPMID,SNAPMKT,SNAPREL,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF",  # noqa
            validExchanges="SMART,AMEX,CBOE,PHLX,PSE,ISE,BOX,BATS,NASDAQOM,CBOE2,NASDAQBX,MIAX,GEMINI,EDGX,MERCURY,PEARL,EMERALD",  # noqa
            priceMagnifier=1,
            underConId=76792991,
            longName="TESLA INC",
            contractMonth="202301",
            industry="",
            category="",
            subcategory="",
            timeZoneId="US/Eastern",
            tradingHours="20221207:0930-20221207:1600;20221208:0930-20221208:1600;20221209:0930-20221209:1600;20221210:CLOSED;20221211:CLOSED;20221212:0930-20221212:1600",  # noqa
            liquidHours="20221207:0930-20221207:1600;20221208:0930-20221208:1600;20221209:0930-20221209:1600;20221210:CLOSED;20221211:CLOSED;20221212:0930-20221212:1600",  # noqa
            evRule="",
            evMultiplier=0,
            mdSizeMultiplier=1,
            aggGroup=2,
            underSymbol="TSLA",
            underSecType="STK",
            marketRuleIds="32,109,109,109,109,109,109,109,32,109,32,109,109,109,109,109,109",
            secIdList=[],
            realExpirationDate="20230120",
            lastTradeTime="",
            stockType="",
            minSize=1.0,
            sizeIncrement=1.0,
            suggestedSizeIncrement=1.0,
            cusip="",
            ratings="",
            descAppend="",
            bondType="",
            couponType="",
            callable=False,
            putable=False,
            coupon=0,
            convertible=False,
            maturity="",
            issueDate="",
            nextOptionDate="",
            nextOptionType="",
            nextOptionPartial=False,
            notes="",
        )

    @staticmethod
    def aapl_instrument() -> Equity:
        contract_details = IBTestProviderStubs.aapl_equity_contract_details()
        return parse_instrument(contract_details=contract_details)

    @staticmethod
    def eurusd_instrument() -> CurrencyPair:
        contract_details = IBTestProviderStubs.eurusd_forex_contract_details()
        return parse_instrument(contract_details=contract_details)


class IBTestDataStubs:
    @staticmethod
    def contract(secType="STK", symbol="AAPL", exchange="NASDAQ", **kwargs):
        return Contract(secType=secType, symbol=symbol, exchange=exchange, **kwargs)

    @staticmethod
    def account_values(fn: str = "account_values.json") -> list[AccountValue]:
        with open(RESPONSES_PATH / fn, "rb") as f:
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


class IBTestExecStubs:
    @staticmethod
    def create_order(
        order_id: int = 1,
        client_id: int = 1,
        permId: int = 0,
        kind: str = "LIMIT",
        action: str = "BUY",
        quantity: int = 100000,
        limit_price: float = 105.0,
        client_order_id: ClientOrderId = ClientOrderId("C-1"),
    ):
        if kind == "LIMIT":
            return IBLimitOrder(
                orderId=order_id,
                clientId=client_id,
                action=action,
                totalQuantity=quantity,
                lmtPrice=limit_price,
                permId=permId,
                orderRef=client_order_id.value,
            )
        else:
            raise RuntimeError

    @staticmethod
    def trade_pending_submit(contract=None, order: IBOrder = None) -> Trade:
        contract = contract or IBTestProviderStubs.aapl_equity_contract_details().contract
        order = order or IBTestExecStubs.create_order()
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
    def trade_pre_submit(
        contract=None,
        order: IBOrder = None,
        client_order_id: Optional[ClientOrderId] = None,
    ) -> Trade:
        contract = contract or IBTestProviderStubs.aapl_equity_contract_details().contract
        order = order or IBTestExecStubs.create_order(client_order_id=client_order_id)
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
    def trade_submitted(
        contract=None,
        order: IBOrder = None,
        client_order_id: Optional[ClientOrderId] = None,
    ) -> Trade:
        contract = contract or IBTestProviderStubs.aapl_equity_contract_details().contract
        order = order or IBTestExecStubs.create_order(client_order_id=client_order_id)
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
        contract = contract or IBTestProviderStubs.aapl_equity_contract_details().contract
        order = order or IBTestExecStubs.create_order()
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
        contract = contract or IBTestProviderStubs.aapl_equity_contract_details().contract
        order = order or IBTestExecStubs.create_order()
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


def filter_out_options(instrument) -> bool:
    return not isinstance(instrument, Option)
