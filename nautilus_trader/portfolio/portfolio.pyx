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
from nautilus_trader.core.rust.model cimport FIXED_PRECISION
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
from nautilus_trader.model.functions cimport price_type_to_str
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.betting cimport BettingInstrument
from nautilus_trader.model.instruments.betting cimport order_side_to_bet_side
from nautilus_trader.model.instruments.crypto_perpetual cimport CryptoPerpetual
from nautilus_trader.model.instruments.currency_pair cimport CurrencyPair
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

        cdef Instrument instrument
        if self._use_mark_xrates:
            instrument = self._cache.instrument(tick.instrument_id)
            if isinstance(instrument, (CurrencyPair, CryptoPerpetual)):
                self._update_mark_xrate(
                    instrument=instrument,
                    xrate=(tick.bid_price.as_double() + tick.ask_price.as_double()) / 2.0,
                    instrument_id=tick.instrument_id,
                )

    cpdef void update_mark_price(self, object mark_price):
        """
        Update the portfolio with the given mark price.
        """
        Condition.not_none(mark_price, "mark_price")

        self._update_instrument_id(mark_price.instrument_id)

        cdef Instrument instrument
        if self._use_mark_xrates:
            instrument = self._cache.instrument(mark_price.instrument_id)
            if isinstance(instrument, (CurrencyPair, CryptoPerpetual)):
                self._update_mark_xrate(
                    instrument=instrument,
                    xrate=(<Price>mark_price.value).as_double(),
                    instrument_id=mark_price.instrument_id,
                )

    cdef void _update_mark_xrate(self, Instrument instrument, double xrate, InstrumentId instrument_id):
        if xrate > 0:
            self._cache.set_mark_xrate(
                from_currency=instrument.base_currency,
                to_currency=instrument.quote_currency,
                xrate=xrate,
            )
        else:
            self._log.debug(f"Skipping mark xrate update for {instrument_id}: zero price")

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

        cdef:
            Account account
            Instrument instrument
            PositionId position_id

        account, instrument = self._validate_event_account_and_instrument(event, "order")
        if account is None or instrument is None:
            return

        if not account.calculate_account_state:
            return  # Nothing to calculate

        if not isinstance(event, _UPDATE_ORDER_EVENTS):
            return  # No change to account state

        cdef Order order = self._cache.order(event.client_order_id)

        # Allow OrderFilled events to proceed even without order in cache
        # (e.g., leg fills from spread orders or option exercise)
        # Balance updates only need the fill and position, not the order
        if order is None and not isinstance(event, OrderFilled):
            self._log.error(
                f"Cannot update order: "
                f"{event.client_order_id!r} not found in the cache",
            )
            return  # No order found

        if isinstance(event, OrderRejected) and order.order_type != OrderType.STOP_LIMIT:
            return  # No change to account state

        if self._debug:
            self._log.debug(f"Updating with {order!r}", LogColor.MAGENTA)

        cdef Money unrealized_pnl
        if isinstance(event, OrderFilled):
            # Skip balance updates for spread instrument fills (combo fills)
            # Spread instruments don't create positions, and only leg fills should update balances
            # to avoid double-counting (combo fill + leg fills)
            if not instrument.is_spread():
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

        # Invalidate cached PnLs for this instrument and account
        # For realized PnL, also check if this is a new position cycle (NETTING OMS)
        # that would affect all accounts, not just this one
        cdef:
            bint invalidate_all_accounts = False
            Position updated_position
            set[PositionId] snapshot_ids
            Money last_snapshot_pnl

        # Check if this position update represents a new cycle (closed position with realized_pnl)
        # For NETTING OMS, if a position is closed and has snapshots, it might be a new cycle
        updated_position = self._cache.position(event.position_id)
        if updated_position is not None and updated_position.is_closed_c() and updated_position.realized_pnl is not None:
            # Ensure snapshot data is cached so we can compare
            self._ensure_snapshot_pnls_cached_for(event.instrument_id)

            # Check if this position_id has snapshots
            snapshot_ids = self._cache.position_snapshot_ids(event.instrument_id)
            if event.position_id in snapshot_ids:
                # Compare with last snapshot PnL - if different, it's a new cycle
                last_snapshot_pnl = self._snapshot_last_per_position.get(event.position_id)
                if last_snapshot_pnl is not None and updated_position.realized_pnl is not None:
                    if (
                        updated_position.realized_pnl.currency != last_snapshot_pnl.currency
                        or updated_position.realized_pnl != last_snapshot_pnl
                    ):
                        # New cycle detected - invalidate all accounts for this instrument
                        invalidate_all_accounts = True
                else:
                    # Position has snapshots but we don't have last_pnl cached yet
                    # This could be a new cycle - invalidate to be safe
                    invalidate_all_accounts = True

        if invalidate_all_accounts:
            # Invalidate all accounts for this instrument (new cycle affects all)
            self._realized_pnls.pop(event.instrument_id, None)
        else:
            # Invalidate only this account
            if event.instrument_id in self._realized_pnls:
                self._realized_pnls[event.instrument_id].pop(event.account_id, None)

        if event.instrument_id in self._unrealized_pnls:
            self._unrealized_pnls[event.instrument_id].pop(event.account_id, None)

        cdef:
            Account account
            Instrument instrument
        account, instrument = self._validate_event_account_and_instrument(event, "position")
        if account is None or instrument is None:
            return

        if account.type != AccountType.MARGIN or not account.calculate_account_state:
            return  # Nothing to calculate

        # Fetch positions filtered by account_id for account-specific update
        cdef list account_positions = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=event.instrument_id,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=event.account_id,  # Filter by account_id
        )

        cdef AccountState account_state
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
        return self._get_account(venue, account_id, "account", "venue or account_id must be provided")

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
        cdef Account account = self._get_account(venue, account_id, "balances locked", "'venue' or 'account_id' must be provided")

        return account.balances_locked() if account is not None else None

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
        cdef Account account = self._get_account(venue, account_id, "initial (order) margins", "'venue' or 'account_id' must be provided")
        if account is None or account.is_cash_account:
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
        cdef Account account = self._get_account(venue, account_id, "maintenance (position) margins", "'venue' or 'account_id' must be provided")
        if account is None or account.is_cash_account:
            return None

        return account.margins_maint()

    cpdef dict realized_pnls(self, Venue venue=None, AccountId account_id=None, Currency target_currency=None):
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
        target_currency : Currency, optional
            The currency to convert the PnLs into.

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

        return self._aggregate_pnls_by_instrument(positions, True, account_id, target_currency)

    cpdef dict unrealized_pnls(self, Venue venue=None, AccountId account_id=None, Currency target_currency=None):
        """
        Return the unrealized PnLs for the given venue (if found).

        Parameters
        ----------
        venue : Venue, optional
            The venue for the unrealized PnLs.
        account_id : AccountId, optional
            The account ID for the unrealized PnLs.
        target_currency : Currency, optional
            The currency to convert the PnLs into.

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

        return self._aggregate_pnls_by_instrument(positions_open, False, account_id, target_currency)

    cdef dict _aggregate_pnls_by_instrument(self, list positions, bint is_realized, AccountId account_id, Currency target_currency):
        if not positions:
            return {}  # Nothing to calculate

        cdef set[InstrumentId] instrument_ids = {p.instrument_id for p in positions}
        cdef dict[Currency, double] aggregated_pnls = {}
        cdef:
            InstrumentId instrument_id
            Money pnl
        for instrument_id in instrument_ids:
            if is_realized:
                pnl = self.realized_pnl(instrument_id, account_id, target_currency=target_currency)
            else:
                pnl = self.unrealized_pnl(instrument_id, price=None, account_id=account_id, target_currency=target_currency)

            if pnl is None:
                continue

            aggregated_pnls[pnl.currency] = aggregated_pnls.get(pnl.currency, 0.0) + pnl.as_f64_c()

        return {k: Money(v, k) for k, v in aggregated_pnls.items()}

    cpdef dict total_pnls(self, Venue venue=None, AccountId account_id=None, Currency target_currency=None):
        """
        Return the total PnLs for the given venue (if found).

        Parameters
        ----------
        venue : Venue, optional
            The venue for the total PnLs.
        account_id : AccountId, optional
            The account ID for the total PnLs.
        target_currency : Currency, optional
            The currency to convert the PnLs into.

        Returns
        -------
        dict[Currency, Money]

        """
        cdef dict realized = self.realized_pnls(venue, account_id, target_currency=target_currency)
        cdef dict unrealized = self.unrealized_pnls(venue, account_id, target_currency=target_currency)

        cdef:
            double total_amount = 0.0
            dict[Currency, double] total_pnls = {}
            Currency currency
            Money amount
        if target_currency is not None:
            # When target_currency is provided, both dicts should contain only that currency
            # Sum them together. If neither dict contains target_currency (e.g., conversion failed
            # for all instruments or no positions exist), total_amount remains 0.0, which is correct.
            if target_currency in realized:
                total_amount += realized[target_currency].as_double()

            if target_currency in unrealized:
                total_amount += unrealized[target_currency].as_double()

            # Return dict with target_currency (0.0 if no PnL found, which represents zero total PnL)
            return {target_currency: Money(total_amount, target_currency)}

        # No target_currency: aggregate by currency
        # Sum realized PnLs
        for currency, amount in realized.items():
            total_pnls[currency] = amount.as_double()

        # Add unrealized PnLs
        for currency, amount in unrealized.items():
            total_pnls[currency] = total_pnls.get(currency, 0.0) + amount.as_double()

        return {k: Money(v, k) for k, v in total_pnls.items()}

    cpdef dict net_exposures(self, Venue venue=None, AccountId account_id=None, Currency target_currency=None):
        """
        Return the net exposures for the given venue (if found).

        Parameters
        ----------
        venue : Venue, optional
            The venue for the market value.
        account_id : AccountId, optional
            The account ID for the net exposures.
        target_currency : Currency, optional
            The currency to convert the exposures into.

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

        cdef set[AccountId] involved_accounts = set()
        cdef Position pos
        for pos in positions_open:
            if not pos.is_closed_c():
                involved_accounts.add(pos.account_id)

        # Prevent silent currency mixing across accounts with different base currencies
        cdef:
            set[Currency] base_currencies = set()
            AccountId involved_account_id
            Account involved_account
            list[str] currency_strs
            Currency currency
            str currencies_str
        if target_currency is None and account_id is None and len(involved_accounts) > 1:
            for involved_account_id in involved_accounts:
                involved_account = self._cache.account(involved_account_id)
                if involved_account is not None and involved_account.base_currency is not None:
                    base_currencies.add(involved_account.base_currency)

            if len(base_currencies) > 1:
                currency_strs = []
                for currency in base_currencies:
                    currency_strs.append(str(currency))

                currencies_str = ", ".join(currency_strs)
                self._log.error(
                    f"Cannot calculate net exposures: multiple accounts with different base currencies "
                    f"({currencies_str}). "
                    f"Provide an explicit target_currency to aggregate across accounts."
                )
                return None

        cdef:
            dict[Currency, double] net_exposures = {}
            Position position
            Money exposure
            set[InstrumentId] processed_instruments = set()
        for position in positions_open:
            # Skip closed positions (they should not contribute to net_exposures)
            if position.is_closed_c():
                continue

            if position.instrument_id in processed_instruments:
                continue

            processed_instruments.add(position.instrument_id)

            account = self._cache.account(position.account_id)
            current_target = target_currency or (account.base_currency if account else None)

            exposure = self.net_exposure(
                position.instrument_id,
                price=None,
                account_id=account_id,
                target_currency=current_target,
            )
            if exposure is None or exposure.as_f64_c() == 0.0:
                continue

            net_exposures[exposure.currency] = net_exposures.get(exposure.currency, 0.0) + exposure.as_f64_c()

        if not net_exposures:
            return {}

        # If target_currency is provided, all exposures should be in that currency
        if target_currency is not None:
            if target_currency in net_exposures:
                return {target_currency: Money(net_exposures[target_currency], target_currency)}
            else:
                # This shouldn't happen if conversion worked, but handle gracefully
                return {}

        return {k: Money(v, k) for k, v in net_exposures.items()}

    cpdef Money realized_pnl(self, InstrumentId instrument_id, AccountId account_id=None, Currency target_currency=None):
        """
        Return the realized PnL for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the realized PnL.
        account_id : AccountId, optional
            The account ID for the realized PnL. If None, aggregates across all accounts.
        target_currency : Currency, optional
            The currency to convert the PnL into.

        Returns
        -------
        Money or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        if account_id is not None:
            # Single account: check cache and calculate if needed
            native_pnl = self._calculate_realized_pnl(instrument_id, account_id)
            if native_pnl is None:
                return None

            return self._convert_money_if_needed(native_pnl, target_currency, venue=instrument_id.venue)
        else:
            # Aggregate across all accounts using cache where possible
            return self._aggregate_pnl_from_cache(instrument_id, is_realized=True, target_currency=target_currency)

    cpdef Money unrealized_pnl(self, InstrumentId instrument_id, Price price=None, AccountId account_id=None, Currency target_currency=None):
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
        target_currency : Currency, optional
            The currency to convert the PnL into.

        Returns
        -------
        Money or ``None``
            The unrealized PnL or None if the calculation cannot be performed.

        """
        Condition.not_none(instrument_id, "instrument_id")

        if price is not None or account_id is not None:
            # Fresh calculation or single account
            native_pnl = self._calculate_unrealized_pnl(instrument_id, price, account_id)
            if native_pnl is None:
                return None

            return self._convert_money_if_needed(native_pnl, target_currency, venue=instrument_id.venue)
        else:
            # Aggregate across all accounts using cache where possible
            return self._aggregate_pnl_from_cache(instrument_id, is_realized=False, target_currency=target_currency)

    cpdef Money total_pnl(self, InstrumentId instrument_id, Price price=None, AccountId account_id=None, Currency target_currency=None):
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
        target_currency : Currency, optional
            The currency to convert the PnL into.

        Returns
        -------
        Money or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef Money realized = self.realized_pnl(instrument_id, account_id, target_currency=target_currency)
        cdef Money unrealized = self.unrealized_pnl(instrument_id, price, account_id, target_currency=target_currency)
        if realized is None and unrealized is None:
            return None

        if realized is None:
            return unrealized

        if unrealized is None:
            return realized

        # Both should be in the same currency when target_currency is provided
        # If not provided, they should still match due to convert_to_account_base_currency logic
        cdef:
            Money converted_realized
            Money converted_unrealized
        if realized.currency != unrealized.currency:
            # This shouldn't happen with target_currency, but handle gracefully
            if target_currency is not None:
                # Try to convert both to target_currency
                converted_realized = self._convert_money(realized, target_currency, venue=instrument_id.venue)
                converted_unrealized = self._convert_money(unrealized, target_currency, venue=instrument_id.venue)
                if converted_realized is not None and converted_unrealized is not None:
                    return Money.from_raw_c(converted_realized._mem.raw + converted_unrealized._mem.raw, target_currency)

                return None

            # Without target_currency, this is a currency mismatch error
            self._log.warning(
                f"Currency mismatch in total_pnl: {realized.currency} vs {unrealized.currency}. "
                f"Provide target_currency to convert both to a common currency."
            )
            return None

        return Money.from_raw_c(realized._mem.raw + unrealized._mem.raw, realized.currency)

    cpdef Money net_exposure(self, InstrumentId instrument_id, Price price=None, AccountId account_id=None, Currency target_currency=None):
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
        target_currency : Currency, optional
            The currency to convert the exposure into.

        Returns
        -------
        Money or ``None``

        """
        cdef list positions = self._cache.positions_open(
            venue=None,
            instrument_id=instrument_id,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=account_id,
        )

        # Validate consistent base currency across accounts when aggregating
        # (only needed if no target_currency is provided, as we can convert to target_currency)
        cdef:
            set[AccountId] account_ids
            Currency first_base_currency = None
            Account account
            AccountId acc_id
            Position position

        if account_id is None and target_currency is None and positions:
            account_ids = set()

            # Collect unique account IDs from positions
            for position in positions:
                if position.account_id is not None:
                    account_ids.add(position.account_id)

            # Validate accounts from cache
            for acc_id in account_ids:
                account = self._cache.account(acc_id)
                if account is not None:
                    if first_base_currency is None:
                        first_base_currency = account.base_currency
                    elif account.base_currency is not None and account.base_currency != first_base_currency:
                        self._log.error(
                            f"Cannot calculate net exposure: "
                            f"accounts have different base currencies "
                            f"({first_base_currency} vs {account.base_currency}); "
                            f"multi-account aggregation requires consistent base currencies",
                        )
                        return None

        cdef Instrument instrument_obj = self._cache.instrument(instrument_id)
        if instrument_obj is None:
            return None

        cdef:
            Currency settlement_currency = instrument_obj.get_cost_currency()
            bint is_betting = isinstance(instrument_obj, BettingInstrument)
            bint used_cross_notional = False
            double total_notional
            PriceType price_type
        if is_betting:
            total_notional = self._handle_betting_instrument_exposure(positions, instrument_obj, target_currency, settlement_currency)
            if total_notional is None:
                return None

            price_type = PriceType.MARK
        else:
            total_notional, price_type, used_cross_notional = self._calculate_non_betting_exposure(
                positions=positions,
                instrument_id=instrument_id,
                instrument_obj=instrument_obj,
                price=price,
                target_currency=target_currency,
            )
            if total_notional is None:
                return None

        # Finalize the exposure result with currency conversion
        if not is_betting:
            total_notional = abs(total_notional)

        # If we used cross_notional_value, the result is already in target_currency
        if used_cross_notional and target_currency is not None:
            return Money(total_notional, target_currency)

        cdef Money exposure = Money(total_notional, settlement_currency)

        # Early return for zero exposure (always convertible)
        if target_currency is not None and total_notional == 0.0:
            return Money(0.0, target_currency)

        return self._convert_money_if_needed(
            exposure,
            target_currency,
            venue=instrument_id.venue,
            price_type=price_type,
        )

    cdef double _handle_betting_instrument_exposure(
        self,
        list positions,
        Instrument instrument_obj,
        Currency target_currency,
        Currency settlement_currency,
    ):
        # Handle exposure calculation for betting instruments
        cdef:
            double total_notional = 0.0
            Position position
            object bet_position

        if not positions:
            # No open positions - return zero exposure
            # (Closed positions should not contribute to net exposure)
            return 0.0

        # Sum exposures from all bet positions for open positions
        for position in positions:
            bet_position = self._get_bet_position(position, instrument_obj)
            if bet_position is not None:
                # Use exposure directly (it's already signed correctly)
                total_notional += float(bet_position.exposure)

        return total_notional

    cdef tuple _calculate_non_betting_exposure(
        self,
        list positions,
        InstrumentId instrument_id,
        Instrument instrument_obj,
        Price price,
        Currency target_currency,
    ):
        # Calculate exposure for non-betting instruments
        cdef:
            double total_notional = 0.0
            PriceType price_type = PriceType.MARK  # Default for conversion
            Position position
            double val
            bint used_cross_notional = False

        if not positions:
            if instrument_obj is not None:
                return (0.0, price_type, False)

            return (None, price_type, False)

        cdef bint is_currency_pair = isinstance(instrument_obj, CurrencyPair)
        cdef bint has_long = False
        cdef bint has_short = False

        for position in positions:
            val, used_cross = self._calculate_position_exposure_value(
                position=position,
                instrument_obj=instrument_obj,
                instrument_id=instrument_id,
                price=price,
                target_currency=target_currency,
                is_currency_pair=is_currency_pair,
                positions=positions,
            )
            if val is None:
                return (None, price_type, False)

            if used_cross:
                used_cross_notional = True

            if position.side == PositionSide.LONG:
                has_long = True
                total_notional += val

                if not price:  # Only override if we used _get_price
                    price_type = PriceType.BID
            elif position.side == PositionSide.SHORT:
                has_short = True
                total_notional -= val

                if not price:  # Only override if we used _get_price
                    price_type = PriceType.ASK

        # Use neutral pricing when positions are mixed (both long and short)
        if has_long and has_short and not price:
            price_type = PriceType.MARK if self._use_mark_xrates else PriceType.MID

        return (total_notional, price_type, used_cross_notional)

    cdef tuple _calculate_position_exposure_value(
        self,
        Position position,
        Instrument instrument_obj,
        InstrumentId instrument_id,
        Price price,
        Currency target_currency,
        bint is_currency_pair,
        list positions,
    ):
        # Calculate exposure value for a single position
        cdef:
            Price p = price or self._get_price(position)
            object val_result
            double val
            Money exposure_money
            bint used_cross = False

        if p is None:
            self._log.debug(f"Cannot calculate net exposure: no price for {position.instrument_id}")
            return (None, False)

        # For CurrencyPair with target_currency, use cross_notional_value for accurate conversion
        if is_currency_pair and target_currency is not None:
            val_result = self._calculate_currency_pair_exposure(
                position=position,
                instrument_obj=instrument_obj,
                instrument_id=instrument_id,
                price=p,
                target_currency=target_currency,
                positions=positions,
                price_param=price,
            )
            if val_result is not None:
                val = <double>val_result
                used_cross = True
            else:
                # Fall back to standard conversion if required rates are missing
                exposure_money = position.notional_value(p)
                val = exposure_money.as_f64_c()
        else:
            # Standard path: get notional value and convert if needed
            exposure_money = position.notional_value(p)
            val = exposure_money.as_f64_c()

        return (val, used_cross)

    cdef object _calculate_currency_pair_exposure(
        self,
        Position position,
        Instrument instrument_obj,
        InstrumentId instrument_id,
        Price price,
        Currency target_currency,
        list positions,
        Price price_param,
    ):
        # Calculate exposure for currency pair using cross_notional_value when possible
        cdef:
            PriceType conv_price_type = PriceType.MID
            CurrencyPair currency_pair = <CurrencyPair>instrument_obj
            object quote_xrate = None
            object base_xrate = None
            bint can_use_cross = False
            Money exposure_money

        # Determine price type for conversion lookups
        if not price_param:  # Only override if we used _get_price
            if position.side == PositionSide.LONG:
                conv_price_type = PriceType.BID
            elif position.side == PositionSide.SHORT:
                conv_price_type = PriceType.ASK

        # If mixed positions, MARK is probably better for conversion
        if len(positions) > 1 and not price_param:
            conv_price_type = PriceType.MARK if self._use_mark_prices else conv_price_type

        # Try mark xrates first if enabled
        if self._use_mark_xrates:
            quote_xrate = self._cache.get_mark_xrate(currency_pair.quote_currency, target_currency)
            base_xrate = self._cache.get_mark_xrate(currency_pair.base_currency, target_currency)

        # Fallback to standard xrate lookup
        if quote_xrate is None:
            quote_xrate = self._cache.get_xrate(
                venue=instrument_id.venue,
                from_currency=currency_pair.quote_currency,
                to_currency=target_currency,
                price_type=conv_price_type,
            )

        if base_xrate is None:
            base_xrate = self._cache.get_xrate(
                venue=instrument_id.venue,
                from_currency=currency_pair.base_currency,
                to_currency=target_currency,
                price_type=conv_price_type,
            )

        # For non-inverse pairs, we only need quote_price (base_price is ignored)
        # For inverse pairs, we only need base_price (quote_price is ignored)
        # If we have the required rate, use cross_notional_value
        if not position.is_inverse:
            # Non-inverse: need quote_price
            if quote_xrate is not None and quote_xrate > 0.0:
                can_use_cross = True

                # Use dummy base_price since it won't be used
                if base_xrate is None or base_xrate <= 0.0:
                    base_xrate = 1.0
        else:
            # Inverse: need base_price
            if base_xrate is not None and base_xrate > 0.0:
                can_use_cross = True

                # Use dummy quote_price since it won't be used
                if quote_xrate is None or quote_xrate <= 0.0:
                    quote_xrate = 1.0

        if can_use_cross:
            # Use cross_notional_value for accurate FX conversion
            exposure_money = position.cross_notional_value(
                price=price,
                quote_price=Price(<double>quote_xrate, FIXED_PRECISION),
                base_price=Price(<double>base_xrate, FIXED_PRECISION),
                target_currency=target_currency,
            )
            return exposure_money.as_f64_c()

        return None  # Cannot use cross_notional, fall back to standard

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

    cdef object _net_position(self, InstrumentId instrument_id, AccountId account_id=None):
        # Get net position for instrument and account. If account_id is None, aggregate across all accounts.
        cdef dict account_positions = self._net_positions.get(instrument_id)
        if account_positions is None:
            return Decimal(0)

        if account_id is not None:
            return account_positions.get(account_id, Decimal(0))

        # Aggregate across all accounts
        return sum(account_positions.values(), Decimal(0))

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
            dict account_dict
            AccountId acc_id
            object net_position
        for instrument_id, account_dict in self._net_positions.items():
            for acc_id, net_position in account_dict.items():
                # Filter by account_id if provided
                if account_id is not None and acc_id != account_id:
                    continue

                if net_position != Decimal(0):
                    return False

        return True

# -- INTERNAL -------------------------------------------------------------------------------------

    cdef tuple _validate_event_account_and_instrument(self, object event, str caller_name):
        if event.account_id is None:
            return None, None

        cdef Account account = self._cache.account(event.account_id)
        if account is None:
            self._log.error(
                f"Cannot update {caller_name}: "
                f"no account registered for {event.account_id}",
            )
            return None, None

        cdef Instrument instrument = self._cache.instrument(event.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot update {caller_name}: "
                f"no instrument found for {event.instrument_id}",
            )
            return None, None

        return account, instrument

    cdef void _update_account(self, AccountState event):
        cdef Account account = self._cache.account(event.account_id)
        if account is None:
            # Generate account
            account = AccountFactory.create_c(event)
            self._cache.add_account(account)
        else:
            account.apply(event)
            self._cache.update_account(account)

        cdef:
            bint should_log = True
            uint64_t ts_last_logged
        if self._min_account_state_logging_interval_ns:
            ts_last_logged = self._last_account_state_log_ts.get(event.account_id, 0)
            if (not ts_last_logged) or (event.ts_init - ts_last_logged) >= self._min_account_state_logging_interval_ns:
                self._last_account_state_log_ts[event.account_id] = event.ts_init
            else:
                should_log = False

        if should_log:
            self._log.info(f"Updated {event}")

    cdef Account _get_account(self, Venue venue, AccountId account_id, str caller_name, str message=None):
        Condition.not_none(venue or account_id, message or "'venue' or 'account_id' must be provided")

        cdef Account account = self._cache.account_for_venue(venue, account_id)
        if account is None:
            self._log.error(
                f"Cannot get {caller_name}: "
                f"no account registered for {venue=} and {account_id=}",
            )

        return account

    cdef void _update_net_position(self, InstrumentId instrument_id, list positions_open):
        # Update net positions per account for the given instrument.
        cdef:
            dict[AccountId, Decimal] net_positions_by_account = {}
            Position position
            AccountId account_id
            object net_position  # Decimal
            set[AccountId] accounts_with_positions = set()
            list accounts_to_remove

        # Calculate net position per account
        for position in positions_open:
            account_id = position.account_id
            accounts_with_positions.add(account_id)
            net_positions_by_account[account_id] = (
                net_positions_by_account.get(account_id, Decimal(0)) + position.signed_decimal_qty()
            )

        # Ensure instrument entry exists
        self._net_positions.setdefault(instrument_id, {})

        # Update cache for each account with positions
        for account_id, net_position in net_positions_by_account.items():
            existing_position = self._net_positions[instrument_id].get(account_id, Decimal(0))
            if existing_position != net_position:
                self._net_positions[instrument_id][account_id] = net_position
                self._log.info(f"{instrument_id} account={account_id} net_position={net_position}")

        # Clear cache entries for accounts that no longer have open positions
        if instrument_id in self._net_positions:
            accounts_to_remove = []
            for acc_id in self._net_positions[instrument_id].keys():
                if acc_id not in accounts_with_positions:
                    accounts_to_remove.append(acc_id)

            for acc_id in accounts_to_remove:
                self._net_positions[instrument_id].pop(acc_id, None)

            # Remove instrument_id entry if empty
            if not self._net_positions[instrument_id]:
                self._net_positions.pop(instrument_id, None)

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
        cdef:
            dict result = {}
            object item  # Order or Position
            AccountId account_id
        for item in items:
            account_id = item.account_id
            if account_id is not None:
                result.setdefault(account_id, []).append(item)

        return result

    cdef Money _aggregate_pnl_from_cache(self, InstrumentId instrument_id, bint is_realized, Currency target_currency=None):
        # Aggregate PnL from cache for the given instrument across all accounts.
        # If cache is empty, calculates PnL for all accounts with positions.
        cdef:
            dict pnl_cache = self._realized_pnls if is_realized else self._unrealized_pnls
            str pnl_type = "realized" if is_realized else "unrealized"
            Money total_pnl = None
            Money pnl
            dict account_pnls

        # For realized PnL, ensure snapshots are processed (which also invalidates PnL cache if needed)
        if is_realized:
            self._ensure_snapshot_pnls_cached_for(instrument_id)

        account_pnls = pnl_cache.get(instrument_id)

        # If nothing in cache for this instrument, calculate PnL
        if account_pnls is None or len(account_pnls) == 0:
            return self._aggregate_pnl_by_calculation(instrument_id, price=None, is_realized=is_realized, target_currency=target_currency)

        for pnl in account_pnls.values():
            if pnl is not None:
                total_pnl = self._add_pnl_to_total(total_pnl, pnl, pnl_type, venue=instrument_id.venue, target_currency=target_currency)
                if total_pnl is None:
                    return None  # Currency mismatch

        # If cache has entries but total_pnl is None, check if instrument exists and return zero
        if total_pnl is None:
            return self._get_zero_or_none_for_instrument(instrument_id, target_currency=target_currency)

        return total_pnl

    cdef Money _aggregate_pnl_by_calculation(self, InstrumentId instrument_id, Price price, bint is_realized, Currency target_currency=None):
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
            return self._get_zero_or_none_for_instrument(instrument_id, target_currency=target_currency)

        # Get the appropriate cache dictionary
        cdef dict pnl_cache = self._realized_pnls if is_realized else self._unrealized_pnls

        # Determine if we should cache: only cache when price is None (use current market price)
        # If price is provided, it's a fresh calculation with a specific price, so don't cache
        cdef bint should_cache = (price is None)

        # Calculate for each account and sum
        # Always calculate in native currency first for caching, then convert if needed
        cdef bint attempted_calculation = False
        cdef bint any_conversion_failed = False
        cdef Money native_pnl
        for account_id in account_ids:
            # Calculate in native currency for caching
            if is_realized:
                native_pnl = self._calculate_realized_pnl(instrument_id, account_id)
            else:
                native_pnl = self._calculate_unrealized_pnl(instrument_id, price, account_id)

            attempted_calculation = True

            # Cache the native currency PnL (only if using current market price, not a specific price)
            if native_pnl is not None:
                if should_cache:
                    pnl_cache.setdefault(instrument_id, {})[account_id] = native_pnl

                # Convert to target_currency if needed for aggregation
                if target_currency is not None:
                    account_pnl = self._convert_money_if_needed(native_pnl, target_currency, venue=instrument_id.venue)
                    if account_pnl is None:
                        any_conversion_failed = True
                        self._log.error(
                            f"Cannot aggregate PnL: conversion failed for account {account_id} "
                            f"from {native_pnl.currency} to {target_currency}"
                        )
                else:
                    account_pnl = native_pnl

                if account_pnl is not None:
                    total_pnl = self._add_pnl_to_total(total_pnl, account_pnl, "unrealized" if not is_realized else "realized", venue=instrument_id.venue, target_currency=target_currency)

        # Return None if any conversion failed (prevents partial totals)
        if any_conversion_failed:
            return None

        if total_pnl is None:
            # If we attempted calculations and all returned None (e.g., conversion failed),
            # and target_currency was provided, return None to indicate conversion failure.
            # Otherwise, return zero (no positions or no PnL).
            if attempted_calculation and target_currency is not None:
                return None

            return self._get_zero_or_none_for_instrument(instrument_id, target_currency=target_currency)

        return total_pnl

    cdef Money _add_pnl_to_total(self, Money total_pnl, Money pnl, str pnl_type, Venue venue=None, Currency target_currency=None):
        # Add a PnL to a running total, handling currency mismatches.
        # Returns the new total, or None if currency mismatch occurs.
        if total_pnl is None:
            return self._convert_money_if_needed(pnl, target_currency, venue=venue)
        elif total_pnl.currency == pnl.currency:
            return Money(total_pnl.as_double() + pnl.as_double(), total_pnl.currency)
        else:
            if target_currency is not None:
                # This should not happen if pnl was already converted, but just in case
                if pnl.currency != target_currency:
                    pnl = self._convert_money(pnl, target_currency, venue=venue)

                if pnl is None:
                    return total_pnl

                return Money(total_pnl.as_double() + pnl.as_double(), target_currency)

            # Currency mismatch - would need conversion, but for now return None
            self._log.warning(
                f"Currency mismatch in aggregated {pnl_type} PnL: "
                f"{total_pnl.currency} vs {pnl.currency}. "
                f"Compute pnl {pnl_type} by account_id instead"
            )
            return None

    cdef Money _get_zero_or_none_for_instrument(self, InstrumentId instrument_id, Currency target_currency=None):
        # Helper method to return appropriate zero or None for missing instruments
        cdef Instrument inst = self._cache.instrument(instrument_id)
        if inst is not None:
            # If target_currency is provided, we can always return a zero Money object
            # in that currency, as 0 value is independent of exchange rates.
            if target_currency is not None:
                return Money(0, target_currency)

            return Money(0, inst.get_cost_currency())
        else:
            self._log.warning(f"Returning None for {instrument_id} because instrument not found in cache")
            return None

    cdef Money _calculate_realized_pnl(self, InstrumentId instrument_id, AccountId account_id):
        # account_id is mandatory here; aggregation is handled by _aggregate_pnl_by_calculation
        cdef:
            Account account
            Instrument instrument

        account, instrument = self._validate_account_and_instrument(instrument_id, account_id, "realized", is_error=False)
        if account is None or instrument is None:
            return None

        self._ensure_snapshot_pnls_cached_for(instrument_id)
        cdef list[Position] positions = self._cache.positions(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=account_id,
        )
        cdef Currency currency = self._determine_pnl_currency(account, instrument)

        if self._debug:
            self._log.debug(f"Found {len(positions)} positions for {instrument_id}")

        cdef tuple snapshot_result = self._process_snapshot_pnl_contributions(
            instrument_id=instrument_id,
            account_id=account_id,
            positions=positions,
            currency=currency,
            account=account,
        )
        if snapshot_result is None:
            return None

        cdef:
            double total_pnl = 0.0
            set[PositionId] processed_ids

        total_pnl, processed_ids = snapshot_result

        cdef object active_pnl_result = self._process_active_position_realized_pnl(
            positions=positions,
            instrument_id=instrument_id,
            instrument=instrument,
            account=account,
            currency=currency,
            processed_ids=processed_ids,
        )
        if active_pnl_result is None:
            return None

        total_pnl += <double>active_pnl_result
        cdef Money result = Money(total_pnl, currency)

        return result

    cdef void _ensure_snapshot_pnls_cached_for(self, InstrumentId instrument_id):  # noqa: C901
        # Performance: This method maintains an incremental cache of snapshot PnLs
        # It only unpickles new snapshots that haven't been processed yet
        # Tracks sum and last PnL per position for efficient NETTING OMS support

        # Get all position IDs that have snapshots for this instrument
        cdef set[PositionId] snapshot_position_ids = self._cache.position_snapshot_ids(instrument_id)
        if not snapshot_position_ids:
            return  # Nothing to process

        cdef bint rebuild = False
        cdef bint has_new_snapshots = False
        cdef bint has_purge = False

        cdef:
            PositionId position_id
            list position_id_snapshots
            int prev_count
            int curr_count
            dict[PositionId, list] snapshot_data = {}

        # Pre-fetch and detect changes
        for position_id in snapshot_position_ids:
            position_id_snapshots = self._cache.position_snapshot_bytes(position_id)
            curr_count = len(position_id_snapshots)
            snapshot_data[position_id] = position_id_snapshots

            prev_count = self._snapshot_processed_counts.get(position_id, 0)
            if prev_count > curr_count:
                rebuild = True
                has_purge = True
            elif curr_count > prev_count:
                has_new_snapshots = True

        cdef:
            Position snapshot
            Money sum_pnl
            Money last_pnl

        if rebuild:
            # Full rebuild: process all snapshots from scratch
            for position_id in snapshot_position_ids:
                sum_pnl = None
                last_pnl = None
                position_id_snapshots = snapshot_data[position_id]
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
                position_id_snapshots = snapshot_data[position_id]
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

        # If positions were purged, invalidate PnL cache
        if stale_ids:
            has_purge = True

        for stale_position_id in stale_ids:
            self._snapshot_processed_counts.pop(stale_position_id, None)
            self._snapshot_sum_per_position.pop(stale_position_id, None)
            self._snapshot_last_per_position.pop(stale_position_id, None)
            self._snapshot_account_ids.pop(stale_position_id, None)

        # Invalidate PnL cache when snapshots change (new snapshots or purges)
        if has_new_snapshots or has_purge:
            self._realized_pnls.pop(instrument_id, None)

    cdef tuple _process_snapshot_pnl_contributions(
        self,
        InstrumentId instrument_id,
        AccountId account_id,
        list positions,
        Currency currency,
        Account account,
    ):
        # Process snapshot PnL contributions using the 3-case combination rule
        cdef:
            set[PositionId] active_position_ids = {p.id for p in positions}
            set[PositionId] snapshot_ids = self._cache.position_snapshot_ids(instrument_id)
            set[PositionId] processed_ids = set()
            double total_pnl = 0.0
            PositionId position_id
            Money sum_pnl
            Money last_pnl
            object contribution_result
            double contribution
            AccountId snapshot_account_id
            Position position
            double xrate
            PriceType conv_price_type
            Instrument instrument

        for position_id in snapshot_ids:
            # Only process snapshots for the requested account
            snapshot_account_id = self._snapshot_account_ids.get(position_id)
            if snapshot_account_id != account_id:
                continue  # Skip snapshots from other accounts

            sum_pnl = self._snapshot_sum_per_position.get(position_id)
            if sum_pnl is None:
                continue  # No PnL for this position

            contribution_result = self._calculate_snapshot_contribution(
                position_id=position_id,
                active_position_ids=active_position_ids,
                positions=positions,
                sum_pnl=sum_pnl,
                processed_ids=processed_ids,
            )
            if contribution_result is None:
                continue

            contribution = <double>contribution_result

            # Add contribution with currency conversion if needed
            if sum_pnl.currency == currency:
                total_pnl += contribution
            else:
                # Respect use_mark_xrates config for snapshot conversions
                instrument = self._cache.instrument(instrument_id)
                conv_price_type = PriceType.MARK if self._use_mark_xrates else PriceType.MID
                xrate = self._cache.get_xrate(
                    venue=instrument.id.venue,
                    from_currency=sum_pnl.currency,
                    to_currency=currency,
                    price_type=conv_price_type,
                )

                # Fallback to MID if MARK not available
                if xrate is None and conv_price_type == PriceType.MARK:
                    xrate = self._cache.get_xrate(
                        venue=instrument.id.venue,
                        from_currency=sum_pnl.currency,
                        to_currency=currency,
                        price_type=PriceType.MID,
                    )

                if xrate is None or xrate <= 0.0:
                    return None  # Cannot convert currency

                total_pnl += contribution * xrate

        return (round(total_pnl, currency.get_precision()), processed_ids)

    cdef object _calculate_snapshot_contribution(
        self,
        PositionId position_id,
        set active_position_ids,
        list positions,
        Money sum_pnl,
        set processed_ids,
    ):
        # Calculate the contribution from a snapshot using the 3-case combination rule
        cdef:
            double contribution = 0.0
            Position position
            Money last_pnl
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
                return None  # Should not happen

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

        return contribution

    cdef object _process_active_position_realized_pnl(
        self,
        list positions,
        InstrumentId instrument_id,
        Instrument instrument,
        Account account,
        Currency currency,
        set processed_ids,
    ):
        # Process realized PnL from active positions
        cdef:
            double total_pnl = 0.0
            Position position
            double pnl
            object xrate_result
            double xrate
            object bet_position
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
                bet_position = self._get_bet_position(position, instrument)
                if bet_position is None:
                    self._log.debug(
                        f"Cannot calculate realized PnL: no `BetPosition` for {position.id}",
                    )
                    return None  # Cannot calculate

                pnl = float(bet_position.realized_pnl)
            else:
                pnl = position.realized_pnl.as_f64_c()

            if self._convert_to_account_base_currency and account.base_currency is not None:
                xrate_result = self._get_xrate_to_account_base(
                    instrument=instrument,
                    account=account,
                    instrument_id=instrument_id,
                )
                if xrate_result is None or xrate_result == 0:
                    self._log.debug(
                        f"Cannot calculate realized PnL: "
                        f"no {self._log_xrate} exchange rate yet for {instrument.get_cost_currency()}/{account.base_currency}",
                    )
                    self._pending_calcs.add(instrument.id)
                    return None  # Cannot calculate

                xrate = <double>xrate_result
                pnl = pnl * xrate

            total_pnl += pnl

        return round(total_pnl, currency.get_precision())

    cdef Money _calculate_unrealized_pnl(self, InstrumentId instrument_id, Price price=None, AccountId account_id=None):
        # account_id can be None when aggregating in _aggregate_pnl_by_calculation (which calculates fresh with price)
        cdef:
            Account account
            Instrument instrument

        account, instrument = self._validate_account_and_instrument(instrument_id, account_id, "unrealized", is_error=True)
        if account is None or instrument is None:
            return None

        cdef Currency currency = self._determine_pnl_currency(account, instrument)
        cdef list positions_open = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
            strategy_id=None,
            side=PositionSide.NO_POSITION_SIDE,
            account_id=account_id,
        )
        if not positions_open:
            return Money(0, currency)

        cdef object total_pnl = self._calculate_total_unrealized_pnl(
            positions_open=positions_open,
            instrument_id=instrument_id,
            instrument=instrument,
            account=account,
            currency=currency,
            price=price,
        )

        if total_pnl is None:
            return None

        cdef Money result = Money(<double>total_pnl, currency)

        return result

    cdef tuple _validate_account_and_instrument(self, InstrumentId instrument_id, AccountId account_id, str caller_name, bint is_error):
        cdef Account account = self._cache.account_for_venue(instrument_id.venue, account_id)
        if account is None:
            msg = f"Cannot calculate {caller_name} PnL: no account registered for {instrument_id.venue} and {account_id}"
            if is_error:
                self._log.error(msg)
            else:
                self._log.warning(msg)
            return None, None

        cdef Instrument instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            msg = f"Cannot calculate {caller_name} PnL: no instrument for {instrument_id}"
            if is_error:
                self._log.error(msg)
            else:
                self._log.warning(msg)
            return None, None

        if self._debug:
            self._log.debug(
                f"Calculating {caller_name} PnL for instrument {instrument_id} with {account}", LogColor.MAGENTA,
            )

        return account, instrument

    cdef Currency _determine_pnl_currency(self, Account account, Instrument instrument):
        if self._convert_to_account_base_currency and account.base_currency is not None:
            return account.base_currency
        else:
            return instrument.get_cost_currency()

    cdef object _calculate_total_unrealized_pnl(
        self,
        list positions_open,
        InstrumentId instrument_id,
        Instrument instrument,
        Account account,
        Currency currency,
        Price price,
    ):
        # Calculate total unrealized PnL from all open positions
        cdef:
            double total_pnl = 0.0
            Position position
            object pnl_result
            double pnl
            object xrate_result
            double xrate
        for position in positions_open:
            if position.instrument_id != instrument_id:
                continue  # Nothing to calculate

            if position.side == PositionSide.FLAT:
                continue  # Nothing to calculate

            pnl_result = self._calculate_position_unrealized_pnl(
                position=position,
                instrument=instrument,
                account=account,
                currency=currency,
                instrument_id=instrument_id,
                price=price,
            )
            if pnl_result is None:
                return None  # Cannot calculate

            pnl = <double>pnl_result
            total_pnl += pnl

        return round(total_pnl, currency.get_precision())

    cdef object _calculate_position_unrealized_pnl(
        self,
        Position position,
        Instrument instrument,
        Account account,
        Currency currency,
        InstrumentId instrument_id,
        Price price,
    ):
        # Calculate unrealized PnL for a single position
        cdef:
            Price p
            double pnl
            object bet_position
            object xrate_result
            double xrate

        p = price or self._get_price(position)
        if p is None:
            self._log.debug(
                f"Cannot calculate unrealized PnL: no {self._log_price} for {instrument_id}",
            )
            self._pending_calcs.add(instrument.id)
            return None  # Cannot calculate

        if self._debug:
            self._log.debug(f"Calculating unrealized PnL for {position}")

        if isinstance(instrument, BettingInstrument):
            bet_position = self._get_bet_position(position, instrument)
            if bet_position is None:
                self._log.debug(
                    f"Cannot calculate unrealized PnL: no `BetPosition` for {position.id}",
                )
                return None  # Cannot calculate

            pnl = float(bet_position.unrealized_pnl(p.as_decimal()))
        else:
            pnl = position.unrealized_pnl(p).as_f64_c()

        if self._debug:
            self._log.debug(
                f"Unrealized PnL for {instrument.id}: {pnl} {currency}", LogColor.MAGENTA,
            )

        if self._convert_to_account_base_currency and account.base_currency is not None:
            xrate_result = self._get_xrate_to_account_base(
                instrument=instrument,
                account=account,
                instrument_id=instrument_id,
            )
            if xrate_result is None or xrate_result == 0:
                self._log.debug(
                    f"Cannot calculate unrealized PnL: "
                    f"no {self._log_xrate} exchange rate for {instrument.get_cost_currency()}/{account.base_currency}",
                )
                self._pending_calcs.add(instrument.id)
                return None  # Cannot calculate

            xrate = <double>xrate_result
            pnl = pnl * xrate

        return pnl

    cdef object _get_bet_position(self, Position position, Instrument instrument):
        # Helper method to get bet_position with fallback to instrument ID for netting positions
        cdef object bet_position = self._bet_positions.get(position.id)
        if bet_position is None:
            # Try fallback to instrument ID for netting positions
            bet_position = self._bet_positions.get(PositionId(instrument.id.value))

        return bet_position

    cdef object _get_xrate_to_account_base(
        self,
        Instrument instrument,
        Account account,
        InstrumentId instrument_id,
    ):
        # Get the exchange rate from instrument cost currency to account base currency.
        # Uses mark xrates if enabled, falling back to MID xrate.
        if account.base_currency is None:
            return None

        cdef PriceType price_type = PriceType.MARK if self._use_mark_xrates else PriceType.MID
        # Use the instrument's venue for xrate lookup, not the account venue
        cdef Venue venue = instrument_id.venue

        cdef object xrate = self._cache.get_xrate(
            venue=venue,
            from_currency=instrument.get_cost_currency(),
            to_currency=account.base_currency,
            price_type=price_type,
        )

        # Fallback to MID if MARK not available
        if xrate is None and price_type == PriceType.MARK:
            xrate = self._cache.get_xrate(
                venue=venue,
                from_currency=instrument.get_cost_currency(),
                to_currency=account.base_currency,
                price_type=PriceType.MID,
            )

        return xrate

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

    cdef Money _convert_money_if_needed(
        self,
        Money money,
        Currency target_currency,
        Venue venue=None,
        PriceType price_type=PriceType.MID,
    ):
        # Helper method to convert money if target_currency is provided and different from money's currency
        if target_currency is not None and money.currency != target_currency:
            return self._convert_money(money, target_currency, venue=venue, price_type=price_type)

        return money

    cdef Money _convert_money(
            self,
            Money money,
            Currency target_currency,
            Venue venue=None,
            PriceType price_type=PriceType.MID,
    ):
        if money.currency == target_currency:
            return money

        # Use mark xrates if enabled and price_type is not explicitly set to something else
        cdef PriceType effective_price_type = price_type
        if self._use_mark_xrates and price_type == PriceType.MID:
            effective_price_type = PriceType.MARK

        cdef object xrate = self._cache.get_xrate(
            venue=venue,
            from_currency=money.currency,
            to_currency=target_currency,
            price_type=effective_price_type,
        )

        if xrate is None and effective_price_type == PriceType.MARK:
            # Fallback to standard xrate lookup (using venue if provided)
            xrate = self._cache.get_xrate(
                venue=venue,
                from_currency=money.currency,
                to_currency=target_currency,
                price_type=PriceType.MID,
            )

        if xrate is None or xrate <= 0.0:
            self._log.error(f"Cannot convert {money} to {target_currency}: {'no' if xrate is None else 'invalid'} exchange rate for {money.currency} using {price_type_to_str(effective_price_type)}")
            return None

        return Money(round(money.as_f64_c() * (<double>xrate), target_currency.get_precision()), target_currency)
