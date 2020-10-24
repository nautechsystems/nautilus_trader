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

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.decimal cimport Decimal
from nautilus_trader.model.c_enums.asset_class cimport AssetClass
from nautilus_trader.model.c_enums.asset_type cimport AssetType
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.events cimport PositionClosed
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.events cimport PositionModified
from nautilus_trader.model.events cimport PositionOpened
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.position cimport Position
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.trading.account cimport Account
from nautilus_trader.trading.calculators cimport ExchangeRateCalculator


cdef class PortfolioFacade:
    """
    Provides a read-only facade for a `Portfolio`.
    """

# -- QUERIES ---------------------------------------------------------------------------------------

    cpdef Account account(self, Venue venue):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money order_margin(self, Venue venue):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money position_margin(self, Venue venue):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money unrealized_pnl_for_venue(self, Venue venue):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money unrealized_pnl_for_symbol(self, Symbol symbol):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money open_value(self, Venue venue):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Decimal net_position(self, Symbol symbol):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_net_long(self, Symbol symbol) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_net_short(self, Symbol symbol) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_flat(self, Symbol symbol) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef bint is_completely_flat(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class Portfolio(PortfolioFacade):
    """
    Provides a trading portfolio.
    """

    def __init__(
            self,
            Clock clock not None,
            UUIDFactory uuid_factory not None,
            Logger logger=None,
    ):
        """
        Initialize a new instance of the Portfolio class.

        Parameters
        ----------
        clock : Clock
            The clock for the component.
        uuid_factory : UUIDFactory
            The uuid factory for the component.
        logger : Logger
            The logger for the component.

        """
        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter(self.__class__.__name__, logger)
        self._xrate_calculator = ExchangeRateCalculator()

        self._instruments = {}             # type: {Symbol: Instrument}
        self._ticks = {}                   # type: {Symbol: QuoteTick}
        self._accounts = {}                # type: {Venue: Account}
        self._orders_working = {}          # type: {Venue: {Order}}
        self._positions_open = {}          # type: {Venue: {Position}}
        self._positions_closed = {}        # type: {Venue: {Position}}
        self._net_positions = {}           # type: {Symbol: Decimal}
        self._unrealized_pnls_symbol = {}  # type: {Symbol: Money}
        self._unrealized_pnls_venue = {}   # type: {Venue: Money}
        self._xrate_symbols = {}           # type: {Symbol: str}

# -- COMMANDS --------------------------------------------------------------------------------------

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
        Condition.not_in(account.id.issuer, self._accounts, "venue", "_accounts")

        self._accounts[account.id.issuer_as_venue()] = account
        account.register_portfolio(self)

    cpdef void update_instrument(self, Instrument instrument) except *:
        """
        Update the portfolio with the given instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument to update.

        """
        Condition.not_none(instrument, "instrument")

        self._instruments[instrument.symbol] = instrument

        if self._is_crypto_spot_or_swap(instrument) or self._is_fx_spot(instrument):
            self._xrate_symbols[instrument.symbol] = (f"{instrument.base_currency}/"
                                                      f"{instrument.quote_currency}")

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
            # Clear cached unrealized PNLs
            self._unrealized_pnls_symbol[tick.symbol] = None
            self._unrealized_pnls_venue[tick.symbol.venue] = None

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
            if order.is_working():
                orders_working = self._orders_working.get(order.symbol.venue, set())
                orders_working.add(order)
                self._orders_working[order.symbol.venue] = orders_working
                self._log.debug(f"Added working {order}")

        self._log.info(f"Updated {len(orders)} order(s) working.")

        cdef Venue venue
        for venue in self._orders_working.keys():
            self._update_order_margin(venue)

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
        if order.is_working():
            orders_working.add(order)
            self._orders_working[venue] = orders_working
            self._log.debug(f"Added working {order}")
        elif order.is_completed():
            orders_working.discard(order)

        self._update_order_margin(venue)

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
        self._unrealized_pnls_symbol.clear()

        cdef Position position
        cdef set positions_open
        cdef set positions_closed
        cdef int open_count = 0
        cdef int closed_count = 0
        for position in positions:
            if position.is_open():
                positions_open = self._positions_open.get(position.symbol.venue, set())
                positions_open.add(position)
                self._positions_open[position.symbol.venue] = positions_open
                self._log.debug(f"Added open {position}")
                open_count += 1
            elif position.is_closed():
                positions_closed = self._positions_closed.get(position.symbol.venue, set())
                positions_closed.add(position)
                self._positions_closed[position.symbol.venue] = positions_closed
                closed_count += 1

        self._log.info(f"Updated {open_count} position(s) open.")
        self._log.info(f"Updated {closed_count} position(s) closed.")

        cdef Venue venue
        cdef Symbol symbol
        for venue in self._positions_open.keys():
            self._update_position_margin(venue)
            for symbol in self._symbols_open_for_venue(venue):
                self._unrealized_pnls_symbol[symbol] = self._calculate_unrealized_pnl(symbol)

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
        self._update_position_margin(symbol.venue)
        self._unrealized_pnls_symbol[symbol] = self._calculate_unrealized_pnl(symbol)

    cpdef void reset(self) except *:
        """
        Reset the portfolio by returning all stateful values to their initial
        value.
        """
        self._log.debug(f"Resetting...")

        self._instruments.clear()
        self._ticks.clear()
        self._accounts.clear()
        self._orders_working.clear()
        self._positions_open.clear()
        self._positions_closed.clear()
        self._net_positions.clear()
        self._unrealized_pnls_symbol.clear()
        self._unrealized_pnls_venue.clear()
        self._xrate_symbols.clear()

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

    cpdef Decimal net_position(self, Symbol symbol):
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

        return self._net_position(symbol) > 0

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

        return self._net_position(symbol) < 0

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

        return self._net_position(symbol) == 0

    cpdef bint is_completely_flat(self) except *:
        """
        Return a value indicating whether the portfolio is completely flat.

        Returns
        -------
        bool
            True if net flat across all symbols, else False.

        """
        cdef Decimal net_position
        for net_position in self._net_positions.values():
            if net_position != 0:
                return False

        return True

    cpdef Money order_margin(self, Venue venue):
        """
        Return the order margin for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the order margin.

        Returns
        -------
        Money or None

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot get order margin (no account registered for {venue}).")
            return None

        return account.order_margin()

    cpdef Money position_margin(self, Venue venue):
        """
        Return the position margin for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the position margin.

        Returns
        -------
        Money or None

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot get position margin (no account registered for {venue}).")
            return None

        return account.position_margin()

    cpdef Money unrealized_pnl_for_venue(self, Venue venue):
        """
        Return the unrealized pnl for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the unrealized pnl.

        Returns
        -------
        Money or None

        """
        Condition.not_none(venue, "venue")

        cdef Money unrealized_pnl = self._unrealized_pnls_venue.get(venue)

        if unrealized_pnl is not None:
            return unrealized_pnl

        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot calculate unrealized PNL "
                            f"(no account registered for {venue}).")
            return None

        cdef set symbols = self._symbols_open_for_venue(venue)
        if not symbols:
            return Money(0, account.currency)

        cdef double cum_pnl = 0

        cdef Symbol symbol
        cdef Money pnl
        for symbol in symbols:
            pnl = self._unrealized_pnls_symbol.get(symbol)
            if pnl is not None:
                cum_pnl += pnl.as_double()
                continue
            pnl = self._calculate_unrealized_pnl(symbol)
            if pnl is None:
                return None
            cum_pnl += pnl.as_double()

        unrealized_pnl = Money(cum_pnl, account.currency)
        self._unrealized_pnls_venue[venue] = unrealized_pnl

        return unrealized_pnl

    cpdef Money unrealized_pnl_for_symbol(self, Symbol symbol):
        """
        Return the unrealized PNL for the given symbol (if found).

        Parameters
        ----------
        symbol : Symbol
            The symbol for the unrealized PNL.

        Returns
        -------
        Money or None

        """
        Condition.not_none(symbol, "symbol")

        cdef Money pnl = self._unrealized_pnls_symbol.get(symbol)
        if pnl is not None:
            return pnl

        pnl = self._calculate_unrealized_pnl(symbol)
        self._unrealized_pnls_symbol[symbol] = pnl

        return pnl

    cpdef Money open_value(self, Venue venue):
        """
        Return the open value for the given venue (if found).

        Parameters
        ----------
        venue : Venue
            The venue for the open value.

        Returns
        -------
        Money or None

        """
        Condition.not_none(venue, "venue")

        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot calculate open value (no account registered for {venue}).")
            return None

        cdef set positions_open = self._positions_open.get(venue)
        if not positions_open:
            return Money(0, account.currency)

        cdef tuple quotes = self._build_quote_table(venue)
        cdef dict bid_quotes = quotes[0]
        cdef dict ask_quotes = quotes[1]

        cdef double xrate
        cdef double open_value = 0
        cdef Position position
        cdef Instrument instrument
        cdef QuoteTick last
        for position in positions_open:
            instrument = self._instruments.get(position.symbol)
            if instrument is None:
                self._log.error(f"Cannot calculate open value "
                                f"(no instrument for {position.symbol}).")
                return None  # Cannot calculate

            last = self._ticks.get(position.symbol)
            if last is None:
                self._log.error(f"Cannot calculate position maintenance margin "
                                f"(no last tick for {position.symbol}).")
                continue  # Cannot calculate

            xrate = self._xrate_calculator.get_rate(
                from_currency=instrument.base_currency,
                to_currency=account.currency,
                price_type=PriceType.BID if position.entry == OrderSide.BUY else PriceType.ASK,
                bid_quotes=bid_quotes,
                ask_quotes=ask_quotes,
            )
            if xrate == 0:
                self._log.error(f"Cannot calculate open value (insufficient data for "
                                f"{position.base_currency}/{account.currency}).")
                return None  # Cannot calculate

            open_value += instrument.calculate_open_value(
                position.side,
                position.quantity,
                last,
            ) * xrate

        return Money(open_value, account.currency)

