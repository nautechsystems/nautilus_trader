# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
import pickle
from decimal import Decimal

import msgspec
import pandas as pd
from ibapi.commission_report import CommissionReport
from ibapi.common import BarData
from ibapi.contract import Contract  # We use this for the expected response from IB
from ibapi.contract import ContractDetails
from ibapi.execution import Execution
from ibapi.tag_value import TagValue

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import OptionsContract
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT / "integration_tests" / "adapters" / "interactive_brokers/"
RESPONSES_PATH = TEST_PATH / "responses"
STREAMING_PATH = TEST_PATH / "streaming"
CONTRACT_PATH = RESPONSES_PATH / "contracts"


class IBTestProviderStubs:
    @staticmethod
    def aapl_equity_contract_details() -> ContractDetails:
        details = ContractDetails()
        details.contract = Contract()
        details.contract.secType = "STK"
        details.contract.conId = 265598
        details.contract.symbol = "AAPL"
        details.contract.exchange = "SMART"
        details.contract.primaryExchange = "NASDAQ"
        details.contract.currency = "USD"
        details.contract.localSymbol = "AAPL"
        details.contract.tradingClass = "NMS"
        details.marketName = "NMS"
        details.minTick = 0.01
        details.orderTypes = "ACTIVETIM,AD,ADJUST,ALERT,ALLOC,AVGCOST,BASKET,BENCHPX,CASHQTY,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,IOC,LIT,LMT,MIT,MKT,MTL,NGCOMB,NONALGO,OCA,PEGBENCH,SCALE,SCALERST,SNAPMID,SNAPMKT,SNAPREL,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF"  # noqa
        details.validExchanges = "SMART,AMEX,NYSE,CBOE,PHLX,ISE,CHX,ARCA,ISLAND,DRCTEDGE,BEX,BATS,EDGEA,CSFBALGO,JEFFALGO,BYX,IEX,EDGX,FOXRIVER,PEARL,NYSENAT,LTSE,MEMX,TPLUS1,PSX"  # noqa
        details.priceMagnifier = 1
        details.underConId = 0
        details.longName = "APPLE INC"
        details.contractMonth = ""
        details.industry = "Technology"
        details.category = "Computers"
        details.subcategory = "Computers"
        details.timeZoneId = "US/Eastern"
        details.tradingHours = "20221207:0700-20221207:2000;20221208:0700-20221208:2000;20221209:0700-20221209:2000;20221210:CLOSED;20221211:CLOSED;20221212:0700-20221212:2000"  # noqa
        details.liquidHours = "20221207:0700-20221207:2000;20221208:0700-20221208:2000;20221209:0700-20221209:2000;20221210:CLOSED;20221211:CLOSED;20221212:0700-20221212:2000"  # noqa
        details.evRule = ""
        details.evMultiplier = 0
        details.mdSizeMultiplier = 1
        details.aggGroup = 1
        details.underSymbol = ""
        details.underSecType = ""
        details.marketRuleIds = (
            "26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26"
        )
        details.secIdList = [TagValue(tag="ISIN", value="US0378331005")]
        details.realExpirationDate = ""
        details.lastTradeTime = ""
        details.stockType = "COMMON"
        details.minSize = 1.0
        details.sizeIncrement = 1.0
        details.suggestedSizeIncrement = 100.0
        details.cusip = ""
        details.ratings = ""
        details.descAppend = ""
        details.bondType = ""
        details.couponType = ""
        details.callable = False
        details.putable = False
        details.coupon = 0
        details.convertible = False
        details.maturity = ""
        details.issueDate = ""
        details.nextOptionDate = ""
        details.nextOptionType = ""
        details.nextOptionPartial = False
        details.notes = ""
        return details

    @staticmethod
    def cl_future_contract_details() -> ContractDetails:
        details = ContractDetails()
        details.contract = Contract()
        details.contract.secType = "FUT"
        details.contract.conId = 174230596
        details.contract.symbol = "CL"
        details.contract.lastTradeDateOrContractMonth = "20231120"
        details.contract.multiplier = "1000"
        details.contract.exchange = "NYMEX"
        details.contract.currency = "USD"
        details.contract.localSymbol = "CLZ3"
        details.contract.tradingClass = "CL"
        details.marketName = "CL"
        details.minTick = 0.01
        details.orderTypes = "ACTIVETIM,AD,ADJUST,ALERT,ALGO,ALLOC,AVGCOST,BASKET,BENCHPX,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,ICE,IOC,LIT,LMT,LTH,MIT,MKT,MTL,NGCOMB,NONALGO,OCA,PEGBENCH,SCALE,SCALERST,SIZECHK,SNAPMID,SNAPMKT,SNAPREL,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF"  # noqa
        details.validExchanges = "NYMEX,QBALGO"
        details.priceMagnifier = 1
        details.underConId = 17340715
        details.longName = "Light Sweet Crude Oil"
        details.contractMonth = "202312"
        details.industry = ""
        details.category = ""
        details.subcategory = ""
        details.timeZoneId = "US/Eastern"
        details.tradingHours = "20221206:1800-20221207:1700;20221207:1800-20221208:1700;20221208:1800-20221209:1700;20221210:CLOSED;20221211:1800-20221212:1700;20221212:1800-20221213:1700"  # noqa
        details.liquidHours = "20221207:0930-20221207:1700;20221208:0930-20221208:1700;20221209:0930-20221209:1700;20221210:CLOSED;20221211:CLOSED;20221212:0930-20221212:1700;20221212:1800-20221213:1700"  # noqa
        details.evRule = ""
        details.evMultiplier = 0
        details.mdSizeMultiplier = 1
        details.aggGroup = 2147483647
        details.underSymbol = "CL"
        details.underSecType = "IND"
        details.marketRuleIds = "32,32"
        details.secIdList = []
        details.realExpirationDate = "20231120"
        details.lastTradeTime = "14:30:00"
        details.stockType = ""
        details.minSize = 1.0
        details.sizeIncrement = 1.0
        details.suggestedSizeIncrement = 1.0
        details.cusip = ""
        details.ratings = ""
        details.descAppend = ""
        details.bondType = ""
        details.couponType = ""
        details.callable = False
        details.putable = False
        details.coupon = 0
        details.convertible = False
        details.maturity = ""
        details.issueDate = ""
        details.nextOptionDate = ""
        details.nextOptionType = ""
        details.nextOptionPartial = False
        details.notes = ""
        return details

    @staticmethod
    def eurusd_forex_contract_details() -> ContractDetails:
        details = ContractDetails()
        details.contract = Contract()
        details.contract.secType = "CASH"
        details.contract.conId = 12087792
        details.contract.symbol = "EUR"
        details.contract.exchange = "IDEALPRO"
        details.contract.currency = "USD"
        details.contract.localSymbol = "EUR.USD"
        details.contract.tradingClass = "EUR.USD"
        details.marketName = "EUR.USD"
        details.minTick = 5e-05
        details.orderTypes = "ACTIVETIM,AD,ADJUST,ALERT,ALGO,ALLOC,AVGCOST,BASKET,CASHQTY,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,IOC,LIT,LMT,MIT,MKT,NONALGO,OCA,REL,RELPCTOFS,SCALE,SCALERST,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF"  # noqa
        details.validExchanges = "IDEALPRO"
        details.priceMagnifier = 1
        details.underConId = 0
        details.longName = "European Monetary Union Euro"
        details.contractMonth = ""
        details.industry = ""
        details.category = ""
        details.subcategory = ""
        details.timeZoneId = "US/Eastern"
        details.tradingHours = "20221205:1715-20221206:1700;20221206:1715-20221207:1700;20221207:1715-20221208:1700;20221208:1715-20221209:1700;20221210:CLOSED;20221211:1715-20221212:1700"  # noqa
        details.liquidHours = "20221205:1715-20221206:1700;20221206:1715-20221207:1700;20221207:1715-20221208:1700;20221208:1715-20221209:1700;20221210:CLOSED;20221211:1715-20221212:1700"  # noqa
        details.evRule = ""
        details.evMultiplier = 0
        details.mdSizeMultiplier = 1
        details.aggGroup = 4
        details.underSymbol = ""
        details.underSecType = ""
        details.marketRuleIds = "239"
        details.secIdList = []
        details.realExpirationDate = ""
        details.lastTradeTime = ""
        details.stockType = ""
        details.minSize = 1.0
        details.sizeIncrement = 1.0
        details.suggestedSizeIncrement = 1.0
        details.cusip = ""
        details.ratings = ""
        details.descAppend = ""
        details.bondType = ""
        details.couponType = ""
        details.callable = False
        details.putable = False
        details.coupon = 0
        details.convertible = False
        details.maturity = ""
        details.issueDate = ""
        details.nextOptionDate = ""
        details.nextOptionType = ""
        details.nextOptionPartial = False
        details.notes = ""
        return details

    @staticmethod
    def tsla_option_contract_details() -> ContractDetails:
        details = ContractDetails()
        details.contract = Contract()
        details.contract.secType = "OPT"
        details.contract.conId = 445067953
        details.contract.symbol = "TSLA"
        details.contract.lastTradeDateOrContractMonth = "20230120"
        details.contract.strike = 100.0
        details.contract.right = "C"
        details.contract.multiplier = "100"
        details.contract.exchange = "MIAX"
        details.contract.currency = "USD"
        details.contract.localSymbol = "TSLA  230120C00100000"
        details.contract.tradingClass = "TSLA"
        details.marketName = "TSLA"
        details.minTick = 0.01
        details.orderTypes = "ACTIVETIM,AD,ADJUST,ALERT,ALLOC,AVGCOST,BASKET,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,IOC,LIT,LMT,MIT,MKT,MTL,NGCOMB,NONALGO,OCA,OPENCLOSE,SCALE,SCALERST,SNAPMID,SNAPMKT,SNAPREL,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF"  # noqa
        details.validExchanges = "SMART,AMEX,CBOE,PHLX,PSE,ISE,BOX,BATS,NASDAQOM,CBOE2,NASDAQBX,MIAX,GEMINI,EDGX,MERCURY,PEARL,EMERALD"
        details.priceMagnifier = 1
        details.underConId = 76792991
        details.longName = "TESLA INC"
        details.contractMonth = "202301"
        details.industry = ""
        details.category = ""
        details.subcategory = ""
        details.timeZoneId = "US/Eastern"
        details.tradingHours = "20221207:0930-20221207:1600;20221208:0930-20221208:1600;20221209:0930-20221209:1600;20221210:CLOSED;20221211:CLOSED;20221212:0930-20221212:1600"  # noqa
        details.liquidHours = "20221207:0930-20221207:1600;20221208:0930-20221208:1600;20221209:0930-20221209:1600;20221210:CLOSED;20221211:CLOSED;20221212:0930-20221212:1600"  # noqa
        details.evRule = ""
        details.evMultiplier = 0
        details.mdSizeMultiplier = 1
        details.aggGroup = 2
        details.underSymbol = "TSLA"
        details.underSecType = "STK"
        details.marketRuleIds = "32,109,109,109,109,109,109,109,32,109,32,109,109,109,109,109,109"
        details.secIdList = []
        details.realExpirationDate = "20230120"
        details.lastTradeTime = ""
        details.stockType = ""
        details.minSize = 1.0
        details.sizeIncrement = 1.0
        details.suggestedSizeIncrement = 1.0
        details.cusip = ""
        details.ratings = ""
        details.descAppend = ""
        details.bondType = ""
        details.couponType = ""
        details.callable = False
        details.putable = False
        details.coupon = 0
        details.convertible = False
        details.maturity = ""
        details.issueDate = ""
        details.nextOptionDate = ""
        details.nextOptionType = ""
        details.nextOptionPartial = (False,)
        details.notes = ""
        return details

    @staticmethod
    def aapl_instrument() -> Equity:
        contract_details = IBTestProviderStubs.aapl_equity_contract_details()
        contract_details.contract = IBContract(**contract_details.contract.__dict__)
        contract_details = IBContractDetails(**contract_details.__dict__)
        return parse_instrument(contract_details=contract_details)

    @staticmethod
    def eurusd_instrument() -> CurrencyPair:
        contract_details = IBTestProviderStubs.eurusd_forex_contract_details()
        contract_details.contract = IBContract(**contract_details.contract.__dict__)
        contract_details = IBContractDetails(**contract_details.__dict__)
        return parse_instrument(contract_details=contract_details)


