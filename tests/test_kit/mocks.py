# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import datetime
import inspect
from typing import List, Optional

from nautilus_trader.common.clock import Clock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.uuid import UUID
from nautilus_trader.data.client import MarketDataClient
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.client import ExecutionClient
from nautilus_trader.execution.database import ExecutionDatabase
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.data import DataType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.order.base import Order
from nautilus_trader.model.position import Position
from nautilus_trader.trading.account import Account
from nautilus_trader.trading.strategy import TradingStrategy


class ObjectStorer:
    """
    A test class which stores objects to assists with test assertions.
    """

    def __init__(self):
        """
        Initialize a new instance of the `ObjectStorer` class.
        """
        self.count = 0
        self._store = []

    def get_store(self) -> list:
        """
        Return the list or stored objects.

        Returns
        -------
        list[Object]

        """
        return self._store

    def store(self, obj) -> None:
        """Store the given object.

        Parameters
        ----------
        obj : object
            The object to store.

        """
        self.count += 1
        self._store.append(obj)

    def store_2(self, obj1, obj2) -> None:
        """Store the given objects as a tuple.

        Parameters
        ----------
        obj1 : object
            The first object to store.
        obj2 : object
            The second object to store.

        """
        self.store((obj1, obj2))


class MockStrategy(TradingStrategy):
    """
    Provides a mock trading strategy for testing.
    """

    def __init__(self, bar_type: BarType):
        """
        Initialize a new instance of the `MockStrategy` class.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the strategy.

        """
        super().__init__(order_id_tag="001")

        self.object_storer = ObjectStorer()
        self.bar_type = bar_type

        self.ema1 = ExponentialMovingAverage(10)
        self.ema2 = ExponentialMovingAverage(20)

        self.position_id = None

        self.calls = []

    def on_start(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.register_indicator_for_bars(self.bar_type, self.ema1)
        self.register_indicator_for_bars(self.bar_type, self.ema2)

    def on_instrument(self, instrument) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(instrument)

    def on_quote_tick(self, tick):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(tick)

    def on_trade_tick(self, tick) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(tick)

    def on_bar(self, bar) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(bar)

        if bar.type != self.bar_type:
            return

        if self.ema1.value > self.ema2.value:
            buy_order = self.order_factory.market(
                self.bar_type.instrument_id,
                OrderSide.BUY,
                100000,
            )

            self.submit_order(buy_order)
            self.position_id = buy_order.cl_ord_id
        elif self.ema1.value < self.ema2.value:
            sell_order = self.order_factory.market(
                self.bar_type.instrument_id,
                OrderSide.SELL,
                100000,
            )

            self.submit_order(sell_order)
            self.position_id = sell_order.cl_ord_id

    def on_data(self, data) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(data)

    def on_event(self, event) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(event)

    def on_stop(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_resume(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_reset(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_save(self) -> dict:
        self.calls.append(inspect.currentframe().f_code.co_name)
        return {"UserState": 1}

    def on_load(self, state) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(state)

    def on_dispose(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)


class KaboomStrategy(TradingStrategy):
    """
    Provides a mock trading strategy where every called method blows up.
    """

    def __init__(self):
        """
        Initialize a new instance of the `KaboomStrategy` class.
        """
        super().__init__(order_id_tag="000")

        self._explode_on_start = True
        self._explode_on_stop = True

    def set_explode_on_start(self, setting) -> None:
        self._explode_on_start = setting

    def set_explode_on_stop(self, setting) -> None:
        self._explode_on_stop = setting

    def on_start(self) -> None:
        if self._explode_on_start:
            raise RuntimeError(f"{self} BOOM!")

    def on_stop(self) -> None:
        if self._explode_on_stop:
            raise RuntimeError(f"{self} BOOM!")

    def on_resume(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_reset(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_save(self) -> dict:
        raise RuntimeError(f"{self} BOOM!")

    def on_load(self, state) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_dispose(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_instrument(self, instrument) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_quote_tick(self, tick) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_trade_tick(self, tick) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_bar(self, bar) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_data(self, data) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_event(self, event) -> None:
        raise RuntimeError(f"{self} BOOM!")


class MockMarketDataClient(MarketDataClient):
    """
    Provides a mock data client for testing.

    The client will append all method calls to the calls list.
    """

    def __init__(
        self,
        name: str,
        engine: DataEngine,
        clock: Clock,
        logger: Logger,
    ):
        """
        Initialize a new instance of the `DataClient` class.

        Parameters
        ----------
        name : str
            The venue the client can provide data for.
        engine : DataEngine
            The data engine to connect to the client.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.

        """
        super().__init__(
            name,
            engine,
            clock,
            logger,
        )

        self.calls = []

    # -- COMMANDS ----------------------------------------------------------------------------------

    def connect(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def disconnect(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def reset(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def dispose(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    # -- SUBSCRIPTIONS -----------------------------------------------------------------------------

    def subscribe(self, data_type: DataType) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def subscribe_order_book(self, instrument_id, level, depth=0, kwargs=None) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def subscribe_bars(self, bar_type: BarType) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def unsubscribe(self, data_type: DataType) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def unsubscribe_bars(self, bar_type: BarType) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def unsubscribe_instrument(self, instrument_id: InstrumentId) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def unsubscribe_order_book(self, instrument_id: InstrumentId) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    # -- REQUESTS ----------------------------------------------------------------------------------

    def request(self, datatype: DataType, correlation_id: UUID) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def request_instrument(
        self, instrument_id: InstrumentId, correlation_id: UUID
    ) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def request_instruments(self, correlation_id: UUID) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        from_datetime: datetime,
        to_datetime: datetime,
        limit: int,
        correlation_id: UUID,
    ) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        from_datetime: datetime,
        to_datetime: datetime,
        limit: int,
        correlation_id: UUID,
    ) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def request_bars(
        self,
        bar_type: BarType,
        from_datetime: datetime,
        to_datetime: datetime,
        limit: int,
        correlation_id: UUID,
    ) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)


class MockExecutionClient(ExecutionClient):
    """
    Provides a mock execution client for testing.

    The client will append all method calls to the calls list.
    """

    def __init__(
        self,
        name,
        account_id,
        engine,
        clock,
        logger,
    ):
        """
        Initialize a new instance of the `MockExecutionClient` class.

        Parameters
        ----------
        name : str
            The venue for the client.
        account_id : AccountId
            The account_id for the client.
        engine : ExecutionEngine
            The execution engine for the component.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.

        """
        super().__init__(
            name,
            account_id,
            engine,
            clock,
            logger,
        )

        self.calls = []
        self.commands = []

    def connect(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self._set_connected()

    def disconnect(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self._set_connected(False)

    def dispose(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def reset(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    # -- COMMANDS ----------------------------------------------------------------------------------

    def account_inquiry(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_bracket_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def update_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def cancel_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)


class MockLiveExecutionClient(LiveExecutionClient):
    """
    Provides a mock execution client for testing.

    The client will append all method calls to the calls list.
    """

    def __init__(
        self,
        name,
        account_id,
        engine,
        instrument_provider,
        clock,
        logger,
    ):
        """
        Initialize a new instance of the `MockExecutionClient` class.

        Parameters
        ----------
        name : str
            The venue for the client.
        account_id : AccountId
            The account_id for the client.
        engine : ExecutionEngine
            The execution engine for the component.
        instrument_provider : InstrumentProvider
            The instrument provider for the client.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.

        """
        super().__init__(
            name,
            account_id,
            engine,
            instrument_provider,
            clock,
            logger,
        )

        self._order_status_reports = {}  # type: dict[OrderId, OrderStatusReport]
        self._trades_lists = {}  # type: dict[OrderId, list[ExecutionReport]]

        self.calls = []
        self.commands = []

    def add_order_status_report(self, report: OrderStatusReport) -> None:
        self._order_status_reports[report.order_id] = report

    def add_trades_list(self, order_id: OrderId, trades: List[ExecutionReport]) -> None:
        self._trades_lists[order_id] = trades

    def connect(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self._set_connected()

    def disconnect(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self._set_connected(False)

    def dispose(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def reset(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    # -- COMMANDS ----------------------------------------------------------------------------------

    def account_inquiry(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_bracket_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def update_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def cancel_order(self, command) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    async def generate_order_status_report(
        self, order: Order
    ) -> Optional[OrderStatusReport]:
        self.calls.append(inspect.currentframe().f_code.co_name)
        return self._order_status_reports[order.id]

    async def generate_exec_reports(
        self,
        order_id: OrderId,
        symbol: Symbol,
        since: datetime = None,
    ) -> List[ExecutionReport]:
        print(self._trades_lists)
        self.calls.append(inspect.currentframe().f_code.co_name)
        return self._trades_lists[order_id]


class MockExecutionDatabase(ExecutionDatabase):
    """
    Provides a mock execution database for testing.

    """

    def __init__(self, trader_id: TraderId, logger: Logger):
        """
        Initialize a new instance of the `BypassExecutionDatabase` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier to associate with the database.
        logger : Logger
            The logger for the database.

        """
        super().__init__(trader_id, logger)

        self.accounts = {}
        self.orders = {}
        self.positions = {}

    def flush(self) -> None:
        self.accounts = {}
        self.orders = {}
        self.positions = {}

    def load_accounts(self) -> dict:
        return self.accounts.copy()

    def load_orders(self) -> dict:
        return self.orders.copy()

    def load_positions(self) -> dict:
        return self.positions.copy()

    def load_account(self, account_id: AccountId) -> Account:
        return self.accounts.get(account_id)

    def load_order(self, cl_ord_id: ClientOrderId) -> Order:
        return self.orders.get(cl_ord_id)

    def load_position(self, position_id: PositionId) -> Position:
        return self.positions.get(position_id)

    def load_strategy(self, strategy_id: StrategyId) -> dict:
        return {}

    def delete_strategy(self, strategy_id: StrategyId) -> None:
        pass

    def add_account(self, account: Account) -> None:
        self.accounts[account.id] = account

    def add_order(self, order: Order) -> None:
        self.orders[order.cl_ord_id] = order

    def add_position(self, position: Position) -> None:
        self.positions[position.id] = position

    def update_account(self, event: Account) -> None:
        pass  # Would persist the event

    def update_order(self, order: Order) -> None:
        pass  # Would persist the event

    def update_position(self, position: Position) -> None:
        pass  # Would persist the event

    def update_strategy(self, strategy: TradingStrategy) -> None:
        pass  # Would persist the user state dict
