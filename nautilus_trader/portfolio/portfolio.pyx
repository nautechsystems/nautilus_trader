# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

"""
The `Portfolio` facilitates the management of trading operations.

The intended use case is for a single ``Portfolio`` instance per running system,
a fleet of trading strategies will organize around a portfolio with the help
of the `Trader`` class.

The portfolio can satisfy queries for account information, margin balances,
total risk exposures and total net positions.
"""

import pickle
from collections import defaultdict
from decimal import Decimal

from nautilus_trader.analysis import AvgLoser
from nautilus_trader.analysis import AvgWinner
from nautilus_trader.analysis import Expectancy
from nautilus_trader.analysis import LongRatio
from nautilus_trader.analysis import MaxLoser
from nautilus_trader.analysis import MaxWinner
from nautilus_trader.analysis import MinLoser
from nautilus_trader.analysis import MinWinner
from nautilus_trader.analysis import PortfolioAnalyzer
from nautilus_trader.analysis import ProfitFactor
from nautilus_trader.analysis import ReturnsAverage
from nautilus_trader.analysis import ReturnsAverageLoss
from nautilus_trader.analysis import ReturnsAverageWin
from nautilus_trader.analysis import ReturnsVolatility
from nautilus_trader.analysis import RiskReturnRatio
from nautilus_trader.analysis import SharpeRatio
from nautilus_trader.analysis import SortinoRatio
from nautilus_trader.analysis import WinRate
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.portfolio.config import PortfolioConfig

from libc.stdint cimport uint64_t

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.factory cimport AccountFactory
from nautilus_trader.accounting.manager cimport AccountsManager
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport LogColor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport NANOSECONDS_IN_MILLISECOND
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.model.data cimport Bar
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderExpired
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.events.position cimport PositionEvent
from nautilus_trader.model.functions cimport position_side_to_str
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.betting cimport BettingInstrument
from nautilus_trader.model.instruments.betting cimport order_side_to_bet_side
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef tuple[OrderEvent] _UPDATE_ORDER_EVENTS = (
    OrderAccepted,
    OrderCanceled,
    OrderExpired,
    OrderRejected,
    OrderUpdated,
    OrderFilled,
)


