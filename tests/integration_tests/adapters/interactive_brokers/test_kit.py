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

import datetime as dt
import gzip
import pickle
from decimal import Decimal

import msgspec
import pandas as pd
import pytz
from ibapi.commission_report import CommissionReport
from ibapi.common import UNSET_DECIMAL
from ibapi.common import BarData
from ibapi.contract import Contract  # We use this for the expected response from IB
from ibapi.contract import ContractDetails
from ibapi.execution import Execution
from ibapi.order import Order as IBOrder
from ibapi.order_state import OrderState as IBOrderState
from ibapi.softdollartier import SoftDollarTier
from ibapi.tag_value import TagValue

from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.common import IBContractDetails
from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import OptionContract
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT / "integration_tests" / "adapters" / "interactive_brokers/"
RESPONSES_PATH = TEST_PATH / "resources" / "responses"
STREAMING_PATH = TEST_PATH / "resources" / "streaming"
CONTRACT_PATH = RESPONSES_PATH / "contracts"


def set_attributes(obj, params: dict):
    for key, value in params.items():
        setattr(obj, key, value)
    return obj


class IBTestIncomingMessages:
    @staticmethod
    def get_msg(msg_type: str) -> bytes:
        with open(RESPONSES_PATH / f"{msg_type}.txt", "rb") as f:
            return f.read()