class IBTestDataStubs:
    @staticmethod
    def contract(secType="STK", symbol="AAPL", exchange="NASDAQ", **kwargs):
        return IBContract(secType=secType, symbol=symbol, exchange=exchange, **kwargs)

    @staticmethod
    def account_values(fn: str = "account_values.json") -> list[dict]:
        with open(RESPONSES_PATH / fn, "rb") as f:
            raw = msgspec.json.decode(f.read())
            return raw

    @staticmethod
    def market_depth(name: str = "eurusd"):
        with open(STREAMING_PATH / f"{name}_depth.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301

    @staticmethod
    def tickers(name: str = "eurusd"):
        with open(STREAMING_PATH / f"{name}_ticker.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301

    @staticmethod
    def historic_bars():
        trades = []
        with gzip.open(RESPONSES_PATH / "historic/bars.json.gz", "rb") as f:
            for line in f:
                data = msgspec.json.decode(line)
                data["date"] = str(pd.Timestamp(data["date"]).to_pydatetime())
                tick = BarData()
                for key, value in data.items():
                    setattr(tick, key, value)
                trades.append(tick)
        return trades


class IBTestExecStubs:
    # @staticmethod
    # def create_order(
    #     order_id: int = 1,
    #     client_id: int = 1,
    #     permId: int = 0,
    #     kind: str = "LIMIT",
    #     action: str = "BUY",
    #     quantity: int = 100000,
    #     limit_price: float = 105.0,
    #     client_order_id: ClientOrderId = ClientOrderId("C-1"),
    #     account: str = 'DU123456',
    # ):
    #     if kind == "LIMIT":
    #         return IBLimitOrder(
    #             orderId=order_id,
    #             clientId=client_id,
    #             action=action,
    #             totalQuantity=quantity,
    #             lmtPrice=limit_price,
    #             permId=permId,
    #             orderRef=client_order_id.value,
    #         )
    #     else:
    #         raise RuntimeError
    #
    # @staticmethod
    # def trade_pending_submit(contract=None, order: IBOrder = None) -> Trade:
    #     contract = contract or IBTestProviderStubs.aapl_equity_contract_details().contract
    #     order = order or IBTestExecStubs.create_order()
    #     return Trade(
    #         contract=contract,
    #         order=order,
    #         orderStatus=OrderStatus(
    #             orderId=41,
    #             status="PendingSubmit",
    #             filled=0.0,
    #             remaining=0.0,
    #             avgFillPrice=0.0,
    #             permId=0,
    #             parentId=0,
    #             lastFillPrice=0.0,
    #             clientId=0,
    #             whyHeld="",
    #             mktCapPrice=0.0,
    #         ),
    #         fills=[],
    #         log=[
    #             TradeLogEntry(
    #                 time=datetime.datetime(
    #                     2022,
    #                     3,
    #                     5,
    #                     3,
    #                     6,
    #                     23,
    #                     492613,
    #                     tzinfo=datetime.timezone.utc,
    #                 ),
    #                 status="PendingSubmit",
    #                 message="",
    #                 errorCode=0,
    #             ),
    #         ],
    #     )
    #
    # @staticmethod
    # def trade_pre_submit(
    #     contract=None,
    #     order: IBOrder = None,
    #     client_order_id: ClientOrderId | None = None,
    # ) -> Trade:
    #     contract = contract or IBTestProviderStubs.aapl_equity_contract_details().contract
    #     order = order or IBTestExecStubs.create_order(client_order_id=client_order_id)
    #     return Trade(
    #         contract=contract,
    #         order=order,
    #         orderStatus=OrderStatus(
    #             orderId=41,
    #             status="PreSubmitted",
    #             filled=0.0,
    #             remaining=1.0,
    #             avgFillPrice=0.0,
    #             permId=189868420,
    #             parentId=0,
    #             lastFillPrice=0.0,
    #             clientId=1,
    #             whyHeld="",
    #             mktCapPrice=0.0,
    #         ),
    #         fills=[],
    #         log=[
    #             TradeLogEntry(
    #                 time=datetime.datetime(
    #                     2022,
    #                     3,
    #                     5,
    #                     3,
    #                     6,
    #                     23,
    #                     492613,
    #                     tzinfo=datetime.timezone.utc,
    #                 ),
    #                 status="PendingSubmit",
    #                 message="",
    #                 errorCode=0,
    #             ),
    #             TradeLogEntry(
    #                 time=datetime.datetime(
    #                     2022,
    #                     3,
    #                     5,
    #                     3,
    #                     6,
    #                     26,
    #                     871811,
    #                     tzinfo=datetime.timezone.utc,
    #                 ),
    #                 status="PreSubmitted",
    #                 message="",
    #                 errorCode=0,
    #             ),
    #         ],
    #     )
    #
    # @staticmethod
    # def trade_submitted(
    #     contract=None,
    #     order: IBOrder = None,
    #     client_order_id: ClientOrderId | None = None,
    # ) -> Trade:
    #     contract = contract or IBTestProviderStubs.aapl_equity_contract_details().contract
    #     order = order or IBTestExecStubs.create_order(client_order_id=client_order_id)
    #     from ibapi.order import Order as IBOrder
    #     from ibapi.order_state import OrderState as IBOrderState
    #
    #     return Trade(
    #         contract=contract,
    #         order=order,
    #         orderStatus=OrderStatus(
    #             orderId=41,
    #             status="Submitted",
    #             filled=0.0,
    #             remaining=1.0,
    #             avgFillPrice=0.0,
    #             permId=order.permId,
    #             parentId=0,
    #             lastFillPrice=0.0,
    #             clientId=1,
    #             whyHeld="",
    #             mktCapPrice=0.0,
    #         ),
    #         fills=[],
    #         log=[
    #             TradeLogEntry(
    #                 time=datetime.datetime(
    #                     2022,
    #                     3,
    #                     5,
    #                     3,
    #                     6,
    #                     23,
    #                     492613,
    #                     tzinfo=datetime.timezone.utc,
    #                 ),
    #                 status="PendingSubmit",
    #                 message="",
    #                 errorCode=0,
    #             ),
    #             TradeLogEntry(
    #                 time=datetime.datetime(
    #                     2022,
    #                     3,
    #                     5,
    #                     3,
    #                     6,
    #                     26,
    #                     871811,
    #                     tzinfo=datetime.timezone.utc,
    #                 ),
    #                 status="PreSubmitted",
    #                 message="",
    #                 errorCode=0,
    #             ),
    #             TradeLogEntry(
    #                 time=datetime.datetime(
    #                     2022,
    #                     3,
    #                     5,
    #                     3,
    #                     6,
    #                     28,
    #                     378175,
    #                     tzinfo=datetime.timezone.utc,
    #                 ),
    #                 status="Submitted",
    #                 message="",
    #                 errorCode=0,
    #             ),
    #         ],
    #     )
    #
    # @staticmethod
    # def trade_pre_cancel(contract=None, order: IBOrder = None) -> Trade:
    #     contract = contract or IBTestProviderStubs.aapl_equity_contract_details().contract
    #     order = order or IBTestExecStubs.create_order()
    #     return Trade(
    #         contract=contract,
    #         order=order,
    #         orderStatus=OrderStatus(
    #             orderId=41,
    #             status="PendingCancel",
    #             filled=0.0,
    #             remaining=1.0,
    #             avgFillPrice=0.0,
    #             permId=189868420,
    #             parentId=0,
    #             lastFillPrice=0.0,
    #             clientId=1,
    #             whyHeld="",
    #             mktCapPrice=0.0,
    #         ),
    #         fills=[],
    #         log=[
    #             TradeLogEntry(
    #                 time=datetime.datetime(
    #                     2022,
    #                     3,
    #                     6,
    #                     2,
    #                     17,
    #                     18,
    #                     455087,
    #                     tzinfo=datetime.timezone.utc,
    #                 ),
    #                 status="PendingCancel",
    #                 message="",
    #                 errorCode=0,
    #             ),
    #         ],
    #     )
    #
    # @staticmethod
    # def trade_canceled(contract=None, order: IBOrder = None) -> Trade:
    #     contract = contract or IBTestProviderStubs.aapl_equity_contract_details().contract
    #     order = order or IBTestExecStubs.create_order()
    #     return Trade(
    #         contract=contract,
    #         order=order,
    #         orderStatus=OrderStatus(
    #             orderId=41,
    #             status="Cancelled",
    #             filled=0.0,
    #             remaining=1.0,
    #             avgFillPrice=0.0,
    #             permId=order.permId,
    #             parentId=0,
    #             lastFillPrice=0.0,
    #             clientId=1,
    #             whyHeld="",
    #             mktCapPrice=0.0,
    #         ),
    #         fills=[],
    #         log=[
    #             TradeLogEntry(
    #                 time=datetime.datetime(
    #                     2022,
    #                     3,
    #                     6,
    #                     2,
    #                     17,
    #                     18,
    #                     455087,
    #                     tzinfo=datetime.timezone.utc,
    #                 ),
    #                 status="PendingCancel",
    #                 message="",
    #                 errorCode=0,
    #             ),
    #             TradeLogEntry(
    #                 time=datetime.datetime(2022, 3, 6, 2, 23, 2, 847, tzinfo=datetime.timezone.utc),
    #                 status="Cancelled",
    #                 message="Error 10148, reqId 45: OrderId 45 that needs to be cancelled cannot be cancelled, state: PendingCancel.",
    #                 errorCode=10148,
    #             ),
    #         ],
    #     )

    @staticmethod
    def execution(
        client_order_id: ClientOrderId = ClientOrderId("C-1"),
        venue_order_id: VenueOrderId = VenueOrderId("101"),
        account: str = "DU123456",
    ) -> Execution:
        params = {
            "execId": "1",
            "time": "19700101 00:00:00 America/New_York",
            "acctNumber": account,
            "exchange": "NYSE",
            "side": "BOT",
            "shares": Decimal(100),
            "price": 50.0,
            "permId": 0,
            "clientId": 1,
            "orderId": int(venue_order_id.value),
            "liquidation": 0,
            "cumQty": Decimal(100),
            "avgPrice": 50.0,
            "orderRef": f"{client_order_id.value}:{venue_order_id.value}",
            "evRule": "",
            "evMultiplier": 0.0,
            "modelCode": "",
            "lastLiquidity": 0,
        }
        execution = Execution()
        for key, value in params.items():
            setattr(execution, key, value)
        return execution

    @staticmethod
    def commission() -> CommissionReport:
        params = {
            "execId": "1",
            "commission": 1.0,
            "currency": "USD",
            "realizedPNL": 0.0,
            "yield_": 0.0,
            "yieldRedemptionDate": 0,
        }
        commission = CommissionReport()
        for key, value in params.items():
            setattr(commission, key, value)
        return commission


def filter_out_options(instrument) -> bool:
    return not isinstance(instrument, OptionsContract)
