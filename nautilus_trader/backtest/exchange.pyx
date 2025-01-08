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

from collections import deque
from decimal import Decimal
from heapq import heappush

from nautilus_trader.common.config import InvalidConfiguration

from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.backtest.execution_client cimport BacktestExecClient
from nautilus_trader.backtest.matching_engine cimport OrderMatchingEngine
from nautilus_trader.backtest.models cimport FeeModel
from nautilus_trader.backtest.models cimport FillModel
from nautilus_trader.backtest.models cimport LatencyModel
from nautilus_trader.backtest.models cimport MakerTakerFeeModel
from nautilus_trader.backtest.modules cimport SimulationModule
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport TestClock
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.rust.model cimport OmsType
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.execution.messages cimport TradingCommand
from nautilus_trader.model.book cimport OrderBook
from nautilus_trader.model.data cimport InstrumentClose
from nautilus_trader.model.data cimport InstrumentStatus
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.functions cimport account_type_to_str
from nautilus_trader.model.functions cimport oms_type_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.crypto_future cimport CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual cimport CryptoPerpetual
from nautilus_trader.model.objects cimport AccountBalance
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef class SimulatedExchange:
    """
    Provides a simulated exchange venue.

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
    modules : list[SimulatedModule]
        The simulation modules for the exchange.
    portfolio : PortfolioFacade
        The read-only portfolio for the exchange.
    msgbus : MessageBus
        The message bus for the exchange.
    cache : CacheFacade
        The read-only cache for the exchange.
    clock : TestClock
        The clock for the exchange.
    fill_model : FillModel
        The fill model for the exchange.
    fee_model : FeeModel
        The fee model for the exchange.
    latency_model : LatencyModel, optional
        The latency model for the exchange.
    book_type : BookType
        The order book type for the exchange.
    frozen_account : bool, default False
        If the account for this exchange is frozen (balances will not change).
    reject_stop_orders : bool, default True
        If stop orders are rejected on submission if in the market.
    support_gtd_orders : bool, default True
        If orders with GTD time in force will be supported by the exchange.
    support_contingent_orders : bool, default True
        If contingent orders will be supported/respected by the exchange.
        If False, then its expected the strategy will be managing any contingent orders.
    use_position_ids : bool, default True
        If venue position IDs will be generated on order fills.
    use_random_ids : bool, default False
        If all exchange generated identifiers will be random UUID4's.
    use_reduce_only : bool, default True
        If the `reduce_only` execution instruction on orders will be honored.
    use_message_queue : bool, default True
        If an internal message queue should be used to process trading commands in sequence after
        they have initially arrived. Setting this to False would be appropriate for real-time
        sandbox environments, where we don't want to introduce additional latency of waiting for
        the next data event before processing the trading command.
    bar_execution : bool, default True
        If bars should be processed by the matching engine(s) (and move the market).
    bar_adaptive_high_low_ordering : bool, default False
        Determines whether the processing order of bar prices is adaptive based on a heuristic.
        This setting is only relevant when `bar_execution` is True.
        If False, bar prices are always processed in the fixed order: Open, High, Low, Close.
        If True, the processing order adapts with the heuristic:
        - If High is closer to Open than Low then the processing order is Open, High, Low, Close.
        - If Low is closer to Open than High then the processing order is Open, Low, High, Close.
    trade_execution : bool, default False
        If trades should be processed by the matching engine(s) (and move the market).

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
        Currency base_currency: Currency | None,
        default_leverage not None: Decimal,
        leverages not None: dict[InstrumentId, Decimal],
        list modules not None,
        PortfolioFacade portfolio not None,
        MessageBus msgbus not None,
        CacheFacade cache not None,
        TestClock clock not None,
        FillModel fill_model not None,
        FeeModel fee_model not None,
        LatencyModel latency_model = None,
        BookType book_type = BookType.L1_MBP,
        bint frozen_account = False,
        bint reject_stop_orders = True,
        bint support_gtd_orders = True,
        bint support_contingent_orders = True,
        bint use_position_ids = True,
        bint use_random_ids = False,
        bint use_reduce_only = True,
        bint use_message_queue = True,
        bint bar_execution = True,
        bint bar_adaptive_high_low_ordering = False,
        bint trade_execution = False,
    ) -> None:
        Condition.not_empty(starting_balances, "starting_balances")
        Condition.list_type(starting_balances, Money, "starting_balances")
        Condition.list_type(modules, SimulationModule, "modules", "SimulationModule")
        if base_currency:
            Condition.is_true(len(starting_balances) == 1, "single-currency account has multiple starting currencies")
        if default_leverage and default_leverage > 1 or leverages:
            Condition.is_true(account_type == AccountType.MARGIN, "leverages defined when account type is not `MARGIN`")

        self._clock = clock
        self._log = Logger(name=f"{type(self).__name__}({venue})")

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

        # Execution config
        self.reject_stop_orders = reject_stop_orders
        self.support_gtd_orders = support_gtd_orders
        self.support_contingent_orders = support_contingent_orders
        self.use_position_ids = use_position_ids
        self.use_random_ids = use_random_ids
        self.use_reduce_only = use_reduce_only
        self.use_message_queue = use_message_queue
        self.bar_execution = bar_execution
        self.bar_adaptive_high_low_ordering = bar_adaptive_high_low_ordering
        self.trade_execution = trade_execution

        # Execution models
        self.fill_model = fill_model
        self.fee_model = fee_model
        self.latency_model = latency_model

        # Load modules
        self.modules = []
        for module in modules:
            Condition.not_in(module, self.modules, "module", "modules")
            module.register_venue(self)
            module.register_base(
                portfolio=portfolio,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
            )
            self.modules.append(module)
            self._log.info(f"Loaded {module}")

        # Markets
        self.instruments: dict[InstrumentId, Instrument] = {}
        self._matching_engines: dict[InstrumentId, OrderMatchingEngine] = {}

        self._message_queue = deque()
        self._inflight_queue: list[tuple[(uint64_t, uint64_t), TradingCommand]] = []
        self._inflight_counter: dict[uint64_t, uint64_t] = {}

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

        self._log.info(f"Registered ExecutionClient-{client}")

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
                f"to {self.fill_model}",
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

        self._log.info("Changed latency model")

    cpdef void initialize_account(self):
        """
        Initialize the account to the starting balances.

        """
        self._generate_fresh_account_state()

    cpdef void add_instrument(self, Instrument instrument):
        """
        Add the given instrument to the exchange.

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
            fee_model=self.fee_model,
            book_type=self.book_type,
            oms_type=self.oms_type,
            account_type=self.account_type,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self._clock,
            reject_stop_orders=self.reject_stop_orders,
            support_gtd_orders=self.support_gtd_orders,
            support_contingent_orders=self.support_contingent_orders,
            use_position_ids=self.use_position_ids,
            use_random_ids=self.use_random_ids,
            use_reduce_only=self.use_reduce_only,
            bar_execution=self.bar_execution,
            bar_adaptive_high_low_ordering=self.bar_adaptive_high_low_ordering,
            trade_execution=self.trade_execution,
        )

        self._matching_engines[instrument.id] = matching_engine

        self._log.info(f"Added instrument {instrument.id} and created matching engine")

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

        if not self.use_message_queue:
            self._process_trading_command(command)
        elif self.latency_model is None:
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
            instrument = self.cache.instrument(delta.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {delta.instrument_id}")
            self.add_instrument(instrument)
            matching_engine = self._matching_engines[delta.instrument_id]

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
            instrument = self.cache.instrument(deltas.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {deltas.instrument_id}")
            self.add_instrument(instrument)
            matching_engine = self._matching_engines[deltas.instrument_id]

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
            instrument = self.cache.instrument(tick.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {tick.instrument_id}")
            self.add_instrument(instrument)
            matching_engine = self._matching_engines[tick.instrument_id]

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
            instrument = self.cache.instrument(tick.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {tick.instrument_id}")
            self.add_instrument(instrument)
            matching_engine = self._matching_engines[tick.instrument_id]

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
            instrument = self.cache.instrument(bar.bar_type.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {bar.bar_type.instrument_id}")
            self.add_instrument(instrument)
            matching_engine = self._matching_engines[bar.bar_type.instrument_id]

        matching_engine.process_bar(bar)

    cpdef void process_instrument_status(self, InstrumentStatus data):
        """
        Process a specific instrument status.

        Parameters
        ----------
        data : InstrumentStatus
            The instrument status update to process.

        """
        Condition.not_none(data, "data")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(data)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(data.instrument_id)
        if matching_engine is None:
            instrument = self.cache.instrument(data.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {data.instrument_id}")
            self.add_instrument(instrument)
            matching_engine = self._matching_engines[data.instrument_id]

        matching_engine.process_status(data.action)

    cpdef void process_instrument_close(self, InstrumentClose close):
        """
        Process the exchanges market for the given instrument close.

        Parameters
        ----------
        close : InstrumentClose
            The instrument close to process.

        """
        Condition.not_none(close, "close")

        cdef SimulationModule module
        for module in self.modules:
            module.pre_process(close)

        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(close.instrument_id)
        if matching_engine is None:
            instrument = self.cache.instrument(close.instrument_id)
            if instrument is None:
                raise RuntimeError(f"No matching engine found for {close.instrument_id}")
            self.add_instrument(instrument)
            matching_engine = self._matching_engines[close.instrument_id]

        matching_engine.process_instrument_close(close)

    cpdef void process(self, uint64_t ts_now):
        """
        Process the exchange to the given time.

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

        cdef TradingCommand command
        while self._message_queue:
            command = self._message_queue.pop()
            self._process_trading_command(command)

        # Iterate over modules
        cdef SimulationModule module
        for module in self.modules:
            module.process(ts_now)

    cpdef void reset(self):
        """
        Reset the simulated exchange.

        All stateful fields are reset to their initial value.
        """
        self._log.debug(f"Resetting")

        for module in self.modules:
            module.reset()

        self._generate_fresh_account_state()

        for matching_engine in self._matching_engines.values():
            matching_engine.reset()

        self._message_queue = deque()
        self._inflight_queue.clear()
        self._inflight_counter.clear()

        self._log.info("Reset")

    cdef void _process_trading_command(self, TradingCommand command):
        cdef OrderMatchingEngine matching_engine = self._matching_engines.get(command.instrument_id)
        if matching_engine is None:
            raise RuntimeError(f"Cannot process command: no matching engine for {command.instrument_id}")

        cdef:
            Order order
            list[Order] orders
        if isinstance(command, SubmitOrder):
            matching_engine.process_order(command.order, self.exec_client.account_id)
        elif isinstance(command, SubmitOrderList):
            for order in command.order_list.orders:
                matching_engine.process_order(order, self.exec_client.account_id)
        elif isinstance(command, ModifyOrder):
            matching_engine.process_modify(command, self.exec_client.account_id)
        elif isinstance(command, CancelOrder):
            matching_engine.process_cancel(command, self.exec_client.account_id)
        elif isinstance(command, CancelAllOrders):
            matching_engine.process_cancel_all(command, self.exec_client.account_id)
        elif isinstance(command, BatchCancelOrders):
            matching_engine.process_batch_cancel(command, self.exec_client.account_id)

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
