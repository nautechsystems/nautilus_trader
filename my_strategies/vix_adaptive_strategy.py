"""
VIX-Adaptive TQQQ Strategy for NautilusTrader
- SPY SMA200 기반 매매 결정
- VIX 레벨에 따른 동적 배분:
  - SMA 아래: 현금 100%
  - SMA 위 + VIX < 20: TQQQ 95% (공격적)
  - SMA 위 + VIX 20-30: TQQQ 70% (조심)
  - SMA 위 + VIX > 30: TQQQ 30% (방어적)
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


class VIXAdaptiveConfig(StrategyConfig, frozen=True):
    """Configuration for VIX-Adaptive Strategy."""

    spy_instrument_id: InstrumentId
    tqqq_instrument_id: InstrumentId
    vix_instrument_id: InstrumentId  # VIX ETF or index
    spy_bar_type: BarType
    tqqq_bar_type: BarType
    vix_bar_type: BarType
    sma_period: PositiveInt = 200
    vix_low: PositiveFloat = 20.0     # Low VIX threshold
    vix_high: PositiveFloat = 30.0    # High VIX threshold
    alloc_aggressive: PositiveFloat = 0.95  # VIX < 20
    alloc_moderate: PositiveFloat = 0.70    # VIX 20-30
    alloc_defensive: PositiveFloat = 0.30   # VIX > 30
    stop_loss_pct: PositiveFloat = 0.15
    take_profit_pct: PositiveFloat = 0.50
    close_positions_on_stop: bool = True


class VIXAdaptiveStrategy(Strategy):
    """
    VIX-Adaptive TQQQ Strategy.
    Allocates based on VIX level when above 200SMA.
    """

    def __init__(self, config: VIXAdaptiveConfig) -> None:
        super().__init__(config)

        self.spy_instrument: Instrument = None
        self.tqqq_instrument: Instrument = None

        self.sma = SimpleMovingAverage(config.sma_period)

        self.last_spy_price: float = 0.0
        self.last_sma_value: float = 0.0
        self.last_vix: float = 20.0  # Default VIX
        self.above_sma: bool | None = None
        self.current_regime: str = "UNKNOWN"
        self.entry_price: float | None = None
        self.target_allocation: float = 0.0

    def on_start(self) -> None:
        """Actions on strategy start."""
        self.spy_instrument = self.cache.instrument(self.config.spy_instrument_id)
        self.tqqq_instrument = self.cache.instrument(self.config.tqqq_instrument_id)

        if self.spy_instrument is None or self.tqqq_instrument is None:
            self.log.error("Could not find instruments")
            self.stop()
            return

        self.register_indicator_for_bars(self.config.spy_bar_type, self.sma)

        self.request_bars(
            self.config.spy_bar_type,
            start=self._clock.utc_now() - pd.Timedelta(days=250),
        )

        self.subscribe_bars(self.config.spy_bar_type)
        self.subscribe_bars(self.config.tqqq_bar_type)
        self.subscribe_bars(self.config.vix_bar_type)

        self.log.info("VIX-Adaptive Strategy started", LogColor.GREEN)

    def on_bar(self, bar: Bar) -> None:
        """Handle incoming bars."""
        if bar.bar_type == self.config.spy_bar_type:
            self._handle_spy_bar(bar)
        elif bar.bar_type == self.config.tqqq_bar_type:
            self._handle_tqqq_bar(bar)
        elif bar.bar_type == self.config.vix_bar_type:
            self._handle_vix_bar(bar)

    def _handle_vix_bar(self, bar: Bar) -> None:
        """Update VIX level."""
        self.last_vix = float(bar.close)

    def _handle_spy_bar(self, bar: Bar) -> None:
        """Process SPY bar for regime detection."""
        if not self.sma.initialized:
            return

        self.last_spy_price = float(bar.close)
        self.last_sma_value = self.sma.value

        was_above = self.above_sma
        self.above_sma = self.last_spy_price >= self.last_sma_value

        # Determine regime based on SMA and VIX
        if not self.above_sma:
            new_regime = "CASH"
            self.target_allocation = 0.0
        elif self.last_vix < self.config.vix_low:
            new_regime = "AGGRESSIVE"
            self.target_allocation = self.config.alloc_aggressive
        elif self.last_vix < self.config.vix_high:
            new_regime = "MODERATE"
            self.target_allocation = self.config.alloc_moderate
        else:
            new_regime = "DEFENSIVE"
            self.target_allocation = self.config.alloc_defensive

        # Only rebalance on regime change
        if new_regime != self.current_regime:
            self.log.info(
                f"Regime: {self.current_regime} -> {new_regime} | "
                f"VIX: {self.last_vix:.1f} | Alloc: {self.target_allocation*100:.0f}%",
                LogColor.MAGENTA,
            )
            self.current_regime = new_regime
            self._rebalance()

    def _handle_tqqq_bar(self, bar: Bar) -> None:
        """Risk management on TQQQ."""
        if self.entry_price is None:
            return

        current_price = float(bar.close)
        pnl_pct = (current_price - self.entry_price) / self.entry_price

        if pnl_pct <= -self.config.stop_loss_pct:
            self.log.warning(f"STOP LOSS! PnL: {pnl_pct * 100:.1f}%")
            self._close_all()

        elif pnl_pct >= self.config.take_profit_pct:
            self.log.info(f"TAKE PROFIT! PnL: {pnl_pct * 100:.1f}%")
            self._close_all()

    def _rebalance(self) -> None:
        """Rebalance portfolio."""
        account = self.portfolio.account(self.tqqq_instrument.id.venue)
        if account is None:
            return

        is_long = self.portfolio.is_net_long(self.config.tqqq_instrument_id)

        if self.current_regime == "CASH":
            if is_long:
                self._close_all()
            return

        last_bar = self.cache.bar(self.config.tqqq_bar_type)
        if last_bar is None:
            return

        tqqq_price = float(last_bar.close)
        if tqqq_price <= 0:
            return

        balance = float(account.balance_total())
        target_value = balance * self.target_allocation * 0.95
        target_qty = int(target_value / tqqq_price)

        current_qty = 0
        positions = self.cache.positions_open(instrument_id=self.config.tqqq_instrument_id)
        if positions:
            current_qty = sum(int(p.quantity) for p in positions)

        qty_diff = target_qty - current_qty

        if qty_diff > 0:
            self._buy_tqqq(qty_diff)
        elif qty_diff < 0:
            self._sell_tqqq(abs(qty_diff))

    def _buy_tqqq(self, quantity: int) -> None:
        """Buy TQQQ."""
        if quantity <= 0:
            return

        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.tqqq_instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.tqqq_instrument.make_qty(Decimal(quantity)),
            time_in_force=TimeInForce.DAY,
        )
        self.submit_order(order)

        last_bar = self.cache.bar(self.config.tqqq_bar_type)
        if last_bar:
            self.entry_price = float(last_bar.close)

    def _sell_tqqq(self, quantity: int) -> None:
        """Sell TQQQ."""
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
        """Close all positions."""
        self.close_all_positions(self.config.tqqq_instrument_id)
        self.entry_price = None

    def on_event(self, event) -> None:
        """Handle events."""
        if isinstance(event, PositionOpened):
            if event.instrument_id == self.config.tqqq_instrument_id:
                self.entry_price = float(event.avg_px_open)

    def on_stop(self) -> None:
        """Cleanup."""
        self.cancel_all_orders(self.config.tqqq_instrument_id)
        if self.config.close_positions_on_stop:
            self.close_all_positions(self.config.tqqq_instrument_id)

    def on_reset(self) -> None:
        """Reset state."""
        self.sma.reset()
        self.last_spy_price = 0.0
        self.last_sma_value = 0.0
        self.last_vix = 20.0
        self.above_sma = None
        self.current_regime = "UNKNOWN"
        self.entry_price = None
        self.target_allocation = 0.0
