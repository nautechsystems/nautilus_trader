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

from decimal import Decimal

from libc.math cimport fabs
from libc.math cimport fmin

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.functions cimport order_side_from_str
from nautilus_trader.model.functions cimport order_side_to_str
from nautilus_trader.model.functions cimport position_side_from_str
from nautilus_trader.model.functions cimport position_side_to_str
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport TradeId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class Position:
    """
    Represents a position in a market.

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
        If `fill.position_id` is ``None``.
    """

    def __init__(
        self,
        Instrument instrument not None,
        OrderFilled fill not None,
    ) -> None:
        Condition.equal(instrument.id, fill.instrument_id, "instrument.id", "fill.instrument_id")
        Condition.not_none(fill.position_id, "fill.position_id")

        # Create a new position with the opening fill
        Position._init_(
            self=self,
            trader_id=fill.trader_id,
            strategy_id=fill.strategy_id,
            instrument_id=fill.instrument_id,
            position_id=fill.position_id,
            account_id=fill.account_id,
            opening_order_id=fill.client_order_id,
            closing_order_id=None,
            entry=fill.order_side,
            side=Position.side_from_order_side_c(fill.order_side),
            signed_qty=0.0,
            quantity=Quantity.zero_c(precision=instrument.size_precision),
            peak_qty=Quantity.zero_c(precision=instrument.size_precision),
            price_precision=instrument.price_precision,
            size_precision=instrument.size_precision,
            multiplier=instrument.multiplier,
            is_inverse=instrument.is_inverse,
            quote_currency=instrument.quote_currency,
            base_currency=instrument.get_base_currency(),
            settlement_currency=instrument.get_settlement_currency(),
            ts_init=fill.ts_init,
            ts_opened=fill.ts_event,
            ts_last=fill.ts_event,
            ts_closed=0,
            duration_ns=0,
            avg_px_open=fill.last_px.as_f64_c(),
            avg_px_close=0.0,
            realized_return=0.0,
            realized_pnl=None,
            buy_qty=Quantity.zero_c(precision=instrument.size_precision),
            sell_qty=Quantity.zero_c(precision=instrument.size_precision),
            commissions_dict={},
            events=[],
            trade_ids=[],
        )

        # Apply the opening fill
        self.apply(fill)

    @staticmethod
    cdef Position _init_(
        Position self,
        TraderId trader_id,
        StrategyId strategy_id,
        InstrumentId instrument_id,
        PositionId position_id,
        AccountId account_id,
        ClientOrderId opening_order_id,
        ClientOrderId closing_order_id,
        OrderSide entry,
        PositionSide side,
        double signed_qty,
        Quantity quantity,
        Quantity peak_qty,
        uint8_t price_precision,
        uint8_t size_precision,
        Quantity multiplier,
        bint is_inverse,
        Currency quote_currency,
        Currency base_currency,
        Currency settlement_currency,
        uint64_t ts_init,
        uint64_t ts_opened,
        uint64_t ts_last,
        uint64_t ts_closed,
        uint64_t duration_ns,
        double avg_px_open,
        double avg_px_close,
        double realized_return,
        Money realized_pnl,
        Quantity buy_qty,
        Quantity sell_qty,
        dict commissions_dict=None,
        list events=None,
        list trade_ids=None,
    ):
        """
        Initialize a Position self with all parameters.

        This is an internal method used for initialization by both the constructor
        and factory methods.
        """
        # Identifiers
        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.instrument_id = instrument_id
        self.id = position_id
        self.account_id = account_id
        self.opening_order_id = opening_order_id
        self.closing_order_id = closing_order_id

        # Properties
        self.entry = entry
        self.side = side
        self.signed_qty = signed_qty
        self.quantity = quantity
        self.peak_qty = peak_qty
        self.ts_init = ts_init
        self.ts_opened = ts_opened
        self.ts_last = ts_last
        self.ts_closed = ts_closed
        self.duration_ns = duration_ns
        self.avg_px_open = avg_px_open
        self.avg_px_close = avg_px_close
        self.price_precision = price_precision
        self.size_precision = size_precision
        self.multiplier = multiplier
        self.is_inverse = is_inverse
        self.quote_currency = quote_currency
        self.base_currency = base_currency
        self.settlement_currency = settlement_currency

        self.realized_return = realized_return
        self.realized_pnl = realized_pnl

        # Set quantities
        self._buy_qty = buy_qty
        self._sell_qty = sell_qty

        # Set collections
        self._events = events if events is not None else []
        self._trade_ids = trade_ids if trade_ids is not None else []
        self._commissions = commissions_dict if commissions_dict is not None else {}

        return self

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
        cdef str quantity = " " if self.quantity._mem.raw == 0 else f" {self.quantity.to_formatted_str()} "
        return f"{position_side_to_str(self.side)}{quantity}{self.instrument_id}"

    cpdef dict to_dict(self):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return {
            "position_id": self.id.to_str(),
            "trader_id": self.trader_id.to_str(),
            "strategy_id": self.strategy_id.to_str(),
            "instrument_id": self.instrument_id.to_str(),
            "account_id": self.account_id.to_str(),
            "opening_order_id": self.opening_order_id.to_str(),
            "closing_order_id": self.closing_order_id.to_str() if self.closing_order_id is not None else None,
            "entry": order_side_to_str(self.entry),
            "side": position_side_to_str(self.side),
            "signed_qty": self.signed_qty,
            "quantity": str(self.quantity),
            "peak_qty": str(self.peak_qty),
            "ts_init": self.ts_init,
            "ts_opened": self.ts_opened,
            "ts_last": self.ts_last,
            "ts_closed": self.ts_closed if self.ts_closed > 0 else None,
            "duration_ns": self.duration_ns if self.duration_ns > 0 else None,
            "avg_px_open": self.avg_px_open,
            "avg_px_close": self.avg_px_close if self.avg_px_close > 0 else None,
            "quote_currency": self.quote_currency.code,
            "base_currency": self.base_currency.code if self.base_currency is not None else None,
            "settlement_currency": self.settlement_currency.code,
            "commissions": sorted([str(c) for c in self.commissions()]),
            "realized_return": round(self.realized_return, 5),
            "realized_pnl": str(self.realized_pnl),
            "buy_qty": str(self._buy_qty),
            "sell_qty": str(self._sell_qty),
            "trade_ids": [str(trade_id) for trade_id in self._trade_ids],
            "events": [OrderFilled.to_dict(event) for event in self._events],
            "price_precision": self.price_precision,
            "size_precision": self.size_precision,
            "multiplier": str(self.multiplier),
            "is_inverse": self.is_inverse,
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

    cdef bint has_trade_id_c(self, TradeId trade_id):
        Condition.not_none(trade_id, "trade_id")
        return trade_id in self._trade_ids

    cdef int event_count_c(self):
        return len(self._events)

    cdef bint is_open_c(self):
        return self.side != PositionSide.FLAT

    cdef bint is_closed_c(self):
        return self.side == PositionSide.FLAT

    cdef bint is_long_c(self):
        return self.side == PositionSide.LONG

    cdef bint is_short_c(self):
        return self.side == PositionSide.SHORT

    @property
    def symbol(self):
        """
        Return the positions ticker symbol.

        Returns
        -------
        Symbol

        """
        return self.instrument_id.symbol

    @property
    def venue(self):
        """
        Return the positions trading venue.

        Returns
        -------
        Venue

        """
        return self.instrument_id.venue

    @property
    def client_order_ids(self):
        """
        Return the client order IDs associated with the position.

        Returns
        -------
        list[ClientOrderId]

        Notes
        -----
        Guaranteed not to contain duplicate IDs.

        """
        return self.client_order_ids_c()

    @property
    def venue_order_ids(self):
        """
        Return the venue order IDs associated with the position.

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
        Return the trade match IDs associated with the position.

        Returns
        -------
        list[TradeId]

        """
        return self.trade_ids_c()

    @property
    def events(self):
        """
        Return the order fill events for the position.

        Returns
        -------
        list[Event]

        """
        return self.events_c()

    @property
    def last_event(self):
        """
        Return the last order fill event.

        Returns
        -------
        OrderFilled

        """
        return self.last_event_c()

    @property
    def last_trade_id(self):
        """
        Return the last trade match ID for the position.

        Returns
        -------
        TradeId

        """
        return self.last_trade_id_c()

    @property
    def event_count(self):
        """
        Return the count of order fill events applied to the position.

        Returns
        -------
        int

        """
        return self.event_count_c()

    @property
    def is_open(self):
        """
        Return whether the position side is **not** ``FLAT``.

        Returns
        -------
        bool

        """
        return self.is_open_c()

    @property
    def is_closed(self):
        """
        Return whether the position side is ``FLAT``.

        Returns
        -------
        bool

        """
        return self.is_closed_c()

    @property
    def is_long(self):
        """
        Return whether the position side is ``LONG``.

        Returns
        -------
        bool

        """
        return self.is_long_c()

    @property
    def is_short(self):
        """
        Return whether the position side is ``SHORT``.

        Returns
        -------
        bool

        """
        return self.is_short_c()

    @staticmethod
    cdef PositionSide side_from_order_side_c(OrderSide side):
        if side == OrderSide.BUY:
            return PositionSide.LONG
        elif side == OrderSide.SELL:
            return PositionSide.SHORT
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `OrderSide`, was {side}",  # pragma: no cover (design-time error)
            )

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

    cpdef signed_decimal_qty(self):
        """
        Return a signed decimal representation of the position quantity.

         - If the position is LONG, the value is positive (e.g. Decimal('10.25'))
         - If the position is SHORT, the value is negative (e.g. Decimal('-10.25'))
         - If the position is FLAT, the value is zero (e.g. Decimal('0'))

        Returns
        -------
        Decimal

        """
        return Decimal(f"{self.signed_qty:.{self.size_precision}f}")

    cpdef bint is_opposite_side(self, OrderSide side):
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

    cpdef void apply(self, OrderFilled fill):
        """
        Applies the given order fill event to the position.

        If the position is FLAT prior to applying `fill`, the position state is reset
        (clearing existing events, commissions, etc.) before processing the new fill.

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
        self._check_duplicate_trade_id(fill)

        if self.side == PositionSide.FLAT:
            # Reset position
            self._events.clear()
            self._trade_ids.clear()
            self._buy_qty = Quantity.zero_c(precision=self.size_precision)
            self._sell_qty = Quantity.zero_c(precision=self.size_precision)
            self._commissions = {}
            self.opening_order_id = fill.client_order_id
            self.closing_order_id = None
            self.peak_qty = Quantity.zero_c(precision=self.size_precision)
            self.ts_init = fill.ts_init
            self.ts_opened = fill.ts_event
            self.ts_closed = 0
            self.duration_ns = 0
            self.avg_px_open = fill.last_px.as_f64_c()
            self.avg_px_close = 0.0
            self.realized_return = 0.0
            self.realized_pnl = None

        self._events.append(fill)
        self._trade_ids.append(fill.trade_id)

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
        else:
            raise ValueError(  # pragma: no cover (design-time error)
                f"invalid `OrderSide`, was {fill.order_side}",  # pragma: no cover (design-time error)
            )

        # Set quantities
        self.quantity = Quantity(abs(self.signed_qty), self.size_precision)
        if self.quantity._mem.raw > self.peak_qty._mem.raw:
            self.peak_qty = self.quantity

        # Set state
        if self.signed_qty > 0.0:
            self.entry = OrderSide.BUY
            self.side = PositionSide.LONG
        elif self.signed_qty < 0.0:
            self.entry = OrderSide.SELL
            self.side = PositionSide.SHORT
        else:
            self.side = PositionSide.FLAT
            self.closing_order_id = fill.client_order_id
            self.ts_closed = fill.ts_event
            self.duration_ns = self.ts_closed - self.ts_opened

        self.ts_last = fill.ts_event

    cpdef Money notional_value(self, Price price):
        """
        Return the current notional value of the position, using a reference
        price for the calculation (e.g., bid, ask, mid, last, or mark).

        - For a standard (non-inverse) instrument, the notional is returned in the quote currency.
        - For an inverse instrument, the notional is returned in the base currency, with
          the calculation scaled by 1 / price.

        Parameters
        ----------
        price : Price
            The reference price for the calculation. This could be the last, mid, bid, ask,
            a mark-to-market price, or any other suitably representative value.

        Returns
        -------
        Money
            Denominated in quote currency for standard instruments, or base currency if inverse.

        """
        Condition.not_none(price, "price")

        if self.is_inverse:
            return Money(
                self.quantity.as_f64_c() * self.multiplier.as_f64_c() * (1.0 / price.as_f64_c()),
                self.base_currency,
            )
        else:
            return Money(
                self.quantity.as_f64_c() * self.multiplier.as_f64_c() * price.as_f64_c(),
                self.quote_currency,
            )

    cpdef Money calculate_pnl(
        self,
        double avg_px_open,
        double avg_px_close,
        Quantity quantity,
    ):
        """
        Return a calculated PnL in the instrument's settlement currency.

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
            Denominated in settlement currency.

        """
        cdef double pnl = self._calculate_pnl(
            avg_px_open=avg_px_open,
            avg_px_close=avg_px_close,
            quantity=quantity.as_f64_c(),
        )

        return Money(pnl, self.settlement_currency)

    cpdef Money unrealized_pnl(self, Price price):
        """
        Return the unrealized PnL for the position, using a reference
        price for the calculation (e.g., bid, ask, mid, last, or mark).

        Parameters
        ----------
        price : Price
            The reference price for the calculation. This could be the last, mid, bid, ask,
            a mark-to-market price, or any other suitably representative value.

        Returns
        -------
        Money
            Denominated in settlement currency.

        """
        Condition.not_none(price, "price")

        if self.side == PositionSide.FLAT:
            return Money(0, self.settlement_currency)

        cdef double pnl = self._calculate_pnl(
            avg_px_open=self.avg_px_open,
            avg_px_close=price.as_f64_c(),
            quantity=self.quantity.as_f64_c(),
        )

        return Money(pnl, self.settlement_currency)

    cpdef Money total_pnl(self, Price price):
        """
        Return the total PnL for the position, using a reference
        price for the calculation (e.g., bid, ask, mid, last, or mark).

        Parameters
        ----------
        price : Price
            The reference price for the calculation. This could be the last, mid, bid, ask,
            a mark-to-market price, or any other suitably representative value.

        Returns
        -------
        Money
            Denominated in settlement currency.

        """
        Condition.not_none(price, "price")

        cdef double realized_pnl = self.realized_pnl.as_f64_c() if self.realized_pnl is not None else 0.0
        return Money(realized_pnl + self.unrealized_pnl(price).as_f64_c(), self.settlement_currency)

    cpdef list commissions(self):
        """
        Return the total commissions generated by the position.

        Returns
        -------
        list[Money]

        """
        return list(self._commissions.values())

    cdef void _check_duplicate_trade_id(self, OrderFilled fill):
        # Check all previous fills for matching trade ID and composite key
        cdef:
            OrderFilled p_fill
        for p_fill in self._events:
            if fill.trade_id != p_fill.trade_id:
                continue
            if (
                fill.order_side == p_fill.order_side
                and fill.last_px == p_fill.last_px
                and fill.last_qty == p_fill.last_qty
            ):
                raise KeyError(f"Duplicate {fill.trade_id!r} in events {fill} {p_fill}")

    cdef void _handle_buy_order_fill(self, OrderFilled fill):
        # Initialize realized PnL for fill
        cdef double realized_pnl
        if fill.commission.currency == self.settlement_currency:
            realized_pnl = -fill.commission.as_f64_c()
        else:
            realized_pnl = 0.0

        cdef double last_px = fill.last_px.as_f64_c()
        cdef double last_qty = fill.last_qty.as_f64_c()
        cdef Quantity last_qty_obj = fill.last_qty
        if self.base_currency is not None and fill.commission.currency == self.base_currency:
            last_qty_obj = Quantity(last_qty, self.size_precision)

        # LONG POSITION
        if self.signed_qty > 0:
            self.avg_px_open = self._calculate_avg_px_open_px(last_px, last_qty)
        # SHORT POSITION
        elif self.signed_qty < 0:
            self.avg_px_close = self._calculate_avg_px_close_px(last_px, last_qty)
            self.realized_return = self._calculate_return(self.avg_px_open, self.avg_px_close)
            realized_pnl += self._calculate_pnl(self.avg_px_open, last_px, last_qty)

        if self.realized_pnl is None:
            self.realized_pnl = Money(realized_pnl, self.settlement_currency)
        else:
            self.realized_pnl = Money(self.realized_pnl.as_f64_c() + realized_pnl, self.settlement_currency)

        self._buy_qty.add_assign(last_qty_obj)
        self.signed_qty += last_qty
        self.signed_qty = round(self.signed_qty, self.size_precision)

    cdef void _handle_sell_order_fill(self, OrderFilled fill):
        # Initialize realized PnL for fill
        cdef double realized_pnl
        if fill.commission.currency == self.settlement_currency:
            realized_pnl = -fill.commission.as_f64_c()
        else:
            realized_pnl = 0.0

        cdef double last_px = fill.last_px.as_f64_c()
        cdef double last_qty = fill.last_qty.as_f64_c()
        cdef Quantity last_qty_obj = fill.last_qty
        if self.base_currency is not None and fill.commission.currency == self.base_currency:
            last_qty_obj = Quantity(last_qty, self.size_precision)

        # SHORT POSITION
        if self.signed_qty < 0:
            self.avg_px_open = self._calculate_avg_px_open_px(last_px, last_qty)
        # LONG POSITION
        elif self.signed_qty > 0:
            self.avg_px_close = self._calculate_avg_px_close_px(last_px, last_qty)
            self.realized_return = self._calculate_return(self.avg_px_open, self.avg_px_close)
            realized_pnl += self._calculate_pnl(self.avg_px_open, last_px, last_qty)

        if self.realized_pnl is None:
            self.realized_pnl = Money(realized_pnl, self.settlement_currency)
        else:
            self.realized_pnl = Money(self.realized_pnl.as_f64_c() + realized_pnl, self.settlement_currency)

        self._sell_qty.add_assign(last_qty_obj)
        self.signed_qty -= last_qty
        self.signed_qty = round(self.signed_qty, self.size_precision)

    cdef double _calculate_avg_px_open_px(self, double last_px, double last_qty):
        return self._calculate_avg_px(self.quantity.as_f64_c(), self.avg_px_open, last_px, last_qty)

    cdef double _calculate_avg_px_close_px(self, double last_px, double last_qty):
        if not self.avg_px_close:
            return last_px
        close_qty = self._sell_qty if self.side == PositionSide.LONG else self._buy_qty
        return self._calculate_avg_px(close_qty.as_f64_c(), self.avg_px_close, last_px, last_qty)

    cdef double _calculate_avg_px(
        self,
        double qty,
        double avg_px,
        double last_px,
        double last_qty,
    ):
        cdef double start_cost = avg_px * qty
        cdef double event_cost = last_px * last_qty
        return (start_cost + event_cost) / (qty + last_qty)

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
        # Only book open quantity towards PnL
        quantity = fmin(quantity, fabs(self.signed_qty))

        if self.is_inverse:
            # In base currency
            return quantity * self.multiplier.as_f64_c() * self._calculate_points_inverse(avg_px_open, avg_px_close)
        else:
            # In quote currency
            return quantity * self.multiplier.as_f64_c() * self._calculate_points(avg_px_open, avg_px_close)

    @staticmethod
    cdef Position from_dict_c(dict values):
        """
        Internal cdef implementation of from_dict.
        """
        Condition.not_none(values, "values")

        # Create a new self without calling __init__
        cdef Position position = Position.__new__(Position)

        # Handle opening/closing order IDs
        cdef ClientOrderId opening_order_id = ClientOrderId(values["opening_order_id"])
        cdef ClientOrderId closing_order_id = None
        if values.get("closing_order_id"):
            closing_order_id = ClientOrderId(values["closing_order_id"])

        # Handle quantity values
        cdef Quantity quantity = Quantity.from_str(values["quantity"])
        cdef Quantity peak_qty = Quantity.from_str(values["peak_qty"])
        cdef int size_precision = int(values.get("size_precision", 8))

        # Handle buy/sell quantities
        cdef Quantity buy_qty
        if "buy_qty" in values:
            buy_qty = Quantity.from_str(values["buy_qty"])
        else:
            buy_qty = Quantity.zero_c(precision=size_precision)

        cdef Quantity sell_qty
        if "sell_qty" in values:
            sell_qty = Quantity.from_str(values["sell_qty"])
        else:
            sell_qty = Quantity.zero_c(precision=size_precision)

        # Handle multiplier
        cdef Quantity multiplier = Quantity.from_str(values.get("multiplier", "1"))

        # Handle currencies
        cdef Currency quote_currency = Currency.from_str(values["quote_currency"])
        cdef Currency base_currency = None
        if values.get("base_currency"):
            base_currency = Currency.from_str(values["base_currency"])
        cdef Currency settlement_currency = Currency.from_str(values["settlement_currency"])

        # Handle timestamps
        cdef uint64_t ts_closed = 0
        if values.get("ts_closed") not in (None, "None"):
            ts_closed = int(values["ts_closed"])

        cdef uint64_t duration_ns = 0
        if values.get("duration_ns") not in (None, "None"):
            duration_ns = int(values["duration_ns"])

        # Handle prices
        cdef double avg_px_close = 0.0
        if values.get("avg_px_close") not in (None, "None"):
            avg_px_close = float(values["avg_px_close"])

        # Handle PnL
        cdef Money realized_pnl = None
        if values.get("realized_pnl") not in (None, "None"):
            realized_pnl = Money.from_str(values["realized_pnl"])

        # Handle commissions
        cdef dict commissions_dict = {}
        if isinstance(values.get("commissions"), list):
            # List format
            for commission_str in values["commissions"]:
                commission = Money.from_str(commission_str)
                commissions_dict[commission.currency] = commission
        elif isinstance(values.get("commissions"), dict):
            # Dict format
            for currency_code, commission_str in values["commissions"].items():
                commission = Money.from_str(commission_str)
                commissions_dict[commission.currency] = commission

        # Handle events and trade_ids
        cdef list events = []
        cdef list trade_ids = []

        if "events" in values and isinstance(values["events"], list):
            from nautilus_trader.model.events.order import OrderFilled

            for event_values in values["events"]:
                if event_values.get("type") == "OrderFilled":
                    fill = OrderFilled.from_dict(event_values)
                    events.append(fill)
                    trade_ids.append(fill.trade_id)

        # Handle trade_ids directly if events not present
        if not trade_ids and "trade_ids" in values and isinstance(values["trade_ids"], list):
            for trade_id_str in values["trade_ids"]:
                trade_ids.append(TradeId(trade_id_str))

        # Initialize self with all values
        return Position._init_(
            self=position,
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            position_id=PositionId(values["position_id"]),
            account_id=AccountId(values["account_id"]),
            opening_order_id=opening_order_id,
            closing_order_id=closing_order_id,
            entry=order_side_from_str(values["entry"]),
            side=position_side_from_str(values["side"]),
            signed_qty=float(values["signed_qty"]),
            quantity=quantity,
            peak_qty=peak_qty,
            price_precision=int(values.get("price_precision", 8)),
            size_precision=size_precision,
            multiplier=multiplier,
            is_inverse=bool(values.get("is_inverse", False)),
            quote_currency=quote_currency,
            base_currency=base_currency,
            settlement_currency=settlement_currency,
            ts_init=int(values["ts_init"]),
            ts_opened=int(values["ts_opened"]),
            ts_last=int(values["ts_last"]),
            ts_closed=ts_closed,
            duration_ns=duration_ns,
            avg_px_open=float(values["avg_px_open"]),
            avg_px_close=avg_px_close,
            realized_return=float(values["realized_return"]),
            realized_pnl=realized_pnl,
            buy_qty=buy_qty,
            sell_qty=sell_qty,
            commissions_dict=commissions_dict,
            events=events,
            trade_ids=trade_ids,
        )

    @staticmethod
    def from_dict(dict values):
        """
        Create a position from a dictionary representation.

        This method allows recreating a position from a serialized state, without
        requiring an external Instrument object.

        Parameters
        ----------
        values : dict
            The dictionary containing position values.

        Returns
        -------
        Position
            A new position self.
        """
        return Position.from_dict_c(values)

    @staticmethod
    def create(Instrument instrument, OrderFilled fill):
        """
        Create a position from an instrument and fill.

        This factory method provides an alternative to using the constructor directly.

        Parameters
        ----------
        instrument : Instrument
            The trading instrument for the position.
        fill : OrderFilled
            The order fill event which opened the position.

        Returns
        -------
        Position
            A new position self.

        Raises
        ------
        ValueError
            If `instrument.id` is not equal to `fill.instrument_id`.
        ValueError
            If `fill.position_id` is ``None``.
        """
        return Position(instrument, fill)