class IBTestContractStubs:
    @staticmethod
    def create_contract(
        conId=0,
        symbol="",
        secType="",
        lastTradeDateOrContractMonth="",
        strike=0.0,
        right="",
        multiplier="",
        exchange="",
        currency="",
        localSymbol="",
        primaryExchange="",
        tradingClass="",
        includeExpired=False,
        secIdType="",
        secId="",
        description="",
        issuerId="",
        comboLegsDescrip="",
        comboLegs=None,
        deltaNeutralContract=None,
    ) -> Contract:
        return set_attributes(Contract(), locals())

    def convert_contract_to_ib_contract(contract: Contract) -> IBContract:
        return IBContract(**contract.__dict__)

    @staticmethod
    def create_contract_details(
        contract=Contract(),
        marketName="",
        minTick=0.0,
        orderTypes="",
        validExchanges="",
        priceMagnifier=0,
        underConId=0,
        longName="",
        contractMonth="",
        industry="",
        category="",
        subcategory="",
        timeZoneId="",
        tradingHours="",
        liquidHours="",
        evRule="",
        evMultiplier=0,
        mdSizeMultiplier=None,
        aggGroup=0,
        underSymbol="",
        underSecType="",
        marketRuleIds="",
        secIdList=None,
        realExpirationDate="",
        lastTradeTime="",
        stockType="",
        minSize=UNSET_DECIMAL,
        sizeIncrement=UNSET_DECIMAL,
        suggestedSizeIncrement=UNSET_DECIMAL,
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
    ) -> ContractDetails:
        return set_attributes(ContractDetails(), locals())

    @staticmethod
    def convert_contract_details_to_ib_contract_details(
        contract_details: ContractDetails,
    ) -> IBContractDetails:
        contract_details.contract = IBTestContractStubs.convert_contract_to_ib_contract(
            contract_details.contract,
        )
        return IBContractDetails(**contract_details.__dict__)

    @staticmethod
    def create_instrument(contract_details: ContractDetails) -> Instrument:
        contract_details = IBTestContractStubs.convert_contract_details_to_ib_contract_details(
            contract_details,
        )
        return parse_instrument(contract_details, contract_details.contract.primaryExchange)

    @staticmethod
    def aapl_equity_contract() -> Contract:
        params = {
            "secType": "STK",
            "conId": 265598,
            "symbol": "AAPL",
            "exchange": "SMART",
            "primaryExchange": "NASDAQ",
            "currency": "USD",
            "localSymbol": "AAPL",
            "tradingClass": "NMS",
        }
        return IBTestContractStubs.create_contract(**params)

    @staticmethod
    def aapl_equity_ib_contract() -> IBContract:
        contract = IBTestContractStubs.aapl_equity_contract()
        return IBTestContractStubs.convert_contract_to_ib_contract(contract)

    @staticmethod
    def aapl_equity_contract_details() -> ContractDetails:
        params = {
            "contract": IBTestContractStubs.aapl_equity_contract(),
            "marketName": "NMS",
            "minTick": 0.01,
            "orderTypes": "ACTIVETIM,AD,ADJUST,ALERT,ALLOC,AVGCOST,BASKET,BENCHPX,CASHQTY,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,IOC,LIT,LMT,MIT,MKT,MTL,NGCOMB,NONALGO,OCA,PEGBENCH,SCALE,SCALERST,SNAPMID,SNAPMKT,SNAPREL,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF",
            "validExchanges": "SMART,AMEX,NYSE,CBOE,PHLX,ISE,CHX,ARCA,ISLAND,DRCTEDGE,BEX,BATS,EDGEA,CSFBALGO,JEFFALGO,BYX,IEX,EDGX,FOXRIVER,PEARL,NYSENAT,LTSE,MEMX,TPLUS1,PSX",
            "priceMagnifier": 1,
            "underConId": 0,
            "longName": "APPLE INC",
            "contractMonth": "",
            "industry": "Technology",
            "category": "Computers",
            "subcategory": "Computers",
            "timeZoneId": "US/Eastern",
            "tradingHours": "20221207:0700-20221207:2000;20221208:0700-20221208:2000;20221209:0700-20221209:2000;20221210:CLOSED;20221211:CLOSED;20221212:0700-20221212:2000",
            "liquidHours": "20221207:0700-20221207:2000;20221208:0700-20221208:2000;20221209:0700-20221209:2000;20221210:CLOSED;20221211:CLOSED;20221212:0700-20221212:2000",
            "evRule": "",
            "evMultiplier": 0,
            "mdSizeMultiplier": 1,
            "aggGroup": 1,
            "underSymbol": "",
            "underSecType": "",
            "marketRuleIds": "26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26,26",
            "secIdList": [TagValue(tag="ISIN", value="US0378331005")],
            "realExpirationDate": "",
            "lastTradeTime": "",
            "stockType": "COMMON",
            "minSize": 1.0,
            "sizeIncrement": 1.0,
            "suggestedSizeIncrement": 100.0,
            "cusip": "",
            "ratings": "",
            "descAppend": "",
            "bondType": "",
            "couponType": "",
            "callable": False,
            "putable": False,
            "coupon": 0,
            "convertible": False,
            "maturity": "",
            "issueDate": "",
            "nextOptionDate": "",
            "nextOptionType": "",
            "nextOptionPartial": False,
            "notes": "",
        }
        return IBTestContractStubs.create_contract_details(**params)

    @staticmethod
    def aapl_equity_ib_contract_details() -> IBContractDetails:
        contract_details = IBTestContractStubs.aapl_equity_contract_details()
        return IBTestContractStubs.convert_contract_details_to_ib_contract_details(contract_details)

    @staticmethod
    def cl_future_contract() -> Contract:
        params = {
            "secType": "FUT",
            "conId": 174230596,
            "symbol": "CL",
            "lastTradeDateOrContractMonth": "20231120",
            "multiplier": "1000",
            "exchange": "NYMEX",
            "currency": "USD",
            "localSymbol": "CLZ3",
            "tradingClass": "CL",
        }
        return IBTestContractStubs.create_contract(**params)

    @staticmethod
    def cl_future_contract_details() -> ContractDetails:
        params = {
            "contract": IBTestContractStubs.cl_future_contract(),
            "marketName": "CL",
            "minTick": 0.01,
            "orderTypes": "ACTIVETIM,AD,ADJUST,ALERT,ALGO,ALLOC,AVGCOST,BASKET,BENCHPX,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,ICE,IOC,LIT,LMT,LTH,MIT,MKT,MTL,NGCOMB,NONALGO,OCA,PEGBENCH,SCALE,SCALERST,SIZECHK,SNAPMID,SNAPMKT,SNAPREL,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF",
            "validExchanges": "NYMEX,QBALGO",
            "priceMagnifier": 1,
            "underConId": 17340715,
            "longName": "Light Sweet Crude Oil",
            "contractMonth": "202312",
            "industry": "",
            "category": "",
            "subcategory": "",
            "timeZoneId": "US/Eastern",
            "tradingHours": "20221206:1800-20221207:1700;20221207:1800-20221208:1700;20221208:1800-20221209:1700;20221210:CLOSED;20221211:1800-20221212:1700;20221212:1800-20221213:1700",
            "liquidHours": "20221207:0930-20221207:1700;20221208:0930-20221208:1700;20221209:0930-20221209:1700;20221210:CLOSED;20221211:CLOSED;20221212:0930-20221212:1700;20221212:1800-20221213:1700",
            "evRule": "",
            "evMultiplier": 0,
            "mdSizeMultiplier": 1,
            "aggGroup": 2147483647,
            "underSymbol": "CL",
            "underSecType": "IND",
            "marketRuleIds": "32,32",
            "secIdList": [],
            "realExpirationDate": "20231120",
            "lastTradeTime": "14:30:00",
            "stockType": "",
            "minSize": 1.0,
            "sizeIncrement": 1.0,
            "suggestedSizeIncrement": 1.0,
            "cusip": "",
            "ratings": "",
            "descAppend": "",
            "bondType": "",
            "couponType": "",
            "callable": False,
            "putable": False,
            "coupon": 0,
            "convertible": False,
            "maturity": "",
            "issueDate": "",
            "nextOptionDate": "",
            "nextOptionType": "",
            "nextOptionPartial": False,
            "notes": "",
        }
        return IBTestContractStubs.create_contract_details(**params)

    @staticmethod
    def es_future_option_contract() -> Contract:
        params = {
            "secType": "FOP",
            "conId": 715834345,
            "symbol": "ES",
            "lastTradeDateOrContractMonth": "20240722",
            "strike": 5655.0,
            "right": "C",
            "multiplier": "50",
            "exchange": "CME",
            "primaryExchange": "",
            "currency": "USD",
            "localSymbol": "E4AN4 C5655",
            "tradingClass": "E4A",
            "includeExpired": False,
            "secIdType": "",
            "secId": "",
            "description": "",
            "issuerId": "",
            "comboLegsDescrip": "",
            "comboLegs": [],
            "deltaNeutralContract": None,
        }
        return IBTestContractStubs.create_contract(**params)

    @classmethod
    def es_future_option_contract_details(cls) -> ContractDetails:
        params = {
            "contract": cls.es_future_option_contract(),
            "marketName": "E4A",
            "minTick": 0.05,
            "orderTypes": "ACTIVETIM,AD,ADJUST,ALERT,ALLOC,AVGCOST,BASKET,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,IOC,LIT,LMT,LTH,MIT,MKT,MTL,NGCOMB,NONALGO,OCA,SCALE,SCALERST,SNAPMID,SNAPMKT,SNAPREL,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,VOLAT,WHATIF",
            "validExchanges": "CME",
            "priceMagnifier": 1,
            "underConId": 568550526,
            "longName": "E-mini S&P 500",
            "contractMonth": "202407",
            "industry": "",
            "category": "",
            "subcategory": "",
            "timeZoneId": "US/Central",
            "tradingHours": "20240720:CLOSED;20240721:1700-20240722:1500",
            "liquidHours": "20240720:CLOSED;20240721:CLOSED;20240722:0830-20240722:1500",
            "evRule": "",
            "evMultiplier": 0,
            "mdSizeMultiplier": 1,
            "aggGroup": 2147483647,
            "underSymbol": "ESU4",
            "underSecType": "FUT",
            "marketRuleIds": "3541",
            "secIdList": [],
            "realExpirationDate": "20240722",
            "lastTradeTime": "23:00:00",
            "stockType": "",
            "minSize": 1.0,
            "sizeIncrement": 1.0,
            "suggestedSizeIncrement": 1.0,
            "cusip": "",
            "ratings": "",
            "descAppend": "",
            "bondType": "",
            "couponType": "",
            "callable": False,
            "putable": False,
            "coupon": 0,
            "convertible": False,
            "maturity": "",
            "issueDate": "",
            "nextOptionDate": "",
            "nextOptionType": "",
            "nextOptionPartial": False,
            "notes": "",
        }
        return IBTestContractStubs.create_contract_details(**params)

    @staticmethod
    def eurusd_forex_contract() -> Contract:
        params = {
            "secType": "CASH",
            "conId": 12087792,
            "symbol": "EUR",
            "exchange": "IDEALPRO",
            "currency": "USD",
            "localSymbol": "EUR.USD",
            "tradingClass": "EUR.USD",
        }
        return IBTestContractStubs.create_contract(**params)

    @staticmethod
    def eurusd_forex_contract_details() -> ContractDetails:
        params = {
            "contract": IBTestContractStubs.eurusd_forex_contract(),
            "marketName": "EUR.USD",
            "minTick": 5e-05,
            "orderTypes": "ACTIVETIM,AD,ADJUST,ALERT,ALGO,ALLOC,AVGCOST,BASKET,CASHQTY,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,IOC,LIT,LMT,MIT,MKT,NONALGO,OCA,REL,RELPCTOFS,SCALE,SCALERST,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF",
            "validExchanges": "IDEALPRO",
            "priceMagnifier": 1,
            "underConId": 0,
            "longName": "European Monetary Union Euro",
            "contractMonth": "",
            "industry": "",
            "category": "",
            "subcategory": "",
            "timeZoneId": "US/Eastern",
            "tradingHours": "20221205:1715-20221206:1700;20221206:1715-20221207:1700;20221207:1715-20221208:1700;20221208:1715-20221209:1700;20221210:CLOSED;20221211:1715-20221212:1700",
            "liquidHours": "20221205:1715-20221206:1700;20221206:1715-20221207:1700;20221207:1715-20221208:1700;20221208:1715-20221209:1700;20221210:CLOSED;20221211:1715-20221212:1700",
            "evRule": "",
            "evMultiplier": 0,
            "mdSizeMultiplier": 1,
            "aggGroup": 4,
            "underSymbol": "",
            "underSecType": "",
            "marketRuleIds": "239",
            "secIdList": [],
            "realExpirationDate": "",
            "lastTradeTime": "",
            "stockType": "",
            "minSize": 1.0,
            "sizeIncrement": 1.0,
            "suggestedSizeIncrement": 1.0,
            "cusip": "",
            "ratings": "",
            "descAppend": "",
            "bondType": "",
            "couponType": "",
            "callable": False,
            "putable": False,
            "coupon": 0,
            "convertible": False,
            "maturity": "",
            "issueDate": "",
            "nextOptionDate": "",
            "nextOptionType": "",
            "nextOptionPartial": False,
            "notes": "",
        }
        return IBTestContractStubs.create_contract_details(**params)

    @staticmethod
    def tsla_option_contract() -> Contract:
        params = {
            "secType": "OPT",
            "conId": 445067953,
            "symbol": "TSLA",
            "lastTradeDateOrContractMonth": "20230120",
            "strike": 100.0,
            "right": "C",
            "multiplier": "100",
            "exchange": "MIAX",
            "currency": "USD",
            "localSymbol": "TSLA  230120C00100000",
            "tradingClass": "TSLA",
        }
        return IBTestContractStubs.create_contract(**params)

    @staticmethod
    def tsla_option_contract_details() -> ContractDetails:
        params = {
            "contract": IBTestContractStubs.tsla_option_contract(),
            "marketName": "TSLA",
            "minTick": 0.01,
            "orderTypes": "ACTIVETIM,AD,ADJUST,ALERT,ALLOC,AVGCOST,BASKET,COND,CONDORDER,DAY,DEACT,DEACTDIS,DEACTEOD,GAT,GTC,GTD,GTT,HID,IOC,LIT,LMT,MIT,MKT,MTL,NGCOMB,NONALGO,OCA,OPENCLOSE,SCALE,SCALERST,SNAPMID,SNAPMKT,SNAPREL,STP,STPLMT,TRAIL,TRAILLIT,TRAILLMT,TRAILMIT,WHATIF",
            "validExchanges": "SMART,AMEX,CBOE,PHLX,PSE,ISE,BOX,BATS,NASDAQOM,CBOE2,NASDAQBX,MIAX,GEMINI,EDGX,MERCURY,PEARL,EMERALD",
            "priceMagnifier": 1,
            "underConId": 76792991,
            "longName": "TESLA INC",
            "contractMonth": "202301",
            "industry": "",
            "category": "",
            "subcategory": "",
            "timeZoneId": "US/Eastern",
            "tradingHours": "20221207:0930-20221207:1600;20221208:0930-20221208:1600;20221209:0930-20221209:1600;20221210:CLOSED;20221211:CLOSED;20221212:0930-20221212:1600",
            "liquidHours": "20221207:0930-20221207:1600;20221208:0930-20221208:1600;20221209:0930-20221209:1600;20221210:CLOSED;20221211:CLOSED;20221212:0930-20221212:1600",
            "evRule": "",
            "evMultiplier": 0,
            "mdSizeMultiplier": 1,
            "aggGroup": 2,
            "underSymbol": "TSLA",
            "underSecType": "STK",
            "marketRuleIds": "32,109,109,109,109,109,109,109,32,109,32,109,109,109,109,109,109",
            "secIdList": [],
            "realExpirationDate": "20230120",
            "lastTradeTime": "",
            "stockType": "",
            "minSize": 1.0,
            "sizeIncrement": 1.0,
            "suggestedSizeIncrement": 1.0,
            "cusip": "",
            "ratings": "",
            "descAppend": "",
            "bondType": "",
            "couponType": "",
            "callable": False,
            "putable": False,
            "coupon": 0,
            "convertible": False,
            "maturity": "",
            "issueDate": "",
            "nextOptionDate": "",
            "nextOptionType": "",
            "nextOptionPartial": (False,),
            "notes": "",
        }
        return IBTestContractStubs.create_contract_details(**params)

    @staticmethod
    def aapl_instrument() -> Equity:
        contract_details = IBTestContractStubs.aapl_equity_contract_details()
        instrument = IBTestContractStubs.create_instrument(contract_details)
        return instrument

    @staticmethod
    def eurusd_instrument() -> CurrencyPair:
        contract_details = IBTestContractStubs.eurusd_forex_contract_details()
        return IBTestContractStubs.create_instrument(contract_details)


