# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Main Strategy
# -------------------------------------------------------------------------------------------------
"""
VWAP Wave trading strategy implementing four Auction Market Theory setups.

Classifies market into Balance (mean reversion) or Imbalance (trend following)
regimes and trades appropriate setups based on acceptance, exhaustion, and
volume confirmation.
"""

from __future__ import annotations

from decimal import Decimal
from typing import List
from typing import Optional

import pandas as pd

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import StrategyConfig
from nautilus_trader.indicators import AverageTrueRange
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.trading.strategy import Strategy

from vwap_wave.analysis.acceptance import AcceptanceEngine
from vwap_wave.analysis.exhaustion import ExhaustionEngine
from vwap_wave.analysis.regime_classifier import MarketRegime
from vwap_wave.analysis.regime_classifier import RegimeClassifier
from vwap_wave.analysis.regime_classifier import RegimeState
from vwap_wave.analysis.rejection import RejectionEngine
from vwap_wave.config.settings import VWAPWaveConfig
from vwap_wave.core.cvd_calculator import CVDCalculator
from vwap_wave.core.initial_balance import InitialBalanceTracker
from vwap_wave.core.volume_profile import VolumeProfileBuilder
from vwap_wave.core.vwap_engine import VWAPEngine
from vwap_wave.execution.order_factory import VWAPWaveOrderFactory
from vwap_wave.execution.trade_manager import TradeManager
from vwap_wave.risk.correlation_manager import CorrelationManager
from vwap_wave.risk.drawdown_manager import DrawdownManager
from vwap_wave.risk.position_sizer import PositionSizer
from vwap_wave.setups.base_setup import BaseSetup
from vwap_wave.setups.base_setup import SetupSignal
from vwap_wave.setups.base_setup import TradeDirection
from vwap_wave.setups.discovery_continuation import DiscoveryContinuationSetup
from vwap_wave.setups.fade_extremes import FadeExtremesSetup
from vwap_wave.setups.return_to_value import ReturnToValueSetup
from vwap_wave.setups.vwap_bounce import VWAPBounceSetup


class VWAPWaveStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``VWAPWaveStrategy`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    bar_type : BarType
        The bar type for the strategy.
    vwap_wave_config : VWAPWaveConfig, optional
        The VWAP Wave system configuration.
    atr_period : int, default 14
        The ATR period for volatility calculations.
    volume_ma_period : int, default 20
        The period for average volume calculation.
    request_bars : bool, default True
        If historical bars should be requested on strategy start.
    close_positions_on_stop : bool, default True
        If all open positions should be closed on strategy stop.

    """

    instrument_id: InstrumentId
    bar_type: BarType
    vwap_wave_config: Optional[VWAPWaveConfig] = None
    atr_period: int = 14
    volume_ma_period: int = 20
    request_bars: bool = True
    close_positions_on_stop: bool = True


class VWAPWaveStrategy(Strategy):
    """
    VWAP Wave trading strategy.

    Implements four Auction Market Theory setups:
    - Price Discovery Continuation
    - Fade Value Area Extremes
    - Return to Value
    - VWAP Bounce

    Parameters
    ----------
    config : VWAPWaveStrategyConfig
        The configuration for the instance.

    """

    def __init__(self, config: VWAPWaveStrategyConfig) -> None:
        super().__init__(config)

        # Configuration
        self._vwap_config = config.vwap_wave_config or VWAPWaveConfig()

        # Instrument
        self.instrument: Optional[Instrument] = None

        # Built-in indicators
        self.atr = AverageTrueRange(config.atr_period)

        # Core components (initialized in on_start)
        self.vwap_engine: Optional[VWAPEngine] = None
        self.ib_tracker: Optional[InitialBalanceTracker] = None
        self.cvd_calculator: Optional[CVDCalculator] = None
        self.volume_profile: Optional[VolumeProfileBuilder] = None

        # Analysis components
        self.acceptance_engine: Optional[AcceptanceEngine] = None
        self.rejection_engine: Optional[RejectionEngine] = None
        self.exhaustion_engine: Optional[ExhaustionEngine] = None
        self.regime_classifier: Optional[RegimeClassifier] = None

        # Setups
        self.setups: List[BaseSetup] = []

        # Risk management
        self.position_sizer: Optional[PositionSizer] = None
        self.drawdown_manager: Optional[DrawdownManager] = None
        self.correlation_manager: Optional[CorrelationManager] = None
        self.trade_manager: Optional[TradeManager] = None

        # Order factory wrapper
        self._order_factory: Optional[VWAPWaveOrderFactory] = None

        # State
        self._volume_history: List[float] = []
        self._current_regime: Optional[RegimeState] = None
        self._last_regime: Optional[MarketRegime] = None

    def on_start(self) -> None:
        """Initialize all components when strategy starts."""
        # Get instrument
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        # Initialize order factory wrapper
        self._order_factory = VWAPWaveOrderFactory(self.order_factory, self.instrument)

        # Initialize core components
        self.vwap_engine = VWAPEngine(self._vwap_config.vwap)
        self.ib_tracker = InitialBalanceTracker(self._vwap_config.ib)
        self.cvd_calculator = CVDCalculator(self._vwap_config.cvd)
        self.volume_profile = VolumeProfileBuilder(self._vwap_config.volume_profile)

        # Initialize analysis components
        self.acceptance_engine = AcceptanceEngine(self._vwap_config.acceptance)
        self.rejection_engine = RejectionEngine()
        self.exhaustion_engine = ExhaustionEngine(
            self._vwap_config.exhaustion,
            self.cvd_calculator,
            self.vwap_engine,
            self.ib_tracker,
        )
        self.regime_classifier = RegimeClassifier(
            self.vwap_engine,
            self.acceptance_engine,
        )

        # Initialize setups
        self.setups = [
            DiscoveryContinuationSetup(
                self._vwap_config,
                self.vwap_engine,
                self.ib_tracker,
                self.acceptance_engine,
            ),
            FadeExtremesSetup(
                self._vwap_config,
                self.exhaustion_engine,
                self.vwap_engine,
                self.volume_profile,
            ),
            ReturnToValueSetup(
                self._vwap_config,
                self.vwap_engine,
                self.acceptance_engine,
                self.rejection_engine,
            ),
            VWAPBounceSetup(
                self._vwap_config,
                self.vwap_engine,
                self.cvd_calculator,
                self.regime_classifier,
            ),
        ]

        # Initialize risk management
        self.drawdown_manager = DrawdownManager(self._vwap_config.risk)
        self.correlation_manager = CorrelationManager(self._vwap_config.risk)
        self.position_sizer = PositionSizer(
            self._vwap_config.risk,
            self.drawdown_manager,
            self.correlation_manager,
        )
        self.trade_manager = TradeManager(self._vwap_config.trade_mgmt)

        # Register built-in indicators
        self.register_indicator_for_bars(self.config.bar_type, self.atr)

        # Request historical data for warmup
        if self.config.request_bars:
            self.request_bars(
                self.config.bar_type,
                start=self._clock.utc_now() - pd.Timedelta(days=2),
            )

        # Subscribe to real-time data
        self.subscribe_bars(self.config.bar_type)

        self.log.info("VWAP Wave Strategy initialized", LogColor.GREEN)

    def on_bar(self, bar: Bar) -> None:
        """Process each bar update."""
        # Update built-in indicators are updated automatically via registration

        # Update custom indicators
        self._update_indicators(bar)

        # Get current ATR and average volume
        atr = self.atr.value if self.atr.initialized else 0.0
        avg_volume = self._get_average_volume()

        # Update core components
        self.vwap_engine.handle_bar(bar)
        self.ib_tracker.handle_bar(bar)
        self.cvd_calculator.handle_bar(bar)
        self.volume_profile.handle_bar(bar)

        # Update analysis components
        self.acceptance_engine.update(bar, atr, avg_volume)
        self.rejection_engine.update(bar, atr)
        self.exhaustion_engine.update(bar, atr, avg_volume)

        # Check if indicators are ready
        if not self._indicators_ready():
            self.log.info(
                f"Warming up indicators [{self.cache.bar_count(self.config.bar_type)}]",
                LogColor.BLUE,
            )
            return

        # Classify regime
        regime_state = self.regime_classifier.update(bar)
        self._current_regime = regime_state

        # Log regime changes
        self._log_regime_if_changed(regime_state)

        # Update drawdown tracking
        account = self.portfolio.account(self.config.instrument_id.venue)
        if account:
            self.drawdown_manager.update(account.balance_total())

        # Check if trading halted
        if self.drawdown_manager.is_halted:
            self.log.warning(
                f"Trading halted: {self.drawdown_manager.halt_reason}",
                LogColor.RED,
            )
            self._manage_existing_trades(bar, atr)
            return

        # Check position limits
        if self._count_open_positions() >= self._vwap_config.risk.max_concurrent_positions:
            self._manage_existing_trades(bar, atr)
            return

        # Evaluate setups based on regime
        signal = self._evaluate_setups(regime_state, bar, atr)

        # Execute signal if valid
        if signal.valid:
            self._execute_signal(signal, bar)

        # Manage existing trades
        self._manage_existing_trades(bar, atr)

    def _update_indicators(self, bar: Bar) -> None:
        """Update custom indicators and volume history."""
        volume = bar.volume.as_double()
        self._volume_history.append(volume)

        # Keep limited history
        max_history = self.config.volume_ma_period * 2
        if len(self._volume_history) > max_history:
            self._volume_history = self._volume_history[-max_history:]

    def _get_average_volume(self) -> float:
        """Calculate average volume."""
        if len(self._volume_history) < self.config.volume_ma_period:
            return sum(self._volume_history) / len(self._volume_history) if self._volume_history else 0.0

        return sum(self._volume_history[-self.config.volume_ma_period:]) / self.config.volume_ma_period

    def _indicators_ready(self) -> bool:
        """Check if all indicators are initialized."""
        return (
            self.atr.initialized
            and self.vwap_engine.initialized
            and self.ib_tracker.is_complete
            and len(self._volume_history) >= self.config.volume_ma_period
        )

    def _evaluate_setups(
        self,
        regime_state: RegimeState,
        bar: Bar,
        atr: float,
    ) -> SetupSignal:
        """Evaluate setups based on current regime."""
        # Regime-based setup priority
        if regime_state.regime == MarketRegime.BALANCE:
            priority_order = [1, 2, 0, 3]  # Fade, Return, Discovery, Bounce
        elif regime_state.regime in [
            MarketRegime.IMBALANCE_BULLISH,
            MarketRegime.IMBALANCE_BEARISH,
        ]:
            priority_order = [0, 3, 2, 1]  # Discovery, Bounce, Return, Fade
        else:  # BREAKOUT_UNCONFIRMED
            priority_order = [2, 1, 0, 3]  # Return, Fade, Discovery, Bounce

        for idx in priority_order:
            if idx < len(self.setups):
                setup = self.setups[idx]
                if setup.is_eligible(regime_state):
                    signal = setup.evaluate(regime_state, bar, atr)
                    if signal.valid:
                        return signal

        return SetupSignal.no_signal()

    def _execute_signal(self, signal: SetupSignal, bar: Bar) -> None:
        """Execute a trading signal."""
        # Check correlation constraints
        symbol = str(self.config.instrument_id)
        if not self.correlation_manager.can_open_position(symbol, signal.direction.value):
            self.log.info(f"Signal rejected: correlation constraint for {symbol}")
            return

        # Calculate position size
        account = self.portfolio.account(self.config.instrument_id.venue)
        if account is None:
            self.log.warning("Cannot get account for position sizing")
            return

        equity = account.balance_total()
        position_result = self.position_sizer.calculate(
            signal,
            equity,
            symbol,
        )

        if position_result.quantity == 0:
            self.log.info("Signal rejected: zero position size")
            return

        # Round quantity to instrument precision
        quantity = self._order_factory.round_quantity(position_result.quantity)
        if quantity < self._order_factory.min_quantity:
            self.log.info("Signal rejected: quantity below minimum")
            return

        # Create and submit order
        order_side = OrderSide.BUY if signal.direction == TradeDirection.LONG else OrderSide.SELL

        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=order_side,
            quantity=Quantity.from_str(str(quantity)),
            time_in_force=TimeInForce.GTC,
        )

        self.submit_order(order)

        # Register with trade manager
        self.trade_manager.register_trade(
            order.client_order_id,
            signal,
            position_result,
        )

        # Register correlation exposure
        self.correlation_manager.register_position(symbol, signal.direction.value)

        self.log.info(
            f"ENTRY: {signal.setup_type} | "
            f"Dir: {signal.direction.value} | "
            f"Qty: {quantity} | "
            f"Risk: {float(position_result.risk_percent):.2%} | "
            f"Conf: {signal.confidence:.2f}",
            LogColor.GREEN,
        )

    def _manage_existing_trades(self, bar: Bar, atr: float) -> None:
        """Manage all open positions."""
        for position in self.portfolio.positions_open(self.config.instrument_id.venue):
            if position.instrument_id != self.config.instrument_id:
                continue

            actions = self.trade_manager.manage(
                position.id,
                bar,
                atr,
                self.regime_classifier,
            )

            if actions:
                self._handle_trade_actions(position, actions)

    def _handle_trade_actions(self, position: Position, actions: dict) -> None:
        """Handle trade management actions."""
        if "close" in actions:
            self.log.info(
                f"Closing position: {actions['close'].get('reason', 'unknown')}",
                LogColor.YELLOW,
            )
            self.close_position(position)

        if "partial" in actions:
            partial = actions["partial"]
            partial_qty = position.quantity * Decimal(str(partial["percent"]))
            partial_qty = self._order_factory.round_quantity(partial_qty)

            if partial_qty >= self._order_factory.min_quantity:
                order_side = OrderSide.SELL if position.is_long else OrderSide.BUY
                order = self.order_factory.market(
                    instrument_id=self.config.instrument_id,
                    order_side=order_side,
                    quantity=Quantity.from_str(str(partial_qty)),
                    time_in_force=TimeInForce.GTC,
                    reduce_only=True,
                )
                self.submit_order(order)
                self.log.info(
                    f"Partial exit: {partial_qty} @ {partial['r_multiple']:.1f}R",
                    LogColor.CYAN,
                )

        if "modify_stop" in actions:
            # Note: Actual stop modification requires bracket order support
            # This logs the intended action for now
            stop_info = actions["modify_stop"]
            self.log.info(
                f"Trail stop update: {stop_info['new_stop']:.5f}",
                LogColor.CYAN,
            )

        if "review" in actions:
            review = actions["review"]
            self.log.warning(
                f"Trade review needed: {review['reason']}",
                LogColor.YELLOW,
            )

    def _count_open_positions(self) -> int:
        """Count open positions for this instrument."""
        count = 0
        for position in self.portfolio.positions_open(self.config.instrument_id.venue):
            if position.instrument_id == self.config.instrument_id:
                count += 1
        return count

    def _log_regime_if_changed(self, regime_state: RegimeState) -> None:
        """Log regime changes."""
        if self._last_regime != regime_state.regime:
            color = LogColor.GREEN if regime_state.regime != MarketRegime.BALANCE else LogColor.BLUE
            self.log.info(
                f"Regime: {regime_state.regime.value} | "
                f"Confidence: {regime_state.acceptance_confidence:.2f} | "
                f"Volume: {'YES' if regime_state.volume_confirmed else 'NO'}",
                color,
            )
            self._last_regime = regime_state.regime

    def on_order_filled(self, event: OrderFilled) -> None:
        """Handle order fill events."""
        self.trade_manager.on_fill(
            event.client_order_id,
            event.position_id,
            event.last_px.as_double(),
        )

    def on_position_opened(self, event: PositionOpened) -> None:
        """Handle position opened events."""
        self.log.info(
            f"Position opened: {event.position_id} | "
            f"Side: {event.opening_order_side.name}",
            LogColor.GREEN,
        )

    def on_position_closed(self, event: PositionClosed) -> None:
        """Handle position closed events."""
        trade = self.trade_manager.on_close(event.position_id)

        # Unregister correlation exposure
        symbol = str(self.config.instrument_id)
        self.correlation_manager.unregister_position(symbol)

        self.log.info(
            f"Position closed: {event.position_id} | "
            f"PnL: {event.realized_pnl}",
            LogColor.CYAN,
        )

    def on_stop(self) -> None:
        """Actions to be performed when the strategy is stopped."""
        self.cancel_all_orders(self.config.instrument_id)

        if self.config.close_positions_on_stop:
            self.close_all_positions(self.config.instrument_id)

        self.unsubscribe_bars(self.config.bar_type)

        self.log.info("VWAP Wave Strategy stopped", LogColor.YELLOW)

    def on_reset(self) -> None:
        """Actions to be performed when the strategy is reset."""
        # Reset indicators
        self.atr.reset()

        if self.vwap_engine:
            self.vwap_engine.reset()
        if self.ib_tracker:
            self.ib_tracker.reset()
        if self.cvd_calculator:
            self.cvd_calculator.reset()
        if self.volume_profile:
            self.volume_profile.reset()

        if self.acceptance_engine:
            self.acceptance_engine.reset()
        if self.rejection_engine:
            self.rejection_engine.reset()
        if self.exhaustion_engine:
            self.exhaustion_engine.reset()
        if self.regime_classifier:
            self.regime_classifier.reset()

        for setup in self.setups:
            setup.reset()

        if self.trade_manager:
            self.trade_manager.reset()
        if self.drawdown_manager:
            self.drawdown_manager.reset()
        if self.correlation_manager:
            self.correlation_manager.reset()

        self._volume_history.clear()
        self._current_regime = None
        self._last_regime = None

    def on_dispose(self) -> None:
        """Cleanup any resources used by the strategy."""
        pass
