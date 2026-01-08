"""
Adaptive Regime Strategy for NautilusTrader
- SPY SMA200 기반 시장 국면 파악
- 이격도에 따른 동적 배분
  - SMA 아래: 현금 100%
  - SMA 위, 5% 이내: TQQQ 100%
  - SMA 위, 5% 초과: TQQQ 50% (조정 대비)
"""

from decimal import Decimal

import pandas as pd

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import PositiveFloat, PositiveInt, StrategyConfig
from nautilus_trader.indicators import SimpleMovingAverage
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.model.enums import OrderSide, TimeInForce
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.trading.strategy import Strategy


class AdaptiveRegimeConfig(StrategyConfig, frozen=True):
    """Configuration for Adaptive Regime Strategy."""

    spy_instrument_id: InstrumentId
    tqqq_instrument_id: InstrumentId
    spy_bar_type: BarType
    tqqq_bar_type: BarType
    sma_period: PositiveInt = 200
    near_sma_threshold: PositiveFloat = 0.05  # 5% threshold for "near SMA"
    high_allocation: PositiveFloat = 0.95  # 95% when near SMA
    low_allocation: PositiveFloat = 0.50   # 50% when far from SMA
    stop_loss_pct: PositiveFloat = 0.15
    take_profit_pct: PositiveFloat = 0.50
    close_positions_on_stop: bool = True


