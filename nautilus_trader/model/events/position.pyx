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

from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.message cimport Event
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.core.rust.model cimport PositionSide
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.functions cimport order_side_from_str
from nautilus_trader.model.functions cimport order_side_to_str
from nautilus_trader.model.functions cimport position_side_from_str
from nautilus_trader.model.functions cimport position_side_to_str
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.objects cimport Currency
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.position cimport Position


cdef class PositionEvent(Event):
    """
    The base class for all position events.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    position_id : PositionId
        The position IDt.
    account_id : AccountId
        The strategy ID.
    opening_order_id : ClientOrderId
        The client order ID for the order which opened the position.
    closing_order_id : ClientOrderId
        The client order ID for the order which closed the position.
    entry : OrderSide {``BUY``, ``SELL``}
        The position entry order side.
    side : PositionSide {``FLAT``, ``LONG``, ``SHORT``}
        The current position side.
    signed_qty : double
        The current signed quantity (positive for ``LONG``, negative for ``SHORT``).
    quantity : Quantity
        The current open quantity.
    peak_qty : Quantity
        The peak directional quantity reached by the position.
    last_qty : Quantity
        The last fill quantity for the position.
    last_px : Price
        The last fill price for the position (not average price).
    currency : Currency
        The position quote currency.
    avg_px_open : double
        The average open price.
    avg_px_close : double
        The average close price.
    realized_return : double
        The realized return for the position.
    realized_pnl : Money
        The realized PnL for the position.
    unrealized_pnl : Money
        The unrealized PnL for the position.
    event_id : UUID4
        The event ID.
    ts_opened : uint64_t
        UNIX timestamp (nanoseconds) when the position opened event occurred.
    ts_closed : uint64_t
        UNIX timestamp (nanoseconds) when the position closed event occurred.
    duration_ns : uint64_t
        The total open duration (nanoseconds), will be 0 if still open.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        PositionId position_id not None,
        AccountId account_id not None,
        ClientOrderId opening_order_id not None,
        ClientOrderId closing_order_id: ClientOrderId | None,
        OrderSide entry,
        PositionSide side,
        double signed_qty,
        Quantity quantity not None,
        Quantity peak_qty not None,
        Quantity last_qty not None,
        Price last_px not None,
        Currency currency not None,
        double avg_px_open,
        double avg_px_close,
        double realized_return,
        Money realized_pnl not None,
        Money unrealized_pnl not None,
        UUID4 event_id not None,
        uint64_t ts_opened,
        uint64_t ts_closed,
        uint64_t duration_ns,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        self.trader_id = trader_id
        self.strategy_id = strategy_id
        self.instrument_id = instrument_id
        self.position_id = position_id
        self.account_id = account_id
        self.opening_order_id = opening_order_id
        self.closing_order_id = closing_order_id
        self.entry = entry
        self.side = side
        self.signed_qty = signed_qty
        self.quantity = quantity
        self.peak_qty = peak_qty
        self.last_qty = last_qty
        self.last_px = last_px
        self.currency = currency
        self.avg_px_open = avg_px_open
        self.avg_px_close = avg_px_close
        self.realized_return = realized_return
        self.realized_pnl = realized_pnl
        self.unrealized_pnl = unrealized_pnl
        self.ts_opened = ts_opened
        self.ts_closed = ts_closed
        self.duration_ns = duration_ns

        self._event_id = event_id
        self._ts_event = ts_event
        self._ts_init = ts_init

    def __eq__(self, Event other) -> bool:
        return self._event_id == other.id

    def __hash__(self) -> int:
        return hash(self._event_id)

    def __str__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"instrument_id={self.instrument_id.to_str()}, "
            f"position_id={self.position_id.to_str()}, "
            f"account_id={self.account_id.to_str()}, "
            f"opening_order_id={self.opening_order_id.to_str()}, "
            f"closing_order_id={self.closing_order_id}, "  # Can be None
            f"entry={order_side_to_str(self.entry)}, "
            f"side={position_side_to_str(self.side)}, "
            f"signed_qty={self.signed_qty}, "
            f"quantity={self.quantity.to_formatted_str()}, "
            f"peak_qty={self.peak_qty.to_formatted_str()}, "
            f"currency={self.currency.code}, "
            f"avg_px_open={self.avg_px_open}, "
            f"avg_px_close={self.avg_px_close}, "
            f"realized_return={self.realized_return:.5f}, "
            f"realized_pnl={self.realized_pnl.to_formatted_str()}, "
            f"unrealized_pnl={self.unrealized_pnl.to_formatted_str()}, "
            f"ts_opened={self.ts_opened}, "
            f"ts_last={self.ts_event}, "
            f"ts_closed={self.ts_closed}, "
            f"duration_ns={self.duration_ns})"
        )

    def __repr__(self) -> str:
        return (
            f"{type(self).__name__}("
            f"trader_id={self.trader_id.to_str()}, "
            f"strategy_id={self.strategy_id.to_str()}, "
            f"instrument_id={self.instrument_id.to_str()}, "
            f"position_id={self.position_id.to_str()}, "
            f"account_id={self.account_id.to_str()}, "
            f"opening_order_id={self.opening_order_id.to_str()}, "
            f"closing_order_id={self.closing_order_id}, "  # Can be None
            f"entry={order_side_to_str(self.entry)}, "
            f"side={position_side_to_str(self.side)}, "
            f"signed_qty={self.signed_qty}, "
            f"quantity={self.quantity.to_formatted_str()}, "
            f"peak_qty={self.peak_qty.to_formatted_str()}, "
            f"currency={self.currency.code}, "
            f"avg_px_open={self.avg_px_open}, "
            f"avg_px_close={self.avg_px_close}, "
            f"realized_return={self.realized_return:.5f}, "
            f"realized_pnl={self.realized_pnl.to_formatted_str()}, "
            f"unrealized_pnl={self.unrealized_pnl.to_formatted_str()}, "
            f"ts_opened={self.ts_opened}, "
            f"ts_last={self._ts_event}, "
            f"ts_closed={self.ts_closed}, "
            f"duration_ns={self.duration_ns}, "
            f"event_id={self._event_id.to_str()})"
        )

    @property
    def id(self) -> UUID4:
        """
        The event message identifier.

        Returns
        -------
        UUID4

        """
        return self._event_id

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the event occurred.

        Returns
        -------
        int

        """
        return self._ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._ts_init


