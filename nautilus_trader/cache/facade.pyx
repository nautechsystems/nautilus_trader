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

from nautilus_trader.cache.config import CacheConfig

from cpython.datetime cimport datetime
from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.common.actor cimport Actor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport OrderListId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.synthetic cimport SyntheticInstrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.trading.strategy cimport Strategy


cdef class CacheDatabaseFacade:
    """
    The base class for all cache databases.

    Parameters
    ----------
    config : CacheConfig, optional
        The configuration for the database.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, config: CacheConfig | None = None) -> None:
        self._log = Logger(name=type(self).__name__)

        self._log.info("READY")

    cpdef void close(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `close` must be implemented in the subclass")  # pragma: no cover

    cpdef void flush(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `flush` must be implemented in the subclass")  # pragma: no cover

    cpdef list[str] keys(self, str pattern = "*"):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `keys` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load_all(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load_currencies(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_currencies` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load_instruments(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_instruments` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load_synthetics(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_synthetics` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load_accounts(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_accounts` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load_orders(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_orders` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load_positions(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_positions` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load_index_order_position(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_index_order_position` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load_index_order_client(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_index_order_client` must be implemented in the subclass")  # pragma: no cover

    cpdef Currency load_currency(self, str code):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_currency` must be implemented in the subclass")  # pragma: no cover

    cpdef Instrument load_instrument(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_instrument` must be implemented in the subclass")  # pragma: no cover

    cpdef SyntheticInstrument load_synthetic(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_synthetic` must be implemented in the subclass")  # pragma: no cover

    cpdef Account load_account(self, AccountId account_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_account` must be implemented in the subclass")  # pragma: no cover

    cpdef Order load_order(self, ClientOrderId client_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_order` must be implemented in the subclass")  # pragma: no cover

    cpdef Position load_position(self, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_position` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load_actor(self, ComponentId component_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_actor` must be implemented in the subclass")  # pragma: no cover

    cpdef void delete_actor(self, ComponentId component_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `delete_actor` must be implemented in the subclass")  # pragma: no cover

    cpdef dict load_strategy(self, StrategyId strategy_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `load_strategy` must be implemented in the subclass")  # pragma: no cover

    cpdef void delete_strategy(self, StrategyId strategy_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `delete_strategy` must be implemented in the subclass")  # pragma: no cover

    cpdef void add(self, str key, bytes value):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `add` must be implemented in the subclass")  # pragma: no cover

    cpdef void add_currency(self, Currency currency):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `add_currency` must be implemented in the subclass")  # pragma: no cover

    cpdef void add_instrument(self, Instrument instrument):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `add_instrument` must be implemented in the subclass")  # pragma: no cover

    cpdef void add_synthetic(self, SyntheticInstrument synthetic):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `add_synthetic` must be implemented in the subclass")  # pragma: no cover

    cpdef void add_account(self, Account account):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `add_account` must be implemented in the subclass")  # pragma: no cover

    cpdef void add_order(self, Order order, PositionId position_id = None, ClientId client_id = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `add_order` must be implemented in the subclass")  # pragma: no cover

    cpdef void add_position(self, Position position):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `add_position` must be implemented in the subclass")  # pragma: no cover

    cpdef void index_venue_order_id(self, ClientOrderId client_order_id, VenueOrderId venue_order_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `index_venue_order_id` must be implemented in the subclass")  # pragma: no cover

    cpdef void index_order_position(self, ClientOrderId client_order_id, PositionId position_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `index_order_position` must be implemented in the subclass")  # pragma: no cover

    cpdef void update_account(self, Account event):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `update_account` must be implemented in the subclass")  # pragma: no cover

    cpdef void update_order(self, Order order):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `update_order` must be implemented in the subclass")  # pragma: no cover

    cpdef void update_position(self, Position position):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `update_position` must be implemented in the subclass")  # pragma: no cover

    cpdef void update_actor(self, Actor actor):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `update_actor` must be implemented in the subclass")  # pragma: no cover

    cpdef void update_strategy(self, Strategy strategy):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `update_strategy` must be implemented in the subclass")  # pragma: no cover

    cpdef void snapshot_order_state(self, Order order):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `snapshot_order_state` must be implemented in the subclass")  # pragma: no cover

    cpdef void snapshot_position_state(self, Position position, uint64_t ts_snapshot, Money unrealized_pnl = None):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `snapshot_position_state` must be implemented in the subclass")  # pragma: no cover

    cpdef void heartbeat(self, datetime timestamp):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method `heartbeat` must be implemented in the subclass")  # pragma: no cover
