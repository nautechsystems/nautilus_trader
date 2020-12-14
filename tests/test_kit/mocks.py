# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.clock import Clock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.uuid import UUID
from nautilus_trader.data.client import DataClient
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.client import ExecutionClient
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
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

    def store(self, obj):
        """Store the given object.

        Parameters
        ----------
        obj : object
            The object to store.

        """
        self.count += 1
        self._store.append(obj)

    def store_2(self, obj1, obj2):
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

    def on_start(self):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.register_indicator_for_bars(self.bar_type, self.ema1)
        self.register_indicator_for_bars(self.bar_type, self.ema2)

    def on_quote_tick(self, tick):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_bar(self, bar_type, bar):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store((bar_type, Bar))

        if bar_type != self.bar_type:
            return

        if self.ema1.value > self.ema2.value:
            buy_order = self.order_factory.market(
                self.bar_type.symbol,
                OrderSide.BUY,
                100000,
            )

            self.submit_order(buy_order)
            self.position_id = buy_order.cl_ord_id
        elif self.ema1.value < self.ema2.value:
            sell_order = self.order_factory.market(
                self.bar_type.symbol,
                OrderSide.SELL,
                100000,
            )

            self.submit_order(sell_order)
            self.position_id = sell_order.cl_ord_id

    def on_instrument(self, instrument):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(instrument)

    def on_event(self, event):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(event)

    def on_stop(self):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_resume(self):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_reset(self):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_save(self):
        self.calls.append(inspect.currentframe().f_code.co_name)
        return {}

    def on_load(self, state):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_dispose(self):
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

    def set_explode_on_start(self, setting):
        self._explode_on_start = setting

    def set_explode_on_stop(self, setting):
        self._explode_on_stop = setting

    def on_start(self):
        if self._explode_on_start:
            raise RuntimeError(f"{self} BOOM!")

    def on_stop(self):
        if self._explode_on_stop:
            raise RuntimeError(f"{self} BOOM!")

    def on_resume(self):
        raise RuntimeError(f"{self} BOOM!")

    def on_reset(self):
        raise RuntimeError(f"{self} BOOM!")

    def on_save(self):
        raise RuntimeError(f"{self} BOOM!")

    def on_load(self, state):
        raise RuntimeError(f"{self} BOOM!")

    def on_dispose(self):
        raise RuntimeError(f"{self} BOOM!")

    def on_quote_tick(self, tick):
        raise RuntimeError(f"{self} BOOM!")

    def on_trade_tick(self, tick):
        raise RuntimeError(f"{self} BOOM!")

    def on_bar(self, bar_type, bar):
        raise RuntimeError(f"{self} BOOM!")

    def on_data(self, data):
        raise RuntimeError(f"{self} BOOM!")

    def on_event(self, event):
        raise RuntimeError(f"{self} BOOM!")


class MockDataClient(DataClient):
    """
    Provides a mock data client for testing.

    The client will append all method calls to the calls list.
    The client will append all received commands to the commands list.
    """

    def __init__(
        self,
        venue: Venue,
        engine: DataEngine,
        clock: Clock,
        logger: Logger,
    ):
        """
        Initialize a new instance of the `DataClient` class.

        Parameters
        ----------
        venue : Venue
            The venue the client can provide data for.
        engine : DataEngine
            The data engine to connect to the client.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.

        """
        super().__init__(
            venue,
            engine,
            clock,
            logger,
        )

        self.calls = []

    def is_connected(self):
        self.calls.append(inspect.currentframe().f_code.co_name)

# -- COMMANDS --------------------------------------------------------------------------------------

    def connect(self):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def disconnect(self):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def reset(self):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def dispose(self):
        self.calls.append(inspect.currentframe().f_code.co_name)

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    def subscribe_quote_ticks(self, symbol: Symbol):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def subscribe_trade_ticks(self, symbol: Symbol):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def subscribe_bars(self, bar_type: BarType):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def subscribe_instrument(self, symbol: Symbol):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def unsubscribe_quote_ticks(self, symbol: Symbol):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def unsubscribe_trade_ticks(self, symbol: Symbol):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def unsubscribe_bars(self, bar_type: BarType):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def unsubscribe_instrument(self, symbol: Symbol):
        self.calls.append(inspect.currentframe().f_code.co_name)

# -- REQUESTS --------------------------------------------------------------------------------------
    def request_instrument(self, symbol: Symbol, correlation_id: UUID):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def request_instruments(self, correlation_id: UUID):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def request_quote_ticks(
        self,
        symbol: Symbol,
        from_datetime: datetime,
        to_datetime: datetime,
        limit: int,
        correlation_id: UUID,
    ):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def request_trade_ticks(
            self,
            symbol: Symbol,
            from_datetime: datetime,
            to_datetime: datetime,
            limit: int,
            correlation_id: UUID,
    ):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def request_bars(
            self,
            bar_type: BarType,
            from_datetime: datetime,
            to_datetime: datetime,
            limit: int,
            correlation_id: UUID,
    ):
        self.calls.append(inspect.currentframe().f_code.co_name)


class MockExecutionClient(ExecutionClient):
    """
    Provides a mock execution client for testing.

    The client will append all method calls to the calls list.
    The client will append all received commands to the commands list.
    """

    def __init__(
        self,
        venue,
        account_id,
        exec_engine,
        clock,
        logger,
    ):
        """
        Initialize a new instance of the `MockExecutionClient` class.

        Parameters
        ----------
        venue : Venue
            The venue for the client.
        account_id : AccountId
            The account_id for the client.
        exec_engine : ExecutionEngine
            The execution engine for the component.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.

        """
        super().__init__(
            venue,
            account_id,
            exec_engine,
            clock,
            logger,
        )

        self._is_connected = False
        self.calls = []
        self.commands = []

    def connect(self):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self._is_connected = True

    def disconnect(self):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self._is_connected = False

    def dispose(self):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def reset(self):
        self.calls.append(inspect.currentframe().f_code.co_name)

    def is_connected(self):
        return self._is_connected

# -- COMMANDS --------------------------------------------------------------------------------------

    def account_inquiry(self, command):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_order(self, command):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def submit_bracket_order(self, command):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def modify_order(self, command):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)

    def cancel_order(self, command):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.commands.append(command)
