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
"""
Order Flow Strategy - POI-Based Dynamic Trading.

Core Logic:
1. POI-FIRST: Only consider trading when price is at a Point of Interest
2. ONE POSITION: Either long, short, or flat - no stacking
3. ORDERFLOW CONFIRMATION: At POI, read delta/footprint/imbalances to determine direction
4. DYNAMIC EXITS: TP/SL/Trailing Stop + close when orderflow flips at POI

Points of Interest (POIs):
- Volume Profile: POC, VAL, VAH, HVN, LVN
- VWAP Bands: VWAP, ±1std, ±2std, ±3std
- Initial Balance: IB High, IB Low, IB Mid, extensions

Orderflow Signals:
- Cumulative Delta direction and momentum
- Footprint delta per price level
- Stacked bid/ask imbalances

Risk Management:
- Take Profit: Percentage-based (default 0.3%)
- Stop Loss: Percentage-based (default 0.3%)
- Trailing Stop: Activates at 0.25% profit, trails at 0.1%
"""

from decimal import Decimal
from enum import Enum
from typing import Optional

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import (
    OrderSide,
    PositionSide,
    TimeInForce,
    TriggerType,
    TrailingOffsetType,
)
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.trading.strategy import Strategy

from nautilus_trader.examples.indicators.orderflow import (
    VolumeProfile,
    VWAPBands,
    InitialBalance,
    CumulativeDelta,
    FootprintAggregator,
    StackedImbalanceDetector,
)


class Bias(Enum):
    """Orderflow bias direction."""
    BULLISH = "BULLISH"
    BEARISH = "BEARISH"
    NEUTRAL = "NEUTRAL"


class POIType(Enum):
    """Type of Point of Interest."""
    SUPPORT = "SUPPORT"      # Expect bounce (long)
    RESISTANCE = "RESISTANCE"  # Expect rejection (short)
    NEUTRAL = "NEUTRAL"      # Could go either way (POC, VWAP)


class OrderFlowStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for OrderFlowStrategy.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    tick_size : float
        The price tick size for volume aggregation.
    trade_size : Decimal
        The size for each trade (single entry, no stacking).
    poi_tolerance : float
        How close price must be to POI (in ticks) to be considered "at POI".
    warmup_ticks : int
        Number of ticks before strategy starts trading (indicator warmup).
    tp_pct : float
        Take profit percentage (default 0.3% = 30 basis points).
    sl_pct : float
        Stop loss percentage (default 0.3% = 30 basis points).
    trailing_activation_pct : float
        Trailing stop activation percentage (default 0.25% = 25 basis points).
    trailing_offset_pct : float
        Trailing stop offset percentage (default 0.1% = 10 basis points).
    use_emulated_orders : bool
        Whether to use emulated orders for SL/TP (required for backtest).
    """

    instrument_id: InstrumentId
    tick_size: float = 0.01
    trade_size: Decimal = Decimal("0.01")
    poi_tolerance: float = 5.0  # Within 5 ticks of POI
    warmup_ticks: int = 1000    # Wait for 1000 ticks before trading
    tp_pct: float = 0.30        # 0.3% take profit
    sl_pct: float = 0.30        # 0.3% stop loss
    trailing_activation_pct: float = 0.25  # 0.25% to activate trailing stop
    trailing_offset_pct: float = 0.10      # 0.1% trailing offset
    use_emulated_orders: bool = True       # Emulated for backtest compatibility


class OrderFlowStrategy(Strategy):
    """
    Order Flow Strategy - POI-Based Dynamic Trading.

    Trading Logic:
    1. Wait for price to reach a Point of Interest (POI)
    2. At POI, read orderflow to determine bias
    3. Enter ONE position based on bias + POI type
    4. Exit when orderflow flips or price leaves POI zone
    5. Can reverse: close long → open short (and vice versa)
    """

    def __init__(self, config: OrderFlowStrategyConfig) -> None:
        super().__init__(config)

        # Configuration
        self.instrument_id = config.instrument_id
        self.tick_size = config.tick_size
        self.trade_size = config.trade_size
        self.poi_tolerance = config.poi_tolerance
        self.warmup_ticks = config.warmup_ticks

        # Risk management config
        self.tp_pct = config.tp_pct
        self.sl_pct = config.sl_pct
        self.trailing_activation_pct = config.trailing_activation_pct
        self.trailing_offset_pct = config.trailing_offset_pct
        self.use_emulated_orders = config.use_emulated_orders

        # Instrument reference
        self.instrument: Optional[Instrument] = None

        # Initialize indicators
        self.volume_profile = VolumeProfile(tick_size=self.tick_size)
        self.vwap = VWAPBands(reset_hour_utc=0, num_std_bands=3)
        self.initial_balance = InitialBalance(num_extensions=3)
        self.cumulative_delta = CumulativeDelta(reset_hour_utc=0)
        self.footprint = FootprintAggregator(tick_size=self.tick_size)
        self.stacked_imbalance = StackedImbalanceDetector(
            tick_size=self.tick_size,
            imbalance_ratio=3.0,
            min_stack_count=3,
        )

        # State tracking
        self._last_price: float = 0.0
        self._tick_count: int = 0
        self._prev_delta: float = 0.0
        self._last_poi: Optional[str] = None  # Track which POI we're at
        self._trade_count: int = 0
        self._entry_price: float = 0.0  # Track entry price for SL/TP

    def on_start(self) -> None:
        """Actions to be performed on strategy start."""
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        self.subscribe_trade_ticks(self.instrument_id)

        self.log.info(f"OrderFlowStrategy started for {self.instrument_id}")
        self.log.info(f"  Trade Size: {self.trade_size}")
        self.log.info(f"  POI Tolerance: {self.poi_tolerance} ticks")
        self.log.info(f"  Warmup: {self.warmup_ticks} ticks")
        self.log.info(f"  TP: {self.tp_pct}% | SL: {self.sl_pct}%")
        self.log.info(f"  Trailing: activates at {self.trailing_activation_pct}%, trails at {self.trailing_offset_pct}%")

    def on_trade_tick(self, tick: TradeTick) -> None:
        """Process incoming trade ticks and update all indicators."""
        # Update all indicators
        self.volume_profile.handle_trade_tick(tick)
        self.vwap.handle_trade_tick(tick)
        self.initial_balance.handle_trade_tick(tick)
        self.cumulative_delta.handle_trade_tick(tick)
        self.footprint.handle_trade_tick(tick)
        self.stacked_imbalance.handle_trade_tick(tick)

        self._last_price = tick.price.as_double()
        self._tick_count += 1

        # Skip during warmup period
        if self._tick_count < self.warmup_ticks:
            return

        # Core trading logic
        self._evaluate_and_trade()

    def _evaluate_and_trade(self) -> None:
        """
        Core trading logic:
        1. Check if we're at a POI
        2. If at POI, read orderflow to determine bias
        3. Execute trade based on POI type + orderflow bias
        """
        if not self.instrument:
            return

        # Step 1: Find which POI we're at (if any)
        poi = self._get_poi_context()

        # Get current position
        position = self._get_position()

        # Step 2: If NOT at a POI
        if poi is None:
            # If we have a position, check for exit
            if position is not None:
                self._check_exit_away_from_poi(position)
            return

        # Step 3: We ARE at a POI - read orderflow
        bias = self._get_orderflow_bias()

        self.log.info(
            f"AT POI: {poi['name']} ({poi['type'].value}) | "
            f"Price={self._last_price:.2f} | Level={poi['level']:.2f} | "
            f"Bias={bias.value}"
        )

        # Step 4: Make trading decision
        self._execute_trade_decision(poi, bias, position)

    def _get_poi_context(self) -> Optional[dict]:
        """
        Check if price is at any Point of Interest.
        Returns dict with POI info or None if not at POI.
        """
        tolerance = self.poi_tolerance * self.tick_size
        pois = []

        # Volume Profile levels
        if self.volume_profile.initialized:
            if abs(self._last_price - self.volume_profile.vah) <= tolerance:
                pois.append({"name": "VAH", "level": self.volume_profile.vah,
                            "type": POIType.RESISTANCE})
            if abs(self._last_price - self.volume_profile.val) <= tolerance:
                pois.append({"name": "VAL", "level": self.volume_profile.val,
                            "type": POIType.SUPPORT})
            if abs(self._last_price - self.volume_profile.poc) <= tolerance:
                pois.append({"name": "POC", "level": self.volume_profile.poc,
                            "type": POIType.NEUTRAL})

        # VWAP levels
        if self.vwap.initialized:
            if abs(self._last_price - self.vwap.vwap) <= tolerance:
                pois.append({"name": "VWAP", "level": self.vwap.vwap,
                            "type": POIType.NEUTRAL})
            for i, band in enumerate(self.vwap.upper_bands):
                if abs(self._last_price - band) <= tolerance:
                    pois.append({"name": f"VWAP+{i+1}std", "level": band,
                                "type": POIType.RESISTANCE})
            for i, band in enumerate(self.vwap.lower_bands):
                if abs(self._last_price - band) <= tolerance:
                    pois.append({"name": f"VWAP-{i+1}std", "level": band,
                                "type": POIType.SUPPORT})

        # Initial Balance levels
        if self.initial_balance.is_complete:
            if abs(self._last_price - self.initial_balance.ib_high) <= tolerance:
                pois.append({"name": "IB_HIGH", "level": self.initial_balance.ib_high,
                            "type": POIType.RESISTANCE})
            if abs(self._last_price - self.initial_balance.ib_low) <= tolerance:
                pois.append({"name": "IB_LOW", "level": self.initial_balance.ib_low,
                            "type": POIType.SUPPORT})
            if abs(self._last_price - self.initial_balance.ib_mid) <= tolerance:
                pois.append({"name": "IB_MID", "level": self.initial_balance.ib_mid,
                            "type": POIType.NEUTRAL})

        # Return highest priority POI (resistance/support take priority over neutral)
        if not pois:
            return None

        # Prioritize: RESISTANCE/SUPPORT first, then by proximity
        pois.sort(key=lambda x: (x["type"] == POIType.NEUTRAL,
                                 abs(self._last_price - x["level"])))
        return pois[0]

    def _get_orderflow_bias(self) -> Bias:
        """
        Read orderflow indicators to determine current bias.
        Returns BULLISH, BEARISH, or NEUTRAL.
        """
        bullish_signals = 0
        bearish_signals = 0

        # 1. Cumulative Delta direction
        delta = self.cumulative_delta.value
        if delta > 50:
            bullish_signals += 1
        elif delta < -50:
            bearish_signals += 1

        # 2. Delta momentum (is it accelerating or exhausting?)
        delta_change = delta - self._prev_delta
        self._prev_delta = delta

        if delta > 0 and delta_change < -20:  # Positive but declining = exhaustion
            bearish_signals += 1
        elif delta < 0 and delta_change > 20:  # Negative but rising = recovery
            bullish_signals += 1
        elif abs(delta_change) > 30:  # Strong momentum
            if delta_change > 0:
                bullish_signals += 1
            else:
                bearish_signals += 1

        # 3. Stacked imbalances (strong institutional signal)
        if self.stacked_imbalance.has_bullish_signal:
            bullish_signals += 2
        if self.stacked_imbalance.has_bearish_signal:
            bearish_signals += 2

        # 4. Footprint delta at current price level
        footprint_delta = self.footprint.total_delta
        if footprint_delta > 100:
            bullish_signals += 1
        elif footprint_delta < -100:
            bearish_signals += 1

        # Determine bias
        if bullish_signals > bearish_signals:
            return Bias.BULLISH
        elif bearish_signals > bullish_signals:
            return Bias.BEARISH
        else:
            return Bias.NEUTRAL

    def _get_position(self):
        """Get current open position for this instrument."""
        positions = self.cache.positions_open(instrument_id=self.instrument_id)
        return positions[0] if positions else None

    def _execute_trade_decision(self, poi: dict, bias: Bias, position) -> None:
        """
        Execute trade based on POI type and orderflow bias.

        Logic:
        - At RESISTANCE + BEARISH bias → SHORT (or close long)
        - At SUPPORT + BULLISH bias → LONG (or close short)
        - At NEUTRAL POI → follow bias
        - Conflicting signals → close position, stay flat
        """
        poi_type = poi["type"]

        # Determine desired action
        desired_side: Optional[PositionSide] = None

        if poi_type == POIType.RESISTANCE:
            if bias == Bias.BEARISH:
                desired_side = PositionSide.SHORT
            elif bias == Bias.BULLISH:
                # Bullish at resistance - could be breakout, wait for confirmation
                self.log.info(f"Bullish bias at resistance - watching for breakout")
                return

        elif poi_type == POIType.SUPPORT:
            if bias == Bias.BULLISH:
                desired_side = PositionSide.LONG
            elif bias == Bias.BEARISH:
                # Bearish at support - could be breakdown, wait for confirmation
                self.log.info(f"Bearish bias at support - watching for breakdown")
                return

        elif poi_type == POIType.NEUTRAL:
            # At neutral POI (POC, VWAP), follow bias
            if bias == Bias.BULLISH:
                desired_side = PositionSide.LONG
            elif bias == Bias.BEARISH:
                desired_side = PositionSide.SHORT

        # Execute based on desired side vs current position
        if desired_side is None:
            return

        if position is None:
            # No position - enter new
            self._enter_position(desired_side, poi["name"])
        elif position.side != desired_side:
            # Wrong side - close and reverse
            self._close_and_reverse(position, desired_side, poi["name"])
        # else: already in correct position, hold

    def _check_exit_away_from_poi(self, position) -> None:
        """
        Check if we should exit when price has moved away from POI.
        Exit if orderflow flips against us.
        """
        bias = self._get_orderflow_bias()

        if position.side == PositionSide.LONG and bias == Bias.BEARISH:
            self.log.info("EXIT LONG: Orderflow flipped bearish away from POI")
            self._close_position()
        elif position.side == PositionSide.SHORT and bias == Bias.BULLISH:
            self.log.info("EXIT SHORT: Orderflow flipped bullish away from POI")
            self._close_position()

    def _enter_position(self, side: PositionSide, poi_name: str) -> None:
        """Enter a new position with SL, TP, and trailing stop."""
        if not self.instrument:
            return

        order_side = OrderSide.BUY if side == PositionSide.LONG else OrderSide.SELL
        exit_side = OrderSide.SELL if side == PositionSide.LONG else OrderSide.BUY

        # Store entry info for SL/TP calculation
        self._entry_price = self._last_price
        quantity = self.instrument.make_qty(self.trade_size)

        # Calculate SL and TP prices based on percentage
        if side == PositionSide.LONG:
            sl_price = self._entry_price * (1 - self.sl_pct / 100)
            tp_price = self._entry_price * (1 + self.tp_pct / 100)
        else:  # SHORT
            sl_price = self._entry_price * (1 + self.sl_pct / 100)
            tp_price = self._entry_price * (1 - self.tp_pct / 100)

        # Round to instrument precision
        sl_price = self.instrument.make_price(sl_price)
        tp_price = self.instrument.make_price(tp_price)

        # Calculate trailing stop activation price
        if side == PositionSide.LONG:
            trailing_activation = self._entry_price * (1 + self.trailing_activation_pct / 100)
        else:
            trailing_activation = self._entry_price * (1 - self.trailing_activation_pct / 100)
        trailing_activation_price = self.instrument.make_price(trailing_activation)

        # Trailing offset in basis points (0.1% = 10 bp)
        trailing_offset_bp = Decimal(str(self.trailing_offset_pct * 100))

        # Emulation trigger for backtest (orders managed by strategy engine)
        emulation_trigger = TriggerType.LAST_PRICE if self.use_emulated_orders else TriggerType.NO_TRIGGER

        self._trade_count += 1
        self.log.info(
            f"ENTRY #{self._trade_count}: {side.value} at {poi_name} | "
            f"Price={self._last_price:.2f} | SL={sl_price} | TP={tp_price} | "
            f"Trailing activates at {trailing_activation_price}"
        )

        # 1. Market entry order
        entry_order = self.order_factory.market(
            instrument_id=self.instrument.id,
            order_side=order_side,
            quantity=quantity,
            time_in_force=TimeInForce.IOC,
        )
        self.submit_order(entry_order)

        # 2. Stop Loss order
        sl_order = self.order_factory.stop_market(
            instrument_id=self.instrument.id,
            order_side=exit_side,
            quantity=quantity,
            trigger_price=sl_price,
            trigger_type=TriggerType.LAST_PRICE,
            time_in_force=TimeInForce.GTC,
            reduce_only=True,
            emulation_trigger=emulation_trigger,
        )
        self.submit_order(sl_order)
        self.log.info(f"  SL order submitted: {sl_order.client_order_id}")

        # 3. Take Profit order (limit)
        tp_order = self.order_factory.limit(
            instrument_id=self.instrument.id,
            order_side=exit_side,
            quantity=quantity,
            price=tp_price,
            time_in_force=TimeInForce.GTC,
            reduce_only=True,
            emulation_trigger=emulation_trigger,
        )
        self.submit_order(tp_order)
        self.log.info(f"  TP order submitted: {tp_order.client_order_id}")

        # 4. Trailing Stop order (activates at 0.25%, trails at 0.1%)
        trailing_order = self.order_factory.trailing_stop_market(
            instrument_id=self.instrument.id,
            order_side=exit_side,
            quantity=quantity,
            activation_price=trailing_activation_price,
            trailing_offset=trailing_offset_bp,
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            trigger_type=TriggerType.LAST_PRICE,
            time_in_force=TimeInForce.GTC,
            reduce_only=True,
            emulation_trigger=emulation_trigger,
        )
        self.submit_order(trailing_order)
        self.log.info(f"  Trailing stop submitted: {trailing_order.client_order_id}")

    def _close_position(self) -> None:
        """Close current position and cancel all protective orders."""
        if not self.instrument:
            return
        # Cancel all pending orders (SL, TP, trailing) before closing
        self.cancel_all_orders(self.instrument.id)
        self.close_all_positions(self.instrument.id)

    def _close_and_reverse(self, position, new_side: PositionSide, poi_name: str) -> None:
        """Close current position and open opposite direction."""
        if not self.instrument:
            return

        # First cancel all protective orders and close existing position
        self.log.info(
            f"REVERSING: {position.side.value} → {new_side.value} at {poi_name}"
        )
        self.cancel_all_orders(self.instrument.id)
        self.close_all_positions(self.instrument.id)

        # Then enter new position with new SL/TP/trailing
        self._enter_position(new_side, poi_name)

    def get_indicator_state(self) -> dict:
        """Return current state of all indicators for debugging/logging."""
        return {
            "volume_profile": {
                "poc": self.volume_profile.poc,
                "vah": self.volume_profile.vah,
                "val": self.volume_profile.val,
                "total_volume": self.volume_profile.total_volume,
                "hvn_count": len(self.volume_profile.hvn_levels),
                "lvn_count": len(self.volume_profile.lvn_levels),
            },
            "vwap": {
                "value": self.vwap.vwap,
                "std_dev": self.vwap.std_dev,
                "upper_1": self.vwap.upper_bands[0] if self.vwap.upper_bands else 0,
                "lower_1": self.vwap.lower_bands[0] if self.vwap.lower_bands else 0,
            },
            "initial_balance": {
                "ib_high": self.initial_balance.ib_high,
                "ib_low": self.initial_balance.ib_low,
                "ib_mid": self.initial_balance.ib_mid,
                "is_complete": self.initial_balance.is_complete,
            },
            "cumulative_delta": {
                "value": self.cumulative_delta.value,
                "buy_volume": self.cumulative_delta.buy_volume,
                "sell_volume": self.cumulative_delta.sell_volume,
                "delta_ratio": self.cumulative_delta.delta_ratio,
            },
            "footprint": {
                "poc_price": self.footprint.poc_price,
                "total_delta": self.footprint.total_delta,
                "buy_volume": self.footprint.buy_volume,
                "sell_volume": self.footprint.sell_volume,
            },
            "stacked_imbalance": {
                "bullish_signals": len(self.stacked_imbalance.stacked_ask_imbalances),
                "bearish_signals": len(self.stacked_imbalance.stacked_bid_imbalances),
                "last_signal": self.stacked_imbalance.last_signal.name,
            },
        }

    def on_stop(self) -> None:
        """Actions to be performed when the strategy is stopped."""
        if self.instrument is None:
            return

        # Cancel all orders and close positions
        self.cancel_all_orders(self.instrument.id)
        self.close_all_positions(self.instrument.id)

        self.log.info("OrderFlowStrategy stopped")

    def on_reset(self) -> None:
        """Actions to be performed when the strategy is reset."""
        # Reset all indicators
        self.volume_profile.reset()
        self.vwap.reset()
        self.initial_balance.reset()
        self.cumulative_delta.reset()
        self.footprint.reset()
        self.stacked_imbalance.reset()

        self._last_price = 0.0
        self._entry_price = 0.0
        self._tick_count = 0
        self._prev_delta = 0.0
        self._trade_count = 0
        self.log.info("OrderFlowStrategy reset")