class AdaptiveRegimeStrategy(Strategy):
    """
    Adaptive Regime Strategy based on SPY 200SMA and distance from SMA.

    - Below SMA: 100% Cash
    - Above SMA, within 5%: 95% TQQQ
    - Above SMA, beyond 5%: 50% TQQQ
    """

    def __init__(self, config: AdaptiveRegimeConfig) -> None:
        super().__init__(config)

        self.spy_instrument: Instrument = None
        self.tqqq_instrument: Instrument = None

        # SMA indicator
        self.sma = SimpleMovingAverage(config.sma_period)

        # State tracking
        self.last_spy_price: float = 0.0
        self.last_sma_value: float = 0.0
        self.current_regime: str = "UNKNOWN"  # CASH, AGGRESSIVE, CONSERVATIVE
        self.entry_price: float | None = None
        self.target_allocation: float = 0.0

    def on_start(self) -> None:
        """Actions on strategy start."""
        self.spy_instrument = self.cache.instrument(self.config.spy_instrument_id)
        self.tqqq_instrument = self.cache.instrument(self.config.tqqq_instrument_id)

        if self.spy_instrument is None:
            self.log.error(f"Could not find SPY: {self.config.spy_instrument_id}")
            self.stop()
            return

        if self.tqqq_instrument is None:
            self.log.error(f"Could not find TQQQ: {self.config.tqqq_instrument_id}")
            self.stop()
            return

        self.register_indicator_for_bars(self.config.spy_bar_type, self.sma)

        self.request_bars(
            self.config.spy_bar_type,
            start=self._clock.utc_now() - pd.Timedelta(days=250),
        )

        self.subscribe_bars(self.config.spy_bar_type)
        self.subscribe_bars(self.config.tqqq_bar_type)

        self.log.info(
            f"Adaptive Regime Strategy started: SPY SMA{self.config.sma_period}",
            LogColor.GREEN,
        )

    def on_bar(self, bar: Bar) -> None:
        """Handle incoming bars."""
        if bar.bar_type == self.config.spy_bar_type:
            self._handle_spy_bar(bar)
        elif bar.bar_type == self.config.tqqq_bar_type:
            self._handle_tqqq_bar(bar)

    def _handle_spy_bar(self, bar: Bar) -> None:
        """Process SPY bar for regime detection."""
        if not self.sma.initialized:
            return

        self.last_spy_price = float(bar.close)
        self.last_sma_value = self.sma.value

        # Calculate distance from SMA
        distance_pct = (self.last_spy_price - self.last_sma_value) / self.last_sma_value

        # Determine regime
        if self.last_spy_price < self.last_sma_value:
            new_regime = "CASH"
            self.target_allocation = 0.0
        elif distance_pct <= self.config.near_sma_threshold:
            new_regime = "AGGRESSIVE"
            self.target_allocation = self.config.high_allocation
        else:
            new_regime = "CONSERVATIVE"
            self.target_allocation = self.config.low_allocation

        # Log regime change
        if new_regime != self.current_regime:
            self.log.info(
                f"Regime change: {self.current_regime} -> {new_regime} | "
                f"SPY ${self.last_spy_price:.2f} | SMA ${self.last_sma_value:.2f} | "
                f"Distance: {distance_pct * 100:+.2f}%",
                LogColor.MAGENTA,
            )
            self.current_regime = new_regime
            self._rebalance()

    def _handle_tqqq_bar(self, bar: Bar) -> None:
        """Process TQQQ bar for risk management."""
        if self.entry_price is None:
            return

        current_price = float(bar.close)
        pnl_pct = (current_price - self.entry_price) / self.entry_price

        if pnl_pct <= -self.config.stop_loss_pct:
            self.log.warning(f"STOP LOSS triggered! PnL: {pnl_pct * 100:.1f}%")
            self._close_all()

        elif pnl_pct >= self.config.take_profit_pct:
            self.log.info(f"TAKE PROFIT triggered! PnL: {pnl_pct * 100:.1f}%")
            self._close_all()

    def _rebalance(self) -> None:
        """Rebalance portfolio based on target allocation."""
        account = self.portfolio.account(self.tqqq_instrument.id.venue)
        if account is None:
            self.log.error("No account found")
            return

        # Get current position
        is_flat = self.portfolio.is_flat(self.config.tqqq_instrument_id)
        is_long = self.portfolio.is_net_long(self.config.tqqq_instrument_id)

        # Cash regime - close all
        if self.current_regime == "CASH":
            if is_long:
                self.log.info("Closing position - entering CASH regime")
                self._close_all()
            return

        # Get current TQQQ price
        last_bar = self.cache.bar(self.config.tqqq_bar_type)
        if last_bar is None:
            self.log.error("No TQQQ price data")
            return

        tqqq_price = float(last_bar.close)
        if tqqq_price <= 0:
            return

        # Calculate target quantity
        balance = float(account.balance_total())
        target_value = balance * self.target_allocation * 0.95  # Leave buffer
        target_qty = int(target_value / tqqq_price)

        # Get current quantity
        current_qty = 0
        positions = self.cache.positions_open(instrument_id=self.config.tqqq_instrument_id)
        if positions:
            current_qty = sum(int(p.quantity) for p in positions)

        # Calculate difference
        qty_diff = target_qty - current_qty

        if qty_diff > 0:
            # Need to buy more
            self.log.info(
                f"Buying {qty_diff} TQQQ @ ${tqqq_price:.2f} "
                f"(target: {target_qty}, current: {current_qty})",
                LogColor.GREEN,
            )
            self._buy_tqqq(qty_diff)

        elif qty_diff < 0:
            # Need to sell some
            sell_qty = abs(qty_diff)
            self.log.info(
                f"Selling {sell_qty} TQQQ @ ${tqqq_price:.2f} "
                f"(target: {target_qty}, current: {current_qty})",
                LogColor.YELLOW,
            )
            self._sell_tqqq(sell_qty)

    def _buy_tqqq(self, quantity: int) -> None:
        """Execute TQQQ buy order."""
        if quantity <= 0:
            return

        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.tqqq_instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.tqqq_instrument.make_qty(Decimal(quantity)),
            time_in_force=TimeInForce.DAY,
        )
        self.submit_order(order)

        # Update entry price
        last_bar = self.cache.bar(self.config.tqqq_bar_type)
        if last_bar:
            self.entry_price = float(last_bar.close)

    def _sell_tqqq(self, quantity: int) -> None:
        """Execute TQQQ sell order."""
        if quantity <= 0:
            return

        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.tqqq_instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.tqqq_instrument.make_qty(Decimal(quantity)),
            time_in_force=TimeInForce.DAY,
        )
        self.submit_order(order)

    def _close_all(self) -> None:
        """Close all TQQQ positions."""
        self.close_all_positions(self.config.tqqq_instrument_id)
        self.entry_price = None

    def on_event(self, event) -> None:
        """Handle position events."""
        if isinstance(event, PositionOpened):
            if event.instrument_id == self.config.tqqq_instrument_id:
                self.entry_price = float(event.avg_px_open)

    def on_stop(self) -> None:
        """Cleanup on stop."""
        self.cancel_all_orders(self.config.tqqq_instrument_id)
        if self.config.close_positions_on_stop:
            self.close_all_positions(self.config.tqqq_instrument_id)

        self.unsubscribe_bars(self.config.spy_bar_type)
        self.unsubscribe_bars(self.config.tqqq_bar_type)

    def on_reset(self) -> None:
        """Reset strategy state."""
        self.sma.reset()
        self.last_spy_price = 0.0
        self.last_sma_value = 0.0
        self.current_regime = "UNKNOWN"
        self.entry_price = None
        self.target_allocation = 0.0
