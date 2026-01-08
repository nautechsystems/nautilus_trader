"""
Hybrid Options Strategy for NautilusTrader
- SPY < 200SMA: VIX 스트랭글 매도 (시뮬레이션)
- SPY > 200SMA, 이격도 > 5%: PMCC (시뮬레이션)
- SPY > 200SMA, 이격도 ≤ 5%: TQQQ 100%

옵션 수익률 시뮬레이션:
- VIX 스트랭글: VIX 기반 월 수익률 (VIX 높을수록 프리미엄 높음)
- PMCC: TQQQ 상승률의 70% + 월 1% 프리미엄
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


class HybridOptionsConfig(StrategyConfig, frozen=True):
    """Configuration for Hybrid Options Strategy."""

    spy_instrument_id: InstrumentId
    tqqq_instrument_id: InstrumentId
    vix_instrument_id: InstrumentId
    spy_bar_type: BarType
    tqqq_bar_type: BarType
    vix_bar_type: BarType
    sma_period: PositiveInt = 200
    distance_threshold: PositiveFloat = 0.05  # 5% threshold
    pmcc_upside_capture: PositiveFloat = 0.70  # PMCC captures 70% of TQQQ upside
    pmcc_monthly_premium: PositiveFloat = 0.01  # 1% monthly premium
    strangle_base_return: PositiveFloat = 0.02  # 2% base monthly return
    strangle_vix_multiplier: PositiveFloat = 0.001  # Extra return per VIX point
    allocation_pct: PositiveFloat = 0.95
    stop_loss_pct: PositiveFloat = 0.15
    close_positions_on_stop: bool = True


class HybridOptionsStrategy(Strategy):
    """
    Hybrid Options Strategy.

    Regimes:
    - STRANGLE: SPY below 200SMA -> VIX strangle selling (simulated)
    - PMCC: SPY above 200SMA, far from it -> PMCC (simulated)
    - TQQQ: SPY above 200SMA, close to it -> Full TQQQ
    """

    def __init__(self, config: HybridOptionsConfig) -> None:
        super().__init__(config)

        self.spy_instrument: Instrument = None
        self.tqqq_instrument: Instrument = None

        self.sma = SimpleMovingAverage(config.sma_period)

        # State
        self.last_spy_price: float = 0.0
        self.last_sma_value: float = 0.0
        self.last_vix: float = 20.0
        self.last_tqqq_price: float = 0.0
        self.current_regime: str = "UNKNOWN"
        self.entry_price: float | None = None

        # For PMCC/Strangle simulation
        self.simulated_balance: float = 0.0
        self.last_month: int = 0
        self.pmcc_entry_tqqq: float = 0.0

    def on_start(self) -> None:
        """Start strategy."""
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

        self.log.info(
            "Hybrid Options Strategy: STRANGLE / PMCC / TQQQ",
            LogColor.GREEN,
        )

    def on_bar(self, bar: Bar) -> None:
        """Handle bars."""
        if bar.bar_type == self.config.vix_bar_type:
            self.last_vix = float(bar.close)
        elif bar.bar_type == self.config.tqqq_bar_type:
            self.last_tqqq_price = float(bar.close)
            self._handle_tqqq_bar(bar)
        elif bar.bar_type == self.config.spy_bar_type:
            self._handle_spy_bar(bar)

    def _handle_spy_bar(self, bar: Bar) -> None:
        """Process SPY bar and determine regime."""
        if not self.sma.initialized:
            return

        self.last_spy_price = float(bar.close)
        self.last_sma_value = self.sma.value

        # Check for monthly premium collection (simulated)
        current_month = pd.Timestamp(bar.ts_event, unit='ns').month
        if current_month != self.last_month:
            self._collect_monthly_premium()
            self.last_month = current_month

        # Calculate distance from SMA
        distance_pct = (self.last_spy_price - self.last_sma_value) / self.last_sma_value

        # Determine new regime
        if self.last_spy_price < self.last_sma_value:
            new_regime = "STRANGLE"  # VIX strangle selling
        elif distance_pct > self.config.distance_threshold:
            new_regime = "PMCC"  # Far from SMA, use PMCC
        else:
            new_regime = "TQQQ"  # Close to SMA, full TQQQ

        # Handle regime change
        if new_regime != self.current_regime:
            self.log.info(
                f"Regime: {self.current_regime} -> {new_regime} | "
                f"SPY ${self.last_spy_price:.2f} | SMA ${self.last_sma_value:.2f} | "
                f"Distance: {distance_pct*100:+.1f}% | VIX: {self.last_vix:.1f}",
                LogColor.MAGENTA,
            )
            self._transition_regime(self.current_regime, new_regime)
            self.current_regime = new_regime

    def _transition_regime(self, old_regime: str, new_regime: str) -> None:
        """Handle regime transitions."""
        account = self.portfolio.account(self.tqqq_instrument.id.venue)
        if account is None:
            return

        balance = float(account.balance_total())
        is_long = self.portfolio.is_net_long(self.config.tqqq_instrument_id)

        # Exit old regime
        if old_regime == "TQQQ" and is_long:
            # Close TQQQ position
            self._close_all()
        elif old_regime == "PMCC":
            # PMCC exit - apply final simulated return
            if self.pmcc_entry_tqqq > 0 and self.last_tqqq_price > 0:
                tqqq_return = (self.last_tqqq_price - self.pmcc_entry_tqqq) / self.pmcc_entry_tqqq
                pmcc_return = tqqq_return * self.config.pmcc_upside_capture
                self.simulated_balance *= (1 + pmcc_return)
            if is_long:
                self._close_all()
        elif old_regime == "STRANGLE":
            # Strangle exit - balance already updated via monthly premium
            pass

        # Enter new regime
        if new_regime == "TQQQ":
            # Buy TQQQ with all available capital (including simulated gains)
            self._enter_tqqq_full()
        elif new_regime == "PMCC":
            # Start PMCC simulation
            self.simulated_balance = balance
            self.pmcc_entry_tqqq = self.last_tqqq_price
            # Buy partial TQQQ to simulate LEAPS delta exposure
            self._enter_pmcc()
        elif new_regime == "STRANGLE":
            # Start strangle simulation - keep cash
            self.simulated_balance = balance
            if is_long:
                self._close_all()

    def _enter_tqqq_full(self) -> None:
        """Enter full TQQQ position."""
        account = self.portfolio.account(self.tqqq_instrument.id.venue)
        if account is None:
            return

        balance = float(account.balance_total())
        if self.last_tqqq_price <= 0:
            return

        quantity = int((balance * self.config.allocation_pct * 0.95) / self.last_tqqq_price)
        if quantity <= 0:
            return

        self.log.info(
            f"TQQQ Full: Buying {quantity} @ ${self.last_tqqq_price:.2f}",
            LogColor.GREEN,
        )

        order = self.order_factory.market(
            instrument_id=self.config.tqqq_instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.tqqq_instrument.make_qty(Decimal(quantity)),
            time_in_force=TimeInForce.DAY,
        )
        self.submit_order(order)
        self.entry_price = self.last_tqqq_price

    def _enter_pmcc(self) -> None:
        """Enter PMCC simulation - buy partial TQQQ."""
        account = self.portfolio.account(self.tqqq_instrument.id.venue)
        if account is None:
            return

        balance = float(account.balance_total())
        if self.last_tqqq_price <= 0:
            return

        # PMCC: ~70% exposure via LEAPS
        quantity = int((balance * self.config.allocation_pct * 0.70) / self.last_tqqq_price)
        if quantity <= 0:
            return

        self.log.info(
            f"PMCC: Buying {quantity} TQQQ (70% exposure) @ ${self.last_tqqq_price:.2f}",
            LogColor.CYAN,
        )

        order = self.order_factory.market(
            instrument_id=self.config.tqqq_instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.tqqq_instrument.make_qty(Decimal(quantity)),
            time_in_force=TimeInForce.DAY,
        )
        self.submit_order(order)
        self.entry_price = self.last_tqqq_price

    def _collect_monthly_premium(self) -> None:
        """Simulate monthly premium collection."""
        if self.simulated_balance <= 0:
            return

        if self.current_regime == "STRANGLE":
            # VIX strangle premium: higher VIX = higher premium
            monthly_return = self.config.strangle_base_return + \
                           (self.last_vix - 15) * self.config.strangle_vix_multiplier
            monthly_return = max(0.01, min(monthly_return, 0.10))  # Cap between 1-10%

            self.simulated_balance *= (1 + monthly_return)
            self.log.info(
                f"STRANGLE Premium: +{monthly_return*100:.1f}% | "
                f"VIX: {self.last_vix:.1f} | Balance: ${self.simulated_balance:,.0f}",
                LogColor.YELLOW,
            )

        elif self.current_regime == "PMCC":
            # PMCC covered call premium
            self.simulated_balance *= (1 + self.config.pmcc_monthly_premium)
            self.log.info(
                f"PMCC Premium: +{self.config.pmcc_monthly_premium*100:.1f}% | "
                f"Balance: ${self.simulated_balance:,.0f}",
                LogColor.CYAN,
            )

    def _handle_tqqq_bar(self, bar: Bar) -> None:
        """Risk management on TQQQ."""
        if self.current_regime != "TQQQ" or self.entry_price is None:
            return

        current_price = float(bar.close)
        pnl_pct = (current_price - self.entry_price) / self.entry_price

        if pnl_pct <= -self.config.stop_loss_pct:
            self.log.warning(f"STOP LOSS! PnL: {pnl_pct * 100:.1f}%")
            self._close_all()

    def _close_all(self) -> None:
        """Close all TQQQ positions."""
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
        """Reset."""
        self.sma.reset()
        self.last_spy_price = 0.0
        self.last_sma_value = 0.0
        self.last_vix = 20.0
        self.last_tqqq_price = 0.0
        self.current_regime = "UNKNOWN"
        self.entry_price = None
        self.simulated_balance = 0.0
        self.last_month = 0
        self.pmcc_entry_tqqq = 0.0
