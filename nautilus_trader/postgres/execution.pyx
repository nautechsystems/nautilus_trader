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

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.account cimport Account
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef str _UTF8 = 'utf-8'
cdef str _ACCOUNTS = 'Accounts'
cdef str _TRADER = 'Trader'
cdef str _ORDERS = 'Orders'
cdef str _POSITIONS = 'Positions'
cdef str _STRATEGIES = 'Strategies'


cdef class PostgresExecutionDatabase(ExecutionDatabase):
    """
    Provides an execution database backed by Postgres.

    """

    def __init__(
        self,
        TraderId trader_id not None,
        Logger logger not None,
        CommandSerializer command_serializer not None,
        EventSerializer event_serializer not None,
        dict config,
    ):
        """
        Initialize a new instance of the `PostgresExecutionDatabase` class.

        Parameters
        ----------
        trader_id : TraderId
            The trader identifier to associate with the database.
        logger : Logger
            The logger for the database.
        command_serializer : CommandSerializer
            The command serializer for cache transactions.
        event_serializer : EventSerializer
            The event serializer for cache transactions.

        """
        cdef str host = config["host"]
        cdef int port = int(config["port"])
        Condition.valid_string(host, "host")
        Condition.in_range_int(port, 0, 65535, "port")
        super().__init__(trader_id, logger)

        # Database keys
        self._key_trader     = f"{_TRADER}-{trader_id.value}"       # noqa
        self._key_accounts   = f"{self._key_trader}:{_ACCOUNTS}:"    # noqa
        self._key_orders     = f"{self._key_trader}:{_ORDERS}:"      # noqa
        self._key_positions  = f"{self._key_trader}:{_POSITIONS}:"   # noqa
        self._key_strategies = f"{self._key_trader}:{_STRATEGIES}:"  # noqa

        # Serializers
        self._command_serializer = command_serializer
        self._event_serializer = event_serializer

        # Postgres client
        self._postgres = None

    cpdef void flush(self) except *:
        raise NotImplemented()

    cpdef dict load_accounts(self):
        raise NotImplemented()

    cpdef dict load_orders(self):
        raise NotImplemented()

    cpdef dict load_positions(self):
        raise NotImplemented()

    cpdef Account load_account(self, AccountId account_id):
        raise NotImplemented()

    cpdef Order load_order(self, ClientOrderId cl_ord_id):
        raise NotImplemented()

    cpdef Position load_position(self, PositionId position_id):
        raise NotImplemented()

    cpdef dict load_strategy(self, StrategyId strategy_id):
        raise NotImplemented()

    cpdef void delete_strategy(self, StrategyId strategy_id) except *:
        raise NotImplemented()

    cpdef void add_account(self, Account account) except *:
        raise NotImplemented()

    cpdef void add_order(self, Order order) except *:
        raise NotImplemented()

    cpdef void add_position(self, Position position) except *:
        raise NotImplemented()

    cpdef void update_account(self, Account event) except *:
        raise NotImplemented()

    cpdef void update_order(self, Order order) except *:
        raise NotImplemented()

    cpdef void update_position(self, Position position) except *:
        raise NotImplemented()

    cpdef void update_strategy(self, TradingStrategy strategy) except *:
        raise NotImplemented()
