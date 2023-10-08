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

from collections import deque
from decimal import Decimal
from heapq import heappush
from typing import Optional

from nautilus_trader.config.error import InvalidConfiguration

from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.backtest.execution_client cimport BacktestExecClient
from nautilus_trader.backtest.matching_engine cimport OrderMatchingEngine
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.models cimport LatencyModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.clock cimport TestClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.data.status cimport InstrumentStatus
from nautilus_trader.model.data.status cimport VenueStatus
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick
from nautilus_trader.model.enums_c cimport AccountType
from nautilus_trader.model.enums_c cimport BookType
from nautilus_trader.model.enums_c cimport OmsType
from nautilus_trader.model.enums_c cimport account_type_to_str
from nautilus_trader.model.enums_c cimport oms_type_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.crypto_future cimport CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual cimport CryptoPerpetual
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orderbook.book cimport OrderBook
from nautilus_trader.model.orders.base cimport Order


cdef class SimulatedExchange:
    """
    Provides a simulated financial market exchange.

    Parameters
    ----------
    venue : Venue
        The venue to simulate.
    oms_type : OmsType {``HEDGING``, ``NETTING``}
        The order management system type used by the exchange.
    account_type : AccountType
        The account type for the client.
    starting_balances : list[Money]
        The starting balances for the exchange.
    base_currency : Currency, optional
        The account base currency for the client. Use ``None`` for multi-currency accounts.
    default_leverage : Decimal
        The account default leverage (for margin accounts).
    leverages : dict[InstrumentId, Decimal]
        The instrument specific leverage configuration (for margin accounts).
    msgbus : MessageBus
        The message bus for the exchange.
    cache : CacheFacade
        The read-only cache for the exchange.
    fill_model : FillModel
        The fill model for the exchange.
    latency_model : LatencyModel, optional
        The latency model for the exchange.
    clock : TestClock
        The clock for the exchange.
    logger : Logger
        The logger for the exchange.
    book_type : BookType
        The order book type for the exchange.
    frozen_account : bool, default False
        If the account for this exchange is frozen (balances will not change).
    bar_execution : bool, default True
        If bars should be processed by the matching engine(s) (and move the market).
    reject_stop_orders : bool, default True
        If stop orders are rejected on submission if in the market.
    support_gtd_orders : bool, default True
        If orders with GTD time in force will be supported by the venue.
    use_position_ids : bool, default True
        If venue position IDs will be generated on order fills.
    use_random_ids : bool, default False
        If all venue generated identifiers will be random UUID4's.
    use_reduce_only : bool, default True
        If the `reduce_only` execution instruction on orders will be honored.

    Raises
    ------
    ValueError
        If `instruments` is empty.
    ValueError
        If `instruments` contains a type other than `Instrument`.
    ValueError
        If `starting_balances` is empty.
    ValueError
        If `starting_balances` contains a type other than `Money`.
    ValueError
        If `base_currency` and multiple starting balances.
    ValueError
        If `modules` contains a type other than `SimulationModule`.
    """

    def __init__(
        self,
        Venue venue not None,
        OmsType oms_type,
        AccountType account_type,
        list starting_balances not None,
        Currency base_currency: Optional[Currency],
        default_leverage not None: Decimal,
        leverages not None: dict[InstrumentId, Decimal],
        list instruments not None,
        list modules not None,
        MessageBus msgbus not None,
        CacheFacade cache not None,
        TestClock clock not None,
        Logger logger not None,
        FillModel fill_model not None,
        LatencyModel latency_model = None,
        BookType book_type = BookType.L1_MBP,
        bint frozen_account = False,
        bint bar_execution = True,
        bint reject_stop_orders = True,
        bint support_gtd_orders = True,
        bint use_position_ids = True,
        bint use_random_ids = False,
        bint use_reduce_only = True,
    ):
        Condition.list_type(instruments, Instrument, "instruments", "Instrument")
        Condition.not_empty(starting_balances, "starting_balances")
        Condition.list_type(starting_balances, Money, "starting_balances")
        Condition.list_type(modules, SimulationModule, "modules", "SimulationModule")
        if base_currency:
            Condition.true(len(starting_balances) == 1, "single-currency account has multiple starting currencies")
        if default_leverage and default_leverage > 1 or leverages:
            Condition.true(account_type == AccountType.MARGIN, "leverages defined when account type is not `MARGIN`")

        self._clock = clock
        self._log = LoggerAdapter(
            component_name=f"{type(self).__name__}({venue})",
            logger=logger,
        )

        self.id = venue
        self.oms_type = oms_type
        self._log.info(f"OmsType={oms_type_to_str(oms_type)}")
        self.book_type = book_type

        self.msgbus = msgbus
        self.cache = cache
        self.exec_client = None  # Initialized when execution client registered

        # Accounting
        self.account_type = account_type
        self.base_currency = base_currency
        self.starting_balances = starting_balances
        self.default_leverage = default_leverage
        self.leverages = leverages
        self.is_frozen_account = frozen_account

        # Execution
        self.bar_execution = bar_execution
        self.reject_stop_orders = reject_stop_orders
        self.support_gtd_orders = support_gtd_orders
        self.use_position_ids = use_position_ids
        self.use_random_ids = use_random_ids
        self.use_reduce_only = use_reduce_only
        self.fill_model = fill_model
        self.latency_model = latency_model

        # Load modules
        self.modules = []
        for module in modules:
            Condition.not_in(module, self.modules, "module", "modules")
            module.register_venue(self)
            module.register_base(
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                logger=logger,
            )
            self.modules.append(module)
            self._log.info(f"Loaded {module}.")

        # Markets
        self._matching_engines: dict[InstrumentId, OrderMatchingEngine] = {}

        # Load instruments
        self.instruments: dict[InstrumentId, Instrument] = {}
        for instrument in instruments:
            self.add_instrument(instrument)

        self._message_queue = deque()
        self._inflight_queue: list[tuple[(uint64_t, uint64_t), TradingCommand]] = []
        self._inflight_counter: dict[uint64_t, int] = {}

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"id={self.id}, "
            f"oms_type={oms_type_to_str(self.oms_type)}, "
            f"account_type={account_type_to_str(self.account_type)})"
        )

