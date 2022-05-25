# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import cython

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.order_side cimport OrderSide
from nautilus_trader.model.c_enums.order_side cimport OrderSideParser
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.c_enums.position_side cimport PositionSideParser
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


@cython.auto_pickle(True)
cdef class Position:
    """
    Represents a position in a financial market.

    The position ID may be assigned at the trading venue, or can be system
    generated depending on a strategies OMS (Order Management System) settings.

    Parameters
    ----------
    instrument : Instrument
        The trading instrument for the position.
    fill : OrderFilled
        The order fill event which opened the position.

    Raises
    ------
    ValueError
        If `instrument.id` is not equal to `fill.instrument_id`.
    ValueError
        If `event.position_id` is ``None``.
    """

    def __init__(
        self,
        Instrument instrument not None,
        OrderFilled fill not None,
    ):
        Condition.equal(instrument.id, fill.instrument_id, "instrument.id", "fill.instrument_id")
        Condition.not_none(fill.position_id, "fill.position_id")

        self._events = []     # type: list[OrderFilled]
        self._trade_ids = []  # type: list[TradeId]
        self._buy_qty = Quantity.zero_c(precision=instrument.size_precision)
        self._sell_qty = Quantity.zero_c(precision=instrument.size_precision)
        self._commissions = {}

        # Identifiers
        self.trader_id = fill.trader_id
        self.strategy_id = fill.strategy_id
        self.instrument_id = fill.instrument_id
        self.id = fill.position_id
        self.account_id = fill.account_id
        self.opening_order_id = fill.client_order_id
        self.closing_order_id = None

        # Properties
        self.entry = fill.order_side
        self.side = Position.side_from_order_side(fill.order_side)
        self.net_qty = 0.0
        self.quantity = Quantity.zero_c(precision=instrument.size_precision)
        self.peak_qty = Quantity.zero_c(precision=instrument.size_precision)
        self.ts_init = fill.ts_init
        self.ts_opened = fill.ts_event
        self.ts_last = fill.ts_event
        self.ts_closed = 0
        self.duration_ns = 0
        self.avg_px_open = fill.last_px.as_f64_c()
        self.avg_px_close = 0.0
        self.price_precision = instrument.price_precision
        self.size_precision = instrument.size_precision
        self.multiplier = instrument.multiplier
        self.is_inverse = instrument.is_inverse
        self.quote_currency = instrument.quote_currency
        self.base_currency = instrument.get_base_currency()  # Can be None
        self.cost_currency = instrument.get_cost_currency()

        self.realized_return = 0.0
        self.realized_pnl = Money(0, self.cost_currency)

        self.apply(fill)

    def __eq__(self, Position other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.info()}, id={self.id})"

    cpdef str info(self):
        """
        Return a summary description of the position.

        Returns
        -------
        str

        """
        cdef str quantity = " " if self.quantity._mem.raw == 0 else f" {self.quantity.to_str()} "
        return f"{PositionSideParser.to_str(self.side)}{quantity}{self.instrument_id}"

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "position_id": self.id.to_str(),
            "account_id": self.account_id.to_str(),
            "opening_order_id": self.opening_order_id.to_str(),
            "closing_order_id": self.closing_order_id.to_str() if self.closing_order_id is not None else None,
            "strategy_id": self.strategy_id.to_str(),
            "instrument_id": self.instrument_id.to_str(),
            "entry": OrderSideParser.to_str(self.entry),
            "side": PositionSideParser.to_str(self.side),
            "net_qty": self.net_qty,
            "quantity": str(self.quantity),
            "peak_qty": str(self.peak_qty),
            "ts_opened": self.ts_opened,
            "ts_closed": self.ts_closed,
            "duration_ns": self.duration_ns,
            "avg_px_open": str(self.avg_px_open),
            "avg_px_close": str(self.avg_px_close),
            "quote_currency": self.quote_currency.code,
            "base_currency": self.base_currency.code if self.base_currency is not None else None,
            "cost_currency": self.cost_currency.code,
            "realized_return": str(round(self.realized_return, 5)),
            "realized_pnl": str(self.realized_pnl.to_str()),
            "commissions": str([c.to_str() for c in self.commissions()]),
        }

    cdef list client_order_ids_c(self):
        # Note the inner set {}
        return sorted(list({fill.client_order_id for fill in self._events}))

    cdef list venue_order_ids_c(self):
        # Note the inner set {}
        return sorted(list({fill.venue_order_id for fill in self._events}))

    cdef list trade_ids_c(self):
        # Checked for duplicate before appending to events
        return [fill.trade_id for fill in self._events]

    cdef list events_c(self):
        return self._events.copy()

    cdef OrderFilled last_event_c(self):
        return self._events[-1]

    cdef TradeId last_trade_id_c(self):
        return self._events[-1].trade_id

    cdef int event_count_c(self) except *:
        return len(self._events)

    cdef bint is_open_c(self) except *:
        return self.side != PositionSide.FLAT

    cdef bint is_closed_c(self) except *:
        return self.side == PositionSide.FLAT

    cdef bint is_long_c(self) except *:
        return self.side == PositionSide.LONG

    cdef bint is_short_c(self) except *:
        return self.side == PositionSide.SHORT

    @property
    def symbol(self):
        """
        The positions ticker symbol.

        Returns
        -------
        Symbol

        """
        return self.instrument_id.symbol

    @property
    def venue(self):
        """
        The positions trading venue.

        Returns
        -------
        Venue

        """
        return self.instrument_id.venue

    @property
    def client_order_ids(self):
        """
        The client order IDs associated with the position.

        Returns
        -------
        list[VenueOrderId]

        Notes
        -----
        Guaranteed not to contain duplicate IDs.

        """
        return self.client_order_ids_c()

    @property
    def venue_order_ids(self):
        """
        The venue order IDs associated with the position.

        Returns
        -------
        list[VenueOrderId]

        Notes
        -----
        Guaranteed not to contain duplicate IDs.

        """
        return self.venue_order_ids_c()

    @property
    def trade_ids(self):
        """
        The trade match IDs associated with the position.

        Returns
        -------
        list[TradeId]

        """
        return self.trade_ids_c()

    @property
    def events(self):
        """
        The order fill events of the position.

        Returns
        -------
        list[Event]

        """
        return self.events_c()

    @property
    def last_event(self):
        """
        The last order fill event.

        Returns
        -------
        OrderFilled

        """
        return self.last_event_c()

    @property
    def last_trade_id(self):
        """
        The last trade match ID for the position.

        Returns
        -------
        TradeId

        """
        return self.last_trade_id_c()

    @property
    def event_count(self):
        """
        The count of order fill events applied to the position.

        Returns
        -------
        int

        """
        return self.event_count_c()

    @property
    def is_open(self):
        """
        If the position side is **not** ``FLAT``.

        Returns
        -------
        bool

        """
        return self.is_open_c()

    @property
    def is_closed(self):
        """
        If the position side is ``FLAT``.

        Returns
        -------
        bool

        """
        return self.is_closed_c()

    @property
    def is_long(self):
        """
        If the position side is ``LONG``.

        Returns
        -------
        bool

        """
        return self.is_long_c()

    @property
    def is_short(self):
        """
        If the position side is ``SHORT``.

        Returns
        -------
        bool

        """
        return self.is_short_c()

    @staticmethod
    cdef PositionSide side_from_order_side_c(OrderSide side) except *:
        if side == OrderSide.BUY:
            return PositionSide.LONG
        elif side == OrderSide.SELL:
            return PositionSide.SHORT
        else:
            raise ValueError(f"side was invalid, was {side}")

    @staticmethod
    def side_from_order_side(OrderSide side):
        """
        Return the position side resulting from the given order side (from ``FLAT``).

        Parameters
        ----------
        side : OrderSide {``BUY``, ``SELL``}
            The order side

        Returns
        -------
        PositionSide

        """
        return Position.side_from_order_side_c(side)

    cpdef bint is_opposite_side(self, OrderSide side) except *:
        """
        Return a value indicating whether the given order side is opposite to
        the current position side.

        Parameters
        ----------
        side : OrderSide {``BUY``, ``SELL``}

        Returns
        -------
        bool
            True if side is opposite, else False.

        """
        return self.side != Position.side_from_order_side_c(side)

    cpdef void apply(self, OrderFilled fill) except *:
        """
        Applies the given order fill event to the position.

        Parameters
        ----------
        fill : OrderFilled
            The order fill event to apply.

        Raises
        ------
        KeyError
            If `fill.trade_id` already applied to the position.

        """
        Condition.not_none(fill, "fill")
        Condition.not_in(fill.trade_id, self._trade_ids, "fill.trade_id", "_trade_ids")

        self._events.append(fill)
        self._trade_ids.append(fill.trade_id)

        if self.side == PositionSide.FLAT:
            self.opening_order_id = fill.client_order_id

        # Calculate cumulative commission
        cdef Currency currency = fill.commission.currency
        cdef Money commissions = self._commissions.get(currency)
        cdef double total_commissions = commissions.as_f64_c() if commissions is not None else 0.0
        self._commissions[currency] = Money(total_commissions + fill.commission.as_f64_c(), currency)

        # Calculate avg prices, points, return, PnL
        if fill.order_side == OrderSide.BUY:
            self._handle_buy_order_fill(fill)
        elif fill.order_side == OrderSide.SELL:
            self._handle_sell_order_fill(fill)
        else:  # pragma: no cover
            raise ValueError(f"invalid OrderSide, was {fill.order_side}")

        # Set quantities
        self.quantity = Quantity(abs(self.net_qty), self.size_precision)
        if self.quantity._mem.raw > self.peak_qty._mem.raw:
            self.peak_qty = self.quantity

        # Set state
        if self.net_qty > 0.0:
            self.entry = OrderSide.BUY
            self.side = PositionSide.LONG
            self.ts_closed = 0
            self.duration_ns = 0
        elif self.net_qty < 0.0:
            self.entry = OrderSide.SELL
            self.side = PositionSide.SHORT
            self.ts_closed = 0
            self.duration_ns = 0
        else:
            self.side = PositionSide.FLAT
            self.closing_order_id = fill.client_order_id
            self.ts_closed = fill.ts_event
            self.duration_ns = self.ts_closed - self.ts_opened

        self.ts_last = fill.ts_event

    cpdef Money notional_value(self, Price last):
        """
        Return the current notional value of the position.

        Parameters
        ----------
        last : Price
            The last close price for the position.

        Returns
        -------
        Money
            In quote currency.

        """
        Condition.not_none(last, "last")

        if self.is_inverse:
            return Money(self.quantity.as_f64_c() * self.multiplier.as_f64_c() * (1.0 / last.as_f64_c()), self.base_currency)
        else:
            return Money(self.quantity.as_f64_c() * self.multiplier.as_f64_c() * last.as_f64_c(), self.quote_currency)

    cpdef Money calculate_pnl(
        self,
        double avg_px_open,
        double avg_px_close,
        Quantity quantity,
    ):
        """
        Return a calculated PnL.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        avg_px_open : double
            The average open price.
        avg_px_close : double
            The average close price.
        quantity : Quantity
            The quantity for the calculation.

        Returns
        -------
        Money

        """
        cdef double pnl = self._calculate_pnl(
            avg_px_open=avg_px_open,
            avg_px_close=avg_px_close,
            quantity=quantity.as_f64_c(),
        )

        return Money(pnl, self.cost_currency)

    cpdef Money unrealized_pnl(self, Price last):
        """
        Return the unrealized PnL from the given last quote tick.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        last : Price
            The last price for the calculation.

        Returns
        -------
        Money

        """
        Condition.not_none(last, "last")

        if self.side == PositionSide.FLAT:
            return Money(0, self.quote_currency)

        cdef double pnl = self._calculate_pnl(
            avg_px_open=self.avg_px_open,
            avg_px_close=last.as_f64_c(),
            quantity=self.quantity.as_f64_c(),
        )

        return Money(pnl, self.cost_currency)

    cpdef Money total_pnl(self, Price last):
        """
        Return the total PnL from the given last quote tick.

        Result will be in quote currency for standard instruments, or base
        currency for inverse instruments.

        Parameters
        ----------
        last : Price
            The last price for the calculation.

        Returns
        -------
        Money

        """
        Condition.not_none(last, "last")

        cdef double pnl = self.realized_pnl.as_f64_c() + self.unrealized_pnl(last).as_f64_c()
        return Money(pnl, self.cost_currency)

    cpdef list commissions(self):
        """
        Return the total commissions generated by the position.

        Returns
        -------
        list[Money]

        """
        return list(self._commissions.values())

    cdef void _handle_buy_order_fill(self, OrderFilled fill) except *:
        # Initialize realized PnL for fill
        if fill.commission.currency == self.cost_currency:
            realized_pnl = -fill.commission.as_f64_c()
        else:
            realized_pnl = 0.0

        # LONG POSITION
        if self.net_qty > 0:
            self.avg_px_open = self._calculate_avg_px_open_px(fill)
        # SHORT POSITION
        elif self.net_qty < 0:
            self.avg_px_close = self._calculate_avg_px_close_px(fill)
            self.realized_return = self._calculate_return(self.avg_px_open, self.avg_px_close)
            realized_pnl += self._calculate_pnl(self.avg_px_open, fill.last_px.as_f64_c(), fill.last_qty.as_f64_c())

        self.realized_pnl = Money(self.realized_pnl.as_f64_c() + realized_pnl, self.cost_currency)

        # Update quantities
        self._buy_qty.add_assign(fill.last_qty)
        self.net_qty += fill.last_qty.as_f64_c()
        self.net_qty = round(self.net_qty, self.size_precision)

    cdef void _handle_sell_order_fill(self, OrderFilled fill) except *:
        # Initialize realized PnL for fill
        if fill.commission.currency == self.cost_currency:
            realized_pnl = -fill.commission.as_f64_c()
        else:
            realized_pnl = 0.0

        # SHORT POSITION
        if self.net_qty < 0:
            self.avg_px_open = self._calculate_avg_px_open_px(fill)
        # LONG POSITION
        elif self.net_qty > 0:
            self.avg_px_close = self._calculate_avg_px_close_px(fill)
            self.realized_return = self._calculate_return(self.avg_px_open, self.avg_px_close)
            realized_pnl += self._calculate_pnl(self.avg_px_open, fill.last_px.as_f64_c(), fill.last_qty.as_f64_c())

        self.realized_pnl = Money(self.realized_pnl.as_f64_c() + realized_pnl, self.cost_currency)

        # Update quantities
        self._sell_qty.add_assign(fill.last_qty)
        self.net_qty -= fill.last_qty.as_f64_c()
        self.net_qty = round(self.net_qty, self.size_precision)

    cdef double _calculate_avg_px_open_px(self, OrderFilled fill):
        return self._calculate_avg_px(self.quantity.as_f64_c(), self.avg_px_open, fill)

    cdef double _calculate_avg_px_close_px(self, OrderFilled fill):
        if not self.avg_px_close:
            return fill.last_px
        close_qty = self._sell_qty if self.side == PositionSide.LONG else self._buy_qty
        return self._calculate_avg_px(close_qty.as_f64_c(), self.avg_px_close, fill)

    cdef double _calculate_avg_px(
        self,
        double qty,
        double avg_px,
        OrderFilled fill,
    ):
        cdef double start_cost = avg_px * qty
        cdef double event_cost = fill.last_px.as_f64_c() * fill.last_qty.as_f64_c()
        cdef double cum_qty = qty + fill.last_qty.as_f64_c()
        return (start_cost + event_cost) / cum_qty

    cdef double _calculate_points(self, double avg_px_open, double avg_px_close):
        if self.side == PositionSide.LONG:
            return avg_px_close - avg_px_open
        elif self.side == PositionSide.SHORT:
            return avg_px_open - avg_px_close
        else:
            return 0.0  # FLAT

    cdef double _calculate_points_inverse(self, double avg_px_open, double avg_px_close):
        if self.side == PositionSide.LONG:
            return (1.0 / avg_px_open) - (1.0 / avg_px_close)
        elif self.side == PositionSide.SHORT:
            return (1.0 / avg_px_close) - (1.0 / avg_px_open)
        else:
            return 0.0  # FLAT

    cdef double _calculate_return(self, double avg_px_open, double avg_px_close):
        return self._calculate_points(avg_px_open, avg_px_close) / avg_px_open

    cdef double _calculate_pnl(
        self,
        double avg_px_open,
        double avg_px_close,
        double quantity,
    ):
        if self.is_inverse:
            # In base currency
            return quantity * self.multiplier.as_f64_c() * self._calculate_points_inverse(avg_px_open, avg_px_close)
        else:
            # In quote currency
            return quantity * self.multiplier.as_f64_c() * self._calculate_points(avg_px_open, avg_px_close)
