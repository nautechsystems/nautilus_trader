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

"""
The `Portfolio` components facilitate the management of trading operations.

The intended use case is for a single `Portfolio` instance per running system,
a fleet of trading strategies will organize around a portfolio with the help
of the `Trader` class.

The portfolio can satisfy queries for accounting information, margin balances,
total risk exposures and total net positions.
"""

from decimal import Decimal

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.position_side cimport PositionSideParser
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionModified
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.order cimport PassiveOrder
from nautilus_trader.model.instrument cimport Instrument
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

    cpdef dict init_margins(self, Venue venue):
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

    cpdef Money unrealized_pnl(self, Symbol symbol):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money market_value(self, Symbol symbol):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef object net_position(self, Symbol symbol):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_net_long(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_net_short(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_flat(self, Symbol symbol) except *:
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
        self._log = LoggerAdapter(type(self).__name__, logger)
        self._data = None  # Initialized when cache registered

        self._ticks = {}             # type: dict[Symbol: QuoteTick]
        self._accounts = {}          # type: dict[Venue: Account]
        self._orders_working = {}    # type: dict[Venue: set[Order]]
        self._positions_open = {}    # type: dict[Venue: set[Position]]
        self._positions_closed = {}  # type: dict[Venue: set[Position]]
        self._unrealized_pnls = {}   # type: dict[Symbol: Money]
        self._net_positions = {}     # type: dict[Symbol: Decimal]

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

    cpdef void update_tick(self, QuoteTick tick) except *:
        """
        Update the portfolio with the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick to update with.

        """
        Condition.not_none(tick, "tick")

        cdef QuoteTick last = self._ticks.get(tick.symbol)
        self._ticks[tick.symbol] = tick

        if last is not None and (tick.bid != last.bid or tick.ask != last.ask):
            # Clear cached unrealized P&Ls
            self._unrealized_pnls[tick.symbol] = None

    cpdef void update_orders_working(self, set orders) except *:
        """
        Update the portfolio with the given orders.

        Parameters
        ----------
        orders : set[Order]

        """
        Condition.not_none(orders, "orders")

        # Clean slate
        self._orders_working.clear()

        cdef Order order
        cdef set orders_working
        for order in orders:
            if order.is_working_c():
                orders_working = self._orders_working.get(order.symbol.venue, set())
                orders_working.add(order)
                self._orders_working[order.symbol.venue] = orders_working
                self._log.debug(f"Added working {order}")

        self._log.info(f"Updated {len(orders)} order(s) working.")

        cdef Venue venue
        for venue in self._orders_working.keys():
            self._update_init_margin(venue)

    cpdef void update_order(self, Order order) except *:
        """
        Update the portfolio with the given order.

        Parameters
        ----------
        order : Order
            The order to update with.

        """
        Condition.not_none(order, "order")

        cdef Venue venue = order.symbol.venue

        cdef set orders_working = self._orders_working.get(venue, set())
        if order.is_working_c():
            orders_working.add(order)
            self._orders_working[venue] = orders_working
            self._log.debug(f"Added working {order}")
        elif order.is_completed_c():
            orders_working.discard(order)

        self._update_init_margin(venue)

    cpdef void update_positions(self, set positions) except *:
        """
        Update the portfolio with the given positions.

        Parameters
        ----------
        positions : set[Position]
            The positions to update with.

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
                positions_open = self._positions_open.get(position.symbol.venue, set())
                positions_open.add(position)
                self._positions_open[position.symbol.venue] = positions_open
                self._update_net_position(position.symbol, positions_open)
                self._log.debug(f"Added {position}")
                open_count += 1
            elif position.is_closed_c():
                positions_closed = self._positions_closed.get(position.symbol.venue, set())
                positions_closed.add(position)
                self._positions_closed[position.symbol.venue] = positions_closed
                closed_count += 1

        self._log.info(f"Updated {open_count} position(s) open.")
        self._log.info(f"Updated {closed_count} position(s) closed.")

        cdef Venue venue
        cdef Symbol symbol
        for venue in self._positions_open.keys():
            self._update_maint_margin(venue)
            for symbol in self._symbols_open_for_venue(venue):
                self._unrealized_pnls[symbol] = self._calculate_unrealized_pnl(symbol)

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
        elif isinstance(event, PositionModified):
            self._handle_position_modified(event)
        elif isinstance(event, PositionClosed):
            self._handle_position_closed(event)

        self._log.debug(f"Updated {event.position}.")

        cdef Symbol symbol = event.position.symbol
        self._update_maint_margin(symbol.venue)
        self._unrealized_pnls[symbol] = self._calculate_unrealized_pnl(symbol)

    cpdef void reset(self) except *:
        """
        Reset the portfolio.

        All stateful values are reset to their initial value.
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

    cpdef dict init_margins(self, Venue venue):
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

        return account.init_margins()

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

        cdef set symbols = self._symbols_open_for_venue(venue)
        if not symbols:
            return {}  # Nothing to calculate

        cdef dict unrealized_pnls = {}  # type: dict[Currency, Decimal]

        cdef Symbol symbol
        cdef Money pnl
        for symbol in symbols:
            pnl = self._unrealized_pnls.get(symbol)
            if pnl is not None:
                # P&L pre-calculated
                unrealized_pnls[pnl.currency] = unrealized_pnls.get(pnl.currency, Decimal(0)) + pnl
                continue
            # P&L must be calculated
            pnl = self._calculate_unrealized_pnl(symbol)
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
            The venue for the open value.

        Returns
        -------
        dict[Currency, Money] or None

        """
        Condition.not_none(venue, "venue")
        Condition.not_none(self._data, "self._data")

        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot calculate open value "
                            f"(no account registered for {venue}).")
            return None

        cdef set positions_open = self._positions_open.get(venue)
        if not positions_open:
            return {}  # Nothing to calculate

        cdef dict market_values = {}  # type: dict[Currency, Decimal]

        cdef Position position
        cdef Instrument instrument
        cdef Price last
        for position in positions_open:
            instrument = self._data.instrument(position.symbol)
            if instrument is None:
                self._log.error(f"Cannot calculate open value "
                                f"(no instrument for {position.symbol}).")
                return None  # Cannot calculate

            last = self._get_last_price(position)  # TODO: Optimize
            if last is None:
                self._log.error(f"Cannot calculate open value "
                                f"(no prices for {position.symbol}).")
                continue  # Cannot calculate

            xrate = self._calculate_xrate(
                instrument=instrument,
                account=account,
                side=position.entry,
            )

            if xrate == Decimal(0):
                self._log.error(f"Cannot calculate open value (insufficient data for "
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

    cpdef Money unrealized_pnl(self, Symbol symbol):
        """
        Return the unrealized P&L for the given symbol (if found).

        Parameters
        ----------
        symbol : Symbol
            The symbol for the unrealized P&L.

        Returns
        -------
        Money or None

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data, "self._data")

        cdef Money pnl = self._unrealized_pnls.get(symbol)
        if pnl is not None:
            return pnl

        pnl = self._calculate_unrealized_pnl(symbol)
        self._unrealized_pnls[symbol] = pnl

        return pnl

    cpdef Money market_value(self, Symbol symbol):
        """
        Return the open value for the given symbol (if found).

        Parameters
        ----------
        symbol : Symbol
            The symbol for the open value.

        Returns
        -------
        Money or None

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(self._data, "self._data")

        cdef Account account = self._accounts.get(symbol.venue)
        if account is None:
            self._log.error(f"Cannot calculate open value "
                            f"(no account registered for {symbol.venue}).")
            return None

        cdef instrument = self._data.instrument(symbol)
        if instrument is None:
            self._log.error(f"Cannot calculate open value "
                            f"(no instrument for {symbol}).")
            return None  # Cannot calculate

        cdef set positions_open = self._positions_open.get(symbol.venue)
        if not positions_open:
            return Money(0, instrument.quote_currency)

        market_value: Decimal = Decimal(0)

        cdef Currency currency
        if account.default_currency is not None:
            currency = account.default_currency
        else:
            currency = instrument.base_currency

        cdef Position position
        cdef Price last
        for position in positions_open:
            if position.symbol != symbol:
                continue

            last = self._get_last_price(position)  # TODO: Optimize
            if last is None:
                self._log.error(f"Cannot calculate open value "
                                f"(no prices for {position.symbol}).")
                continue  # Cannot calculate

            xrate = self._calculate_xrate(
                instrument=instrument,
                account=account,
                side=position.entry,
            )

            if xrate == Decimal(0):
                self._log.error(f"Cannot calculate open value (insufficient data for "
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

    cpdef object net_position(self, Symbol symbol):
        """
        Return the net relative position for the given symbol. If no positions
        for symbol then will return `Decimal('0')`.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the query.

        Returns
        -------
        Decimal

        """
        return self._net_position(symbol)

    cpdef bint is_net_long(self, Symbol symbol) except *:
        """
        Return a value indicating whether the portfolio is net long the given
        symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the query.

        Returns
        -------
        bool
            True if net long, else False.

        """
        Condition.not_none(symbol, "symbol")

        return self._net_position(symbol) > Decimal(0)

    cpdef bint is_net_short(self, Symbol symbol) except *:
        """
        Return a value indicating whether the portfolio is net short the given
        symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the query.

        Returns
        -------
        bool
            True if net short, else False.

        """
        Condition.not_none(symbol, "symbol")

        return self._net_position(symbol) < Decimal(0)

    cpdef bint is_flat(self, Symbol symbol) except *:
        """
        Return a value indicating whether the portfolio is flat for the given
        symbol.

        Parameters
        ----------
        symbol : Symbol, optional
            The symbol query filter.

        Returns
        -------
        bool
            True if net flat, else False.

        """
        Condition.not_none(symbol, "symbol")

        return self._net_position(symbol) == Decimal(0)

    cpdef bint is_completely_flat(self) except *:
        """
        Return a value indicating whether the portfolio is completely flat.

        Returns
        -------
        bool
            True if net flat across all symbols, else False.

        """
        for net_position in self._net_positions.values():
            if net_position != Decimal(0):
                return False

        return True

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline object _net_position(self, Symbol symbol):
        return self._net_positions.get(symbol, Decimal(0))

    cdef inline set _symbols_open_for_venue(self, Venue venue):
        cdef Position position
        cdef set positions_open = self._positions_open.get(venue)
        if positions_open is None:
            return set()
        return {position.symbol for position in positions_open}

    cdef inline void _handle_position_opened(self, PositionOpened event) except *:
        cdef Venue venue = event.position.symbol.venue
        cdef Position position = event.position

        # Add to positions open
        cdef set positions_open = self._positions_open.get(venue, set())
        positions_open.add(position)
        self._positions_open[venue] = positions_open

        self._update_net_position(event.position.symbol, positions_open)

    cdef inline void _handle_position_modified(self, PositionModified event) except *:
        cdef Venue venue = event.position.symbol.venue
        self._update_net_position(event.position.symbol, self._positions_open.get(venue, set()))

    cdef inline void _handle_position_closed(self, PositionClosed event) except *:
        cdef Venue venue = event.position.symbol.venue
        cdef Position position = event.position

        # Remove from positions open if found
        cdef set positions_open = self._positions_open.get(venue)
        if positions_open is not None:
            positions_open.discard(position)

        # Add to positions closed
        cdef set positions_closed = self._positions_closed.get(venue, set())
        positions_closed.add(position)
        self._positions_closed[venue] = positions_closed

        self._update_net_position(event.position.symbol, positions_open)

    cdef inline void _update_net_position(self, Symbol symbol, set positions_open) except *:
        net_position = Decimal()
        for position in positions_open:
            if position.symbol == symbol:
                net_position += position.relative_quantity

        self._net_positions[symbol] = net_position
        self._update_maint_margin(symbol.venue)
        self._log.info(f"{symbol} net_position={net_position}")

    cdef inline void _update_init_margin(self, Venue venue) except *:
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
            instrument = self._data.instrument(order.symbol)
            if instrument is None:
                self._log.error(f"Cannot calculate initial margin "
                                f"(no instrument for {order.symbol}).")
                continue  # Cannot calculate

            if instrument.leverage == 1:
                continue  # No margin necessary

            # Calculate margin
            margin = instrument.calculate_init_margin(
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

                if xrate == Decimal(0):
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

        for currency, total_margin in margins.items():
            account.update_init_margin(Money(total_margin, currency))

            self._log.info(f"{venue}-{currency} init_margin={total_margin}")

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
            instrument = self._data.instrument(position.symbol)
            if instrument is None:
                self._log.error(f"Cannot calculate position maintenance margin "
                                f"(no instrument for {position.symbol}).")
                continue  # Cannot calculate

            if instrument.leverage == 1:
                continue  # No margin necessary

            last = self._get_last_price(position)  # TODO: Optimize
            if last is None:
                self._log.error(f"Cannot calculate position maintenance margin "
                                f"(no prices for {position.symbol}).")
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

                if xrate == Decimal(0):
                    self._log.error(f"Cannot calculate unrealized P&L (insufficient data for "
                                    f"{instrument.settlement_currency}/{currency}).")
                    continue  # Cannot calculate

                margin *= xrate
            else:
                currency = instrument.settlement_currency

            # Update total margin
            total_margin = margins.get(currency, Decimal(0))
            total_margin += margin
            margins[currency] = total_margin

        for currency, total_margin in margins.items():
            account.update_maint_margin(Money(total_margin, currency))

            self._log.info(f"{venue}-{currency} maint_margin={total_margin}")

    cdef Money _calculate_unrealized_pnl(self, Symbol symbol):
        cdef Account account = self._accounts.get(symbol.venue)
        if account is None:
            self._log.error(f"Cannot calculate unrealized P&L "
                            f"(no account registered for {symbol.venue}).")
            return None  # Cannot calculate

        cdef Instrument instrument = self._data.instrument(symbol)
        if instrument is None:
            self._log.error(f"Cannot calculate unrealized P&L "
                            f"(no instrument for {symbol}).")
            return None  # Cannot calculate

        cdef Currency currency
        if account.default_currency is not None:
            currency = account.default_currency
        else:
            currency = instrument.settlement_currency

        cdef set positions_open = self._positions_open.get(symbol.venue)
        if positions_open is None:
            if account.default_currency is not None:
                return Money(0, account.default_currency)
            else:
                return Money(0, instrument.settlement_currency)

        total_pnl: Decimal = Decimal(0)

        cdef Position position
        cdef Price last
        for position in positions_open:
            if position.symbol != symbol:
                continue  # Nothing to calculate

            last = self._get_last_price(position)  # TODO: Optimize (could be long or short)
            if last is None:
                self._log.error(f"Cannot calculate unrealized P&L (no prices for {symbol}).")
                return None  # Cannot calculate

            pnl = position.unrealized_pnl(last)

            if account.default_currency is not None:
                xrate = self._calculate_xrate(
                    instrument=instrument,
                    account=account,
                    side=position.entry,
                )

                if xrate == Decimal(0):
                    self._log.error(f"Cannot calculate unrealized P&L (insufficient data for "
                                    f"{instrument.settlement_currency}/{currency}).")
                    return None  # Cannot calculate

                pnl *= xrate

            total_pnl += pnl

        return Money(total_pnl, currency)

    cdef object _calculate_xrate(self, Instrument instrument, Account account, OrderSide side):
        if account.default_currency is not None:
            return self._data.get_xrate(
                venue=instrument.symbol.venue,
                from_currency=instrument.settlement_currency,
                to_currency=account.default_currency,
                price_type=PriceType.BID if side == OrderSide.BUY else PriceType.ASK,
            )

        return Decimal(1)  # No conversion needed

    cdef inline Price _get_last_price(self, Position position):
        cdef QuoteTick quote_tick = self._data.quote_tick(position.symbol)
        if quote_tick is not None:
            if position.side == PositionSide.LONG:
                return quote_tick.bid
            elif position.side == PositionSide.SHORT:
                return quote_tick.ask
            else:
                raise RuntimeError(f"invalid PositionSide, "
                                   f"was {PositionSideParser.to_str(position.side)}")

        cdef TradeTick trade_tick = self._data.trade_tick(position.symbol)
        return trade_tick.price if trade_tick is not None else None
