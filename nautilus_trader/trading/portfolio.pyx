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
The `Portfolio` facilitate the management of trading operations.

The intended use case is for a single `Portfolio` instance per running system,
a fleet of trading strategies will organize around a portfolio with the help
of the `Trader`` class.

The portfolio can satisfy queries for accounting information, margin balances,
total risk exposures and total net positions.
"""

from decimal import Decimal

from nautilus_trader.cache.base cimport CacheFacade
from nautilus_trader.common.logging cimport LogColor
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.position_side cimport PositionSideParser
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.events cimport PositionEvent
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.orders.base cimport PassiveOrder
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

    cpdef dict net_exposures(self, Venue venue):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money unrealized_pnl(self, InstrumentId instrument_id):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money net_exposure(self, InstrumentId instrument_id):
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

    def __init__(
        self,
        CacheFacade cache not None,
        Clock clock not None,
        Logger logger=None,
    ):
        """
        Initialize a new instance of the ``Portfolio`` class.

        Parameters
        ----------
        cache : CacheFacade
            The read-only cache for the portfolio.
        clock : Clock
            The clock for the portfolio.
        logger : Logger
            The logger for the portfolio.

        """
        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = LoggerAdapter(component=type(self).__name__, logger=logger)
        self._cache = cache

        self._unrealized_pnls = {}   # type: dict[InstrumentId, Money]
        self._net_positions = {}     # type: dict[InstrumentId, Decimal]
        self._pending_calcs = set()  # type: set[InstrumentId]

        self.initialized = False

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
            If the account is already registered with the portfolio.

        """
        Condition.not_none(account, "account")

        account.register_portfolio(self)
        self._log.debug(f"Registered account {account.id}.")

    cpdef void initialize_orders(self) except *:
        """
        Initialize the portfolios orders.
        """
        Condition.not_none(self._cache, "self._cache")

        cdef list orders_working = self._cache.orders_working()

        cdef set venues = set()
        for order in orders_working:
            venues.add(order.instrument_id.venue)

        # Update initial margins to initialize portfolio
        initialized = True
        for venue in venues:
            result = self._update_initial_margin(
                venue=venue,
                orders_working=self._cache.orders_working(venue=venue),
            )
            if result is False:
                initialized = False

        cdef int working_count = len(orders_working)
        self._log.info(
            f"Initialized {working_count} working order{'' if working_count == 1 else 's'}.",
            color=LogColor.BLUE if working_count else LogColor.NORMAL,
        )

        self.initialized = initialized

    cpdef void initialize_positions(self) except *:
        """
        Initialize the portfolios positions.
        """
        Condition.not_none(self._cache, "self._cache")

        # Clean slate
        self._unrealized_pnls.clear()

        cdef list positions_open = self._cache.positions_open()

        cdef set venues = set()
        cdef set instruments = set()
        for position in positions_open:
            venues.add(position.instrument_id.venue)
            instruments.add(position.instrument_id)

        # Update maintenance margins to initialize portfolio
        initialized = True
        for venue in venues:
            result = self._update_maint_margin(
                venue=venue,
                positions_open=self._cache.positions_open(venue=venue),
            )
            if result is False:
                initialized = False

        # Update unrealized PnLs
        for instrument_id in instruments:
            self._update_net_position(
                instrument_id=instrument_id,
                positions_open=self._cache.positions_open(
                    venue=None,  # Faster query filtering
                    instrument_id=instrument_id,
                ),
            )
            self._unrealized_pnls[instrument_id] = self._calculate_unrealized_pnl(instrument_id)

        cdef int open_count = len(positions_open)
        self._log.info(
            f"Initialized {open_count} open position{'' if open_count == 1 else 's'}.",
            color=LogColor.BLUE if open_count else LogColor.NORMAL,
        )

        self.initialized = initialized

    cpdef void update_tick(self, QuoteTick tick) except *:
        """
        Update the portfolio with the given tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick to update with.

        """
        Condition.not_none(tick, "tick")

        self._unrealized_pnls[tick.instrument_id] = None

        if not self.initialized and tick.instrument_id in self._pending_calcs:
            orders_working = self._cache.orders_working(
                venue=None,  # Faster query filtering
                instrument_id=tick.instrument_id,
            )
            positions_open = self._cache.positions_open(
                venue=None,  # Faster query filtering
                instrument_id=tick.instrument_id,
            )

            # Initialize initial margin
            result_init = self._update_initial_margin(
                venue=tick.instrument_id.venue,
                orders_working=orders_working,
            )

            # Initialize maintenance margin
            result_maint = self._update_maint_margin(
                venue=tick.instrument_id.venue,
                positions_open=positions_open,
            )

            # Calculate unrealized PnL
            result_unrealized_pnl = self._calculate_unrealized_pnl(tick.instrument_id)

            # Check portfolio initialization
            if result_init and result_maint and result_unrealized_pnl:
                self._pending_calcs.discard(tick.instrument_id)
                if not self._pending_calcs:
                    self.initialized = True

    cpdef void update_order(self, Order order) except *:
        """
        Update the portfolio with the given order.

        Parameters
        ----------
        order : Order
            The order to update with.

        """
        Condition.not_none(order, "order")
        Condition.not_none(self._cache, "self._cache")

        cdef list orders_working = self._cache.orders_working(
            venue=None,  # Faster query filtering
            instrument_id=order.instrument_id,
        )
        self._update_initial_margin(order.instrument_id.venue, orders_working)

    cpdef void update_position(self, PositionEvent event) except *:
        """
        Update the portfolio with the given position event.

        Parameters
        ----------
        event : PositionEvent
            The event to update with.

        """
        Condition.not_none(event, "event")
        Condition.not_none(self._cache, "self._cache")

        cdef list positions_open = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=event.instrument_id,
        )
        self._update_net_position(
            instrument_id=event.instrument_id,
            positions_open=positions_open
        )
        self._update_maint_margin(
            venue=event.instrument_id.venue,
            positions_open=positions_open,
        )

        self._unrealized_pnls[event.instrument_id] = self._calculate_unrealized_pnl(
            instrument_id=event.instrument_id,
        )

        self._log.debug(f"Updated {event.position_status}.")

    cpdef void reset(self) except *:
        """
        Reset the portfolio.

        All stateful fields are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        self._net_positions.clear()
        self._unrealized_pnls.clear()
        self._pending_calcs.clear()

        self.initialized = False

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
        Condition.not_none(self._cache, "self._cache")

        cdef Account account = self._cache.account_for_venue(venue)
        # TODO(cs): Assumption that account.id.issues = venue
        if account is None:
            self._log.error(
                f"Cannot get account: "
                f"no account registered for {venue}."
            )

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
        Condition.not_none(self._cache, "self._cache")

        cdef Account account = self._cache.account_for_venue(venue)
        if account is None:
            self._log.error(
                f"Cannot calculate order margin: "
                f"no account registered for {venue}."
            )
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
        Condition.not_none(self._cache, "self._cache")

        cdef Account account = self._cache.account_for_venue(venue)
        if account is None:
            self._log.error(
                f"Cannot calculate position margin: "
                f"no account registered for {venue}."
            )
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
        Condition.not_none(self._cache, "self._cache")

        cdef list positions_open = self._cache.positions_open(venue)
        if not positions_open:
            return {}  # Nothing to calculate

        cdef set instrument_ids = {p.instrument_id for p in positions_open}
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
                continue  # Error logged in `_calculate_unrealized_pnl`
            unrealized_pnls[pnl.currency] = unrealized_pnls.get(pnl.currency, Decimal(0)) + pnl

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
        dict[Currency, Money] or None

        """
        Condition.not_none(venue, "venue")
        Condition.not_none(self._cache, "self._cache")

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

        cdef dict net_exposures = {}  # type: dict[Currency, Decimal]

        cdef Position position
        cdef Instrument instrument
        cdef Price last
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

            xrate: Decimal = self._calculate_xrate_to_base(
                instrument=instrument,
                account=account,
                side=position.entry,
            )

            if xrate == 0:
                self._log.error(
                    f"Cannot calculate net exposures: "
                    f"insufficient data for {instrument.get_cost_currency()}/{account.base_currency}."
                )
                return None  # Cannot calculate

            net_exposure: Decimal = net_exposures.get(instrument.get_cost_currency(), Decimal(0))
            net_exposure += instrument.notional_value(
                position.quantity,
                last,
            ) * xrate

            if account.base_currency is not None:
                net_exposures[account.base_currency] = net_exposure
            else:
                net_exposures[instrument.get_cost_currency()] = net_exposure

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
        Money or None

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
        Money or None

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(self._cache, "self._cache")

        cdef Account account = self._cache.account_for_venue(instrument_id.venue)
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

        cdef list positions_open = self._cache.positions_open(instrument_id.venue)
        if not positions_open:
            return Money(0, instrument.get_cost_currency())

        net_exposure = Decimal(0)

        cdef Position position
        cdef Price last
        for position in positions_open:
            if position.instrument_id != instrument_id:
                continue

            last = self._get_last_price(position)
            if last is None:
                self._log.error(
                    f"Cannot calculate net exposure: "
                    f"no prices for {position.instrument_id}."
                )
                continue  # Cannot calculate

            xrate: Decimal = self._calculate_xrate_to_base(
                instrument=instrument,
                account=account,
                side=position.entry,
            )

            if xrate == 0:
                self._log.error(
                    f"Cannot calculate net exposure: "
                    f"insufficient data for {instrument.get_cost_currency()}/{account.base_currency}."
                )
                return None  # Cannot calculate

            net_exposure += instrument.notional_value(
                position.quantity,
                last,
            ) * xrate

        if account.base_currency is not None:
            return Money(net_exposure, account.base_currency)
        else:
            return Money(net_exposure, instrument.get_cost_currency())

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
        return self._net_position(instrument_id)

    cpdef bint is_net_long(self, InstrumentId instrument_id) except *:
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

        return self._net_position(instrument_id) > 0

    cpdef bint is_net_short(self, InstrumentId instrument_id) except *:
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

        return self._net_position(instrument_id) < 0

    cpdef bint is_flat(self, InstrumentId instrument_id) except *:
        """
        Return a value indicating whether the portfolio is flat for the given
        instrument ID.

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

    cdef object _net_position(self, InstrumentId instrument_id):
        return self._net_positions.get(instrument_id, Decimal(0))

    cdef void _update_net_position(self, InstrumentId instrument_id, list positions_open) except *:
        net_position = Decimal()
        for position in positions_open:
            net_position += position.net_qty

        self._net_positions[instrument_id] = net_position
        self._log.info(f"{instrument_id} net_position={net_position}")

    cdef bint _update_initial_margin(self, Venue venue, list orders_working) except *:
        # Filter only passive orders
        cdef list passive_orders_working = [o for o in orders_working if o.is_passive]
        if not passive_orders_working:
            return True  # Nothing to calculate

        cdef Account account = self._cache.account_for_venue(venue)
        if account is None:
            self._log.error(
                f"Cannot update initial margin: "
                f"no account registered for {venue}."
            )
            return False  # Cannot calculate

        cdef dict margins = {}  # type: dict[Currency, Decimal]

        cdef PassiveOrder order
        cdef Instrument instrument
        cdef Currency currency
        for order in passive_orders_working:
            instrument = self._cache.instrument(order.instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot calculate initial margin: "
                    f"no instrument for {order.instrument_id}."
                )
                self._pending_calcs.add(instrument.id)
                return False  # Cannot calculate

            # Calculate margin
            margin = instrument.calculate_initial_margin(
                order.quantity,
                order.price,
            )

            if account.base_currency is not None:
                currency = account.base_currency
                xrate: Decimal = self._calculate_xrate_to_base(
                    instrument=instrument,
                    account=account,
                    side=order.side,
                )

                if xrate == 0:
                    self._log.debug(
                        f"Cannot calculate initial margin: "
                        f"insufficient data for {instrument.get_cost_currency()}/{account.base_currency}."
                    )
                    self._pending_calcs.add(instrument.id)
                    return False  # Cannot calculate

                margin *= xrate
            else:
                currency = instrument.get_cost_currency()

            # Update total margin
            total_margin = margins.get(currency, Decimal(0))
            total_margin += margin
            margins[currency] = total_margin

        cdef Money total_margin_money
        for currency, total_margin in margins.items():
            total_margin_money = Money(total_margin, currency)
            account.update_initial_margin(total_margin_money)

            self._log.info(f"{venue} initial_margin={total_margin_money.to_str()}")

        return True

    cdef bint _update_maint_margin(self, Venue venue, list positions_open) except *:
        if not positions_open:
            return True  # Nothing to calculate

        cdef Account account = self._cache.account_for_venue(venue)
        if account is None:
            self._log.error(
                f"Cannot update position maintenance margin: "
                f"no account registered for {venue}."
            )
            return False  # Cannot calculate

        cdef dict margins = {}  # type: dict[Currency, Decimal]

        cdef Position position
        cdef Instrument instrument
        cdef Price last
        cdef Currency currency
        for position in positions_open:
            instrument = self._cache.instrument(position.instrument_id)
            if instrument is None:
                self._log.error(
                    f"Cannot calculate position maintenance margin: "
                    f"no instrument for {position.instrument_id}."
                )
                self._pending_calcs.add(instrument.id)
                return False  # Cannot calculate

            last = self._get_last_price(position)
            if last is None:
                self._log.debug(
                    f"Cannot calculate position maintenance margin: "
                    f"no prices for {position.instrument_id}."
                )
                self._pending_calcs.add(instrument.id)
                return False  # Cannot calculate

            # Calculate margin
            margin = instrument.calculate_maint_margin(
                position.side,
                position.quantity,
                last,
            )

            if account.base_currency is not None:
                currency = account.base_currency
                xrate: Decimal = self._calculate_xrate_to_base(
                    instrument=instrument,
                    account=account,
                    side=position.entry,
                )

                if xrate == 0:
                    self._log.debug(
                        f"Cannot calculate unrealized PnL: "
                        f"insufficient data for {instrument.get_cost_currency()}/{account.base_currency})."
                    )
                    self._pending_calcs.add(instrument.id)
                    return False  # Cannot calculate

                margin *= xrate
            else:
                currency = instrument.get_cost_currency()

            # Update total margin
            total_margin = margins.get(currency, Decimal(0))
            total_margin += margin
            margins[currency] = total_margin

        cdef Money total_margin_money
        for currency, total_margin in margins.items():
            total_margin_money = Money(total_margin, currency)
            account.update_maint_margin(total_margin_money)

            self._log.info(f"{venue} maint_margin={total_margin_money.to_str()}")

        return True

    cdef Money _calculate_unrealized_pnl(self, InstrumentId instrument_id):
        cdef Account account = self._cache.account_for_venue(instrument_id.venue)
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
            currency = instrument.get_cost_currency()

        cdef list positions_open = self._cache.positions_open(
            venue=None,  # Faster query filtering
            instrument_id=instrument_id,
        )
        if not positions_open:
            if account.base_currency is not None:
                return Money(0, account.base_currency)
            else:
                return Money(0, instrument.get_cost_currency())

        total_pnl: Decimal = Decimal(0)

        cdef Position position
        cdef Price last
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

            pnl = position.unrealized_pnl(last)

            if account.base_currency is not None:
                xrate: Decimal = self._calculate_xrate_to_base(
                    instrument=instrument,
                    account=account,
                    side=position.entry,
                )

                if xrate == 0:
                    self._log.debug(
                        f"Cannot calculate unrealized PnL: "
                        f"insufficient data for {instrument.get_cost_currency()}/{account.base_currency}."
                    )
                    self._pending_calcs.add(instrument.id)
                    return None  # Cannot calculate

                pnl *= xrate

            total_pnl += pnl

        return Money(total_pnl, currency)

    cdef object _calculate_xrate_to_base(self, Instrument instrument, Account account, OrderSide side):
        if account.base_currency is not None:
            return self._cache.get_xrate(
                venue=instrument.id.venue,
                from_currency=instrument.get_cost_currency(),
                to_currency=account.base_currency,
                price_type=PriceType.BID if side == OrderSide.BUY else PriceType.ASK,
            )

        return Decimal(1)  # No conversion needed

    cdef Price _get_last_price(self, Position position):
        cdef QuoteTick quote_tick = self._cache.quote_tick(position.instrument_id)
        if quote_tick is not None:
            if position.side == PositionSide.LONG:
                return quote_tick.bid
            elif position.side == PositionSide.SHORT:
                return quote_tick.ask
            else:
                raise RuntimeError(
                    f"invalid PositionSide, was {PositionSideParser.to_str(position.side)}",
                )

        cdef TradeTick trade_tick = self._cache.trade_tick(position.instrument_id)
        return trade_tick.price if trade_tick is not None else None
