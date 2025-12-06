# -------------------------------------------------------------------------------------------------
#  Bot-folio Local Paper Trading Adapter for Nautilus Trader
#  https://github.com/mandeltechnologies/bot-folio
# -------------------------------------------------------------------------------------------------

from __future__ import annotations

import random
from dataclasses import dataclass
from decimal import Decimal

from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


@dataclass
class FillResult:
    """Result of a simulated fill."""

    fill_price: Price
    fill_qty: Quantity
    is_partial: bool
    latency_ms: int


class BotfolioFillModel:
    """
    Configurable fill model for simulating realistic order execution.

    Simulates:
    - Execution latency (base + random jitter)
    - Price slippage based on order notional value
    - Partial fills (optional)

    Parameters
    ----------
    base_latency_ms : int, default 50
        Base execution latency in milliseconds.
    latency_jitter_ms : int, default 20
        Random jitter added to base latency (0 to jitter_ms).
    slippage_bps : float, default 5.0
        Slippage in basis points per $10,000 notional.
        Larger orders incur proportionally more slippage.
    partial_fill_prob : float, default 0.0
        Probability of a partial fill (0.0 to 1.0).
    min_partial_fill_pct : float, default 0.5
        Minimum percentage of order filled on partial fill.

    """

    def __init__(
        self,
        base_latency_ms: int = 50,
        latency_jitter_ms: int = 20,
        slippage_bps: float = 5.0,
        partial_fill_prob: float = 0.0,
        min_partial_fill_pct: float = 0.5,
    ) -> None:
        self.base_latency_ms = base_latency_ms
        self.latency_jitter_ms = latency_jitter_ms
        self.slippage_bps = slippage_bps
        self.partial_fill_prob = partial_fill_prob
        self.min_partial_fill_pct = min_partial_fill_pct

    def simulate_fill(
        self,
        order_side: OrderSide,
        quantity: Quantity,
        market_price: Price,
    ) -> FillResult:
        """
        Simulate a fill for the given order parameters.

        Parameters
        ----------
        order_side : OrderSide
            The side of the order (BUY or SELL).
        quantity : Quantity
            The order quantity.
        market_price : Price
            The current market price.

        Returns
        -------
        FillResult
            The simulated fill result with price, quantity, and latency.

        """
        # Calculate latency
        latency_ms = self.base_latency_ms + random.randint(0, self.latency_jitter_ms)

        # Calculate notional value
        qty_decimal = Decimal(str(quantity))
        price_decimal = Decimal(str(market_price))
        notional = qty_decimal * price_decimal

        # Calculate slippage based on notional
        # slippage_bps per $10,000 notional
        notional_factor = notional / Decimal("10000")
        slippage_decimal = Decimal(str(self.slippage_bps)) * notional_factor / Decimal("10000")

        # Apply slippage in direction unfavorable to the order
        if order_side == OrderSide.BUY:
            # Buyer pays more
            fill_price_decimal = price_decimal * (Decimal("1") + slippage_decimal)
        else:
            # Seller receives less
            fill_price_decimal = price_decimal * (Decimal("1") - slippage_decimal)

        # Round to reasonable precision (2 decimal places for USD)
        fill_price_decimal = fill_price_decimal.quantize(Decimal("0.01"))
        fill_price = Price.from_str(str(fill_price_decimal))

        # Determine if partial fill
        is_partial = random.random() < self.partial_fill_prob
        if is_partial:
            # Fill between min_partial_fill_pct and 100%
            fill_pct = random.uniform(self.min_partial_fill_pct, 1.0)
            fill_qty_decimal = qty_decimal * Decimal(str(fill_pct))
            # Round to reasonable precision
            fill_qty_decimal = fill_qty_decimal.quantize(Decimal("0.00000001"))
            fill_qty = Quantity.from_str(str(fill_qty_decimal))
        else:
            fill_qty = quantity

        return FillResult(
            fill_price=fill_price,
            fill_qty=fill_qty,
            is_partial=is_partial,
            latency_ms=latency_ms,
        )

    def calculate_slippage_price(
        self,
        order_side: OrderSide,
        quantity: Quantity,
        market_price: Price,
    ) -> Price:
        """
        Calculate the fill price with slippage applied.

        This is a convenience method that returns just the price without
        the full FillResult.

        Parameters
        ----------
        order_side : OrderSide
            The side of the order (BUY or SELL).
        quantity : Quantity
            The order quantity.
        market_price : Price
            The current market price.

        Returns
        -------
        Price
            The fill price with slippage applied.

        """
        result = self.simulate_fill(order_side, quantity, market_price)
        return result.fill_price