class IBTestDataStubs:
    @staticmethod
    def account_values(fn: str = "account_values.json") -> list[dict]:
        with open(RESPONSES_PATH / fn, "rb") as f:
            raw = msgspec.json.decode(f.read())
            return raw

    @staticmethod
    def market_depth(name: str = "eurusd"):
        with open(STREAMING_PATH / f"{name}_depth.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301 (pickle is safe here)

    @staticmethod
    def tickers(name: str = "eurusd"):
        with open(STREAMING_PATH / f"{name}_ticker.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301 (pickle is safe here)

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
    ORDER_STATE_DEFAULT = {
        "status": "",
        "initMarginBefore": "1.7976931348623157E308",
        "maintMarginBefore": "1.7976931348623157E308",
        "equityWithLoanBefore": "1.7976931348623157E308",
        "initMarginChange": "1.7976931348623157E308",
        "maintMarginChange": "1.7976931348623157E308",
        "equityWithLoanChange": "1.7976931348623157E308",
        "initMarginAfter": "1.7976931348623157E308",
        "maintMarginAfter": "1.7976931348623157E308",
        "equityWithLoanAfter": "1.7976931348623157E308",
        "commission": 1.7976931348623157e308,
        "minCommission": 1.7976931348623157e308,
        "maxCommission": 1.7976931348623157e308,
        "commissionCurrency": "",
        "warningText": "",
        "completedTime": "",
        "completedStatus": "",
    }

    @staticmethod
    def aapl_buy_ib_order(
        order_id: int = 600,
        client_id: int = 2,
        total_quantity: str = "100",
        account_id: str = "DU123456",
    ) -> IBOrder:
        params = {
            "softDollarTier": SoftDollarTier("", "", ""),
            # order identifier
            "orderId": order_id,
            "clientId": client_id,
            "permId": 1916994655,
            # main order fields
            "action": "BUY",
            "totalQuantity": Decimal(total_quantity),
            "orderType": "MKT",
            "lmtPrice": 0.0,
            "auxPrice": 0.0,
            # extended order fields
            "tif": "IOC",
            "activeStartTime": "",
            "activeStopTime": "",
            "ocaGroup": "",
            "ocaType": 3,
            "orderRef": f"O-20240102-1754-001-000-1:{order_id}",
            "transmit": True,
            "parentId": 0,
            "blockOrder": False,
            "sweepToFill": False,
            "displaySize": 2147483647,
            "triggerMethod": 0,
            "outsideRth": False,
            "hidden": False,
            "goodAfterTime": "",
            "goodTillDate": "",
            "rule80A": "",
            "allOrNone": False,
            "minQty": 2147483647,
            "percentOffset": 1.7976931348623157e308,
            "overridePercentageConstraints": False,
            "trailStopPrice": 1.7976931348623157e308,
            "trailingPercent": 1.7976931348623157e308,
            # financial advisors only
            "faGroup": "",
            "faProfile": "",
            "faMethod": "",
            "faPercentage": "",
            # institutional (ie non-cleared) only
            "designatedLocation": "",
            "openClose": "",
            "origin": 0,
            "shortSaleSlot": 0,
            "exemptCode": -1,
            # SMART routing only
            "discretionaryAmt": 0.0,
            "optOutSmartRouting": False,
            # BOX exchange orders only
            "auctionStrategy": 0,
            "startingPrice": 1.7976931348623157e308,
            "stockRefPrice": 1.7976931348623157e308,
            "delta": 1.7976931348623157e308,
            # pegged to stock and VOL orders only
            "stockRangeLower": 1.7976931348623157e308,
            "stockRangeUpper": 1.7976931348623157e308,
            "randomizePrice": False,
            "randomizeSize": False,
            # VOLATILITY ORDERS ONLY
            "volatility": 1.7976931348623157e308,
            "volatilityType": 0,
            "deltaNeutralOrderType": "None",
            "deltaNeutralAuxPrice": 1.7976931348623157e308,
            "deltaNeutralConId": 0,
            "deltaNeutralSettlingFirm": "",
            "deltaNeutralClearingAccount": "",
            "deltaNeutralClearingIntent": "",
            "deltaNeutralOpenClose": "?",
            "deltaNeutralShortSale": False,
            "deltaNeutralShortSaleSlot": 0,
            "deltaNeutralDesignatedLocation": "",
            "continuousUpdate": False,
            "referencePriceType": 0,
            # COMBO ORDERS ONLY
            "basisPoints": 1.7976931348623157e308,
            "basisPointsType": 2147483647,
            # SCALE ORDERS ONLY
            "scaleInitLevelSize": 2147483647,
            "scaleSubsLevelSize": 2147483647,
            "scalePriceIncrement": 1.7976931348623157e308,
            "scalePriceAdjustValue": 1.7976931348623157e308,
            "scalePriceAdjustInterval": 2147483647,
            "scaleProfitOffset": 1.7976931348623157e308,
            "scaleAutoReset": False,
            "scaleInitPosition": 2147483647,
            "scaleInitFillQty": 2147483647,
            "scaleRandomPercent": False,
            "scaleTable": "",
            # HEDGE ORDERS
            "hedgeType": "",
            "hedgeParam": "",
            # Clearing info
            "account": account_id,
            "settlingFirm": "",
            "clearingAccount": "",
            "clearingIntent": "IB",
            # ALGO ORDERS ONLY
            "algoStrategy": "",
            "algoParams": None,
            "smartComboRoutingParams": None,
            "algoId": "",
            # What-if
            "whatIf": False,
            # Not Held
            "notHeld": False,
            "solicited": False,
            # models
            "modelCode": "",
            # order combo legs
            "orderComboLegs": None,
            "orderMiscOptions": None,
            # VER PEG2BENCH fields
            "referenceContractId": 0,
            "peggedChangeAmount": 0.0,
            "isPeggedChangeAmountDecrease": False,
            "referenceChangeAmount": 0.0,
            "referenceExchangeId": "",
            "adjustedOrderType": "None",
            "triggerPrice": 1.7976931348623157e308,
            "adjustedStopPrice": 1.7976931348623157e308,
            "adjustedStopLimitPrice": 1.7976931348623157e308,
            "adjustedTrailingAmount": 1.7976931348623157e308,
            "adjustableTrailingUnit": 0,
            "lmtPriceOffset": 1.7976931348623157e308,
            "conditions": [],
            "conditionsCancelOrder": False,
            "conditionsIgnoreRth": False,
            # ext operator
            "extOperator": "",
            # native cash quantity
            "cashQty": 0.0,
            "mifid2DecisionMaker": "",
            "mifid2DecisionAlgo": "",
            "mifid2ExecutionTrader": "",
            "mifid2ExecutionAlgo": "",
            "dontUseAutoPriceForHedge": True,
            "isOmsContainer": False,
            "discretionaryUpToLimitPrice": False,
            "autoCancelDate": "",
            "filledQuantity": Decimal("170141183460469231731687303715884105727"),
            "refFuturesConId": 0,
            "autoCancelParent": False,
            "shareholder": "",
            "imbalanceOnly": False,
            "routeMarketableToBbo": False,
            "parentPermId": 0,
            "usePriceMgmtAlgo": False,
            "duration": 2147483647,
            "postToAts": 2147483647,
            "advancedErrorOverride": "",
            "manualOrderTime": "",
            "minTradeQty": 2147483647,
            "minCompeteSize": 100,
            "competeAgainstBestOffset": 0.02,
            "midOffsetAtWhole": 1.7976931348623157e308,
            "midOffsetAtHalf": 1.7976931348623157e308,
        }
        return set_attributes(IBOrder(), params)

    @staticmethod
    def ib_order_state(state: str = "PreSubmitted"):
        params = IBTestExecStubs.ORDER_STATE_DEFAULT
        params["status"] = state
        if state == "Filled":
            params["commission"] = 1.8
            params["commissionCurrency"] = "USD"
        return set_attributes(IBOrderState(), params)

    @staticmethod
    def execution(
        order_id: int,
        account_id: str = "DU123456",
        exec_timestamp: dt.datetime | None = None,
        tz: str = "US/Eastern",
    ) -> Execution:
        random_default = dt.datetime(2022, 1, 4, 19, 32, 36, 0, tzinfo=dt.UTC)
        exec_timestamp = exec_timestamp or random_default
        exec_timestamp = exec_timestamp.astimezone(pytz.timezone(tz))
        params = {
            "execId": "0000e0d5.6596b0d2.01.01",
            "time": exec_timestamp.strftime("%Y%m%d %H:%M:%S %Z"),
            "acctNumber": account_id,
            "exchange": "NYSE",
            "side": "BOT",
            "shares": Decimal(100),
            "price": 50.0,
            "permId": 395704644,
            "clientId": 1,
            "orderId": order_id,
            "liquidation": 0,
            "cumQty": Decimal(100),
            "avgPrice": 50.0,
            "orderRef": f"O-{exec_timestamp.strftime('%Y%m%d')}-{exec_timestamp.strftime('%H%M')}-001-000-1:{order_id}",
            "evRule": "",
            "evMultiplier": 0.0,
            "modelCode": "",
            "lastLiquidity": 2,
        }
        return set_attributes(Execution(), params)

    @staticmethod
    def commission() -> CommissionReport:
        params = {
            "execId": "0000e0d5.6596b0d2.01.01",
            "commission": 1.0,
            "currency": "USD",
            "realizedPNL": 0.0,
            "yield_": 0.0,
            "yieldRedemptionDate": 0,
        }
        return set_attributes(CommissionReport(), params)


def filter_out_options(instrument) -> bool:
    return not isinstance(instrument, OptionContract)