cdef class Portfolio(PortfolioFacade):
    """
    Provides a trading portfolio.

    Currently there is a limitation of one account per ``ExecutionClient``
    instance.

    Parameters
    ----------
    msgbus : MessageBus
        The message bus for the engine.
    cache : CacheFacade
        The read-only cache for the portfolio.
    clock : Clock
        The clock for the portfolio.
    config : PortfolioConfig
       The configuration for the instance.

    Raises
    ------
    TypeError
        If `config` is not of type `PortfolioConfig`.
    """

    def __init__(
        self,
        MessageBus msgbus not None,
        CacheFacade cache not None,
        Clock clock not None,
        config: PortfolioConfig | None = None,
    ) -> None:
        if config is None:
            config = PortfolioConfig()

        Condition.type(config, PortfolioConfig, "config")

        self._clock = clock
        self._log = Logger(name=type(self).__name__)
        self._msgbus = msgbus
        self._cache = cache
        self._accounts = AccountsManager(
            cache=cache,
            clock=clock,
            logger=self._log,
        )

        # Configuration
        self._config: PortfolioConfig = config
        self._debug: bool = config.debug
        self._use_mark_prices: bool = config.use_mark_prices
        self._use_mark_xrates: bool = config.use_mark_xrates
        self._convert_to_account_base_currency: bool = config.convert_to_account_base_currency
        self._log_price: str = "mark price" if config.use_mark_prices else "quote, trade, or bar price"
        self._log_xrate: str = "mark" if config.use_mark_xrates else "data to calculate"

        if config.min_account_state_logging_interval_ms:
            interval_ns = config.min_account_state_logging_interval_ms * NANOSECONDS_IN_MILLISECOND
            self._min_account_state_logging_interval_ns = interval_ns

        self._unrealized_pnls: dict[InstrumentId, dict[AccountId, Money]] = {}
        self._realized_pnls: dict[InstrumentId, dict[AccountId, Money]] = {}
        self._snapshot_sum_per_position: dict[PositionId, Money] = {}
        self._snapshot_last_per_position: dict[PositionId, Money] = {}
        self._snapshot_processed_counts: dict[PositionId, int] = {}
        self._snapshot_account_ids: dict[PositionId, AccountId] = {}
        self._net_positions: dict[InstrumentId, dict[AccountId, Decimal]] = {}
        self._bet_positions: dict[InstrumentId, object] = {}
        self._index_bet_positions: dict[InstrumentId, set[PositionId]] = defaultdict(set)
        self._pending_calcs: set[InstrumentId] = set()
        self._bar_close_prices: dict[InstrumentId, Price] = {}
        self._last_account_state_log_ts: dict[AccountId, uint64_t] = {}

        self.analyzer = PortfolioAnalyzer()

        # Register default statistics
        self.analyzer.register_statistic(MaxWinner())
        self.analyzer.register_statistic(AvgWinner())
        self.analyzer.register_statistic(MinWinner())
        self.analyzer.register_statistic(MinLoser())
        self.analyzer.register_statistic(AvgLoser())
        self.analyzer.register_statistic(MaxLoser())
        self.analyzer.register_statistic(Expectancy())
        self.analyzer.register_statistic(WinRate())
        self.analyzer.register_statistic(ReturnsVolatility())
        self.analyzer.register_statistic(ReturnsAverage())
        self.analyzer.register_statistic(ReturnsAverageLoss())
        self.analyzer.register_statistic(ReturnsAverageWin())
        self.analyzer.register_statistic(SharpeRatio())
        self.analyzer.register_statistic(SortinoRatio())
        self.analyzer.register_statistic(ProfitFactor())
        self.analyzer.register_statistic(RiskReturnRatio())
        self.analyzer.register_statistic(LongRatio())

        # Register endpoints
        self._msgbus.register(endpoint="Portfolio.update_account", handler=self.update_account)
        self._msgbus.register(endpoint="Portfolio.update_order", handler=self.update_order)
        self._msgbus.register(endpoint="Portfolio.update_position", handler=self.update_position)

        # Required subscriptions
        self._msgbus.subscribe(topic="events.order.*", handler=self.on_order_event, priority=10)
        self._msgbus.subscribe(topic="events.position.*", handler=self.on_position_event, priority=10)

        if config.use_mark_prices:
            self._msgbus.subscribe(topic="data.mark_prices.*", handler=self.update_mark_price, priority=10)
        else:
            self._msgbus.subscribe(topic="data.quotes.*", handler=self.update_quote_tick, priority=10)

        if config.bar_updates:
            self._msgbus.subscribe(topic="data.bars.*EXTERNAL", handler=self.update_bar, priority=10)

        self.initialized = False

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void set_use_mark_prices(self, bint value):
        """
        Set the `use_mark_prices` setting with the given `value`.

        Parameters
        ----------
        value : bool
            The value to set.

        """
        self._use_mark_prices = value

    cpdef void set_use_mark_xrates(self, bint value):
        """
        Set the `use_mark_xrates` setting with the given `value`.

        Parameters
        ----------
        value : bool
            The value to set.

        """
        self._use_mark_xrates = value

    cpdef void initialize_orders(self):
        """
        Initialize the portfolios orders.

        Performs all account calculations for the current orders state.
        """
        cdef list all_orders_open = self._cache.orders_open()
        cdef set instruments = set()
        cdef Order order
        for order in all_orders_open:
            instruments.add(order.instrument_id)

        # Update initial (order) margins to initialize portfolio
        cdef bint initialized = True
        cdef:
            Order o
            list orders_open
            bint result
            dict orders_by_account
            AccountId account_id
            Account account
            list account_orders
        for instrument_id in instruments:
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot update initial (order) margin: "
                    f"no instrument found for {instrument_id}",
                )
                initialized = False
                break

            orders_open = self._cache.orders_open(
                venue=None,  # Faster query filtering
                instrument_id=instrument.id,
            )
            orders_by_account = self._group_by_account_id(orders_open)

            for account_id, account_orders in orders_by_account.items():
                account = self._cache.account(account_id)
                if account is None:
                    self._log.error(
                        f"Cannot update initial (order) margin: "
                        f"account {account_id} not found in cache",
                    )
                    initialized = False
                    break

                result = self._accounts.update_orders(
                    account=account,
                    instrument=instrument,
                    orders_open=[o for o in account_orders if o.is_passive_c()],
                    ts_event=account.last_event_c().ts_event,
                )
                if not result:
                    initialized = False

            if not initialized:
                break

        cdef int open_count = len(all_orders_open)
        self._log.info(
            f"Initialized {open_count} open order{'' if open_count == 1 else 's'}",
            color=LogColor.BLUE if open_count else LogColor.NORMAL,
        )
        self.initialized = initialized

    cpdef void initialize_positions(self):
        """
        Initialize the portfolios positions.

        Performs all account calculations for the current position state.
        """
        # Clean slate
        self._realized_pnls.clear()
        self._unrealized_pnls.clear()

        cdef list all_positions_open = self._cache.positions_open()
        cdef set instruments = set()
        cdef Position position

        for position in all_positions_open:
            instruments.add(position.instrument_id)

        cdef bint initialized = True

        # Update maintenance (position) margins to initialize portfolio
        cdef:
            InstrumentId instrument_id
            Instrument instrument
            list positions_open
            Account account
            bint result
            dict positions_by_account
            list account_positions
            AccountId account_id
            Money realized_pnl
            Money unrealized_pnl
        for instrument_id in instruments:
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot update maintenance (position) margin: "
                    f"no instrument found for {instrument_id}",
                )
                initialized = False
                break

            positions_open = self._cache.positions_open(
                venue=None,  # Faster query filtering
                instrument_id=instrument_id,
            )

            self._update_net_position(
                instrument_id=instrument_id,
                positions_open=positions_open,
            )

            positions_by_account = self._group_by_account_id(positions_open)

            for account_id, account_positions in positions_by_account.items():
                account = self._cache.account(account_id)
                if account is None:
                    self._log.error(
                        f"Cannot update maintenance (position) margin: "
                        f"account {account_id} not found in cache",
                    )
                    initialized = False
                    continue

                # Calculate and cache PnL for this account
                realized_pnl = self.realized_pnl(instrument_id, account_id)
                unrealized_pnl = self.unrealized_pnl(instrument_id, price=None, account_id=account_id)

                if account.type != AccountType.MARGIN:
                    continue

                result = self._accounts.update_positions(
                    account=account,
                    instrument=instrument,
                    positions_open=account_positions,
                    ts_event=account.last_event_c().ts_event,
                )
                if not result:
                    initialized = False

        cdef int open_count = len(all_positions_open)
        self._log.info(
            f"Initialized {open_count} open position{'' if open_count == 1 else 's'}",
            color=LogColor.BLUE if open_count else LogColor.NORMAL,
        )
        self.initialized = initialized

    cpdef void update_quote_tick(self, QuoteTick tick):
        """
        Update the portfolio with the given quote tick.

        Clears the cached unrealized PnL for the associated instrument, and
        performs any initialization calculations which may have been pending
        an update.

        Parameters
        ----------
        quote_tick : QuoteTick
            The quote tick to update with.

        """
        Condition.not_none(tick, "tick")

        self._update_instrument_id(tick.instrument_id)

    cpdef void update_mark_price(self, object mark_price):
        """
        TBD
        """
        # TODO: Feature is WIP
        self._log.warning("Mark price updates not yet supported")

    cpdef void update_bar(self, Bar bar):
        """
        Update the portfolio with the given bar.

        Clears the cached unrealized PnL for the associated instrument, and
        performs any initialization calculations which may have been pending
        an update.

        Parameters
        ----------
        bar : Bar
            The bar to update with.

        """
        Condition.not_none(bar, "bar")

        cdef InstrumentId instrument_id = bar.bar_type.instrument_id
        self._bar_close_prices[instrument_id] = bar.close
        self._update_instrument_id(instrument_id)

    cpdef void update_account(self, AccountState event):
        """
        Apply the given account state.

        Parameters
        ----------
        event : AccountState
            The account state to apply.

        """
        Condition.not_none(event, "event")

        self._update_account(event)

        self._msgbus.publish_c(
            topic=f"events.account.{event.account_id}",
            msg=event,
        )

    cpdef void update_order(self, OrderEvent event):
        """
        Update the portfolio with the given order.

        Parameters
        ----------
        event : OrderEvent
            The event to update with.

        """
        Condition.not_none(event, "event")

        if event.account_id is None:
            return  # No account assigned yet

        cdef:
            Account account = self._cache.account(event.account_id)
            Money unrealized_pnl

        if account is None:
            self._log.error(
                f"Cannot update order: "
                f"no account registered for {event.account_id}",
            )
            return  # No account registered

        if not account.calculate_account_state:
            return  # Nothing to calculate

        if not isinstance(event, _UPDATE_ORDER_EVENTS):
            return  # No change to account state

        cdef Order order = self._cache.order(event.client_order_id)

        if order is None:
            self._log.error(
                f"Cannot update order: "
                f"{repr(event.client_order_id)} not found in the cache",
            )
            return  # No order found

        if isinstance(event, OrderRejected) and order.order_type != OrderType.STOP_LIMIT:
            return  # No change to account state

        cdef Instrument instrument = self._cache.instrument(event.instrument_id)

        if instrument is None:
            self._log.error(
                f"Cannot update order: "
                f"no instrument found for {event.instrument_id}",
            )
            return  # No instrument found

        if self._debug:
            self._log.debug(f"Updating with {order!r}", LogColor.MAGENTA)

        if isinstance(event, OrderFilled):
            self._accounts.update_balances(
                account=account,
                instrument=instrument,
                fill=event,
            )

            if isinstance(instrument, BettingInstrument):
                position_id = event.position_id or PositionId(instrument.id.value)
                bet_position = self._bet_positions.get(position_id)

                if bet_position is None:
                    bet_position = nautilus_pyo3.BetPosition()
                    self._bet_positions[position_id] = bet_position
                    self._index_bet_positions[instrument.id].add(position_id)

                bet = nautilus_pyo3.Bet(
                    price=event.last_px.as_decimal(),
                    stake=event.last_qty.as_decimal(),
                    side=order_side_to_bet_side(order_side=event.order_side),
                )
                if self._debug:
                    self._log.debug(f"Applying {bet} to {bet_position}", LogColor.MAGENTA)

                bet_position.add_bet(bet)

                if self._debug:
                    self._log.debug(f"{bet_position}", LogColor.MAGENTA)

            # Calculate and cache unrealized PnL for this account
            unrealized_pnl = self.unrealized_pnl(event.instrument_id, price=None, account_id=account.id)

        cdef list orders_open = self._cache.orders_open(
            venue=None,  # Faster query filtering
            instrument_id=event.instrument_id,
            strategy_id=None,
            side=OrderSide.NO_ORDER_SIDE,
            account_id=event.account_id,
        )

        cdef Order o
        cdef list passive_orders = [o for o in orders_open if o.is_passive_c()]

        cdef bint result = self._accounts.update_orders(
            account=account,
            instrument=instrument,
            orders_open=passive_orders,
            ts_event=event.ts_event,
        )

        if not result:
            self._log.debug(f"Added pending calculation for {instrument.id}")
            self._pending_calcs.add(instrument.id)

        # Always update account state for cash accounts on non-fill events, or when update_orders succeeded
        if account.is_cash_account or not isinstance(event, OrderFilled):
            # Only update account state for other than fill events (these will be updated on position update)
            self._update_account(self._accounts.generate_account_state(account, event.ts_event))

        self._log.debug(f"Updated from {event}")

    cpdef void update_position(self, PositionEvent event):
        """
        Update the portfolio with the given position event.

        Parameters
        ----------
        event : PositionEvent
            The event to update with.

        """
        Condition.not_none(event, "event")

        # Fetch all positions for the instrument to calculate global net position
        cdef list all_positions_open = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=event.instrument_id,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=None,  # Get all accounts for net position
        )
        self._update_net_position(
            instrument_id=event.instrument_id,
            positions_open=all_positions_open,
        )

        # Invalidate cached PnLs for this instrument and account only
        if event.instrument_id in self._realized_pnls:
            self._realized_pnls[event.instrument_id].pop(event.account_id, None)
        if event.instrument_id in self._unrealized_pnls:
            self._unrealized_pnls[event.instrument_id].pop(event.account_id, None)

        cdef Account account = self._cache.account(event.account_id)

        if account is None:
            self._log.error(
                f"Cannot update position: "
                f"no account registered for {event.account_id}",
            )
            return  # No account registered

        if account.type != AccountType.MARGIN or not account.calculate_account_state:
            return  # Nothing to calculate

        cdef Instrument instrument = self._cache.instrument(event.instrument_id)

        if instrument is None:
            self._log.error(
                f"Cannot update position: "
                f"no instrument found for {event.instrument_id}",
            )
            return  # No instrument found

        # Fetch positions filtered by account_id for account-specific update
        cdef list account_positions = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=event.instrument_id,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=event.account_id,  # Filter by account_id
        )

        cdef bint result = self._accounts.update_positions(
            account=account,
            instrument=instrument,
            positions_open=account_positions,
            ts_event=event.ts_event,
        )

        if result:
            account_state = self._accounts.generate_account_state(account, event.ts_event)
            self._update_account(account_state)

    cpdef void on_order_event(self, OrderEvent event):
        """
        Actions to be performed on receiving an order event.

        Parameters
        ----------
        event : OrderEvent
            The event received.

        """
        Condition.not_none(event, "event")

        if event.account_id is None:
            return  # No account assigned for event

        if not isinstance(event, _UPDATE_ORDER_EVENTS):
            return  # No change to account state

        if isinstance(event, OrderFilled):
            return  # Will publish account event when position event is received

        cdef Account account = self._cache.account(event.account_id)
        if account is None:
            return  # No account registered

        cdef AccountState account_state = account.last_event_c()

        self._msgbus.publish_c(
            topic=f"events.account.{account.id}",
            msg=account_state,
        )

    cpdef void on_position_event(self, PositionEvent event):
        """
        Actions to be performed on receiving a position event.

        Parameters
        ----------
        event : PositionEvent
            The event received.

        """
        Condition.not_none(event, "event")

        if event.account_id is None:
            return  # No account assigned for event

        cdef Account account = self._cache.account(event.account_id)
        if account is None:
            return  # No account registered

        cdef AccountState account_state = account.last_event_c()

        self._msgbus.publish_c(
            topic=f"events.account.{account.id}",
            msg=account_state,
        )

    def _reset(self) -> None:
        self._net_positions.clear()
        self._bet_positions.clear()
        self._index_bet_positions.clear()
        self._realized_pnls.clear()
        self._unrealized_pnls.clear()
        self._pending_calcs.clear()
        self._snapshot_sum_per_position.clear()
        self._snapshot_last_per_position.clear()
        self._snapshot_processed_counts.clear()
        self._snapshot_account_ids.clear()
        self.analyzer.reset()

        self.initialized = False

    def reset(self) -> None:
        """
        Reset the portfolio.

        All stateful fields are reset to their initial value.

        """
        self._log.debug(f"RESETTING")
        self._reset()
        self._log.info("READY")

    def dispose(self) -> None:
        """
        Dispose of the portfolio.

        All stateful fields are reset to their initial value.

        """
        self._log.debug(f"DISPOSING")
        self._reset()
        self._log.info("DISPOSED")

    cdef void _ensure_snapshot_pnls_cached_for(self, InstrumentId instrument_id):
        # Performance: This method maintains an incremental cache of snapshot PnLs
        # It only unpickles new snapshots that haven't been processed yet
        # Tracks sum and last PnL per position for efficient NETTING OMS support

        # Get all position IDs that have snapshots for this instrument
        cdef set[PositionId] snapshot_position_ids = self._cache.position_snapshot_ids(instrument_id)

        if not snapshot_position_ids:
            return  # Nothing to process

        cdef bint rebuild = False

        cdef:
            PositionId position_id
            list position_id_snapshots
            int prev_count
            int curr_count

        # Detect purge/reset (count regression) to trigger full rebuild
        for position_id in snapshot_position_ids:
            position_id_snapshots = self._cache.position_snapshot_bytes(position_id)
            curr_count = len(position_id_snapshots)
            prev_count = self._snapshot_processed_counts.get(position_id, 0)
            if prev_count > curr_count:
                rebuild = True
                break

        cdef:
            Position snapshot
            Money sum_pnl = None
            Money last_pnl = None

        if rebuild:
            # Full rebuild: process all snapshots from scratch
            for position_id in snapshot_position_ids:
                position_id_snapshots = self._cache.position_snapshot_bytes(position_id)
                curr_count = len(position_id_snapshots)

                if curr_count:
                    for s in position_id_snapshots:
                        snapshot = pickle.loads(s)

                        # Track account_id for this position snapshot
                        if snapshot.account_id is not None:
                            self._snapshot_account_ids[position_id] = snapshot.account_id

                        if snapshot.realized_pnl is not None:
                            if sum_pnl is None:
                                sum_pnl = snapshot.realized_pnl
                            elif sum_pnl.currency == snapshot.realized_pnl.currency:
                                # Accumulate all snapshot PnLs
                                sum_pnl = Money(
                                    sum_pnl.as_double() + snapshot.realized_pnl.as_double(),
                                    sum_pnl.currency
                                )
                            # Always update last to the most recent snapshot
                            last_pnl = snapshot.realized_pnl

                # Update tracking structures
                if sum_pnl is not None:
                    self._snapshot_sum_per_position[position_id] = sum_pnl
                    self._snapshot_last_per_position[position_id] = last_pnl
                else:
                    self._snapshot_sum_per_position.pop(position_id, None)
                    self._snapshot_last_per_position.pop(position_id, None)
                self._snapshot_processed_counts[position_id] = curr_count
        else:
            # Incremental path: only process new snapshots
            for position_id in snapshot_position_ids:
                position_id_snapshots = self._cache.position_snapshot_bytes(position_id)
                curr_count = len(position_id_snapshots)
                if curr_count == 0:
                    continue

                prev_count = self._snapshot_processed_counts.get(position_id, 0)
                if prev_count >= curr_count:
                    continue

                sum_pnl = self._snapshot_sum_per_position.get(position_id)
                last_pnl = self._snapshot_last_per_position.get(position_id)

                # Process only new snapshots
                for idx in range(prev_count, curr_count):
                    snapshot = pickle.loads(position_id_snapshots[idx])

                    # Track account_id for this position snapshot
                    if snapshot.account_id is not None:
                        self._snapshot_account_ids[position_id] = snapshot.account_id

                    if snapshot.realized_pnl is not None:
                        if sum_pnl is None:
                            sum_pnl = snapshot.realized_pnl
                        elif sum_pnl.currency == snapshot.realized_pnl.currency:
                            # Add to running sum
                            sum_pnl = Money(
                                sum_pnl.as_double() + snapshot.realized_pnl.as_double(),
                                sum_pnl.currency
                            )

                        # Update last to most recent
                        last_pnl = snapshot.realized_pnl

                # Update tracking structures
                if sum_pnl is not None:
                    self._snapshot_sum_per_position[position_id] = sum_pnl
                    self._snapshot_last_per_position[position_id] = last_pnl
                else:
                    self._snapshot_sum_per_position.pop(position_id, None)
                    self._snapshot_last_per_position.pop(position_id, None)
                self._snapshot_processed_counts[position_id] = curr_count

        # Prune stale entries (positions that no longer have snapshots)
        cdef list[PositionId] stale_ids = []

        cdef PositionId stale_position_id
        for stale_position_id in self._snapshot_processed_counts:
            if stale_position_id not in snapshot_position_ids:
                stale_ids.append(stale_position_id)

        for stale_position_id in stale_ids:
            self._snapshot_processed_counts.pop(stale_position_id, None)
            self._snapshot_sum_per_position.pop(stale_position_id, None)
            self._snapshot_last_per_position.pop(stale_position_id, None)
            self._snapshot_account_ids.pop(stale_position_id, None)

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Account account(self, Venue venue=None, AccountId account_id=None):
        """
        Return the account for the given venue or account ID (if found).

        Parameters
        ----------
        venue : Venue, optional
            The venue for the account.
        account_id : AccountId, optional
            The account ID (takes priority if both venue and account_id are provided).

        Returns
        -------
        Account or ``None``

        """
        Condition.not_none(venue or account_id, "venue or account_id must be provided")
        Condition.not_none(self._cache, "self._cache")

        cdef Account account = self._cache.account_for_venue(venue, account_id)
        if account is None:
            self._log.error(
                f"Cannot get account: "
                f"no account registered for {venue=} and {account_id=}",
            )

        return account

    cpdef dict balances_locked(self, Venue venue=None, AccountId account_id=None):
        """
        Return the balances locked for the given venue or account ID (if found).

        Parameters
        ----------
        venue : Venue, optional
            The venue for the account.
        account_id : AccountId, optional
            The account ID (takes priority if both venue and account_id are provided).

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        Condition.not_none(venue or account_id, "'venue' or 'account_id' must be provided")

        cdef Account account = self._cache.account_for_venue(venue, account_id)

        if account is None:
            self._log.error(
                f"Cannot get balances locked: "
                f"no account registered for {venue=} and {account_id=}",
            )
            return None

        return account.balances_locked()

    cpdef dict margins_init(self, Venue venue=None, AccountId account_id=None):
        """
        Return the initial (order) margins for the given venue or account ID (if found).

        Parameters
        ----------
        venue : Venue, optional
            The venue for the account.
        account_id : AccountId, optional
            The account ID (takes priority if both venue and account_id are provided).

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        Condition.not_none(venue or account_id, "'venue' or 'account_id' must be provided")

        cdef Account account = self._cache.account_for_venue(venue, account_id)

        if account is None:
            self._log.error(
                f"Cannot get initial (order) margins: "
                f"no account registered for {venue=} and {account_id=}",
            )
            return None

        if account.is_cash_account:
            return None

        return account.margins_init()

    cpdef dict margins_maint(self, Venue venue=None, AccountId account_id=None):
        """
        Return the maintenance (position) margins for the given venue or account ID (if found).

        Parameters
        ----------
        venue : Venue, optional
            The venue for the account.
        account_id : AccountId, optional
            The account ID (takes priority if both venue and account_id are provided).

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        Condition.not_none(venue or account_id, "'venue' or 'account_id' must be provided")

        cdef Account account = self._cache.account_for_venue(venue, account_id)
        if account is None:
            self._log.error(
                f"Cannot get maintenance (position) margins: "
                f"no account registered for {venue=} and {account_id=}",
            )
            return None

        if account.is_cash_account:
            return None

        return account.margins_maint()

    cpdef dict realized_pnls(self, Venue venue=None, AccountId account_id=None):
        """
        Return the realized PnLs for the given venue (if found).

        If no positions exist for the venue or if any lookups fail internally,
        an empty dictionary is returned.

        Parameters
        ----------
        venue : Venue, optional
            The venue for the realized PnLs.
        account_id : AccountId, optional
            The account ID for the realized PnLs.

        Returns
        -------
        dict[Currency, Money]

        """
        cdef list positions = self._cache.positions(
            venue=venue,
            instrument_id=None,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=account_id,
        )

        if not positions:
            return {}  # Nothing to calculate

        cdef set[InstrumentId] instrument_ids = {p.instrument_id for p in positions}
        cdef dict[Currency, double] realized_pnls = {}  # type: dict[Currency, 0.0]
        cdef:
            InstrumentId instrument_id
            Money pnl
        for instrument_id in instrument_ids:
            pnl = self.realized_pnl(instrument_id, account_id)

            if pnl is None:
                continue  # Error logged in `_calculate_realized_pnl`

            realized_pnls[pnl.currency] = realized_pnls.get(pnl.currency, 0.0) + pnl.as_f64_c()

        return {k: Money(v, k) for k, v in realized_pnls.items()}

    cpdef dict unrealized_pnls(self, Venue venue=None, AccountId account_id=None):
        """
        Return the unrealized PnLs for the given venue (if found).

        Parameters
        ----------
        venue : Venue, optional
            The venue for the unrealized PnLs.
        account_id : AccountId, optional
            The account ID for the unrealized PnLs.

        Returns
        -------
        dict[Currency, Money]

        """
        cdef list positions_open = self._cache.positions_open(
            venue=venue,
            instrument_id=None,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=account_id,
        )

        if not positions_open:
            return {}  # Nothing to calculate

        cdef set[InstrumentId] instrument_ids = {p.instrument_id for p in positions_open}
        cdef dict[Currency, double] unrealized_pnls = {}  # type: dict[Currency, 0.0]
        cdef:
            InstrumentId instrument_id
            Money pnl
        for instrument_id in instrument_ids:
            pnl = self.unrealized_pnl(instrument_id, price=None, account_id=account_id)

            if pnl is None:
                continue  # Error logged in `_calculate_unrealized_pnl`

            unrealized_pnls[pnl.currency] = unrealized_pnls.get(pnl.currency, 0.0) + pnl.as_f64_c()

        return {k: Money(v, k) for k, v in unrealized_pnls.items()}

    cpdef dict total_pnls(self, Venue venue=None, AccountId account_id=None):
        """
        Return the total PnLs for the given venue (if found).

        Parameters
        ----------
        venue : Venue, optional
            The venue for the total PnLs.
        account_id : AccountId, optional
            The account ID for the total PnLs.

        Returns
        -------
        dict[Currency, Money]

        """
        cdef dict realized = self.realized_pnls(venue, account_id)
        cdef dict unrealized = self.unrealized_pnls(venue, account_id)
        cdef dict[Currency, double] total_pnls = {}

        cdef:
            Currency currency
            Money amount

        # Sum realized PnLs
        for currency, amount in realized.items():
            total_pnls[currency] = amount._mem.raw

        # Add unrealized PnLs
        for currency, amount in unrealized.items():
            total_pnls[currency] = total_pnls.get(currency, 0) + amount._mem.raw

        return {k: Money.from_raw_c(v, k) for k, v in total_pnls.items()}

    cpdef dict net_exposures(self, Venue venue=None, AccountId account_id=None):
        """
        Return the net exposures for the given venue (if found).

        Parameters
        ----------
        venue : Venue, optional
            The venue for the market value.
        account_id : AccountId, optional
            The account ID for the net exposures.

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        cdef list positions_open = self._cache.positions_open(
            venue=venue,
            instrument_id=None,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=account_id,
        )

        cdef Account account = None

        if not positions_open:
            # When no positions, try to determine if account exists
            # (account can be determined from account_id parameter or venue)
            account = self._cache.account_for_venue(venue, account_id)
            if account is not None:
                # Account exists but no positions, return empty dict
                return {}
            else:
                # No account can be determined, return None
                return None

        cdef dict net_exposures = {}  # type: dict[Currency, float]

        cdef:
            Position position
            Instrument instrument
            Price price
            Currency settlement_currency
            double xrate
            double net_exposure
            double total_net_exposure
            AccountId prev_account_id = None
            AccountId current_account_id
        for position in positions_open:
            # Get account for this position if account_id changed
            current_account_id = position.account_id
            if prev_account_id != current_account_id:
                account = self._cache.account(current_account_id)
                if account is None:
                    self._log.error(
                        f"Cannot calculate net exposures: "
                        f"no account registered for {current_account_id}",
                    )
                    return None  # Cannot calculate

                prev_account_id = current_account_id

            instrument = self._cache.instrument(position.instrument_id)

            if instrument is None:
                self._log.error(
                    f"Cannot calculate net exposures: "
                    f"no instrument for {position.instrument_id}",
                )
                return None  # Cannot calculate

            if position.side == PositionSide.FLAT:
                self._log.error(
                    f"Cannot calculate net exposures: "
                    f"position is flat for {position.instrument_id}",
                )
                continue  # Nothing to calculate

            price = self._get_price(position)

            if price is None:
                self._log.error(
                    f"Cannot calculate net exposures: "
                    f"no {self._log_price} for {position.instrument_id}",
                )
                continue  # Cannot calculate

            xrate_result = self._calculate_xrate_to_base(
                instrument=instrument,
                account=account,
                side=position.entry,
            )

            if not xrate_result:
                self._log.error(
                    f"Cannot calculate net exposures: "
                    f"no {self._log_xrate} exchange rate for {instrument.get_cost_currency()}/{account.base_currency}",
                )
                return None  # Cannot calculate

            xrate = xrate_result  # Cast to double

            if self._convert_to_account_base_currency and account.base_currency is not None:
                settlement_currency = account.base_currency
            else:
                settlement_currency = instrument.get_cost_currency()

            if isinstance(instrument, BettingInstrument):
                bet_position = self._bet_positions.get(position.id)
                net_exposure = float(bet_position.exposure) * xrate
            else:
                net_exposure = instrument.notional_value(position.quantity, price).as_f64_c()
                net_exposure = round(net_exposure * xrate, settlement_currency.get_precision())

            total_net_exposure = net_exposures.get(settlement_currency, 0.0)
            total_net_exposure += net_exposure

            net_exposures[settlement_currency] = total_net_exposure

        return {k: Money(v, k) for k, v in net_exposures.items()}

    cpdef Money realized_pnl(self, InstrumentId instrument_id, AccountId account_id=None):
        """
        Return the realized PnL for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the realized PnL.
        account_id : AccountId, optional
            The account ID for the realized PnL. If None, aggregates across all accounts.

        Returns
        -------
        Money or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        if account_id is not None:
            # Single account: check cache and calculate if needed
            return self._get_or_calculate_pnl(instrument_id, account_id, is_realized=True)
        else:
            # Aggregate across all accounts by iterating cache
            return self._aggregate_pnl_from_cache(instrument_id, is_realized=True)

    cpdef Money unrealized_pnl(self, InstrumentId instrument_id, Price price=None, AccountId account_id=None):
        """
        Return the unrealized PnL for the given instrument ID (if found).

        - If `price` is provided, a fresh calculation is performed without using or
          updating the cache.
        - If `price` is omitted, the method returns the cached PnL if available, or
          computes and caches it if not.

        Returns `None` if the calculation fails (e.g., the account or instrument cannot
        be found), or zero-valued `Money` if no positions are open. Otherwise, it returns
        a `Money` object (usually in the account's base currency or the instrument's
        settlement currency).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the unrealized PnL.
        price : Price, optional
            The reference price for the calculation. This could be the last, mid, bid, ask,
            a mark-to-market price, or any other suitably representative value.
        account_id : AccountId, optional
            The account ID for the unrealized PnL. If None, aggregates across all accounts.

        Returns
        -------
        Money or ``None``
            The unrealized PnL or None if the calculation cannot be performed.

        """
        Condition.not_none(instrument_id, "instrument_id")

        if price is not None:
            # Fresh calculation with specific price (no cache)
            if account_id is not None:
                # Single account: direct calculation
                return self._calculate_unrealized_pnl(instrument_id, price, account_id)
            else:
                # Aggregate across all accounts: need to find all accounts and calculate for each
                return self._aggregate_pnl_by_calculation(instrument_id, price, is_realized=False)
        else:
            # Use cache or calculate and cache if missing
            if account_id is not None:
                # Single account: check cache and calculate if needed
                return self._get_or_calculate_pnl(instrument_id, account_id, is_realized=False)
            else:
                # Aggregate across all accounts by iterating cache
                return self._aggregate_pnl_from_cache(instrument_id, is_realized=False)

    cpdef Money total_pnl(self, InstrumentId instrument_id, Price price=None, AccountId account_id=None):
        """
        Return the total PnL for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the total PnL.
        price : Price, optional
            The reference price for the calculation. This could be the last, mid, bid, ask,
            a mark-to-market price, or any other suitably representative value.
        account_id : AccountId, optional
            The account ID for the total PnL.

        Returns
        -------
        Money or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef Money realized = self.realized_pnl(instrument_id, account_id)
        cdef Money unrealized = self.unrealized_pnl(instrument_id, price, account_id)

        if realized is None and unrealized is None:
            return None

        if realized is None:
            return unrealized

        if unrealized is None:
            return realized

        return Money.from_raw_c(realized._mem.raw + unrealized._mem.raw, realized.currency)

    cpdef Money net_exposure(self, InstrumentId instrument_id, Price price=None, AccountId account_id=None):
        """
        Return the net exposure for the given instrument (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the calculation.
        price : Price, optional
            The reference price for the calculation. This could be the last, mid, bid, ask,
            a mark-to-market price, or any other suitably representative value.
        account_id : AccountId, optional
            The account ID for the net exposure.

        Returns
        -------
        Money or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef instrument = self._cache.instrument(instrument_id)

        if instrument is None:
            self._log.error(
                f"Cannot calculate net exposure: "
                f"no instrument for {instrument_id}",
            )
            return None  # Cannot calculate

        if self._debug:
            self._log.debug(
                f"Calculating net exposure for instrument {instrument_id}", LogColor.MAGENTA,
            )

        cdef list positions_open = self._cache.positions_open(
            venue=None,
            instrument_id=instrument_id,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=account_id,
        )

        if not positions_open:
            return Money(0, instrument.get_cost_currency())

        cdef double net_exposure = 0.0

        cdef:
            Position position
            double xrate
            Money notional_value
            Account account = None
            AccountId prev_account_id = None
            AccountId current_account_id
        for position in positions_open:
            # Get account for this position if account_id changed
            current_account_id = position.account_id
            if prev_account_id != current_account_id:
                account = self._cache.account(current_account_id)
                if account is None:
                    self._log.error(
                        f"Cannot calculate net exposure: "
                        f"no account registered for {current_account_id}",
                    )
                    return None

                prev_account_id = current_account_id

            price = price or self._get_price(position)

            if price is None and not isinstance(instrument, BettingInstrument):
                self._log.error(
                    f"Cannot calculate net exposure: "
                    f"no {self._log_price} for {position.instrument_id}",
                )
                continue  # Cannot calculate

            if self._debug:
                self._log.debug(f"Price for exposure calculation: {price}", LogColor.MAGENTA)

            xrate_result = self._calculate_xrate_to_base(
                instrument=instrument,
                account=account,
                side=position.entry,
            )

            if not xrate_result:
                self._log.error(
                    f"Cannot calculate net exposure: "
                    f"no {self._log_xrate} exchange rate for {instrument.get_cost_currency()}/{account.base_currency}",
                )
                return None  # Cannot calculate

            xrate = xrate_result  # Cast to double

            if self._debug:
                self._log.debug(f"Calculating net exposure for {position}")

            if isinstance(instrument, BettingInstrument):
                bet_position = self._bet_positions.get(position.id)

                if self._debug:
                    self._log.debug(f"{bet_position}", LogColor.MAGENTA)

                net_exposure += float(bet_position.exposure) * xrate if bet_position else 0.0
            else:
                notional_value = instrument.notional_value(position.quantity, price)

                if self._debug:
                    self._log.debug(f"Notional value: {notional_value}", LogColor.MAGENTA)

                net_exposure += notional_value.as_f64_c() * xrate

        # Use account from last position for settlement currency
        if account is not None and self._convert_to_account_base_currency and account.base_currency is not None:
            return Money(net_exposure, account.base_currency)
        else:
            return Money(net_exposure, instrument.get_cost_currency())

    cpdef object net_position(self, InstrumentId instrument_id, AccountId account_id=None):
        """
        Return the net position for the given instrument ID.
        If account_id is provided, returns the net position for that account.
        If account_id is None, aggregates across all accounts.
        If no positions for instrument_id then will return `Decimal('0')`.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the query.
        account_id : AccountId, optional
            The account ID. If None, aggregates across all accounts.

        Returns
        -------
        Decimal

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._net_position(instrument_id, account_id)

    cpdef bint is_net_long(self, InstrumentId instrument_id, AccountId account_id=None):
        """
        Return a value indicating whether the portfolio is net long the given
        instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the query.
        account_id : AccountId, optional
            The account ID. If None, aggregates across all accounts.

        Returns
        -------
        bool
            True if net long, else False.

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._net_position(instrument_id, account_id) > 0.0

    cpdef bint is_net_short(self, InstrumentId instrument_id, AccountId account_id=None):
        """
        Return a value indicating whether the portfolio is net short the given
        instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the query.
        account_id : AccountId, optional
            The account ID. If None, aggregates across all accounts.

        Returns
        -------
        bool
            True if net short, else False.

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._net_position(instrument_id, account_id) < 0.0

    cpdef bint is_flat(self, InstrumentId instrument_id, AccountId account_id=None):
        """
        Return a value indicating whether the portfolio is flat for the given
        instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument query filter.
        account_id : AccountId, optional
            The account ID. If None, aggregates across all accounts.

        Returns
        -------
        bool
            True if net flat, else False.

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._net_position(instrument_id, account_id) == 0.0

    cpdef bint is_completely_flat(self, AccountId account_id=None):
        """
        Return a value indicating whether the portfolio is completely flat.

        Parameters
        ----------
        account_id : AccountId, optional
            The account ID. If None, checks across all accounts.

        Returns
        -------
        bool
            True if net flat across all instruments, else False.

        """
        cdef:
            InstrumentId instrument_id
            AccountId acct_id
            dict account_positions
            object net_position

        for instrument_id, account_positions in self._net_positions.items():
            for acct_id, net_position in account_positions.items():
                # Filter by account_id if provided
                if account_id is not None and acct_id != account_id:
                    continue

                if net_position != Decimal(0):
                    return False

        return True

# -- INTERNAL -------------------------------------------------------------------------------------

    cdef void _update_account(self, AccountState event):
        cdef Account account = self._cache.account(event.account_id)

        if account is None:
            # Generate account
            account = AccountFactory.create_c(event)
            self._cache.add_account(account)
        else:
            account.apply(event)
            self._cache.update_account(account)

        cdef bint should_log = True
        cdef uint64_t ts_last_logged

        if self._min_account_state_logging_interval_ns:
            ts_last_logged = self._last_account_state_log_ts.get(event.account_id, 0)

            if (not ts_last_logged) or (event.ts_init - ts_last_logged) >= self._min_account_state_logging_interval_ns:
                self._last_account_state_log_ts[event.account_id] = event.ts_init
            else:
                should_log = False

        if should_log:
            self._log.info(f"Updated {event}")

    cdef void _update_net_position(self, InstrumentId instrument_id, list positions_open):
        # Update net positions per account for the given instrument.
        cdef:
            dict[AccountId, Decimal] net_positions_by_account = {}
            Position position
            AccountId account_id
            object net_position  # Decimal
            object existing_position  # Decimal
            set[AccountId] accounts_with_positions = set()

        # Calculate net position per account
        for position in positions_open:
            account_id = position.account_id
            accounts_with_positions.add(account_id)
            if account_id not in net_positions_by_account:
                net_positions_by_account[account_id] = Decimal(0)

            net_positions_by_account[account_id] += position.signed_decimal_qty()

        # Ensure instrument entry exists
        if instrument_id not in self._net_positions:
            self._net_positions[instrument_id] = {}

        # Update cache for each account with positions
        for account_id, net_position in net_positions_by_account.items():
            existing_position = self._net_positions[instrument_id].get(account_id, Decimal(0))

            if existing_position != net_position:
                self._net_positions[instrument_id][account_id] = net_position
                self._log.info(f"{instrument_id} account={account_id} net_position={net_position}")

        # Clear cache entries for accounts that no longer have open positions
        cdef list accounts_to_remove = []
        for account_id in self._net_positions[instrument_id].keys():
            if account_id not in accounts_with_positions:
                accounts_to_remove.append(account_id)

        for account_id in accounts_to_remove:
            self._net_positions[instrument_id].pop(account_id, None)

    cdef void _update_instrument_id(self, InstrumentId instrument_id):
        # Invalidate cached PnLs for this instrument (all accounts)
        self._unrealized_pnls.pop(instrument_id, None)

        if self.initialized:
            return

        if instrument_id not in self._pending_calcs:
            return

        cdef list orders_open = self._cache.orders_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
        )
        cdef dict orders_by_account = self._group_by_account_id(orders_open)

        cdef:
            AccountId account_id
            list account_orders
            Account account
            Instrument instrument
            Order o
            bint result_init
            bint result_maint
            list positions_open
            Money result_unrealized_pnl
            bint account_initialized
            list accounts_initialized = []
        for account_id, account_orders in orders_by_account.items():
            account = self._cache.account(account_id)
            if account is None:
                self._log.error(
                    f"Cannot update: no account registered for {account_id=}",
                )
                return  # No account registered

            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot update: no instrument found for {instrument_id}",
                )
                return  # No instrument found

            # Initialize initial (order) margin
            result_init = self._accounts.update_orders(
                account=account,
                instrument=instrument,
                orders_open=[o for o in account_orders if o.is_passive_c()],
                ts_event=account.last_event_c().ts_event,
            )

            result_maint = False
            if account.is_margin_account:
                positions_open = self._cache.positions_open(
                    venue=None,  # Faster query filtering
                    instrument_id=instrument_id,
                    strategy_id=None,
                    side=PositionSide.NO_POSITION_SIDE,
                    account_id=account_id,
                )

                # Initialize maintenance (position) margin
                result_maint = self._accounts.update_positions(
                    account=account,
                    instrument=instrument,
                    positions_open=positions_open,
                    ts_event=account.last_event_c().ts_event,
                )

            # Calculate unrealized PnL
            result_unrealized_pnl = self._calculate_unrealized_pnl(
                instrument_id=instrument_id,
                price=None,
                account_id=account_id
            )

            # Check portfolio initialization
            account_initialized = result_init and (account.is_cash_account or (result_maint and result_unrealized_pnl is not None))
            accounts_initialized.append(account_initialized)

        if all(accounts_initialized):
            self._pending_calcs.discard(instrument_id)

            if not self._pending_calcs:
                self.initialized = True

    cdef dict _group_by_account_id(self, list items):
        # Note: could be a generic function in rust
        cdef dict result = {}
        cdef object item  # Order or Position
        cdef AccountId account_id

        for item in items:
            account_id = item.account_id
            if account_id is not None:
                if account_id not in result:
                    result[account_id] = []

                result[account_id].append(item)

        return result

    cdef Money _aggregate_pnl_from_cache(self, InstrumentId instrument_id, bint is_realized):
        # Aggregate PnL from cache for the given instrument across all accounts.
        # If cache is empty, calculates PnL for all accounts with positions.
        cdef:
            dict pnl_cache = self._realized_pnls if is_realized else self._unrealized_pnls
            str pnl_type = "realized" if is_realized else "unrealized"
            Money total_pnl = None
            Money pnl
            Instrument inst
            dict account_pnls = pnl_cache.get(instrument_id)

        # If nothing in cache for this instrument, calculate PnL
        if account_pnls is None or len(account_pnls) == 0:
            return self._aggregate_pnl_by_calculation(instrument_id, price=None, is_realized=is_realized)

        for pnl in account_pnls.values():
            if pnl is not None:
                total_pnl = self._add_pnl_to_total(total_pnl, pnl, pnl_type)
                if total_pnl is None:
                    return None  # Currency mismatch

        # If cache has entries but total_pnl is None, check if instrument exists and return zero
        if total_pnl is None:
            inst = self._cache.instrument(instrument_id)
            if inst is not None:
                return Money(0, inst.get_cost_currency())

            return None

        return total_pnl

    cdef Money _aggregate_pnl_by_calculation(self, InstrumentId instrument_id, Price price, bint is_realized):
        # Aggregate PnL by finding all accounts with positions and calculating for each.
        # Used when price is provided (fresh calculation) or when aggregating across accounts.
        cdef:
            set[AccountId] account_ids = set()
            list all_positions
            list all_positions_open
            Position position
            AccountId account_id
            Money account_pnl
            Money total_pnl = None
            Instrument inst
            set[PositionId] snapshot_ids
            PositionId position_id

        if is_realized:
            # For realized PnL, check all positions (open and closed) and snapshots
            # Ensure snapshots are cached first so account_ids are available
            self._ensure_snapshot_pnls_cached_for(instrument_id)

            all_positions = self._cache.positions(
                venue=None,
                instrument_id=instrument_id,
                strategy_id=None,
                side=PositionSide.NO_POSITION_SIDE,
                account_id=None,
            )
            for position in all_positions:
                account_ids.add(position.account_id)

            # Also check snapshots for account_ids
            snapshot_ids = self._cache.position_snapshot_ids(instrument_id)
            for position_id in snapshot_ids:
                snapshot_account_id = self._snapshot_account_ids.get(position_id)
                if snapshot_account_id is not None:
                    account_ids.add(snapshot_account_id)
        else:
            # For unrealized PnL, only check open positions
            all_positions_open = self._cache.positions_open(
                venue=None,
                instrument_id=instrument_id,
                strategy_id=None,
                side=PositionSide.NO_POSITION_SIDE,
                account_id=None,
            )
            for position in all_positions_open:
                account_ids.add(position.account_id)

        if not account_ids:
            inst = self._cache.instrument(instrument_id)
            if inst is not None:
                return Money(0, inst.get_cost_currency())

            return None

        # Calculate for each account and sum
        for account_id in account_ids:
            if is_realized:
                account_pnl = self._calculate_realized_pnl(instrument_id, account_id)
            else:
                account_pnl = self._calculate_unrealized_pnl(instrument_id, price, account_id)

            if account_pnl is not None:
                total_pnl = self._add_pnl_to_total(total_pnl, account_pnl, "unrealized" if not is_realized else "realized")
                if total_pnl is None:
                    return None  # Currency mismatch

        return total_pnl

    cdef Money _add_pnl_to_total(self, Money total_pnl, Money pnl, str pnl_type):
        # Add a PnL to a running total, handling currency mismatches.
        # Returns the new total, or None if currency mismatch occurs.
        cdef Currency currency

        if total_pnl is None:
            return pnl
        elif total_pnl.currency == pnl.currency:
            return Money(total_pnl.as_double() + pnl.as_double(), total_pnl.currency)
        else:
            # Currency mismatch - would need conversion, but for now return None
            self._log.warning(
                f"Currency mismatch in aggregated {pnl_type} PnL: "
                f"{total_pnl.currency} vs {pnl.currency}. "
                f"Compute pnl {pnl_type} by account_id instead"
            )
            return None

    cdef Money _get_or_calculate_pnl(
        self,
        InstrumentId instrument_id,
        AccountId account_id,
        bint is_realized,
    ):
        # Get PnL from cache or calculate and cache if missing.
        cdef:
            dict pnl_cache = self._realized_pnls if is_realized else self._unrealized_pnls
            dict account_pnls = pnl_cache.get(instrument_id)
            Money pnl

        if account_pnls is not None:
            pnl = account_pnls.get(account_id)
            if pnl is not None:
                return pnl

        if is_realized:
            pnl = self._calculate_realized_pnl(instrument_id, account_id)
        else:
            pnl = self._calculate_unrealized_pnl(instrument_id, price=None, account_id=account_id)

        if pnl is not None:
            if instrument_id not in pnl_cache:
                pnl_cache[instrument_id] = {}
            pnl_cache[instrument_id][account_id] = pnl

        return pnl

    cdef object _net_position(self, InstrumentId instrument_id, AccountId account_id=None):
        # Get net position for instrument and account. If account_id is None, aggregate across all accounts.
        cdef dict account_positions = self._net_positions.get(instrument_id)

        if account_positions is None:
            return Decimal(0)

        if account_id is not None:
            return account_positions.get(account_id, Decimal(0))

        # Aggregate across all accounts
        return sum(account_positions.values(), Decimal(0))

    cdef Money _calculate_realized_pnl(self, InstrumentId instrument_id, AccountId account_id=None):
        cdef Account account = self._cache.account_for_venue(instrument_id.venue, account_id)

        if account is None:
            self._log.error(
                f"Cannot calculate realized PnL: "
                f"no account registered for {instrument_id.venue}",
            )
            return None  # Cannot calculate

        cdef Instrument instrument = self._cache.instrument(instrument_id)

        if instrument is None:
            self._log.error(
                f"Cannot calculate realized PnL: "
                f"no instrument for {instrument_id}",
            )
            return None  # Cannot calculate

        if self._debug:
            self._log.debug(
                f"Calculating realized PnL for instrument {instrument_id} with {account}", LogColor.MAGENTA,
            )

        cdef Currency currency

        if self._convert_to_account_base_currency and account.base_currency is not None:
            currency = account.base_currency
        else:
            currency = instrument.get_cost_currency()

        self._ensure_snapshot_pnls_cached_for(instrument_id)

        cdef:
            list[Position] positions
            double total_pnl = 0.0
            double xrate
            Position position
            double pnl

        positions = self._cache.positions(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=account_id,
        )

        if self._debug:
            self._log.debug(f"Found {len(positions)} positions for {instrument_id}")

        # Build set of active position IDs for quick lookup
        cdef set[PositionId] active_position_ids = {p.id for p in positions}
        cdef set[PositionId] snapshot_ids = self._cache.position_snapshot_ids(instrument_id)
        cdef set[PositionId] processed_ids = set()

        cdef:
            PositionId position_id
            Money sum_pnl
            Money last_pnl
            double contribution
            AccountId snapshot_account_id

        # Apply the 3-case combination rule for positions with snapshots
        # Filter snapshots to only include those for the requested account
        for position_id in snapshot_ids:
            # Only process snapshots for the requested account
            snapshot_account_id = self._snapshot_account_ids.get(position_id)
            if account_id is not None and snapshot_account_id != account_id:
                continue  # Skip snapshots from other accounts

            sum_pnl = self._snapshot_sum_per_position.get(position_id)
            if sum_pnl is None:
                continue  # No PnL for this position

            contribution = 0.0

            if position_id not in active_position_ids:
                # Case 1: Position NOT in cache - add sum of all snapshots
                contribution = sum_pnl.as_double()
                # Mark as fully processed since position doesn't exist
                processed_ids.add(position_id)
            else:
                # Position is in cache - find it
                position = None

                for p in positions:
                    if p.id == position_id:
                        position = p
                        break

                if position is None:
                    continue  # Should not happen

                if position.is_open_c():
                    # Case 2: Position OPEN - add sum (prior cycles) + position's realized PnL
                    contribution = sum_pnl.as_double()
                    # Position's PnL will be added in the positions loop below
                    # Do NOT mark as processed - we still need to add current PnL
                else:
                    # Case 3: Position CLOSED
                    # If last snapshot equals current position realized PnL, subtract it here;
                    # when we add the position realized below, net effect is `sum`.
                    # If not equal (new closed cycle not snapshotted), include full `sum` here
                    # and add the position realized below (net `sum + realized`).
                    last_pnl = self._snapshot_last_per_position.get(position_id)

                    if (
                        last_pnl is not None
                        and position.realized_pnl is not None
                        and last_pnl.currency == position.realized_pnl.currency
                        and last_pnl == position.realized_pnl
                    ):
                        contribution = sum_pnl.as_double() - last_pnl.as_double()
                    else:
                        contribution = sum_pnl.as_double()
                    # Position's PnL will be added in the positions loop below
                    # Do NOT mark as processed - we still need to add current PnL

            # Add contribution with currency conversion if needed
            if sum_pnl.currency == currency:
                total_pnl += contribution
            else:
                xrate = self._cache.get_xrate(
                    venue=account.id.get_issuer(),
                    from_currency=sum_pnl.currency,
                    to_currency=currency,
                    price_type=PriceType.MID,
                )
                if xrate == 0:
                    return None  # Cannot convert currency
                total_pnl += contribution * xrate

        # Second: Add realized PnL from active positions
        for position in positions:
            if position.instrument_id != instrument_id:
                continue  # Nothing to calculate

            # Skip positions that were already processed via snapshots
            if position.id in processed_ids:
                continue  # Already handled in snapshot logic

            if position.realized_pnl is None:
                continue  # No PnL to add

            if self._debug:
                self._log.debug(f"Adding realized PnL for {position}")

            # Add position's realized PnL
            if isinstance(instrument, BettingInstrument):
                bet_position = self._bet_positions.get(position.id)

                if bet_position is None:
                    self._log.debug(
                        f"Cannot calculate unrealized PnL: no `BetPosition` for {position.id}",
                    )
                    return None  # Cannot calculate

                pnl = float(bet_position.realized_pnl)
            else:
                pnl = position.realized_pnl.as_f64_c()

            if self._convert_to_account_base_currency and account.base_currency is not None:
                xrate_result = self._calculate_xrate_to_base(
                    instrument=instrument,
                    account=account,
                    side=position.entry,
                )

                if not xrate_result:
                    self._log.debug(
                        f"Cannot calculate unrealized PnL: "
                        f"no {self._log_xrate} exchange rate yet for {instrument.get_cost_currency()}/{account.base_currency}",
                    )
                    self._pending_calcs.add(instrument.id)
                    return None  # Cannot calculate

                xrate = xrate_result  # Cast to double
                pnl = round(pnl * xrate, currency.get_precision())

            total_pnl += pnl

        return Money(total_pnl, currency)

    cdef Money _calculate_unrealized_pnl(self, InstrumentId instrument_id, Price price=None, AccountId account_id=None):
        cdef Account account = self._cache.account_for_venue(instrument_id.venue, account_id)

        if account is None:
            self._log.error(
                f"Cannot calculate unrealized PnL: "
                f"no account registered for {instrument_id.venue}",
            )
            return None  # Cannot calculate

        cdef Instrument instrument = self._cache.instrument(instrument_id)

        if instrument is None:
            self._log.error(
                f"Cannot calculate unrealized PnL: "
                f"no instrument for {instrument_id}",
            )
            return None  # Cannot calculate

        if self._debug:
            self._log.debug(
                f"Calculating unrealized PnL for instrument {instrument_id} with {account}", LogColor.MAGENTA,
            )

        cdef Currency currency

        if self._convert_to_account_base_currency and account.base_currency is not None:
            currency = account.base_currency
        else:
            currency = instrument.get_cost_currency()

        cdef list positions_open = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=account_id,
        )

        if not positions_open:
            return Money(0, currency)

        cdef double total_pnl = 0.0

        cdef:
            Position position
            double pnl
            double xrate
        for position in positions_open:
            if position.instrument_id != instrument_id:
                continue  # Nothing to calculate

            if position.side == PositionSide.FLAT:
                continue  # Nothing to calculate

            price = price or self._get_price(position)

            if price is None:
                self._log.debug(
                    f"Cannot calculate unrealized PnL: no {self._log_price} for {instrument_id}",
                )
                self._pending_calcs.add(instrument.id)
                return None  # Cannot calculate

            if self._debug:
                self._log.debug(f"Calculating unrealized PnL for {position}")

            if isinstance(instrument, BettingInstrument):
                bet_position = self._bet_positions.get(position.id)

                if bet_position is None:
                    self._log.debug(
                        f"Cannot calculate unrealized PnL: no `BetPosition` for {position.id}",
                    )
                    return None  # Cannot calculate

                pnl = float(bet_position.unrealized_pnl(price.as_decimal()))
            else:
                pnl = position.unrealized_pnl(price).as_f64_c()

            if self._debug:
                self._log.debug(
                    f"Unrealized PnL for {instrument.id}: {pnl} {currency}", LogColor.MAGENTA,
                )

            if self._convert_to_account_base_currency and account.base_currency is not None:
                xrate_result = self._calculate_xrate_to_base(
                    instrument=instrument,
                    account=account,
                    side=position.entry,
                )

                if not xrate_result:
                    self._log.debug(
                        f"Cannot calculate unrealized PnL: "
                        f"no {self._log_xrate} exchange rate for {instrument.get_cost_currency()}/{account.base_currency}",
                    )
                    self._pending_calcs.add(instrument.id)
                    return None  # Cannot calculate

                xrate = xrate_result  # Cast to double
                pnl = round(pnl * xrate, currency.get_precision())

            total_pnl += pnl

        return Money(total_pnl, currency)

    cdef Price _get_price(self, Position position):
        cdef PriceType price_type

        if self._use_mark_prices:
            price_type = PriceType.MARK
        elif position.side == PositionSide.FLAT:
            price_type = PriceType.LAST
        elif position.side == PositionSide.LONG:
            price_type = PriceType.BID
        elif position.side == PositionSide.SHORT:
            price_type = PriceType.ASK
        else:  # pragma: no cover (design-time error)
            raise RuntimeError(
                f"invalid `PositionSide`, was {position_side_to_str(position.side)}",
            )

        cdef InstrumentId instrument_id = position.instrument_id

        return self._cache.price(
            instrument_id=instrument_id,
            price_type=price_type,
        ) or self._cache.price(
            instrument_id=instrument_id,
            price_type=PriceType.LAST,
        ) or self._bar_close_prices.get(instrument_id)

    cdef _calculate_xrate_to_base(self, Account account, Instrument instrument, OrderSide side):
        if not self._convert_to_account_base_currency or account.base_currency is None:
            return 1.0  # No conversion needed

        if self._use_mark_xrates:
            return self._cache.get_mark_xrate(
                from_currency=instrument.get_cost_currency(),
                to_currency=account.base_currency,
            )

        return self._cache.get_xrate(
            venue=instrument.id.venue,
            from_currency=instrument.get_cost_currency(),
            to_currency=account.base_currency,
            price_type=PriceType.BID if side == OrderSide.BUY else PriceType.ASK,
        )
