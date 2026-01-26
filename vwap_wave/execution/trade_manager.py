# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Trade Manager
# -------------------------------------------------------------------------------------------------
"""
Trade management including trailing stops, partial exits, and invalidation.

Manages open positions through their lifecycle with dynamic stop management
and profit taking.
"""

from __future__ import annotations

from dataclasses import dataclass
from dataclasses import field
from enum import Enum
from typing import TYPE_CHECKING
from typing import Callable
from typing import Dict
from typing import List
from typing import Optional

from nautilus_trader.model.data import Bar
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId

from vwap_wave.config.settings import TradeManagementConfig
from vwap_wave.risk.position_sizer import PositionSizeResult
from vwap_wave.setups.base_setup import SetupSignal
from vwap_wave.setups.base_setup import TradeDirection


if TYPE_CHECKING:
    from vwap_wave.analysis.regime_classifier import RegimeClassifier


class TradeState(Enum):
    """State of a managed trade."""

    PENDING = "pending"  # Order submitted but not filled
    ACTIVE = "active"  # Position open, initial stop
    TRAILING = "trailing"  # Trailing stop activated
    PARTIAL_EXIT = "partial_exit"  # Partial exit taken
    CLOSED = "closed"  # Position closed


@dataclass
class ManagedTrade:
    """A trade being actively managed."""

    order_id: ClientOrderId
    position_id: Optional[PositionId]
    signal: SetupSignal
    position_size: PositionSizeResult
    state: TradeState
    entry_price: float
    current_stop: float
    current_target: float
    highest_price: float  # For trailing (long)
    lowest_price: float  # For trailing (short)
    bars_in_trade: int
    partial_taken: bool
    trail_active: bool
    metadata: Dict = field(default_factory=dict)


