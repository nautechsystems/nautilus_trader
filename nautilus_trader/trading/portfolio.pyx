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

"""
The `Portfolio` components facilitate the management of trading operations.

The intended use case is for a single `Portfolio` instance per running system,
a fleet of trading strategies will organize around a portfolio with the help
of the `Trader` class.

The portfolio can satisfy queries for accounting information, margin balances,
total risk exposures and total net positions.
"""

from decimal import Decimal

from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.position_side cimport PositionSideParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport PositionChanged
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.order.base cimport PassiveOrder
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick
from nautilus_trader.trading.account cimport Account


cdef class PortfolioFacade:
    """
    Provides a read-only facade for a `Portfolio`.
    """

# -- QUERIES ---------------------------------------------------------------------------------------

    cpdef Account account(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict initial_margins(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict maint_margins(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict unrealized_pnls(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict market_values(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money unrealized_pnl(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money market_value(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef object net_position(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_net_long(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_net_short(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_flat(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_completely_flat(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")


cdef class Portfolio(PortfolioFacade):
    """
    Provides a trading portfolio.

    Currently there is a limitation of one account per venue.
    """

    def __init__(self, Clock clock not None, Logger logger=None):
        """
        Initialize a new instance of the `Portfolio` class.

        Parameters
        ----------
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.

        """
        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = LoggerAdapter(component=type(self).__name__, logger=logger)
        self._data = None  # Initialized when cache registered

        self._ticks = {}             # type: dict[InstrumentId: QuoteTick]
        self._accounts = {}          # type: dict[Venue: Account]
        self._orders_working = {}    # type: dict[Venue: set[Order]]
        self._positions_open = {}    # type: dict[Venue: set[Position]]
        self._positions_closed = {}  # type: dict[Venue: set[Position]]
        self._unrealized_pnls = {}   # type: dict[InstrumentId: Money]
        self._net_positions = {}     # type: dict[InstrumentId: Decimal]

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void register_cache(self, DataCacheFacade cache) except *:
        """
        Register the given data cache with the portfolio.

        Parameters
        ----------
        cache : DataCacheFacade
            The data cache to register.

        """
        Condition.not_none(cache, "cache")

        self._data = cache

    cpdef void register_account(self, Account account) except *:
        """
        Register the given account with the portfolio.

        Parameters
        ----------
        account : Account
            The account to register.

        Raises
        ------
        KeyError
            If issuer is already registered with the portfolio.

        """
        Condition.not_none(account, "account")
        Condition.not_in(account.id.issuer, self._accounts, "venue", "self._accounts")

        cdef AccountId account_id = account.id
        self._accounts[account_id.issuer_as_venue()] = account
        account.register_portfolio(self)
        self._log.debug(f"Registered account {account_id}.")

    cpdef void initialize_orders(self, set orders) except *:
        """
        Initialize the portfolio with the given orders.

        Parameters
        ----------
        orders : set[Order]
            The orders to initialize with.

        """
        Condition.not_none(orders, "orders")

        # Clean slate
        self._orders_working.clear()

        cdef Order order
        cdef set orders_working
        cdef int working_count = 0
        for order in orders:
            if order.is_passive_c() and order.is_working_c():
                orders_working = self._orders_working.get(order.instrument_id.venue, set())
                orders_working.add(order)
                self._orders_working[order.instrument_id.venue] = orders_working
                self._log.debug(f"Added working {order}")
                working_count += 1

        cdef Venue venue
        for venue in self._orders_working.keys():
            self._update_initial_margin(venue)

        self._log.info(
            f"Initialized {working_count} working order{'' if working_count == 1 else 's'}.",
            color=LogColor.BLUE if working_count else LogColor.NORMAL,
        )

    cpdef void initialize_positions(self, set positions) except *:
        """
        Initialize the portfolio with the given positions.

        Parameters
        ----------
        positions : set[Position]
            The positions to initialize with.

        """
        Condition.not_none(positions, "positions")

        # Clean slate
        self._positions_open.clear()
        self._positions_closed.clear()
        self._unrealized_pnls.clear()

        cdef Position position
        cdef set positions_open
        cdef set positions_closed
        cdef int open_count = 0
        cdef int closed_count = 0
        for position in positions:
            if position.is_open_c():
                positions_open = self._positions_open.get(position.instrument_id.venue, set())
                positions_open.add(position)
                self._positions_open[position.instrument_id.venue] = positions_open
                self._update_net_position(position.instrument_id, positions_open)
                self._log.debug(f"Added {position}")
                open_count += 1
            elif position.is_closed_c():
                positions_closed = self._positions_closed.get(position.instrument_id.venue, set())
                positions_closed.add(position)
                self._positions_closed[position.instrument_id.venue] = positions_closed
                closed_count += 1

        cdef Venue venue
        cdef InstrumentId instrument_id
        for venue in self._positions_open.keys():
            self._update_maint_margin(venue)
            for instrument_id in self._instruments_open_for_venue(venue):
                self._unrealized_pnls[instrument_id] = self._calculate_unrealized_pnl(instrument_id)

        self._log.info(
            f"Initialized {open_count} open position{'' if open_count == 1 else 's'}.",
            color=LogColor.BLUE if open_count else LogColor.NORMAL,
        )

        self._log.info(
            f"Initialized {closed_count} closed position{'' if closed_count == 1 else 's'}.",
            color=LogColor.BLUE if closed_count else LogColor.NORMAL,
        )

    cpdef void update_tick(self, QuoteTick tick) except *:
        """
        Update the portfolio with the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick to update with.

        """
        Condition.not_none(tick, "tick")

        cdef QuoteTick last = self._ticks.get(tick.instrument_id)
        self._ticks[tick.instrument_id] = tick

        if last is not None and (tick.bid != last.bid or tick.ask != last.ask):
            # Clear cached unrealized PnLs
            self._unrealized_pnls[tick.instrument_id] = None

    cpdef void update_order(self, Order order) except *:
        """
        Update the portfolio with the given order.

        Parameters
        ----------
        order : Order
            The order to update with.

        """
        Condition.not_none(order, "order")

        cdef Venue venue = order.instrument_id.venue

        cdef set orders_working = self._orders_working.get(venue, set())
        if order.is_working_c():
            orders_working.add(order)
            self._orders_working[venue] = orders_working
            self._log.debug(f"Added working {order}")
        elif order.is_completed_c():
            orders_working.discard(order)

        self._update_initial_margin(venue)

    cpdef void update_position(self, PositionEvent event) except *:
        """
        Update the portfolio with the given position event.

        Parameters
        ----------
        event : PositionEvent
            The event to update with.

        """
        Condition.not_none(event, "event")

        if isinstance(event, PositionOpened):
            self._handle_position_opened(event)
        elif isinstance(event, PositionChanged):
            self._handle_position_changed(event)
        elif isinstance(event, PositionClosed):
            self._handle_position_closed(event)

        self._log.debug(f"Updated {event.position}.")

        cdef InstrumentId instrument_id = event.position.instrument_id
        self._update_maint_margin(instrument_id.venue)
        self._unrealized_pnls[instrument_id] = self._calculate_unrealized_pnl(instrument_id)

    cpdef void reset(self) except *:
        """
        Reset the portfolio.

        All stateful fields are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        self._ticks.clear()
        self._accounts.clear()
        self._orders_working.clear()
        self._positions_open.clear()
        self._positions_closed.clear()
        self._net_positions.clear()
        self._unrealized_pnls.clear()

        self._log.info("Reset.")

    cpdef Account account(self, Venue venue):
        """
        Return the account for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the account.

        Returns
        -------
        Account or None

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot get account (no account registered for {venue}).")

        return account

# -- QUERIES ---------------------------------------------------------------------------------

    cpdef dict initial_margins(self, Venue venue):
        """
        Return the initial margins for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the margin.

        Returns
        -------
        dict[Currency, Money] or None

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot calculate order margin "
                            f"(no account registered for {venue}).")
            return None

        return account.initial_margins()

    cpdef dict maint_margins(self, Venue venue):
        """
        Return the maintenance margins for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the margin.

        Returns
        -------
        dict[Currency, Money] or None

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot calculate position margin "
                            f"(no account registered for {venue}).")
            return None

        return account.maint_margins()

    cpdef dict unrealized_pnls(self, Venue venue):
        """
        Return the unrealized pnls for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the unrealized pnl.

        Returns
        -------
        dict[Currency, Money] or None

        """
        Condition.not_none(venue, "venue")

        cdef set instrument_ids = self._instruments_open_for_venue(venue)
        if not instrument_ids:
            return {}  # Nothing to calculate

        cdef dict unrealized_pnls = {}  # type: dict[Currency, Decimal]

        cdef InstrumentId instrument_id
        cdef Money pnl
        for instrument_id in instrument_ids:
            pnl = self._unrealized_pnls.get(instrument_id)
            if pnl is not None:
                # PnL already calculated
                unrealized_pnls[pnl.currency] = unrealized_pnls.get(pnl.currency, Decimal(0)) + pnl
                continue  # To next instrument_id
            # Calculate PnL
            pnl = self._calculate_unrealized_pnl(instrument_id)
            if pnl is None:
                return None  # Error already logged in `_calculate_unrealized_pnl`
            unrealized_pnls[pnl.currency] = unrealized_pnls.get(pnl.currency, Decimal(0)) + pnl

        return {k: Money(v, k) for k, v in unrealized_pnls.items()}

    cpdef dict market_values(self, Venue venue):
        """
        Return the market values for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the market value.

        Returns
        -------
        dict[Currency, Money] or None

        """
        Condition.not_none(venue, "venue")
        Condition.not_none(self._data, "self._data")

        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot calculate market value "
                            f"(no account registered for {venue}).")
            return None  # Cannot calculate

        cdef set positions_open = self._positions_open.get(venue)
        if not positions_open:
            return {}  # Nothing to calculate

        cdef dict market_values = {}  # type: dict[Currency, Decimal]

        cdef Position position
        cdef Instrument instrument
        cdef Price last
        for position in positions_open:
            instrument = self._data.instrument(position.instrument_id)
            if instrument is None:
                self._log.error(f"Cannot calculate market value "
                                f"(no instrument for {position.instrument_id}).")
                return None  # Cannot calculate

            last = self._get_last_price(position)
            if last is None:
                self._log.error(f"Cannot calculate market value "
                                f"(no prices for {position.instrument_id}).")
                continue  # Cannot calculate

            xrate = self._calculate_xrate(
                instrument=instrument,
                account=account,
                side=position.entry,
            )

            if xrate == 0:
                self._log.error(f"Cannot calculate market value (insufficient data for "
                                f"{instrument.quote_currency}/{account.default_currency}).")
                return None  # Cannot calculate

            market_value = market_values.get(instrument.settlement_currency, Decimal(0))
            market_value += instrument.market_value(
                position.quantity,
                last,
            ) * xrate

            if account.default_currency is not None:
                market_values[account.default_currency] = market_value
            else:
                market_values[instrument.settlement_currency] = market_value

        return {k: Money(v, k) for k, v in market_values.items()}

    cpdef Money unrealized_pnl(self, InstrumentId instrument_id):
        """
        Return the unrealized PnL for the given instrument identifier (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the unrealized PnL.

        Returns
        -------
        Money or None

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(self._data, "self._data")

        cdef Money pnl = self._unrealized_pnls.get(instrument_id)
        if pnl is not None:
            return pnl

        pnl = self._calculate_unrealized_pnl(instrument_id)
        self._unrealized_pnls[instrument_id] = pnl

        return pnl

    cpdef Money market_value(self, InstrumentId instrument_id):
        """
        Return the market value for the given instrument identifier (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the market value.

        Returns
        -------
        Money or None

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(self._data, "self._data")

        cdef Account account = self._accounts.get(instrument_id.venue)
        if account is None:
            self._log.error(f"Cannot calculate market value "
                            f"(no account registered for {instrument_id.venue}).")
            return None  # Cannot calculate

        cdef instrument = self._data.instrument(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot calculate market value "
                            f"(no instrument for {instrument_id}).")
            return None  # Cannot calculate

        cdef set positions_open = self._positions_open.get(instrument_id.venue)
        if not positions_open:
            return Money(0, instrument.quote_currency)

        market_value: Decimal = Decimal(0)

        cdef Position position
        cdef Price last
        for position in positions_open:
            if position.instrument_id != instrument_id:
                continue

            last = self._get_last_price(position)
            if last is None:
                self._log.error(f"Cannot calculate market value "
                                f"(no prices for {position.instrument_id}).")
                continue  # Cannot calculate

            xrate = self._calculate_xrate(
                instrument=instrument,
                account=account,
                side=position.entry,
            )

            if xrate == 0:
                self._log.error(f"Cannot calculate market value (insufficient data for "
                                f"{instrument.settlement_currency}/{account.default_currency}).")
                return None  # Cannot calculate

            market_value += instrument.market_value(
                position.quantity,
                last,
            ) * xrate

        if account.default_currency is not None:
            return Money(market_value, account.default_currency)
        else:
            return Money(market_value, instrument.settlement_currency)

    cpdef object net_position(self, InstrumentId instrument_id):
        """
        Return the net relative position for the given instrument identifier. If no positions
        for instrument_id then will return `Decimal('0')`.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument for the query.

        Returns
        -------
        Decimal

        """
        return self._net_position(instrument_id)

    cpdef bint is_net_long(self, InstrumentId instrument_id) except *:
        """
        Return a value indicating whether the portfolio is net long the given
        instrument identifier.

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

        return self._net_position(instrument_id) > 0

    cpdef bint is_net_short(self, InstrumentId instrument_id) except *:
        """
        Return a value indicating whether the portfolio is net short the given
        instrument identifier.

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

        return self._net_position(instrument_id) < 0

    cpdef bint is_flat(self, InstrumentId instrument_id) except *:
        """
        Return a value indicating whether the portfolio is flat for the given
        instrument identifier.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument query filter.

        Returns
        -------
        bool
            True if net flat, else False.

        """
        Condition.not_none(instrument_id, "instrument_id")

        return self._net_position(instrument_id) == 0

    cpdef bint is_completely_flat(self) except *:
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

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline object _net_position(self, InstrumentId instrument_id):
        return self._net_positions.get(instrument_id, Decimal(0))

    cdef inline set _instruments_open_for_venue(self, Venue venue):
        cdef set positions_open = self._positions_open.get(venue)
        if positions_open is None:
            return set()
        return {position.instrument_id for position in positions_open}

    cdef inline void _handle_position_opened(self, PositionOpened event) except *:
        cdef Venue venue = event.position.instrument_id.venue
        cdef Position position = event.position

        # Add to positions open
        cdef set positions_open = self._positions_open.get(venue, set())
        positions_open.add(position)
        self._positions_open[venue] = positions_open

        self._update_net_position(event.position.instrument_id, positions_open)

    cdef inline void _handle_position_changed(self, PositionChanged event) except *:
        cdef Venue venue = event.position.instrument_id.venue
        self._update_net_position(event.position.instrument_id, self._positions_open.get(venue, set()))

    cdef inline void _handle_position_closed(self, PositionClosed event) except *:
        cdef Venue venue = event.position.instrument_id.venue
        cdef Position position = event.position

        # Remove from positions open if found
        cdef set positions_open = self._positions_open.get(venue)
        if positions_open is not None:
            positions_open.discard(position)

        # Add to positions closed
        cdef set positions_closed = self._positions_closed.get(venue, set())
        positions_closed.add(position)
        self._positions_closed[venue] = positions_closed

        self._update_net_position(event.position.instrument_id, positions_open)

    cdef inline void _update_net_position(self, InstrumentId instrument_id, set positions_open) except *:
        net_position = Decimal()
        for position in positions_open:
            if position.instrument_id == instrument_id:
                net_position += position.relative_qty

        self._net_positions[instrument_id] = net_position
        self._update_maint_margin(instrument_id.venue)
        self._log.info(f"{instrument_id} net_position={net_position}")

    cdef inline void _update_initial_margin(self, Venue venue) except *:
        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot update initial margin "
                            f"(no account registered for {venue}).")
            return  # Cannot calculate

        cdef set working_orders = self._orders_working.get(venue)
        if working_orders is None:
            return  # Nothing to calculate

        cdef dict margins = {}  # type: dict[Currency, Decimal]

        cdef PassiveOrder order
        cdef Instrument instrument
        cdef Currency currency
        for order in working_orders:
            instrument = self._data.instrument(order.instrument_id)
            if instrument is None:
                self._log.error(f"Cannot calculate initial margin "
                                f"(no instrument for {order.instrument_id}).")
                continue  # Cannot calculate

            # Calculate margin
            margin = instrument.calculate_initial_margin(
                order.quantity,
                order.price,
            )

            if account.default_currency is not None:
                currency = account.default_currency
                xrate = self._calculate_xrate(
                    instrument=instrument,
                    account=account,
                    side=order.side,
                )

                if xrate == 0:
                    self._log.error(f"Cannot calculate initial margin (insufficient data for "
                                    f"{instrument.settlement_currency}/{currency}).")
                    continue  # Cannot calculate

                margin *= xrate
            else:
                currency = instrument.settlement_currency

            # Update total margin
            total_margin = margins.get(currency, Decimal(0))
            total_margin += margin
            margins[currency] = total_margin

        cdef Money total_margin_money
        for currency, total_margin in margins.items():
            total_margin_money = Money(total_margin, currency)
            account.update_initial_margin(total_margin_money)

            self._log.info(f"{venue} initial_margin={total_margin_money}")

    cdef inline void _update_maint_margin(self, Venue venue) except *:
        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot update position maintenance margin "
                            f"(no account registered for {venue}).")
            return  # Cannot calculate

        cdef set open_positions = self._positions_open.get(venue)
        if open_positions is None:
            return  # Nothing to calculate

        cdef dict margins = {}  # type: dict[Currency, Decimal]

        cdef Position position
        cdef Instrument instrument
        cdef Price last
        cdef Currency currency
        for position in open_positions:
            instrument = self._data.instrument(position.instrument_id)
            if instrument is None:
                self._log.error(f"Cannot calculate position maintenance margin "
                                f"(no instrument for {position.instrument_id}).")
                continue  # Cannot calculate

            last = self._get_last_price(position)
            if last is None:
                self._log.error(f"Cannot calculate position maintenance margin "
                                f"(no prices for {position.instrument_id}).")
                continue  # Cannot calculate

            # Calculate margin
            margin = instrument.calculate_maint_margin(
                position.side,
                position.quantity,
                last,
            )

            if account.default_currency is not None:
                currency = account.default_currency
                xrate = self._calculate_xrate(
                    instrument=instrument,
                    account=account,
                    side=position.entry,
                )

                if xrate == 0:
                    self._log.error(f"Cannot calculate unrealized PnL (insufficient data for "
                                    f"{instrument.settlement_currency}/{currency}).")
                    continue  # Cannot calculate

                margin *= xrate
            else:
                currency = instrument.settlement_currency

            # Update total margin
            total_margin = margins.get(currency, Decimal(0))
            total_margin += margin
            margins[currency] = total_margin

        cdef Money total_margin_money
        for currency, total_margin in margins.items():
            total_margin_money = Money(total_margin, currency)
            account.update_maint_margin(total_margin_money)

            self._log.info(f"{venue} maint_margin={total_margin_money}")

    cdef Money _calculate_unrealized_pnl(self, InstrumentId instrument_id):
        cdef Account account = self._accounts.get(instrument_id.venue)
        if account is None:
            self._log.error(f"Cannot calculate unrealized PnL "
                            f"(no account registered for {instrument_id.venue}).")
            return None  # Cannot calculate

        cdef Instrument instrument = self._data.instrument(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot calculate unrealized PnL "
                            f"(no instrument for {instrument_id}).")
            return None  # Cannot calculate

        cdef Currency currency
        if account.default_currency is not None:
            currency = account.default_currency
        else:
            currency = instrument.settlement_currency

        cdef set positions_open = self._positions_open.get(instrument_id.venue)
        if positions_open is None:
            if account.default_currency is not None:
                return Money(0, account.default_currency)
            else:
                return Money(0, instrument.settlement_currency)

        total_pnl: Decimal = Decimal(0)

        cdef Position position
        cdef Price last
        for position in positions_open:
            if position.instrument_id != instrument_id:
                continue  # Nothing to calculate

            last = self._get_last_price(position)
            if last is None:
                self._log.error(f"Cannot calculate unrealized PnL (no prices for {instrument_id}).")
                return None  # Cannot calculate

            pnl = position.unrealized_pnl(last)

            if account.default_currency is not None:
                xrate = self._calculate_xrate(
                    instrument=instrument,
                    account=account,
                    side=position.entry,
                )

                if xrate == 0:
                    self._log.error(f"Cannot calculate unrealized PnL (insufficient data for "
                                    f"{instrument.settlement_currency}/{currency}).")
                    return None  # Cannot calculate

                pnl *= xrate

            total_pnl += pnl

        return Money(total_pnl, currency)

    cdef object _calculate_xrate(self, Instrument instrument, Account account, OrderSide side):
        if account.default_currency is not None:
            return self._data.get_xrate(
                venue=instrument.id.venue,
                from_currency=instrument.settlement_currency,
                to_currency=account.default_currency,
                price_type=PriceType.BID if side == OrderSide.BUY else PriceType.ASK,
            )

        return Decimal(1)  # No conversion needed

    cdef inline Price _get_last_price(self, Position position):
        cdef QuoteTick quote_tick = self._data.quote_tick(position.instrument_id)
        if quote_tick is not None:
            if position.side == PositionSide.LONG:
                return quote_tick.bid
            elif position.side == PositionSide.SHORT:
                return quote_tick.ask
            else:
                raise RuntimeError(f"invalid PositionSide, "
                                   f"was {PositionSideParser.to_str(position.side)}")

        cdef TradeTick trade_tick = self._data.trade_tick(position.instrument_id)
        return trade_tick.price if trade_tick is not None else None
