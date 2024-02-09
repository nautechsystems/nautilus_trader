# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

from nautilus_trader.analysis import statistics
from nautilus_trader.analysis.analyzer import PortfolioAnalyzer

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.accounting.factory cimport AccountFactory
from nautilus_trader.accounting.manager cimport AccountsManager
from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.component cimport LogColor
from nautilus_trader.common.component cimport Logger
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport AccountType
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport OrderType
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.rust.model cimport PriceType
from nautilus_trader.model.data cimport QuoteTick
from nautilus_trader.model.data cimport TradeTick
from nautilus_trader.model.events.account cimport AccountState
from nautilus_trader.model.events.order cimport OrderAccepted
from nautilus_trader.model.events.order cimport OrderCanceled
from nautilus_trader.model.events.order cimport OrderEvent
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.events.order cimport OrderRejected
from nautilus_trader.model.events.order cimport OrderUpdated
from nautilus_trader.model.events.position cimport PositionEvent
from nautilus_trader.model.functions cimport position_side_to_str
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.position cimport Position
from nautilus_trader.portfolio.base cimport PortfolioFacade


cdef tuple _UPDATE_ORDER_EVENTS = (
    OrderAccepted,
    OrderCanceled,
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
    """

    def __init__(
        self,
        MessageBus msgbus not None,
        CacheFacade cache not None,
        Clock clock not None,
    ):
        self._clock = clock
        self._log = Logger(name=type(self).__name__)
        self._msgbus = msgbus
        self._cache = cache
        self._accounts = AccountsManager(
            cache=cache,
            clock=clock,
            logger=self._log,
        )

        self._venue = None  # Venue for specific portfolio behavior (Interactive Brokers)
        self._unrealized_pnls: dict[InstrumentId, Money] = {}
        self._net_positions: dict[InstrumentId, Decimal] = {}
        self._pending_calcs: set[InstrumentId] = set()

        self.analyzer = PortfolioAnalyzer()

        # Register default statistics
        self.analyzer.register_statistic(statistics.winner_max.MaxWinner())
        self.analyzer.register_statistic(statistics.winner_avg.AvgWinner())
        self.analyzer.register_statistic(statistics.winner_min.MinWinner())
        self.analyzer.register_statistic(statistics.loser_min.MinLoser())
        self.analyzer.register_statistic(statistics.loser_avg.AvgLoser())
        self.analyzer.register_statistic(statistics.loser_max.MaxLoser())
        self.analyzer.register_statistic(statistics.expectancy.Expectancy())
        self.analyzer.register_statistic(statistics.win_rate.WinRate())
        self.analyzer.register_statistic(statistics.returns_volatility.ReturnsVolatility())
        self.analyzer.register_statistic(statistics.returns_avg.ReturnsAverage())
        self.analyzer.register_statistic(statistics.returns_avg_loss.ReturnsAverageLoss())
        self.analyzer.register_statistic(statistics.returns_avg_win.ReturnsAverageWin())
        self.analyzer.register_statistic(statistics.sharpe_ratio.SharpeRatio())
        self.analyzer.register_statistic(statistics.sortino_ratio.SortinoRatio())
        self.analyzer.register_statistic(statistics.profit_factor.ProfitFactor())
        self.analyzer.register_statistic(statistics.risk_return_ratio.RiskReturnRatio())
        self.analyzer.register_statistic(statistics.long_ratio.LongRatio())

        # Register endpoints
        self._msgbus.register(endpoint="Portfolio.update_account", handler=self.update_account)

        # Required subscriptions
        self._msgbus.subscribe(topic="data.quotes.*", handler=self.update_quote_tick, priority=10)
        self._msgbus.subscribe(topic="events.order.*", handler=self.update_order, priority=10)
        self._msgbus.subscribe(topic="events.position.*", handler=self.update_position, priority=10)
        self._msgbus.subscribe(topic="events.account.*", handler=self.update_account, priority=10)

        self.initialized = False

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void set_specific_venue(self, Venue venue):
        """
        Set a specific venue for the portfolio.

        Parameters
        ----------
        venue : Venue
            The specific venue to set.

        """
        Condition.not_none(venue, "venue")

        self._venue = venue

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
            AccountState result
        for instrument_id in instruments:
            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot update initial (order) margin: "
                    f"no instrument found for {instrument.id}."
                )
                initialized = False
                break

            account = self._cache.account_for_venue(self._venue or instrument.id.venue)
            if account is None:
                self._log.error(
                    f"Cannot update initial (order) margin: "
                    f"no account registered for {instrument.id.venue}."
                )
                initialized = False
                break

            orders_open = self._cache.orders_open(
                venue=None,  # Faster query filtering
                instrument_id=instrument.id,
            )

            result = self._accounts.update_orders(
                account=account,
                instrument=instrument,
                orders_open=[o for o in orders_open if o.is_passive_c()],
                ts_event=account.last_event_c().ts_event,
            )
            if result is None:
                initialized = False

        cdef int open_count = len(all_orders_open)
        self._log.info(
            f"Initialized {open_count} open order{'' if open_count == 1 else 's'}.",
            color=LogColor.BLUE if open_count else LogColor.NORMAL,
        )

        self.initialized = initialized

    cpdef void initialize_positions(self):
        """
        Initialize the portfolios positions.

        Performs all account calculations for the current position state.
        """
        # Clean slate
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
            AccountState result
        for instrument_id in instruments:
            positions_open = self._cache.positions_open(
                venue=None,  # Faster query filtering
                instrument_id=instrument_id,
            )
            self._update_net_position(
                instrument_id=instrument_id,
                positions_open=positions_open,
            )

            self._unrealized_pnls[instrument_id] = self._calculate_unrealized_pnl(instrument_id)

            account = self._cache.account_for_venue(self._venue or instrument_id.venue)
            if account is None:
                self._log.error(
                    f"Cannot update maintenance (position) margin: "
                    f"no account registered for {instrument_id.venue}."
                )
                initialized = False
                break

            if account.type == AccountType.CASH:
                continue

            instrument = self._cache.instrument(instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot update maintenance (position) margin: "
                    f"no instrument found for {instrument.id}."
                )
                initialized = False
                break

            result = self._accounts.update_positions(
                account=account,
                instrument=instrument,
                positions_open=self._cache.positions_open(
                    venue=None,  # Faster query filtering
                    instrument_id=instrument_id,
                ),
                ts_event=account.last_event_c().ts_event,
            )
            if result is None:
                initialized = False

        cdef int open_count = len(all_positions_open)
        self._log.info(
            f"Initialized {open_count} open position{'' if open_count == 1 else 's'}.",
            color=LogColor.BLUE if open_count else LogColor.NORMAL,
        )

        self.initialized = initialized

    cpdef void update_quote_tick(self, QuoteTick tick):
        """
        Update the portfolio with the given tick.

        Clears the unrealized PnL for the quote ticks instrument, and
        performs any initialization calculations which may have been pending
        a market quote update.

        Parameters
        ----------
        tick : QuoteTick
            The tick to update with.

        """
        Condition.not_none(tick, "tick")

        self._unrealized_pnls.pop(tick.instrument_id, None)

        if self.initialized:
            return

        if tick.instrument_id not in self._pending_calcs:
            return

        cdef Account account = self._cache.account_for_venue(self._venue or tick.instrument_id.venue)
        if account is None:
            self._log.error(
                f"Cannot update tick: "
                f"no account registered for {tick.instrument_id.venue}."
            )
            return  # No account registered

        cdef Instrument instrument = self._cache.instrument(self._venue or tick.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot update tick: "
                f"no instrument found for {tick.instrument_id}"
            )
            return  # No instrument found

        cdef list orders_open = self._cache.orders_open(
            venue=None,  # Faster query filtering
            instrument_id=tick.instrument_id,
        )

        cdef:
            Order o
        # Initialize initial (order) margin
        cdef AccountState result_init = self._accounts.update_orders(
            account=account,
            instrument=instrument,
            orders_open=[o for o in orders_open if o.is_passive_c()],
            ts_event=account.last_event_c().ts_event,
        )

        result_maint = None
        if account.is_margin_account:
            positions_open = self._cache.positions_open(
                venue=None,  # Faster query filtering
                instrument_id=tick.instrument_id,
            )

            # Initialize maintenance (position) margin
            result_maint = self._accounts.update_positions(
                account=account,
                instrument=instrument,
                positions_open=positions_open,
                ts_event=account.last_event_c().ts_event,
            )

        # Calculate unrealized PnL
        cdef Money result_unrealized_pnl = self._calculate_unrealized_pnl(tick.instrument_id)

        # Check portfolio initialization
        if result_init is not None and (account.is_cash_account or (result_maint is not None and result_unrealized_pnl)):
            self._pending_calcs.discard(tick.instrument_id)
            if not self._pending_calcs:
                self.initialized = True

    cpdef void update_account(self, AccountState event):
        """
        Apply the given account state.

        Parameters
        ----------
        event : AccountState
            The account state to apply.

        """
        Condition.not_none(event, "event")

        cdef Account account = self._cache.account(event.account_id)
        if account is None:
            # Generate account
            account = AccountFactory.create_c(event)
            # Add to cache
            self._cache.add_account(account)
        else:
            account.apply(event)
            self._cache.update_account(account)

        self._log.info(f"Updated {event}.")

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

        cdef Account account = self._cache.account(event.account_id)
        if account is None:
            self._log.error(
                f"Cannot update order: "
                f"no account registered for {event.account_id}"
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
                f"{repr(event.client_order_id)} not found in the cache."
            )
            return  # No order found

        if isinstance(event, OrderRejected) and order.order_type != OrderType.STOP_LIMIT:
            return  # No change to account state

        cdef Instrument instrument = self._cache.instrument(event.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot update order: "
                f"no instrument found for {event.instrument_id}"
            )
            return  # No instrument found

        cdef AccountState account_state = None
        if isinstance(event, OrderFilled):
            account_state = self._accounts.update_balances(
                account=account,
                instrument=instrument,
                fill=event,
            )

        cdef list orders_open = self._cache.orders_open(
            venue=None,  # Faster query filtering
            instrument_id=event.instrument_id,
        )

        cdef:
            Order o
        account_state = self._accounts.update_orders(
            account=account,
            instrument=instrument,
            orders_open=[o for o in orders_open if o.is_passive_c()],
            ts_event=event.ts_event,
        )

        if account_state is None:
            self._log.debug(f"Added pending calculation for {instrument.id}.")
            self._pending_calcs.add(instrument.id)
        else:
            self._msgbus.publish_c(
                topic=f"events.account.{account.id}",
                msg=account_state,
            )

        self._log.debug(f"Updated {event}.")

    cpdef void update_position(self, PositionEvent event):
        """
        Update the portfolio with the given position event.

        Parameters
        ----------
        event : PositionEvent
            The event to update with.

        """
        Condition.not_none(event, "event")

        cdef list positions_open = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=event.instrument_id,
        )
        self._update_net_position(
            instrument_id=event.instrument_id,
            positions_open=positions_open
        )

        self._unrealized_pnls[event.instrument_id] = self._calculate_unrealized_pnl(
            instrument_id=event.instrument_id,
        )

        cdef Account account = self._cache.account(event.account_id)
        if account is None:
            self._log.error(
                f"Cannot update position: "
                f"no account registered for {event.account_id}"
            )
            return  # No account registered

        if account.type != AccountType.MARGIN or not account.calculate_account_state:
            return  # Nothing to calculate

        cdef Instrument instrument = self._cache.instrument(event.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot update position: "
                f"no instrument found for {event.instrument_id}"
            )
            return  # No instrument found

        cdef AccountState account_state = self._accounts.update_positions(
            account=account,
            instrument=instrument,
            positions_open=positions_open,
            ts_event=event.ts_event,
        )

        if account_state is None:
            self._log.debug(f"Added pending calculation for {instrument.id}.")
            self._pending_calcs.add(instrument.id)
        else:
            self._msgbus.publish_c(
                topic=f"events.account.{account.id}",
                msg=account_state,
            )

        self._log.debug(f"Updated {event}.")

    def _reset(self) -> None:
        self._net_positions.clear()
        self._unrealized_pnls.clear()
        self._pending_calcs.clear()
        self.analyzer.reset()

        self.initialized = False

    def reset(self) -> None:
        """
        Reset the portfolio.

        All stateful fields are reset to their initial value.

        """
        self._log.debug(f"RESETTING...")

        self._reset()

        self._log.info("READY.")

    def dispose(self) -> None:
        """
        Dispose of the portfolio.

        All stateful fields are reset to their initial value.

        """
        self._log.debug(f"DISPOSING...")

        self._reset()

        self._log.info("DISPOSED.")

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef Account account(self, Venue venue):
        """
        Return the account for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the account.

        Returns
        -------
        Account or ``None``

        """
        Condition.not_none(venue, "venue")
        Condition.not_none(self._cache, "self._cache")

        cdef Account account = self._cache.account_for_venue(venue)
        if account is None:
            self._log.error(
                f"Cannot get account: "
                f"no account registered for {venue}."
            )

        return account

    cpdef dict balances_locked(self, Venue venue):
        """
        Return the balances locked for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the margin.

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._cache.account_for_venue(venue)
        if account is None:
            self._log.error(
                f"Cannot get balances locked: "
                f"no account registered for {venue}."
            )
            return None

        return account.balances_locked()

    cpdef dict margins_init(self, Venue venue):
        """
        Return the initial (order) margins for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the margin.

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._cache.account_for_venue(venue)
        if account is None:
            self._log.error(
                f"Cannot get initial (order) margins: "
                f"no account registered for {venue}."
            )
            return None

        if account.is_cash_account:
            return None

        return account.margins_init()

    cpdef dict margins_maint(self, Venue venue):
        """
        Return the maintenance (position) margins for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the margin.

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._cache.account_for_venue(venue)
        if account is None:
            self._log.error(
                f"Cannot get maintenance (position) margins: "
                f"no account registered for {venue}."
            )
            return None

        if account.is_cash_account:
            return None

        return account.margins_maint()

    cpdef dict unrealized_pnls(self, Venue venue):
        """
        Return the unrealized pnls for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the unrealized pnl.

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        Condition.not_none(venue, "venue")

        cdef list positions_open = self._cache.positions_open(venue)
        if not positions_open:
            return {}  # Nothing to calculate

        cdef set instrument_ids = {p.instrument_id for p in positions_open}

        cdef dict unrealized_pnls = {}  # type: dict[Currency, 0.0]

        cdef:
            InstrumentId instrument_id
            Money pnl
        for instrument_id in instrument_ids:
            pnl = self._unrealized_pnls.get(instrument_id)
            if pnl is not None:
                # PnL already calculated
                unrealized_pnls[pnl.currency] = unrealized_pnls.get(pnl.currency, 0.0) + pnl.as_f64_c()
                continue  # To next instrument_id
            # Calculate PnL
            pnl = self._calculate_unrealized_pnl(instrument_id)
            if pnl is None:
                continue  # Error logged in `_calculate_unrealized_pnl`
            unrealized_pnls[pnl.currency] = unrealized_pnls.get(pnl.currency, 0.0) + pnl.as_f64_c()

        return {k: Money(v, k) for k, v in unrealized_pnls.items()}

    cpdef dict net_exposures(self, Venue venue):
        """
        Return the net exposures for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the market value.

        Returns
        -------
        dict[Currency, Money] or ``None``

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._cache.account_for_venue(venue)
        if account is None:
            self._log.error(
                f"Cannot calculate net exposures: "
                f"no account registered for {venue}."
            )
            return None  # Cannot calculate

        cdef list positions_open = self._cache.positions_open(venue)
        if not positions_open:
            return {}  # Nothing to calculate

        cdef dict net_exposures = {}  # type: dict[Currency, float]

        cdef:
            Position position
            Instrument instrument
            Price last
            Currency settlement_currency
            double xrate
        for position in positions_open:
            instrument = self._cache.instrument(position.instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot calculate net exposures: "
                    f"no instrument for {position.instrument_id}."
                )
                return None  # Cannot calculate

            last = self._get_last_price(position)
            if last is None:
                self._log.error(
                    f"Cannot calculate net exposures: "
                    f"no prices for {position.instrument_id}."
                )
                continue  # Cannot calculate

            xrate = self._calculate_xrate_to_base(
                instrument=instrument,
                account=account,
                side=position.entry,
            )

            if xrate == 0.0:
                self._log.error(
                    f"Cannot calculate net exposures: "
                    f"insufficient data for {instrument.get_settlement_currency()}/{account.base_currency}."
                )
                return None  # Cannot calculate

            if account.base_currency is not None:
                settlement_currency = account.base_currency
            else:
                settlement_currency = instrument.get_settlement_currency()

            net_exposure = instrument.notional_value(
                position.quantity,
                last,
            ).as_f64_c()
            net_exposure = round(net_exposure * xrate, settlement_currency._mem.precision)

            total_net_exposure = net_exposures.get(settlement_currency, 0.0)
            total_net_exposure += net_exposure

            net_exposures[settlement_currency] = total_net_exposure

        return {k: Money(v, k) for k, v in net_exposures.items()}

    cpdef Money unrealized_pnl(self, InstrumentId instrument_id):
        """
        Return the unrealized PnL for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the unrealized PnL.

        Returns
        -------
        Money or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef Money pnl = self._unrealized_pnls.get(instrument_id)
        if pnl is not None:
            return pnl

        pnl = self._calculate_unrealized_pnl(instrument_id)
        self._unrealized_pnls[instrument_id] = pnl

        return pnl

    cpdef Money net_exposure(self, InstrumentId instrument_id):
        """
        Return the net exposure for the given instrument (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the calculation.

        Returns
        -------
        Money or ``None``

        """
        Condition.not_none(instrument_id, "instrument_id")

        cdef Account account = self._cache.account_for_venue(self._venue or instrument_id.venue)
        if account is None:
            self._log.error(
                f"Cannot calculate net exposure: "
                f"no account registered for {instrument_id.venue}."
            )
            return None  # Cannot calculate

        cdef instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot calculate net exposure: "
                f"no instrument for {instrument_id}."
            )
            return None  # Cannot calculate

        cdef list positions_open = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
        )
        if not positions_open:
            return Money(0, instrument.get_settlement_currency())

        cdef double net_exposure = 0.0

        cdef:
            Position position
            Price last
            double xrate
            Money notional_value
        for position in positions_open:
            last = self._get_last_price(position)
            if last is None:
                self._log.error(
                    f"Cannot calculate net exposure: "
                    f"no prices for {position.instrument_id}."
                )
                continue  # Cannot calculate

            xrate = self._calculate_xrate_to_base(
                instrument=instrument,
                account=account,
                side=position.entry,
            )

            if xrate == 0.0:
                self._log.error(
                    f"Cannot calculate net exposure: "
                    f"insufficient data for {instrument.get_settlement_currency()}/{account.base_currency}."
                )
                return None  # Cannot calculate

            notional_value = instrument.notional_value(
                position.quantity,
                last,
            )
            net_exposure += notional_value.as_f64_c() * xrate

        if account.base_currency is not None:
            return Money(net_exposure, account.base_currency)
        else:
            return Money(net_exposure, instrument.get_settlement_currency())

    cpdef object net_position(self, InstrumentId instrument_id):
        """
        Return the total net position for the given instrument ID.
        If no positions for instrument_id then will return `Decimal('0')`.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the query.

        Returns
        -------
        Decimal

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._net_position(instrument_id)

    cpdef bint is_net_long(self, InstrumentId instrument_id):
        """
        Return a value indicating whether the portfolio is net long the given
        instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the query.

        Returns
        -------
        bool
            True if net long, else False.

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._net_position(instrument_id) > 0.0

    cpdef bint is_net_short(self, InstrumentId instrument_id):
        """
        Return a value indicating whether the portfolio is net short the given
        instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the query.

        Returns
        -------
        bool
            True if net short, else False.

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._net_position(instrument_id) < 0.0

    cpdef bint is_flat(self, InstrumentId instrument_id):
        """
        Return a value indicating whether the portfolio is flat for the given
        instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument query filter.

        Returns
        -------
        bool
            True if net flat, else False.

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._net_position(instrument_id) == 0.0

    cpdef bint is_completely_flat(self):
        """
        Return a value indicating whether the portfolio is completely flat.

        Returns
        -------
        bool
            True if net flat across all instruments, else False.

        """
        for net_position in self._net_positions.values():
            if net_position != Decimal(0):
                return False

        return True

# -- INTERNAL -------------------------------------------------------------------------------------

    cdef object _net_position(self, InstrumentId instrument_id):
        return self._net_positions.get(instrument_id, Decimal(0))

    cdef void _update_net_position(self, InstrumentId instrument_id, list positions_open):
        net_position = Decimal(0)

        cdef Position position
        for position in positions_open:
            net_position += position.signed_decimal_qty()

        existing_position: Decimal = self._net_positions.get(instrument_id, Decimal(0))
        if existing_position != net_position:
            self._net_positions[instrument_id] = net_position
            self._log.info(f"{instrument_id} net_position={net_position}")

    cdef Money _calculate_unrealized_pnl(self, InstrumentId instrument_id):
        cdef Account account = self._cache.account_for_venue(self._venue or instrument_id.venue)
        if account is None:
            self._log.error(
                f"Cannot calculate unrealized PnL: "
                f"no account registered for {instrument_id.venue}."
            )
            return None  # Cannot calculate

        cdef Instrument instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot calculate unrealized PnL: "
                f"no instrument for {instrument_id}."
            )
            return None  # Cannot calculate

        cdef Currency currency
        if account.base_currency is not None:
            currency = account.base_currency
        else:
            currency = instrument.get_settlement_currency()

        cdef list positions_open = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
        )
        if not positions_open:
            return Money(0, currency)

        cdef double total_pnl = 0.0

        cdef:
            Position position
            Price last
            double pnl
            double xrate
        for position in positions_open:
            if position.instrument_id != instrument_id:
                continue  # Nothing to calculate

            last = self._get_last_price(position)
            if last is None:
                self._log.debug(
                    f"Cannot calculate unrealized PnL: no prices for {instrument_id}."
                )
                self._pending_calcs.add(instrument.id)
                return None  # Cannot calculate

            pnl = position.unrealized_pnl(last).as_f64_c()

            if account.base_currency is not None:
                xrate = self._calculate_xrate_to_base(
                    instrument=instrument,
                    account=account,
                    side=position.entry,
                )

                if xrate == 0.0:
                    self._log.debug(
                        f"Cannot calculate unrealized PnL: "
                        f"insufficient data for {instrument.get_settlement_currency()}/{account.base_currency}."
                    )
                    self._pending_calcs.add(instrument.id)
                    return None  # Cannot calculate

                pnl = round(pnl * xrate, currency.get_precision())

            total_pnl += pnl

        return Money(total_pnl, currency)

    cdef Price _get_last_price(self, Position position):
        cdef QuoteTick quote_tick = self._cache.quote_tick(position.instrument_id)
        if quote_tick is not None:
            if position.side == PositionSide.LONG:
                return quote_tick.bid_price
            elif position.side == PositionSide.SHORT:
                return quote_tick.ask_price
            else:  # pragma: no cover (design-time error)
                raise RuntimeError(
                    f"invalid `PositionSide`, was {position_side_to_str(position.side)}",
                )

        cdef TradeTick trade_tick = self._cache.trade_tick(position.instrument_id)
        return trade_tick.price if trade_tick is not None else None

    cdef double _calculate_xrate_to_base(self, Account account, Instrument instrument, OrderSide side):
        if account.base_currency is not None:
            return self._cache.get_xrate(
                venue=self._venue or instrument.id.venue,
                from_currency=instrument.get_settlement_currency(),
                to_currency=account.base_currency,
                price_type=PriceType.BID if side == OrderSide.BUY else PriceType.ASK,
            )

        return Decimal(1)  # No conversion needed
