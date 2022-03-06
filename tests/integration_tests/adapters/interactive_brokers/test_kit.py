import datetime
import pathlib
import pickle

from ib_insync import Contract
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


class IBTestStubs:
    @staticmethod
    def contract_details(symbol: str):
        return pickle.load(  # noqa: S301
            open(RESPONSES_PATH / f"contracts/{symbol.upper()}.pkl", "rb")
        )

    @staticmethod
    def contract(secType="STK", symbol="AAPL", exchange="NASDAQ", **kwargs):
        return Contract(secType=secType, symbol=symbol, exchange=exchange, **kwargs)

    @staticmethod
    def instrument(symbol: str) -> Equity:
        contract_details = IBTestStubs.contract_details(symbol)
        return parse_instrument(contract_details=contract_details)

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
        with open(RESPONSES_PATH / "historic/trade_ticks.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301

    @staticmethod
    def historic_bid_ask():
        with open(RESPONSES_PATH / "historic/bid_ask_ticks.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301

    @staticmethod
    def create_order(
        contract: Contract,
        status=OrderStatus.PendingSubmit,
        order_type: IBOrder = IBLimitOrder,
        side="SELL",
        price=1.11,
        size=20000,
    ) -> Trade:
        now = datetime.datetime.now(datetime.timezone.utc)
        orderStatus = OrderStatus(orderId=1, status=status)
        logEntry = TradeLogEntry(now, orderStatus.status)
        order = order_type(side, size, price)
        return Trade(contract, order, orderStatus, [], [logEntry])


class IBExecTestStubs:
    @staticmethod
    def ib_order(
        order_id: int = 1,
        client_id: int = 1,
        kind: str = "LIMIT",
        action: str = "BUY",
        quantity: int = 1,
        limit_price: float = 0.01,
    ):
        if kind == "LIMIT":
            return IBLimitOrder(
                orderId=order_id,
                clientId=client_id,
                action=action,
                totalQuantity=quantity,
                lmtPrice=limit_price,
            )
        else:
            raise RuntimeError

    @staticmethod
    def trade_pending_submit(contract=None, order: IBOrder = None) -> Trade:
        contract = contract or IBTestStubs.contract_details("AAPL").contract
        order = order or IBExecTestStubs.ib_order()
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
                        2022, 3, 5, 3, 6, 23, 492613, tzinfo=datetime.timezone.utc
                    ),
                    status="PendingSubmit",
                    message="",
                    errorCode=0,
                ),
            ],
        )

    @staticmethod
    def trade_pre_submit(contract=None, order: IBOrder = None) -> Trade:
        contract = contract or IBTestStubs.contract_details("AAPL").contract
        order = order or IBExecTestStubs.ib_order()
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
                        2022, 3, 5, 3, 6, 23, 492613, tzinfo=datetime.timezone.utc
                    ),
                    status="PendingSubmit",
                    message="",
                    errorCode=0,
                ),
                TradeLogEntry(
                    time=datetime.datetime(
                        2022, 3, 5, 3, 6, 26, 871811, tzinfo=datetime.timezone.utc
                    ),
                    status="PreSubmitted",
                    message="",
                    errorCode=0,
                ),
            ],
        )

    @staticmethod
    def trade_submitted(contract=None, order: IBOrder = None) -> Trade:
        contract = contract or IBTestStubs.contract_details("AAPL").contract
        order = order or IBExecTestStubs.ib_order()
        return Trade(
            contract=contract,
            order=order,
            orderStatus=OrderStatus(
                orderId=41,
                status="Submitted",
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
                        2022, 3, 5, 3, 6, 23, 492613, tzinfo=datetime.timezone.utc
                    ),
                    status="PendingSubmit",
                    message="",
                    errorCode=0,
                ),
                TradeLogEntry(
                    time=datetime.datetime(
                        2022, 3, 5, 3, 6, 26, 871811, tzinfo=datetime.timezone.utc
                    ),
                    status="PreSubmitted",
                    message="",
                    errorCode=0,
                ),
                TradeLogEntry(
                    time=datetime.datetime(
                        2022, 3, 5, 3, 6, 28, 378175, tzinfo=datetime.timezone.utc
                    ),
                    status="Submitted",
                    message="",
                    errorCode=0,
                ),
            ],
        )
