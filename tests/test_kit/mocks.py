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
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.core.uuid import UUID
from nautilus_trader.data.client import DataClient
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.client import ExecutionClient
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue


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
            uuid_factory: UUIDFactory,
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
        uuid_factory : UUIDFactory
            The UUID factory for the component.
        logger : Logger
            The logger for the component.

        """
        super().__init__(
            venue,
            engine,
            clock,
            uuid_factory,
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
            uuid_factory,
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
        uuid_factory : UUIDFactory
            The UUID factory for the component.
        logger : Logger
            The logger for the component.

        """
        super().__init__(
            venue,
            account_id,
            exec_engine,
            clock,
            uuid_factory,
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
