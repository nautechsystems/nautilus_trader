from decimal import Decimal
from typing import Any


class Position:
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

    trader_id: TraderId
    strategy_id: StrategyId
    instrument_id: InstrumentId
    id: PositionId
    account_id: AccountId
    opening_order_id: ClientOrderId
    closing_order_id: ClientOrderId | None
    entry: OrderSide
    side: PositionSide
    signed_qty: float
    quantity: Quantity
    peak_qty: Quantity
    ts_init: int
    ts_opened: int
    ts_last: int
    ts_closed: int
    duration_ns: int
    avg_px_open: float
    avg_px_close: float
    price_precision: int
    size_precision: int
    multiplier: Quantity
    is_inverse: bool
    quote_currency: Currency
    base_currency: Currency | None
    settlement_currency: Currency
    realized_return: float
    realized_pnl: Money | None

    _events: list[OrderFilled]
    _trade_ids: list[TradeId]
    _buy_qty: Quantity
    _sell_qty: Quantity
    _commissions: dict[Currency, Money]

    def __init__(
        self,
        instrument: Instrument,
        fill: OrderFilled,
    ) -> None: ...
    def __eq__(self, other: Position) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    def purge_events_for_order(self, client_order_id: ClientOrderId) -> None:
        """
        Purge all order events for the given client order ID.

        Parameters
        ----------
        client_order_id : ClientOrderId
            The client order ID for the events to purge.

        """
        ...
    def info(self) -> str:
        """
        Return a summary description of the position.

        Returns
        -------
        str

        """
        ...
    def to_dict(self) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @property
    def symbol(self) -> Symbol:
        """
        Return the positions ticker symbol.

        Returns
        -------
        Symbol

        """
        ...
    @property
    def venue(self) -> Venue:
        """
        Return the positions trading venue.

        Returns
        -------
        Venue

        """
        ...
    @property
    def client_order_ids(self) -> list[ClientOrderId]:
        """
        Return the client order IDs associated with the position.

        Returns
        -------
        list[ClientOrderId]

        Notes
        -----
        Guaranteed not to contain duplicate IDs.

        """
        ...
    @property
    def venue_order_ids(self) -> list[VenueOrderId]:
        """
        Return the venue order IDs associated with the position.

        Returns
        -------
        list[VenueOrderId]

        Notes
        -----
        Guaranteed not to contain duplicate IDs.

        """
        ...
    @property
    def trade_ids(self) -> list[TradeId]:
        """
        Return the trade match IDs associated with the position.

        Returns
        -------
        list[TradeId]

        """
        ...
    @property
    def events(self) -> list[OrderFilled]:
        """
        Return the order fill events for the position.

        Returns
        -------
        list[Event]

        """
        ...
    @property
    def last_event(self) -> OrderFilled | None:
        """
        Return the last order fill event (if any after purging).

        Returns
        -------
        OrderFilled or ``None``

        """
        ...
    @property
    def last_trade_id(self) -> TradeId | None:
        """
        Return the last trade match ID for the position (if any after purging).

        Returns
        -------
        TradeId or ``None``

        """
        ...
    @property
    def event_count(self) -> int:
        """
        Return the count of order fill events applied to the position.

        Returns
        -------
        int

        """
        ...
    @property
    def is_open(self) -> bool:
        """
        Return whether the position side is **not** ``FLAT``.

        Returns
        -------
        bool

        """
        ...
    @property
    def is_closed(self) -> bool:
        """
        Return whether the position side is ``FLAT``.

        Returns
        -------
        bool

        """
        ...
    @property
    def is_long(self) -> bool:
        """
        Return whether the position side is ``LONG``.

        Returns
        -------
        bool

        """
        ...
    @property
    def is_short(self) -> bool:
        """
        Return whether the position side is ``SHORT``.

        Returns
        -------
        bool

        """
        ...
    @staticmethod
    def side_from_order_side(side: OrderSide) -> PositionSide:
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
        ...
    def closing_order_side(self) -> OrderSide:
        """
        Return the closing order side for the position.

        If the position is ``FLAT`` then will return ``NO_ORDER_SIDE``.

        Returns
        -------
        OrderSide

        """
        ...
    def signed_decimal_qty(self) -> Decimal:
        """
        Return a signed decimal representation of the position quantity.

         - If the position is LONG, the value is positive (e.g. Decimal('10.25'))
         - If the position is SHORT, the value is negative (e.g. Decimal('-10.25'))
         - If the position is FLAT, the value is zero (e.g. Decimal('0'))

        Returns
        -------
        Decimal

        """
        ...
    def is_opposite_side(self, side: OrderSide) -> bool:
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
        ...
    def apply(self, fill: OrderFilled) -> None:
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
        ...
    def notional_value(self, price: Price) -> Money:
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
        ...
    def calculate_pnl(
        self,
        avg_px_open: float,
        avg_px_close: float,
        quantity: Quantity,
    ) -> Money:
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
        ...
    def unrealized_pnl(self, price: Price) -> Money:
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
        ...
    def total_pnl(self, price: Price) -> Money:
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
        ...
    def commissions(self) -> list[Money]:
        """
        Return the total commissions generated by the position.

        Returns
        -------
        list[Money]

        """
        ...