cdef class PositionOpened(PositionEvent):
    """
    Represents an event where a position has been opened.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    position_id : PositionId
        The position IDt.
    account_id : AccountId
        The strategy ID.
    opening_order_id : ClientOrderId
        The client order ID for the order which opened the position.
    strategy_id : StrategyId
        The strategy ID associated with the event.
    entry : OrderSide {``BUY``, ``SELL``}
        The position entry order side.
    side : PositionSide {``LONG``, ``SHORT``}
        The current position side.
    signed_qty : double
        The current signed quantity (positive for ``LONG``, negative for ``SHORT``).
    quantity : Quantity
        The current open quantity.
    peak_qty : Quantity
        The peak directional quantity reached by the position.
    last_qty : Quantity
        The last fill quantity for the position.
    last_px : Price
        The last fill price for the position (not average price).
    currency : Currency
        The position quote currency.
    avg_px_open : double
        The average open price.
    realized_pnl : Money
        The realized PnL for the position.
    event_id : UUID4
        The event ID.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the position opened event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        PositionId position_id not None,
        AccountId account_id not None,
        ClientOrderId opening_order_id not None,
        OrderSide entry,
        PositionSide side,
        double signed_qty,
        Quantity quantity not None,
        Quantity peak_qty not None,
        Quantity last_qty not None,
        Price last_px not None,
        Currency currency not None,
        double avg_px_open,
        Money realized_pnl not None,
        UUID4 event_id not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        assert side != PositionSide.FLAT  # Design-time check: position side matches event
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            position_id,
            account_id,
            opening_order_id,
            None,  # Position is still open
            entry,
            side,
            signed_qty,
            quantity,
            peak_qty,
            last_qty,
            last_px,
            currency,
            avg_px_open,
            0.0,
            0.0,
            realized_pnl,
            Money(0, realized_pnl.currency),
            event_id,
            ts_event,
            0,
            0,
            ts_event,
            ts_init,
        )

    @staticmethod
    cdef PositionOpened create_c(
        Position position,
        OrderFilled fill,
        UUID4 event_id,
        uint64_t ts_init,
    ):
        Condition.not_none(position, "position")
        Condition.not_none(fill, "fill")
        Condition.not_none(event_id, "event_id")

        return PositionOpened(
            trader_id=position.trader_id,
            strategy_id=position.strategy_id,
            instrument_id=position.instrument_id,
            position_id=position.id,
            account_id=position.account_id,
            opening_order_id=position.opening_order_id,
            entry=position.entry,
            side=position.side,
            signed_qty=position.signed_qty,
            quantity=position.quantity,
            peak_qty=position.peak_qty,
            last_qty=fill.last_qty,
            last_px=fill.last_px,
            currency=position.quote_currency,
            avg_px_open=position.avg_px_open,
            realized_pnl=position.realized_pnl,
            event_id=event_id,
            ts_event=position.ts_opened,
            ts_init=ts_init,
        )

    @staticmethod
    cdef PositionOpened from_dict_c(dict values):
        Condition.not_none(values, "values")
        return PositionOpened(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            position_id=PositionId(values["position_id"]),
            account_id=AccountId(values["account_id"]),
            opening_order_id=ClientOrderId(values["opening_order_id"]),
            entry=order_side_from_str(values["entry"]),
            side=position_side_from_str(values["side"]),
            signed_qty=values["signed_qty"],
            quantity=Quantity.from_str_c(values["quantity"]),
            peak_qty=Quantity.from_str_c(values["peak_qty"]),
            last_qty=Quantity.from_str_c(values["last_qty"]),
            last_px=Price.from_str_c(values["last_px"]),
            currency=Currency.from_str_c(values["currency"]),
            avg_px_open=values["avg_px_open"],
            realized_pnl=Money.from_str_c(values["realized_pnl"]),
            event_id=UUID4.from_str_c(values["event_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(PositionOpened obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "position_id": obj.position_id.to_str(),
            "account_id": obj.account_id.to_str(),
            "opening_order_id": obj.opening_order_id.to_str(),
            "entry": order_side_to_str(obj.entry),
            "side": position_side_to_str(obj.side),
            "signed_qty": obj.signed_qty,
            "quantity": str(obj.quantity),
            "peak_qty": str(obj.peak_qty),
            "last_qty": str(obj.last_qty),
            "last_px": str(obj.last_px),
            "currency": obj.currency.code,
            "avg_px_open": obj.avg_px_open,
            "realized_pnl": str(obj.realized_pnl),
            "duration_ns": obj.duration_ns,
            "event_id": obj._event_id.to_str(),
            "ts_event": obj._ts_event,
            "ts_init": obj._ts_init,
        }

    @staticmethod
    def create(
        Position position,
        OrderFilled fill,
        UUID4 event_id,
        uint64_t ts_init,
    ):
        """
        Return a position opened event from the given params.

        Parameters
        ----------
        position : Position
            The position for the event.
        fill : OrderFilled
            The order fill for the event.
        event_id : UUID4
            The event ID.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        PositionOpened

        """
        return PositionOpened.create_c(position, fill, event_id, ts_init)

    @staticmethod
    def from_dict(dict values) -> PositionOpened:
        """
        Return a position opened event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        PositionOpened

        """
        return PositionOpened.from_dict_c(values)

    @staticmethod
    def to_dict(PositionOpened obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return PositionOpened.to_dict_c(obj)


cdef class PositionChanged(PositionEvent):
    """
    Represents an event where a position has changed.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    position_id : PositionId
        The position IDt.
    account_id : AccountId
        The strategy ID.
    opening_order_id : ClientOrderId
        The client order ID for the order which opened the position.
    strategy_id : StrategyId
        The strategy ID associated with the event.
    entry : OrderSide {``BUY``, ``SELL``}
        The position entry order side.
    side : PositionSide {``FLAT``, ``LONG``, ``SHORT``}
        The current position side.
    signed_qty : double
        The current signed quantity (positive for ``LONG``, negative for ``SHORT``).
    quantity : Quantity
        The current open quantity.
    peak_qty : Quantity
        The peak directional quantity reached by the position.
    last_qty : Quantity
        The last fill quantity for the position.
    last_px : Price
        The last fill price for the position (not average price).
    currency : Currency
        The position quote currency.
    avg_px_open : double
        The average open price.
    avg_px_close : double
        The average close price.
    realized_return : double
        The realized return for the position.
    realized_pnl : Money
        The realized PnL for the position.
    unrealized_pnl : Money
        The unrealized PnL for the position.
    event_id : UUID4
        The event ID.
    ts_opened : uint64_t
        UNIX timestamp (nanoseconds) when the position opened event occurred.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the position changed event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        PositionId position_id not None,
        AccountId account_id not None,
        ClientOrderId opening_order_id not None,
        OrderSide entry,
        PositionSide side,
        double signed_qty,
        Quantity quantity not None,
        Quantity peak_qty not None,
        Quantity last_qty not None,
        Price last_px not None,
        Currency currency not None,
        double avg_px_open,
        double avg_px_close,
        double realized_return,
        Money realized_pnl not None,
        Money unrealized_pnl not None,
        UUID4 event_id not None,
        uint64_t ts_opened,
        uint64_t ts_event,
        uint64_t ts_init,
    ):
        assert side != PositionSide.FLAT  # Design-time check: position side matches event
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            position_id,
            account_id,
            opening_order_id,
            None,  # Position is still open
            entry,
            side,
            signed_qty,
            quantity,
            peak_qty,
            last_qty,
            last_px,
            currency,
            avg_px_open,
            avg_px_close,
            realized_return,
            realized_pnl,
            unrealized_pnl,
            event_id,
            ts_opened,
            0,
            0,
            ts_event,
            ts_init,
        )

    @staticmethod
    cdef PositionChanged create_c(
        Position position,
        OrderFilled fill,
        UUID4 event_id,
        uint64_t ts_init,
    ):
        Condition.not_none(position, "position")
        Condition.not_none(fill, "fill")
        Condition.not_none(event_id, "event_id")

        return PositionChanged(
            trader_id=position.trader_id,
            strategy_id=position.strategy_id,
            instrument_id=position.instrument_id,
            position_id=position.id,
            account_id=position.account_id,
            opening_order_id=position.opening_order_id,
            entry=position.entry,
            side=position.side,
            signed_qty=position.signed_qty,
            quantity=position.quantity,
            peak_qty=position.peak_qty,
            last_qty=fill.last_qty,
            last_px=fill.last_px,
            currency=position.quote_currency,
            avg_px_open=position.avg_px_open,
            avg_px_close=position.avg_px_close,
            realized_return=position.realized_return,
            realized_pnl=position.realized_pnl,
            unrealized_pnl=position.unrealized_pnl(fill.last_px),
            event_id=event_id,
            ts_opened=position.ts_opened,
            ts_event=position.last_event_c().ts_event,
            ts_init=ts_init,
        )

    @staticmethod
    cdef PositionChanged from_dict_c(dict values):
        Condition.not_none(values, "values")
        return PositionChanged(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            position_id=PositionId(values["position_id"]),
            account_id=AccountId(values["account_id"]),
            opening_order_id=ClientOrderId(values["opening_order_id"]),
            entry=order_side_from_str(values["entry"]),
            side=position_side_from_str(values["side"]),
            signed_qty=values["signed_qty"],
            quantity=Quantity.from_str_c(values["quantity"]),
            peak_qty=Quantity.from_str_c(values["peak_qty"]),
            last_qty=Quantity.from_str_c(values["last_qty"]),
            last_px=Price.from_str_c(values["last_px"]),
            currency=Currency.from_str_c(values["currency"]),
            avg_px_open=values["avg_px_open"],
            avg_px_close=values["avg_px_close"],
            realized_return=values["realized_return"],
            realized_pnl=Money.from_str_c(values["realized_pnl"]),
            unrealized_pnl=Money.from_str_c(values["unrealized_pnl"]),
            event_id=UUID4.from_str_c(values["event_id"]),
            ts_opened=values["ts_opened"],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(PositionChanged obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "position_id": obj.position_id.to_str(),
            "account_id": obj.account_id.to_str(),
            "opening_order_id": obj.opening_order_id.to_str(),
            "entry": order_side_to_str(obj.entry),
            "side": position_side_to_str(obj.side),
            "signed_qty": obj.signed_qty,
            "quantity": str(obj.quantity),
            "peak_qty": str(obj.peak_qty),
            "last_qty": str(obj.last_qty),
            "last_px": str(obj.last_px),
            "currency": obj.currency.code,
            "avg_px_open": obj.avg_px_open,
            "avg_px_close": obj.avg_px_close,
            "realized_return": obj.realized_return,
            "realized_pnl": str(obj.realized_pnl),
            "unrealized_pnl": str(obj.unrealized_pnl),
            "event_id": obj._event_id.to_str(),
            "ts_opened": obj.ts_opened,
            "ts_event": obj._ts_event,
            "ts_init": obj._ts_init,
        }

    @staticmethod
    def create(
        Position position,
        OrderFilled fill,
        UUID4 event_id,
        uint64_t ts_init,
    ):
        """
        Return a position changed event from the given params.

        Parameters
        ----------
        position : Position
            The position for the event.
        fill : OrderFilled
            The order fill for the event.
        event_id : UUID4
            The event ID.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        PositionChanged

        """
        return PositionChanged.create_c(position, fill, event_id, ts_init)

    @staticmethod
    def from_dict(dict values) -> PositionChanged:
        """
        Return a position changed event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        PositionChanged

        """
        return PositionChanged.from_dict_c(values)

    @staticmethod
    def to_dict(PositionChanged obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return PositionChanged.to_dict_c(obj)


cdef class PositionClosed(PositionEvent):
    """
    Represents an event where a position has been closed.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID.
    strategy_id : StrategyId
        The strategy ID.
    instrument_id : InstrumentId
        The instrument ID.
    position_id : PositionId
        The position IDt.
    account_id : AccountId
        The strategy ID.
    opening_order_id : ClientOrderId
        The client order ID for the order which opened the position.
    closing_order_id : ClientOrderId
        The client order ID for the order which closed the position.
    strategy_id : StrategyId
        The strategy ID associated with the event.
    entry : OrderSide {``BUY``, ``SELL``}
        The position entry order side.
    side : PositionSide {``FLAT``}
        The current position side.
    signed_qty : double
        The current signed quantity (positive for ``LONG``, negative for ``SHORT``).
    quantity : Quantity
        The current open quantity.
    peak_qty : Quantity
        The peak directional quantity reached by the position.
    last_qty : Quantity
        The last fill quantity for the position.
    last_px : Price
        The last fill price for the position (not average price).
    currency : Currency
        The position quote currency.
    avg_px_open : Decimal
        The average open price.
    avg_px_close : Decimal
        The average close price.
    realized_return : Decimal
        The realized return for the position.
    realized_pnl : Money
        The realized PnL for the position.
    event_id : UUID4
        The event ID.
    ts_opened : uint64_t
        UNIX timestamp (nanoseconds) when the position opened event occurred.
    ts_closed : uint64_t
        UNIX timestamp (nanoseconds) when the position closed event occurred.
    duration_ns : uint64_t
        The total open duration (nanoseconds).
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    """

    def __init__(
        self,
        TraderId trader_id not None,
        StrategyId strategy_id not None,
        InstrumentId instrument_id not None,
        PositionId position_id not None,
        AccountId account_id not None,
        ClientOrderId opening_order_id not None,
        ClientOrderId closing_order_id not None,
        OrderSide entry,
        PositionSide side,
        double signed_qty,
        Quantity quantity not None,
        Quantity peak_qty not None,
        Quantity last_qty not None,
        Price last_px not None,
        Currency currency not None,
        double avg_px_open,
        double avg_px_close,
        double realized_return,
        Money realized_pnl not None,
        UUID4 event_id not None,
        uint64_t ts_opened,
        uint64_t ts_closed,
        uint64_t duration_ns,
        uint64_t ts_init,
    ):
        assert side == PositionSide.FLAT  # Design-time check: position side matches event
        super().__init__(
            trader_id,
            strategy_id,
            instrument_id,
            position_id,
            account_id,
            opening_order_id,
            closing_order_id,
            entry,
            side,
            signed_qty,
            quantity,
            peak_qty,
            last_qty,
            last_px,
            currency,
            avg_px_open,
            avg_px_close,
            realized_return,
            realized_pnl,
            Money(0, realized_pnl.currency),  # No further unrealized PnL
            event_id,
            ts_opened,
            ts_closed,
            duration_ns,
            ts_closed,  # ts_event = ts_closed
            ts_init,
        )

    @staticmethod
    cdef PositionClosed create_c(
        Position position,
        OrderFilled fill,
        UUID4 event_id,
        uint64_t ts_init,
    ):
        Condition.not_none(position, "position")
        Condition.not_none(fill, "fill")
        Condition.not_none(event_id, "event_id")

        return PositionClosed(
            trader_id=position.trader_id,
            strategy_id=position.strategy_id,
            instrument_id=position.instrument_id,
            position_id=position.id,
            account_id=position.account_id,
            opening_order_id=position.opening_order_id,
            closing_order_id=position.closing_order_id,
            entry=position.entry,
            side=position.side,
            signed_qty=position.signed_qty,
            quantity=position.quantity,
            peak_qty=position.peak_qty,
            last_qty=fill.last_qty,
            last_px=fill.last_px,
            currency=position.quote_currency,
            avg_px_open=position.avg_px_open,
            avg_px_close=position.avg_px_close,
            realized_return=position.realized_return,
            realized_pnl=position.realized_pnl,
            event_id=event_id,
            ts_opened=position.ts_opened,
            ts_closed=position.ts_closed,
            duration_ns=position.duration_ns,
            ts_init=ts_init,
        )

    @staticmethod
    cdef PositionClosed from_dict_c(dict values):
        Condition.not_none(values, "values")
        return PositionClosed(
            trader_id=TraderId(values["trader_id"]),
            strategy_id=StrategyId(values["strategy_id"]),
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            position_id=PositionId(values["position_id"]),
            account_id=AccountId(values["account_id"]),
            opening_order_id=ClientOrderId(values["opening_order_id"]),
            closing_order_id=ClientOrderId(values["closing_order_id"]),
            entry=order_side_from_str(values["entry"]),
            side=position_side_from_str(values["side"]),
            signed_qty=values["signed_qty"],
            quantity=Quantity.from_str_c(values["quantity"]),
            peak_qty=Quantity.from_str_c(values["peak_qty"]),
            last_qty=Quantity.from_str_c(values["last_qty"]),
            last_px=Price.from_str_c(values["last_px"]),
            currency=Currency.from_str_c(values["currency"]),
            avg_px_open=values["avg_px_open"],
            avg_px_close=values["avg_px_close"],
            realized_return=values["realized_return"],
            realized_pnl=Money.from_str_c(values["realized_pnl"]),
            event_id=UUID4.from_str_c(values["event_id"]),
            ts_opened=values["ts_opened"],
            ts_closed=values["ts_closed"],
            duration_ns=values["duration_ns"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(PositionClosed obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "trader_id": obj.trader_id.to_str(),
            "strategy_id": obj.strategy_id.to_str(),
            "instrument_id": obj.instrument_id.to_str(),
            "position_id": obj.position_id.to_str(),
            "account_id": obj.account_id.to_str(),
            "opening_order_id": obj.opening_order_id.to_str(),
            "closing_order_id": obj.closing_order_id.to_str(),
            "entry": order_side_to_str(obj.entry),
            "side": position_side_to_str(obj.side),
            "signed_qty": obj.signed_qty,
            "quantity": str(obj.quantity),
            "peak_qty": str(obj.peak_qty),
            "last_qty": str(obj.last_qty),
            "last_px": str(obj.last_px),
            "currency": obj.currency.code,
            "avg_px_open": obj.avg_px_open,
            "avg_px_close": obj.avg_px_close,
            "realized_return": obj.realized_return,
            "realized_pnl": str(obj.realized_pnl),
            "event_id": obj._event_id.to_str(),
            "ts_opened": obj.ts_opened,
            "ts_closed": obj.ts_closed,
            "duration_ns": obj.duration_ns,
            "ts_init": obj._ts_init,
        }

    @staticmethod
    def create(
        Position position,
        OrderFilled fill,
        UUID4 event_id,
        uint64_t ts_init,
    ):
        """
        Return a position closed event from the given params.

        Parameters
        ----------
        position : Position
            The position for the event.
        fill : OrderFilled
            The order fill for the event.
        event_id : UUID4
            The event ID.
        ts_init : uint64_t
            UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        PositionClosed

        """
        return PositionClosed.create_c(position, fill, event_id, ts_init)

    @staticmethod
    def from_dict(dict values) -> PositionClosed:
        """
        Return a position closed event from the given dict values.

        Parameters
        ----------
        values : dict[str, object]
            The values for initialization.

        Returns
        -------
        PositionClosed

        """
        return PositionClosed.from_dict_c(values)

    @staticmethod
    def to_dict(PositionClosed obj):
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return PositionClosed.to_dict_c(obj)
