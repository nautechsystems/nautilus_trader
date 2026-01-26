# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Order Factory
# -------------------------------------------------------------------------------------------------
"""
Order construction utilities for VWAP Wave strategy.

Provides helper methods for creating orders with proper stops and targets.
"""

from __future__ import annotations

from decimal import Decimal
from typing import TYPE_CHECKING
from typing import List
from typing import Optional

from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import MarketOrder

from vwap_wave.setups.base_setup import SetupSignal
from vwap_wave.setups.base_setup import TradeDirection


if TYPE_CHECKING:
    from nautilus_trader.common.factories import OrderFactory
    from nautilus_trader.model.instruments import Instrument


class VWAPWaveOrderFactory:
    """
    Order construction utilities for VWAP Wave strategy.

    Wraps the NautilusTrader OrderFactory with convenience methods
    for creating orders from SetupSignals.

    Parameters
    ----------
    order_factory : OrderFactory
        The NautilusTrader order factory.
    instrument : Instrument
        The instrument being traded.

    """

    def __init__(
        self,
        order_factory: OrderFactory,
        instrument: Instrument,
    ):
        self._factory = order_factory
        self._instrument = instrument

    def create_entry_order(
        self,
        signal: SetupSignal,
        quantity: Decimal,
    ) -> MarketOrder:
        """
        Create a market entry order from a signal.

        Parameters
        ----------
        signal : SetupSignal
            The setup signal.
        quantity : Decimal
            The position quantity.

        Returns
        -------
        MarketOrder
            The entry order.

        """
        order_side = OrderSide.BUY if signal.direction == TradeDirection.LONG else OrderSide.SELL

        return self._factory.market(
            instrument_id=self._instrument.id,
            order_side=order_side,
            quantity=Quantity.from_str(str(quantity)),
            time_in_force=TimeInForce.GTC,
        )

    def create_stop_order(
        self,
        direction: TradeDirection,
        quantity: Decimal,
        stop_price: float,
    ):
        """
        Create a stop loss order.

        Parameters
        ----------
        direction : TradeDirection
            The position direction (stop is opposite).
        quantity : Decimal
            The position quantity.
        stop_price : float
            The stop price.

        Returns
        -------
        StopMarketOrder
            The stop order.

        """
        # Stop is opposite side
        order_side = OrderSide.SELL if direction == TradeDirection.LONG else OrderSide.BUY

        price = Price.from_str(
            f"{stop_price:.{self._instrument.price_precision}f}"
        )

        return self._factory.stop_market(
            instrument_id=self._instrument.id,
            order_side=order_side,
            quantity=Quantity.from_str(str(quantity)),
            trigger_price=price,
            time_in_force=TimeInForce.GTC,
            reduce_only=True,
        )

    def create_take_profit_order(
        self,
        direction: TradeDirection,
        quantity: Decimal,
        target_price: float,
    ):
        """
        Create a take profit order.

        Parameters
        ----------
        direction : TradeDirection
            The position direction.
        quantity : Decimal
            The position quantity.
        target_price : float
            The target price.

        Returns
        -------
        LimitOrder
            The take profit order.

        """
        # Target is opposite side
        order_side = OrderSide.SELL if direction == TradeDirection.LONG else OrderSide.BUY

        price = Price.from_str(
            f"{target_price:.{self._instrument.price_precision}f}"
        )

        return self._factory.limit(
            instrument_id=self._instrument.id,
            order_side=order_side,
            quantity=Quantity.from_str(str(quantity)),
            price=price,
            time_in_force=TimeInForce.GTC,
            reduce_only=True,
        )

    def create_bracket_orders(
        self,
        signal: SetupSignal,
        quantity: Decimal,
    ) -> dict:
        """
        Create entry with bracket (stop loss + take profit).

        Parameters
        ----------
        signal : SetupSignal
            The setup signal.
        quantity : Decimal
            The position quantity.

        Returns
        -------
        dict
            Dictionary with "entry", "stop", and "target" orders.

        """
        entry = self.create_entry_order(signal, quantity)
        stop = self.create_stop_order(signal.direction, quantity, signal.stop_price)
        target = self.create_take_profit_order(signal.direction, quantity, signal.target_price)

        return {
            "entry": entry,
            "stop": stop,
            "target": target,
        }

    def create_partial_exit_order(
        self,
        direction: TradeDirection,
        quantity: Decimal,
    ) -> MarketOrder:
        """
        Create a market order for partial exit.

        Parameters
        ----------
        direction : TradeDirection
            The position direction.
        quantity : Decimal
            The quantity to exit.

        Returns
        -------
        MarketOrder
            The partial exit order.

        """
        order_side = OrderSide.SELL if direction == TradeDirection.LONG else OrderSide.BUY

        return self._factory.market(
            instrument_id=self._instrument.id,
            order_side=order_side,
            quantity=Quantity.from_str(str(quantity)),
            time_in_force=TimeInForce.GTC,
            reduce_only=True,
        )

    def round_quantity(self, quantity: Decimal) -> Decimal:
        """Round quantity to instrument precision."""
        step = Decimal(str(self._instrument.size_increment))
        return (quantity // step) * step

    def round_price(self, price: float) -> Price:
        """Round price to instrument precision."""
        return Price.from_str(f"{price:.{self._instrument.price_precision}f}")

    @property
    def instrument_id(self) -> InstrumentId:
        """Get the instrument ID."""
        return self._instrument.id

    @property
    def min_quantity(self) -> Decimal:
        """Get the minimum order quantity."""
        return Decimal(str(self._instrument.min_quantity))

    @property
    def tick_size(self) -> Decimal:
        """Get the tick size."""
        return Decimal(str(self._instrument.price_increment))