# -- INTERNAL --------------------------------------------------------------------------------------

    cdef inline Decimal _net_position(self, Symbol symbol):
        cdef Decimal net_position = self._net_positions.get(symbol)
        return net_position if net_position is not None else Decimal()

    cdef inline tuple _build_quote_table(self, Venue venue):
        cdef dict bid_quotes = {}
        cdef dict ask_quotes = {}

        cdef Symbol symbol
        cdef str base_quote
        cdef QuoteTick tick
        for symbol, base_quote in self._xrate_symbols.items():
            if symbol.venue != venue:
                continue

            tick = self._ticks.get(symbol)
            if tick is None:
                continue

            bid_quotes[base_quote] = tick.bid.as_double()
            ask_quotes[base_quote] = tick.ask.as_double()

        return bid_quotes, ask_quotes

    cdef inline set _symbols_open_for_venue(self, Venue venue):
        cdef Position position
        cdef set positions_open = self._positions_open.get(venue)
        if positions_open is None:
            return set()
        return {position.symbol for position in positions_open}

    cdef inline bint _is_crypto_spot_or_swap(self, Instrument instrument) except *:
        return instrument.asset_class == AssetClass.CRYPTO \
            and (instrument.asset_type == AssetType.SPOT or instrument.asset_type == AssetType.SWAP)

    cdef inline bint _is_fx_spot(self, Instrument instrument) except *:
        return instrument.asset_class == AssetClass.FX and instrument.asset_type == AssetType.SPOT

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

    cdef inline void _update_net_position(self, Symbol symbol, set positions_open):
        cdef Decimal net_position = Decimal()
        for position in positions_open:
            if position.symbol == symbol:
                net_position += position.relative_quantity()

        self._net_positions[symbol] = net_position
        self._log.info(f"{symbol} net position = {net_position}")

    cdef inline void _update_order_margin(self, Venue venue):
        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot update order initial margin "
                            f"(no account registered for {venue}).")
            return  # Cannot calculate

        cdef set working_orders = self._orders_working.get(venue)
        if working_orders is None:
            return  # Nothing to calculate

        cdef tuple quotes = self._build_quote_table(venue)
        cdef dict bid_quotes = quotes[0]
        cdef dict ask_quotes = quotes[1]

        cdef double xrate
        cdef double margin = 0
        cdef Order order
        cdef Instrument instrument
        for order in working_orders:
            instrument = self._instruments.get(order.symbol)
            if instrument is None:
                self._log.error(f"Cannot calculate order initial margin "
                                f"(no instrument for {order.symbol}).")
                continue  # Cannot calculate

            if instrument.leverage == 1:
                continue  # No margin necessary

            xrate = self._xrate_calculator.get_rate(
                from_currency=instrument.settlement_currency,
                to_currency=account.currency,
                price_type=PriceType.BID if order.side == OrderSide.SELL else PriceType.ASK,
                bid_quotes=bid_quotes,
                ask_quotes=ask_quotes,
            )
            if xrate == 0:
                self._log.error(f"Cannot calculate order initial margin (insufficient data for "
                                f"{instrument.base_currency}/{account.currency}).")
                continue  # Cannot calculate

            # Calculate margin
            margin += instrument.calculate_order_margin(
                order.quantity,
                order.price,
            ) * xrate

        cdef Money order_margin = Money(margin, account.currency)
        account.update_order_margin(order_margin)

        self._log.info(f"Updated {venue} order initial margin to "
                       f"{order_margin.to_string()}")

    cdef inline void _update_position_margin(self, Venue venue):
        cdef Account account = self._accounts.get(venue)
        if account is None:
            self._log.error(f"Cannot update position maintenance margin "
                            f"(no account registered for {venue}).")
            return  # Cannot calculate

        cdef set open_positions = self._positions_open.get(venue)
        if open_positions is None:
            return  # Nothing to calculate

        cdef tuple quotes = self._build_quote_table(venue)
        cdef dict bid_quotes = quotes[0]
        cdef dict ask_quotes = quotes[1]
        cdef double xrate
        cdef double margin = 0
        cdef Position position
        cdef Instrument instrument
        for position in open_positions:
            instrument = self._instruments.get(position.symbol)
            if instrument is None:
                self._log.error(f"Cannot calculate position maintenance margin "
                                f"(no instrument for {position.symbol}).")
                continue  # Cannot calculate

            if instrument.leverage == 1:
                continue  # No margin necessary

            last = self._ticks.get(position.symbol)
            if last is None:
                self._log.error(f"Cannot calculate position maintenance margin "
                                f"(no last tick for {position.symbol}).")
                continue  # Cannot calculate

            xrate = self._xrate_calculator.get_rate(
                from_currency=instrument.base_currency,
                to_currency=account.currency,
                price_type=PriceType.BID if position.entry == OrderSide.BUY else PriceType.ASK,
                bid_quotes=bid_quotes,
                ask_quotes=ask_quotes,
            )
            if xrate == 0:
                self._log.error(f"Cannot calculate position maintenance margin "
                                f"(insufficient data for {instrument.base_currency}/{account.currency}).")
                continue  # Cannot calculate

            # Calculate margin
            margin += instrument.calculate_position_margin(
                position.side,
                position.quantity,
                last,
            ) * xrate

        cdef Money position_margin = Money(margin, account.currency)
        account.update_position_margin(position_margin)

        self._log.info(f"Updated {venue} position maintenance margin to "
                       f"{position_margin.to_string()}")

    cdef Money _calculate_unrealized_pnl(self, Symbol symbol):
        cdef Account account = self._accounts.get(symbol.venue)
        if account is None:
            self._log.error(f"Cannot calculate unrealized PNL "
                            f"(no account registered for {symbol.venue}).")
            return None

        cdef set positions_open = self._positions_open.get(symbol.venue)
        if positions_open is None:
            return Money(0, account.currency)

        cdef QuoteTick last = self._ticks.get(symbol)
        if last is None:
            self._log.error(f"Cannot calculate unrealized PNL "
                            f"(no quotes for {symbol}).")
            return None  # Cannot calculate

        cdef Instrument instrument = self._instruments.get(symbol)
        if instrument is None:
            self._log.error(f"Cannot calculate unrealized PNL "
                            f"(no instrument for {symbol}).")
            return None  # Cannot calculate

        cdef tuple quotes = self._build_quote_table(symbol.venue)
        cdef dict bid_quotes = quotes[0]
        cdef dict ask_quotes = quotes[1]

        cdef double pnl = 0
        cdef double xrate = 0
        cdef Position position
        for position in positions_open:
            if position.symbol != symbol:
                continue  # Nothing to calculate

            if xrate == 0:
                xrate = self._xrate_calculator.get_rate(
                    from_currency=instrument.base_currency,
                    to_currency=account.currency,
                    price_type=PriceType.BID if position.entry == OrderSide.BUY else PriceType.ASK,
                    bid_quotes=bid_quotes,
                    ask_quotes=ask_quotes,
                )
            if xrate == 0:
                self._log.error(f"Cannot calculate unrealized PNL (insufficient data for "
                                f"{position.base_currency}/{account.currency}).")
                return None  # Cannot calculate

            pnl += instrument.calculate_pnl(
                position.side,
                position.quantity,
                position.avg_open,
                last.bid if position.side == PositionSide.LONG else last.ask,
            ) * xrate

        return Money(pnl, account.currency)
