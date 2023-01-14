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

from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.backtest.execution_client cimport BacktestExecClient
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.models cimport LatencyModel
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.queue cimport Queue
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.data.venue cimport InstrumentStatusUpdate
from nautilus_trader.model.data.venue cimport VenueStatusUpdate
from nautilus_trader.model.enums_c cimport AccountType
from nautilus_trader.model.enums_c cimport BookType
from nautilus_trader.model.enums_c cimport OmsType
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orderbook.data cimport OrderBookData
from nautilus_trader.msgbus.bus cimport MessageBus


cdef class SimulatedExchange:
    cdef Clock _clock
    cdef LoggerAdapter _log

    cdef readonly Venue id
    """The exchange ID.\n\n:returns: `Venue`"""
    cdef readonly OmsType oms_type
    """The exchange order management system type.\n\n:returns: `OmsType`"""
    cdef readonly BookType book_type
    """The exchange default order book type.\n\n:returns: `BookType`"""
    cdef readonly MessageBus msgbus
    """The message bus wired to the exchange.\n\n:returns: `MessageBus`"""
    cdef readonly Cache cache
    """The cache wired to the exchange.\n\n:returns: `CacheFacade`"""
    cdef readonly BacktestExecClient exec_client
    """The execution client wired to the exchange.\n\n:returns: `BacktestExecClient`"""

    cdef readonly AccountType account_type
    """The account base currency.\n\n:returns: `AccountType`"""
    cdef readonly Currency base_currency
    """The account base currency (None for multi-currency accounts).\n\n:returns: `Currency` or ``None``"""
    cdef readonly list starting_balances
    """The account starting balances for each backtest run.\n\n:returns: `bool`"""
    cdef readonly default_leverage
    """The accounts default leverage.\n\n:returns: `Decimal`"""
    cdef readonly dict leverages
    """The accounts instrument specific leverage configuration.\n\n:returns: `dict[InstrumentId, Decimal]`"""
    cdef readonly bint is_frozen_account
    """If the account for the exchange is frozen.\n\n:returns: `bool`"""
    cdef readonly LatencyModel latency_model
    """The latency model for the exchange.\n\n:returns: `LatencyModel`"""
    cdef readonly FillModel fill_model
    """The fill model for the exchange.\n\n:returns: `FillModel`"""
    cdef readonly bint reject_stop_orders
    """If stop orders are rejected on submission if in the market.\n\n:returns: `bool`"""
    cdef readonly bint support_gtd_orders
    """If orders with GTD time in force will be supported by the venue.\n\n:returns: `bool`"""
    cdef readonly list modules
    """The simulation modules registered with the exchange.\n\n:returns: `list[SimulationModule]`"""
    cdef readonly dict instruments
    """The exchange instruments.\n\n:returns: `dict[InstrumentId, Instrument]`"""

    cdef dict _matching_engines
    cdef Queue _message_queue
    cdef list _inflight_queue
    cdef dict _inflight_counter

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register_client(self, BacktestExecClient client) except *
    cpdef void set_fill_model(self, FillModel fill_model) except *
    cpdef void set_latency_model(self, LatencyModel latency_model) except *
    cpdef void initialize_account(self) except *
    cpdef void add_instrument(self, Instrument instrument) except *

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Price best_bid_price(self, InstrumentId instrument_id)
    cpdef Price best_ask_price(self, InstrumentId instrument_id)
    cpdef OrderBook get_book(self, InstrumentId instrument_id)
    cpdef dict get_matching_engines(self)
    cpdef dict get_books(self)
    cpdef list get_open_orders(self, InstrumentId instrument_id=*)
    cpdef list get_open_bid_orders(self, InstrumentId instrument_id=*)
    cpdef list get_open_ask_orders(self, InstrumentId instrument_id=*)
    cpdef Account get_account(self)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void adjust_account(self, Money adjustment) except *
    cdef tuple generate_inflight_command(self, TradingCommand command)
    cpdef void send(self, TradingCommand command) except *
    cpdef void process_order_book(self, OrderBookData data) except *
    cpdef void process_quote_tick(self, QuoteTick tick) except *
    cpdef void process_trade_tick(self, TradeTick tick) except *
    cpdef void process_bar(self, Bar bar) except *
    cpdef void process_venue_status(self, VenueStatusUpdate update) except *
    cpdef void process_instrument_status(self, InstrumentStatusUpdate update) except *
    cpdef void process(self, uint64_t now_ns) except *
    cpdef void reset(self) except *

# -- EVENT GENERATORS -----------------------------------------------------------------------------

    cdef void _generate_fresh_account_state(self) except *