# -- REGISTRATION ---------------------------------------------------------------------------------

    cpdef void register_client(self, BacktestExecClient client):
        """
        Register the given execution client with the simulated exchange.

        Parameters
        ----------
        client : BacktestExecClient
            The client to register

        """
        Condition.not_none(client, "client")

        self.exec_client = client

        self._log.info(f"Registered ExecutionClient-{client}.")

    cpdef void set_fill_model(self, FillModel fill_model):
        """
        Set the fill model for all matching engines.

        Parameters
        ----------
        fill_model : FillModel
            The fill model to set.

        """
        Condition.not_none(fill_model, "fill_model")

        self.fill_model = fill_model

        cdef OrderMatchingEngine matching_engine
        for matching_engine in self._matching_engines.values():
            matching_engine.set_fill_model(fill_model)
            self._log.info(
                f"Changed `FillModel` for {matching_engine.venue} "
                f"to {self.fill_model}.",
            )

    cpdef void set_latency_model(self, LatencyModel latency_model):
        """
        Change the latency model for this exchange.

        Parameters
        ----------
        latency_model : LatencyModel
            The latency model to set.

        """
        Condition.not_none(latency_model, "latency_model")

        self.latency_model = latency_model

        self._log.info("Changed latency model.")

    cpdef void initialize_account(self):
        """
        Initialize the account to the starting balances.
        """
        self._generate_fresh_account_state()

    cpdef void add_instrument(self, Instrument instrument):
        """
        Add the given instrument to the venue.

        A random and unique 32-bit unsigned integer raw ID will be generated.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        Raises
        ------
        ValueError
            If `instrument.id.venue` is not equal to the venue ID.
        InvalidConfiguration
            If `instrument` is invalid for this venue.

        """
        Condition.not_none(instrument, "instrument")
        Condition.equal(instrument.id.venue, self.id, "instrument.id.venue", "self.id")

        # Validate instrument
        if isinstance(instrument, (CryptoPerpetual, CryptoFuture)):
            if self.account_type == AccountType.CASH:
                raise InvalidConfiguration(
                    f"Cannot add a `{type(instrument).__name__}` type instrument "
                    f"to a venue with a `CASH` account type. Add to a "
                    f"venue with a `MARGIN` account type.",
                )

        self.instruments[instrument.id] = instrument

        cdef OrderMatchingEngine matching_engine = OrderMatchingEngine(
            instrument=instrument,
            raw_id=len(self.instruments),
            fill_model=self.fill_model,
            book_type=self.book_type,
            oms_type=self.oms_type,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self._clock,
            logger=self._log.get_logger(),
            bar_execution=self.bar_execution,
            reject_stop_orders=self.reject_stop_orders,
            support_gtd_orders=self.support_gtd_orders,
            use_position_ids=self.use_position_ids,
            use_random_ids=self.use_random_ids,
            use_reduce_only=self.use_reduce_only,
        )

        self._matching_engines[instrument.id] = matching_engine

        self._log.info(f"Added instrument {instrument.id} and created matching engine.")

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Price best_bid_price(self, InstrumentId instrument_id):
        """
        Return the best bid price for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.

        Returns
        -------
        Price or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(instrument_id)
        if matching_engine is None:
            return None

        return matching_engine.best_bid_price()

    cpdef Price best_ask_price(self, InstrumentId instrument_id):
        """
        Return the best ask price for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.

        Returns
        -------
        Price or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(instrument_id)
        if matching_engine is None:
            return None

        return matching_engine.best_ask_price()

    cpdef OrderBook get_book(self, InstrumentId instrument_id):
        """
        Return the order book for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the price.

        Returns
        -------
        OrderBook or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(instrument_id)
        if matching_engine is None:
            return None

        return matching_engine.get_book()

    cpdef OrderMatchingEngine get_matching_engine(self, InstrumentId instrument_id):
        """
        Return the matching engine for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the matching engine.

        Returns
        -------
        OrderMatchingEngine or ``None``

        """
        return self._matching_engines.get(instrument_id)

    cpdef dict get_matching_engines(self):
        """
        Return all matching engines for the exchange (for every instrument).

        Returns
        -------
        dict[InstrumentId, OrderMatchingEngine]

        """
        return self._matching_engines.copy()

    cpdef dict get_books(self):
        """
        Return all order books within the exchange.

        Returns
        -------
        dict[InstrumentId, OrderBook]

        """
        cdef dict books = {}

        cdef OrderMatchingEngine matching_engine
        for matching_engine in self._matching_engines.values():
            books[matching_engine.instrument.id] = matching_engine.get_book()

        return books

    cpdef list get_open_orders(self, InstrumentId instrument_id = None):
        """
        Return the open orders at the exchange.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument_id query filter.

        Returns
        -------
        list[Order]

        """
        cdef OrderMatchingEngine matching_engine
        if instrument_id is not None:
            matching_engine = self._matching_engines.get(instrument_id)
            if matching_engine is None:
                return []
            else:
                return matching_engine.get_open_orders()

        cdef list open_orders = []
        for matching_engine in self._matching_engines.values():
            open_orders += matching_engine.get_open_orders()

        return open_orders

    cpdef list get_open_bid_orders(self, InstrumentId instrument_id = None):
        """
        Return the open bid orders at the exchange.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument_id query filter.

        Returns
        -------
        list[Order]

        """
        cdef OrderMatchingEngine matching_engine
        if instrument_id is not None:
            matching_engine = self._matching_engines.get(instrument_id)
            if matching_engine is None:
                return []
            else:
                return matching_engine.get_open_bid_orders()

        cdef list open_bid_orders = []
        for matching_engine in self._matching_engines.values():
            open_bid_orders += matching_engine.get_open_bid_orders()

        return open_bid_orders

    cpdef list get_open_ask_orders(self, InstrumentId instrument_id = None):
        """
        Return the open ask orders at the exchange.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument_id query filter.

        Returns
        -------
        list[Order]

        """
        cdef OrderMatchingEngine matching_engine
        if instrument_id is not None:
            matching_engine = self._matching_engines.get(instrument_id)
            if matching_engine is None:
                return []
            else:
                return matching_engine.get_open_ask_orders()

        cdef list open_ask_orders = []
        for matching_engine in self._matching_engines.values():
            open_ask_orders += matching_engine.get_open_ask_orders()

        return open_ask_orders

    cpdef Account get_account(self):
        """
        Return the account for the registered client (if registered).

        Returns
        -------
        Account or ``None``

        """
        Condition.not_none(self.exec_client, "self.exec_client")

        return self.exec_client.get_account()

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void adjust_account(self, Money adjustment):
        """
        Adjust the account at the exchange with the given adjustment.

        Parameters
        ----------
        adjustment : Money
            The adjustment for the account.

        """
        Condition.not_none(adjustment, "adjustment")

        if self.is_frozen_account:
            return  # Nothing to adjust

        cdef Account account = self.cache.account_for_venue(self.exec_client.venue)
        if account is None:
            self._log.error(
                f"Cannot adjust account: no account found for {self.exec_client.venue}"
            )
            return

        cdef AccountBalance balance = account.balance(adjustment.currency)
        if balance is None:
            self._log.error(
                f"Cannot adjust account: no balance found for {adjustment.currency}"
            )
            return

        balance.total = Money(balance.total + adjustment, adjustment.currency)
        balance.free = Money(balance.free + adjustment, adjustment.currency)

        cdef list margins = []
        if account.is_margin_account:
            margins = list(account.margins().values())

        # Generate and handle event
        self.exec_client.generate_account_state(
            balances=[balance],
            margins=margins,
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

    cpdef void send(self, TradingCommand command):
        """
        Send the given trading command into the exchange.

        Parameters
        ----------
        command : TradingCommand
            The command to send.

        """
        Condition.not_none(command, "command")

        if self.latency_model is None:
            self._message_queue.appendleft(command)
        else:
            heappush(self._inflight_queue, self.generate_inflight_command(command))

    cdef tuple generate_inflight_command(self, TradingCommand command):
        cdef uint64_t ts
        if isinstance(command, (SubmitOrder, SubmitOrderList)):
            ts = command.ts_init + self.latency_model.insert_latency_nanos
        elif isinstance(command, ModifyOrder):
            ts = command.ts_init + self.latency_model.update_latency_nanos
        elif isinstance(command, (CancelOrder, CancelAllOrders, BatchCancelOrders)):
            ts = command.ts_init + self.latency_model.cancel_latency_nanos
        else:
            raise ValueError(f"invalid `TradingCommand`, was {command}")  # pragma: no cover (design-time error)
        if ts not in self._inflight_counter:
            self._inflight_counter[ts] = 0
        self._inflight_counter[ts] += 1
        cdef (uint64_t, uint64_t) key = (ts, self._inflight_counter[ts])
        return key, command

    cpdef void process_order_book_delta(self, OrderBookDelta delta):
        """
        Process the exchanges market for the given order book delta.

        Parameters
        ----------
        data : OrderBookDelta
            The order book delta to process.

        """
        Condition.not_none(delta, "delta")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(delta)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(delta.instrument_id)
        if matching_engine is None:
            raise RuntimeError(f"No matching engine found for {delta.instrument_id}")

        matching_engine.process_order_book_delta(delta)

    cpdef void process_order_book_deltas(self, OrderBookDeltas deltas):
        """
        Process the exchanges market for the given order book deltas.

        Parameters
        ----------
        data : OrderBookDeltas
            The order book deltas to process.

        """
        Condition.not_none(deltas, "deltas")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(deltas)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(deltas.instrument_id)
        if matching_engine is None:
            raise RuntimeError(f"No matching engine found for {deltas.instrument_id}")

        matching_engine.process_order_book_deltas(deltas)

    cpdef void process_quote_tick(self, QuoteTick tick):
        """
        Process the exchanges market for the given quote tick.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        tick : QuoteTick
            The tick to process.

        """
        Condition.not_none(tick, "tick")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(tick)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(tick.instrument_id)
        if matching_engine is None:
            raise RuntimeError(f"No matching engine found for {tick.instrument_id}")

        matching_engine.process_quote_tick(tick)

    cpdef void process_trade_tick(self, TradeTick tick):
        """
        Process the exchanges market for the given trade tick.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        tick : TradeTick
            The tick to process.

        """
        Condition.not_none(tick, "tick")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(tick)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(tick.instrument_id)
        if matching_engine is None:
            raise RuntimeError(f"No matching engine found for {tick.instrument_id}")

        matching_engine.process_trade_tick(tick)

    cpdef void process_bar(self, Bar bar):
        """
        Process the exchanges market for the given bar.

        Market dynamics are simulated by auctioning open orders.

        Parameters
        ----------
        bar : Bar
            The bar to process.

        """
        Condition.not_none(bar, "bar")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(bar)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(bar.bar_type.instrument_id)
        if matching_engine is None:
            raise RuntimeError(f"No matching engine found for {bar.bar_type.instrument_id}")

        matching_engine.process_bar(bar)

    cpdef void process_venue_status(self, VenueStatus data):
        """
        Process the exchange for the given status.

        Parameters
        ----------
        data : VenueStatus
            The status update to process.

        """
        Condition.not_none(data, "data")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(data)

        cdef OrderMatchingEngine matching_engine
        for matching_engine in self._matching_engines.values():
            matching_engine.process_status(data.status)

    cpdef void process_instrument_status(self, InstrumentStatus data):
        """
        Process a specific instrument status.

        Parameters
        ----------
        data : VenueStatus
            The status update to process.

        """
        Condition.not_none(data, "data")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(data)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(data.instrument_id)
        if matching_engine is None:
            raise RuntimeError(f"No matching engine found for {data.instrument_id}")

        matching_engine.process_status(data.status)

    cpdef void process(self, uint64_t ts_now):
        """
        Process the exchange to the gives time.

        All pending commands will be processed along with all simulation modules.

        Parameters
        ----------
        ts_now : uint64_t
            The current UNIX timestamp (nanoseconds).

        """
        self._clock.set_time(ts_now)

        cdef:
            uint64_t ts
        while self._inflight_queue:
            # Peek at timestamp of next in-flight message
            ts = self._inflight_queue[0][0][0]
            if ts <= ts_now:
                # Place message on queue to be processed
                self._message_queue.appendleft(self._inflight_queue.pop(0)[1])
                self._inflight_counter.pop(ts, None)
            else:
                break

        cdef:
            TradingCommand command
            Order order
            list orders
        while self._message_queue:
            command = self._message_queue.pop()
            if isinstance(command, SubmitOrder):
                self._matching_engines[command.instrument_id].process_order(command.order, self.exec_client.account_id)
            elif isinstance(command, SubmitOrderList):
                for order in command.order_list.orders:
                    self._matching_engines[command.instrument_id].process_order(order, self.exec_client.account_id)
            elif isinstance(command, ModifyOrder):
                self._matching_engines[command.instrument_id].process_modify(command, self.exec_client.account_id)
            elif isinstance(command, CancelOrder):
                self._matching_engines[command.instrument_id].process_cancel(command, self.exec_client.account_id)
            elif isinstance(command, CancelAllOrders):
                self._matching_engines[command.instrument_id].process_cancel_all(command, self.exec_client.account_id)
            elif isinstance(command, BatchCancelOrders):
                self._matching_engines[command.instrument_id].process_batch_cancel(command, self.exec_client.account_id)

        # Iterate over modules
        cdef SimulationModule module
        for module in self.modules:
            module.process(ts_now)

    cpdef void reset(self):
        """
        Reset the simulated exchange.

        All stateful fields are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        for module in self.modules:
            module.reset()

        self._generate_fresh_account_state()

        for matching_engine in self._matching_engines.values():
            matching_engine.reset()

        self._message_queue = deque()
        self._inflight_queue.clear()
        self._inflight_counter.clear()

        self._log.info("Reset.")

# -- EVENT GENERATORS -----------------------------------------------------------------------------

    cdef void _generate_fresh_account_state(self):
        cdef list balances = [
            AccountBalance(
                total=money,
                locked=Money(0, money.currency),
                free=money,
            )
            for money in self.starting_balances
        ]

        self.exec_client.generate_account_state(
            balances=balances,
            margins=[],
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

        # Set leverages
        cdef Account account = self.get_account()
        if account.is_margin_account:
            account.set_default_leverage(self.default_leverage)
            # Set instrument specific leverages
            for instrument_id, leverage in self.leverages.items():
                account.set_leverage(instrument_id, leverage)