class TradeManager:
    """
    Manages open trades with trailing stops, partials, and invalidation.

    Handles the complete lifecycle of trades from entry to exit,
    including dynamic stop adjustment and profit taking.

    Parameters
    ----------
    config : TradeManagementConfig
        The trade management configuration.

    """

    def __init__(self, config: TradeManagementConfig):
        self.config = config
        self._trades: Dict[str, ManagedTrade] = {}  # order_id -> trade
        self._position_trades: Dict[str, str] = {}  # position_id -> order_id

        # Callbacks for order actions
        self._modify_stop_callback: Optional[Callable] = None
        self._close_position_callback: Optional[Callable] = None

    def set_callbacks(
        self,
        modify_stop: Optional[Callable] = None,
        close_position: Optional[Callable] = None,
    ) -> None:
        """Set callbacks for order modifications."""
        self._modify_stop_callback = modify_stop
        self._close_position_callback = close_position

    def register_trade(
        self,
        order_id: ClientOrderId,
        signal: SetupSignal,
        position_size: PositionSizeResult,
    ) -> ManagedTrade:
        """
        Register a new trade for management.

        Parameters
        ----------
        order_id : ClientOrderId
            The entry order ID.
        signal : SetupSignal
            The setup signal that triggered the trade.
        position_size : PositionSizeResult
            The calculated position size.

        Returns
        -------
        ManagedTrade
            The registered trade object.

        """
        trade = ManagedTrade(
            order_id=order_id,
            position_id=None,
            signal=signal,
            position_size=position_size,
            state=TradeState.PENDING,
            entry_price=signal.entry_price,
            current_stop=signal.stop_price,
            current_target=signal.target_price,
            highest_price=signal.entry_price,
            lowest_price=signal.entry_price,
            bars_in_trade=0,
            partial_taken=False,
            trail_active=False,
        )

        self._trades[str(order_id)] = trade
        return trade

    def on_fill(self, order_id: ClientOrderId, position_id: PositionId, fill_price: float) -> None:
        """
        Handle order fill event.

        Parameters
        ----------
        order_id : ClientOrderId
            The filled order ID.
        position_id : PositionId
            The position ID.
        fill_price : float
            The fill price.

        """
        order_key = str(order_id)
        if order_key not in self._trades:
            return

        trade = self._trades[order_key]
        trade.position_id = position_id
        trade.entry_price = fill_price
        trade.highest_price = fill_price
        trade.lowest_price = fill_price
        trade.state = TradeState.ACTIVE

        self._position_trades[str(position_id)] = order_key

    def on_close(self, position_id: PositionId) -> Optional[ManagedTrade]:
        """
        Handle position close event.

        Parameters
        ----------
        position_id : PositionId
            The closed position ID.

        Returns
        -------
        Optional[ManagedTrade]
            The closed trade, if found.

        """
        position_key = str(position_id)
        if position_key not in self._position_trades:
            return None

        order_key = self._position_trades[position_key]
        if order_key in self._trades:
            trade = self._trades[order_key]
            trade.state = TradeState.CLOSED

            # Clean up
            del self._trades[order_key]
            del self._position_trades[position_key]
            return trade

        return None

    def manage(
        self,
        position_id: PositionId,
        bar: Bar,
        atr: float,
        regime_classifier: Optional[RegimeClassifier] = None,
    ) -> Optional[Dict]:
        """
        Manage an open position.

        Parameters
        ----------
        position_id : PositionId
            The position to manage.
        bar : Bar
            Current bar data.
        atr : float
            Current ATR value.
        regime_classifier : RegimeClassifier, optional
            Regime classifier for context.

        Returns
        -------
        Optional[Dict]
            Management action to take, if any.

        """
        position_key = str(position_id)
        if position_key not in self._position_trades:
            return None

        order_key = self._position_trades[position_key]
        if order_key not in self._trades:
            return None

        trade = self._trades[order_key]
        if trade.state == TradeState.CLOSED:
            return None

        trade.bars_in_trade += 1

        high = bar.high.as_double()
        low = bar.low.as_double()
        close = bar.close.as_double()

        # Update extreme prices
        if trade.signal.direction == TradeDirection.LONG:
            trade.highest_price = max(trade.highest_price, high)
        else:
            trade.lowest_price = min(trade.lowest_price, low)

        actions = {}

        # Check for invalidation conditions
        invalidation = self._check_invalidation(trade, bar, regime_classifier)
        if invalidation:
            actions["close"] = invalidation
            return actions

        # Check for partial exit
        if self.config.partial_exit_enabled and not trade.partial_taken:
            partial = self._check_partial_exit(trade, close)
            if partial:
                actions["partial"] = partial
                trade.partial_taken = True
                trade.state = TradeState.PARTIAL_EXIT

        # Check for trailing stop activation and update
        trail_update = self._check_trailing_stop(trade, atr)
        if trail_update:
            actions["modify_stop"] = trail_update

        # Check max duration
        if trade.bars_in_trade >= self.config.max_trade_duration_bars:
            actions["review"] = {
                "reason": "max_duration",
                "bars": trade.bars_in_trade,
            }

        return actions if actions else None

    def _check_invalidation(
        self,
        trade: ManagedTrade,
        bar: Bar,
        regime_classifier: Optional[RegimeClassifier],
    ) -> Optional[Dict]:
        """Check for trade invalidation conditions."""
        close = bar.close.as_double()

        # Stop hit (should be handled by exchange, but double-check)
        if trade.signal.direction == TradeDirection.LONG:
            if close <= trade.current_stop:
                return {"reason": "stop_hit", "price": close}
        else:
            if close >= trade.current_stop:
                return {"reason": "stop_hit", "price": close}

        # Regime change invalidation for continuation trades
        if regime_classifier and trade.signal.setup_type == "DISCOVERY_CONTINUATION":
            if regime_classifier.is_balanced():
                # Trend has ended, consider closing
                return {"reason": "regime_change", "new_regime": "balance"}

        return None

    def _check_partial_exit(self, trade: ManagedTrade, current_price: float) -> Optional[Dict]:
        """Check if partial exit conditions are met."""
        risk = abs(trade.entry_price - trade.signal.stop_price)
        if risk == 0:
            return None

        if trade.signal.direction == TradeDirection.LONG:
            current_r = (current_price - trade.entry_price) / risk
        else:
            current_r = (trade.entry_price - current_price) / risk

        if current_r >= self.config.partial_exit_rr:
            return {
                "percent": self.config.partial_exit_percent,
                "r_multiple": current_r,
                "price": current_price,
            }

        return None

    def _check_trailing_stop(self, trade: ManagedTrade, atr: float) -> Optional[Dict]:
        """Check and update trailing stop."""
        risk = abs(trade.entry_price - trade.signal.stop_price)
        if risk == 0:
            return None

        if trade.signal.direction == TradeDirection.LONG:
            current_r = (trade.highest_price - trade.entry_price) / risk

            # Activate trailing at threshold
            if current_r >= self.config.trail_activation_rr:
                trade.trail_active = True
                trade.state = TradeState.TRAILING

                # Calculate new trail stop
                trail_stop = trade.highest_price - (atr * self.config.trail_distance_atr)

                # Only move stop up
                if trail_stop > trade.current_stop:
                    # Check minimum step
                    step = trail_stop - trade.current_stop
                    if step >= atr * self.config.trail_step_atr:
                        trade.current_stop = trail_stop
                        return {
                            "new_stop": trail_stop,
                            "reason": "trail_update",
                            "highest": trade.highest_price,
                        }

        else:  # SHORT
            current_r = (trade.entry_price - trade.lowest_price) / risk

            if current_r >= self.config.trail_activation_rr:
                trade.trail_active = True
                trade.state = TradeState.TRAILING

                trail_stop = trade.lowest_price + (atr * self.config.trail_distance_atr)

                # Only move stop down
                if trail_stop < trade.current_stop:
                    step = trade.current_stop - trail_stop
                    if step >= atr * self.config.trail_step_atr:
                        trade.current_stop = trail_stop
                        return {
                            "new_stop": trail_stop,
                            "reason": "trail_update",
                            "lowest": trade.lowest_price,
                        }

        return None

    def get_trade(self, order_id: ClientOrderId) -> Optional[ManagedTrade]:
        """Get a managed trade by order ID."""
        return self._trades.get(str(order_id))

    def get_trade_by_position(self, position_id: PositionId) -> Optional[ManagedTrade]:
        """Get a managed trade by position ID."""
        position_key = str(position_id)
        if position_key not in self._position_trades:
            return None
        order_key = self._position_trades[position_key]
        return self._trades.get(order_key)

    def get_active_trades(self) -> List[ManagedTrade]:
        """Get all active trades."""
        return [t for t in self._trades.values() if t.state not in [TradeState.CLOSED, TradeState.PENDING]]

    def get_trade_count(self) -> int:
        """Get count of active trades."""
        return len(self.get_active_trades())

    def reset(self) -> None:
        """Reset all trade tracking."""
        self._trades.clear()
        self._position_trades.clear()
