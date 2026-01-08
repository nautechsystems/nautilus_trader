"""
MNQ 3x + 이중SMA + GDX 전략 for NautilusTrader

검증된 NautilusTrader EMA Cross 패턴 기반으로 구현.
참조: nautilus_trader/examples/strategies/ema_cross.py

전략 로직:
- QQQ > SMA200 AND QQQ > SMA50 → LONG (MNQ 연속선물)
- QQQ < SMA200 AND QQQ < SMA50 → HEDGE (GDX)
- 그 외 → 이전 포지션 유지 (히스테리시스)

밴드 리밸런싱:
- 레버리지가 밴드(target ± band%) 벗어날 때만 조정
- 기본: 3x ± 15% → 2.55x~3.45x 범위 내 유지

자동 롤오버:
- CONTFUT (연속 선물) 사용으로 IBKR이 자동 처리
"""

from decimal import Decimal
from pathlib import Path

import pandas as pd

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import PositiveFloat, PositiveInt, StrategyConfig
from nautilus_trader.core.message import Event
from nautilus_trader.indicators import SimpleMovingAverage
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.model.enums import OrderSide, TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.trading.strategy import Strategy

from slack_bot import SlackNotifier


class MNQDualSMAConfig(StrategyConfig, frozen=True):
    """
    Configuration for MNQ Dual SMA Strategy.

    Parameters
    ----------
    qqq_instrument_id : InstrumentId
        QQQ instrument ID for signal calculation.
    long_instrument_id : InstrumentId
        Long instrument ID (MNQ CONTFUT).
    hedge_instrument_id : InstrumentId
        Hedge instrument ID (GDX).
    qqq_bar_type : BarType
        The bar type for QQQ (daily bars).
    long_bar_type : BarType
        The bar type for long instrument.
    hedge_bar_type : BarType
        The bar type for hedge instrument.
    sma_long_period : int, default 200
        Long SMA period.
    sma_short_period : int, default 50
        Short SMA period.
    target_leverage : float, default 3.0
        Base target leverage for MNQ.
    target_leverage_high : float, default 4.0
        High target leverage when capital is sufficient.
    leverage_4x_threshold : float, default 84000
        Capital threshold for 4x leverage ($84k).
    rebalance_band_pct : float, default 0.15
        Rebalancing band percentage (±15%).
    close_positions_on_stop : bool, default True
        Close all positions on strategy stop.
    """

    qqq_instrument_id: InstrumentId
    long_instrument_id: InstrumentId
    hedge_instrument_id: InstrumentId
    qqq_bar_type: BarType
    long_bar_type: BarType
    hedge_bar_type: BarType
    sma_long_period: PositiveInt = 200
    sma_short_period: PositiveInt = 50
    target_leverage: PositiveFloat = 3.0
    target_leverage_high: PositiveFloat = 4.0
    leverage_4x_threshold: PositiveFloat = 84_000
    rebalance_band_pct: PositiveFloat = 0.15
    close_positions_on_stop: bool = True


class MNQDualSMAStrategy(Strategy):
    """
    MNQ 3x/4x + 이중SMA + GDX 전략.

    동적 레버리지:
    - 자본 < $84k: 3x 레버리지
    - 자본 >= $84k: 4x 레버리지 (밴드 관리 가능)

    검증된 NautilusTrader 패턴 기반:
    - indicators_initialized() 사용
    - portfolio.is_flat() / is_net_long() 사용
    - close_all_positions() 후 새 포지션 진입

    Parameters
    ----------
    config : MNQDualSMAConfig
        The configuration for the instance.
    """

    def __init__(self, config: MNQDualSMAConfig) -> None:
        super().__init__(config)

        # Instruments (loaded in on_start)
        self.qqq_instrument: Instrument | None = None
        self.long_instrument: Instrument | None = None
        self.hedge_instrument: Instrument | None = None

        # SMA indicators for QQQ
        self.sma_long = SimpleMovingAverage(config.sma_long_period)
        self.sma_short = SimpleMovingAverage(config.sma_short_period)

        # Position state for hysteresis: 'LONG', 'HEDGE', or None
        self.current_position: str | None = None
        self.state_file = Path(__file__).parent / ".nautilus_position_state"

        # Current leverage (dynamically updated based on balance)
        self._current_target_leverage: float = config.target_leverage

        # Slack notifier
        self.slack = SlackNotifier()

        # Load last position state
        self._load_position_state()

    def _load_position_state(self) -> None:
        """Load last position state from file (for hysteresis)."""
        try:
            if self.state_file.exists():
                self.current_position = self.state_file.read_text().strip()
                if self.current_position not in ("LONG", "HEDGE"):
                    self.current_position = None
        except Exception:
            self.current_position = None

    def _save_position_state(self, position: str) -> None:
        """Save current position state to file."""
        try:
            self.state_file.write_text(position)
            self.current_position = position
        except Exception:
            pass

    def _get_dynamic_leverage(self, balance: float) -> float:
        """
        예수금에 따른 동적 레버리지 계산.

        - 자본 < threshold: 3x (기본)
        - 자본 >= threshold: 4x (밴드 관리 가능)

        4x 전환 조건:
        - 충분한 계약 수로 ±15% 밴드 관리 가능
        - 1계약 변화가 레버리지에 미치는 영향이 15% 미만
        """
        threshold = self.config.leverage_4x_threshold

        if balance >= threshold:
            new_leverage = self.config.target_leverage_high
        else:
            new_leverage = self.config.target_leverage

        # 레버리지 변경 시 로깅
        if new_leverage != self._current_target_leverage:
            self.log.info(
                f"레버리지 변경: {self._current_target_leverage}x → {new_leverage}x "
                f"(잔고: ${balance:,.0f})",
                LogColor.MAGENTA,
            )
            self._current_target_leverage = new_leverage

        return new_leverage

    # =========================================================================
    # Strategy Lifecycle (검증된 패턴)
    # =========================================================================

    def on_start(self) -> None:
        """Actions to be performed on strategy start."""
        # Load instruments from cache
        self.qqq_instrument = self.cache.instrument(self.config.qqq_instrument_id)
        self.long_instrument = self.cache.instrument(self.config.long_instrument_id)
        self.hedge_instrument = self.cache.instrument(self.config.hedge_instrument_id)

        if self.qqq_instrument is None:
            self.log.error(f"Could not find instrument: {self.config.qqq_instrument_id}")
            self.stop()
            return

        if self.long_instrument is None:
            self.log.error(f"Could not find instrument: {self.config.long_instrument_id}")
            self.stop()
            return

        if self.hedge_instrument is None:
            self.log.error(f"Could not find instrument: {self.config.hedge_instrument_id}")
            self.stop()
            return

        # Register indicators (검증된 패턴)
        self.register_indicator_for_bars(self.config.qqq_bar_type, self.sma_long)
        self.register_indicator_for_bars(self.config.qqq_bar_type, self.sma_short)

        # Request historical bars for warmup
        self.request_bars(
            self.config.qqq_bar_type,
            start=self._clock.utc_now() - pd.Timedelta(days=250),
        )

        # Subscribe to bars
        self.subscribe_bars(self.config.qqq_bar_type)
        self.subscribe_bars(self.config.long_bar_type)
        self.subscribe_bars(self.config.hedge_bar_type)

        self.log.info("=" * 60, LogColor.GREEN)
        self.log.info("MNQ 3x + 이중SMA + GDX 전략 시작", LogColor.GREEN)
        self.log.info("=" * 60, LogColor.GREEN)
        self.log.info(
            f"시그널: {self.config.qqq_instrument_id} "
            f"SMA({self.config.sma_long_period}+{self.config.sma_short_period})",
            LogColor.CYAN,
        )
        self.log.info(f"롱: {self.config.long_instrument_id} (CONTFUT)", LogColor.CYAN)
        self.log.info(f"헤지: {self.config.hedge_instrument_id}", LogColor.CYAN)
        self.log.info(
            f"레버리지: {self.config.target_leverage}x "
            f"(밴드: ±{self.config.rebalance_band_pct*100:.0f}%)",
            LogColor.CYAN,
        )
        self.log.info(f"현재 포지션: {self.current_position or 'N/A'}", LogColor.CYAN)

    def on_bar(self, bar: Bar) -> None:
        """Actions to be performed when receiving a bar."""
        # Only process QQQ bars for signal
        if bar.bar_type != self.config.qqq_bar_type:
            return

        # 검증된 패턴: indicators_initialized() 사용
        if not self.indicators_initialized():
            self.log.info(
                f"Waiting for indicators to warm up "
                f"[{self.cache.bar_count(self.config.qqq_bar_type)}/{self.config.sma_long_period}]",
                color=LogColor.BLUE,
            )
            return

        # Calculate signal
        qqq_price = float(bar.close)
        sma_long_value = self.sma_long.value
        sma_short_value = self.sma_short.value

        above_sma_long = qqq_price > sma_long_value
        above_sma_short = qqq_price > sma_short_value

        dist_long = (qqq_price - sma_long_value) / sma_long_value * 100
        dist_short = (qqq_price - sma_short_value) / sma_short_value * 100

        # Determine target position
        target_position = self._get_target_position(above_sma_long, above_sma_short)

        # Log current state
        self.log.info(
            f"QQQ ${qqq_price:.2f} | "
            f"SMA200 {dist_long:+.1f}% | SMA50 {dist_short:+.1f}% | "
            f"Signal: {target_position}",
            LogColor.CYAN,
        )

        # Execute position switch if needed
        if target_position != self.current_position:
            self._switch_position(
                target_position,
                qqq_price,
                sma_long_value,
                sma_short_value,
            )
        else:
            # Band rebalancing
            self._rebalance_if_needed()

    def _get_target_position(self, above_long: bool, above_short: bool) -> str:
        """Determine target position based on dual SMA with hysteresis."""
        if above_long and above_short:
            return "LONG"
        elif not above_long and not above_short:
            return "HEDGE"
        else:
            # Hysteresis: maintain previous position
            return self.current_position or "HEDGE"

    # =========================================================================
    # Position Switching (검증된 패턴)
    # =========================================================================

    def _switch_position(
        self,
        target: str,
        qqq_price: float,
        sma_long: float,
        sma_short: float,
    ) -> None:
        """Switch from current position to target position."""
        self.log.info(
            f"Position switch: {self.current_position} → {target}",
            LogColor.MAGENTA,
        )

        # Slack notification
        self.slack.notify_signal(target, qqq_price, sma_long, sma_short)

        # 검증된 패턴: close_all_positions() 후 새 포지션 진입
        if self.current_position == "LONG":
            self.close_all_positions(self.config.long_instrument_id)
        elif self.current_position == "HEDGE":
            self.close_all_positions(self.config.hedge_instrument_id)

        # Open new position
        if target == "LONG":
            self._open_long_position()
        elif target == "HEDGE":
            self._open_hedge_position()

        self._save_position_state(target)

    def _open_long_position(self) -> None:
        """Open long position (MNQ CONTFUT)."""
        account = self.portfolio.account(self.long_instrument.id.venue)
        if account is None:
            self.log.error("No account found for long instrument")
            return

        balance = float(account.balance_total())
        last_bar = self.cache.bar(self.config.long_bar_type)
        if last_bar is None:
            self.log.error("No price data for long instrument")
            return

        price = float(last_bar.close)
        if price <= 0:
            return

        # 동적 레버리지 계산 (예수금에 따라 3x 또는 4x)
        target_leverage = self._get_dynamic_leverage(balance)
        target_value = balance * target_leverage
        quantity = int(target_value / price)

        if quantity <= 0:
            self.log.warning("Insufficient funds for long position")
            return

        self.log.info(
            f"LONG: {self.config.long_instrument_id} {quantity} contracts @ ${price:.2f}",
            LogColor.GREEN,
        )

        # Slack notification
        self.slack.notify_position_change(
            action="BUY",
            symbol=str(self.config.long_instrument_id),
            quantity=quantity,
            price=price,
            from_position=self.current_position,
            to_position="LONG",
        )

        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.long_instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.long_instrument.make_qty(Decimal(quantity)),
            time_in_force=TimeInForce.DAY,
        )
        self.submit_order(order)

    def _open_hedge_position(self) -> None:
        """Open hedge position (GDX)."""
        account = self.portfolio.account(self.hedge_instrument.id.venue)
        if account is None:
            self.log.error("No account found for hedge instrument")
            return

        balance = float(account.balance_total())
        last_bar = self.cache.bar(self.config.hedge_bar_type)
        if last_bar is None:
            self.log.error("No price data for hedge instrument")
            return

        price = float(last_bar.close)
        if price <= 0:
            return

        # GDX: 1x leverage (full position)
        quantity = int((balance * 0.95) / price)

        if quantity <= 0:
            self.log.warning("Insufficient funds for hedge position")
            return

        self.log.info(
            f"HEDGE: {self.config.hedge_instrument_id} {quantity} shares @ ${price:.2f}",
            LogColor.YELLOW,
        )

        # Slack notification
        self.slack.notify_position_change(
            action="BUY",
            symbol=str(self.config.hedge_instrument_id),
            quantity=quantity,
            price=price,
            from_position=self.current_position,
            to_position="HEDGE",
        )

        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.hedge_instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.hedge_instrument.make_qty(Decimal(quantity)),
            time_in_force=TimeInForce.DAY,
        )
        self.submit_order(order)

    # =========================================================================
    # Band Rebalancing
    # =========================================================================

    def _rebalance_if_needed(self) -> None:
        """
        Band rebalancing: only adjust when leverage is outside band.

        Only applies to LONG position (MNQ).
        HEDGE (GDX) is always 1x, no rebalancing needed.
        """
        if self.current_position != "LONG":
            return

        # Check if we have a position
        if self.portfolio.is_flat(self.config.long_instrument_id):
            return

        account = self.portfolio.account(self.long_instrument.id.venue)
        if account is None:
            return

        balance = float(account.balance_total())
        if balance <= 0:
            return

        last_bar = self.cache.bar(self.config.long_bar_type)
        if last_bar is None:
            return

        price = float(last_bar.close)
        if price <= 0:
            return

        # Get current position
        net_position = float(self.portfolio.net_position(self.config.long_instrument_id))
        if net_position <= 0:
            return

        # Calculate current leverage
        position_value = net_position * price
        current_leverage = position_value / balance

        # 동적 레버리지 계산 (예수금에 따라 3x 또는 4x)
        target = self._get_dynamic_leverage(balance)
        band = self.config.rebalance_band_pct
        lower_bound = target * (1 - band)
        upper_bound = target * (1 + band)

        # Check if within band
        if lower_bound <= current_leverage <= upper_bound:
            return  # Within band, no action needed

        # Outside band → rebalance to target
        target_value = balance * target
        target_qty = int(target_value / price)
        diff = target_qty - int(net_position)

        if abs(diff) < 1:
            return

        self.log.info(
            f"Rebalance: {current_leverage:.2f}x → {target:.1f}x "
            f"(band: {lower_bound:.2f}x~{upper_bound:.2f}x)",
            LogColor.BLUE,
        )

        # Slack notification
        self.slack.notify_rebalance(
            str(self.config.long_instrument_id),
            int(diff),
            price,
        )

        if diff > 0:
            order = self.order_factory.market(
                instrument_id=self.config.long_instrument_id,
                order_side=OrderSide.BUY,
                quantity=self.long_instrument.make_qty(Decimal(int(diff))),
                time_in_force=TimeInForce.DAY,
            )
            self.submit_order(order)
        else:
            order = self.order_factory.market(
                instrument_id=self.config.long_instrument_id,
                order_side=OrderSide.SELL,
                quantity=self.long_instrument.make_qty(Decimal(abs(int(diff)))),
                time_in_force=TimeInForce.DAY,
            )
            self.submit_order(order)

    # =========================================================================
    # Strategy Lifecycle (검증된 패턴)
    # =========================================================================

    def on_stop(self) -> None:
        """Actions to be performed when the strategy is stopped."""
        self.cancel_all_orders(self.config.long_instrument_id)
        self.cancel_all_orders(self.config.hedge_instrument_id)

        if self.config.close_positions_on_stop:
            self.close_all_positions(self.config.long_instrument_id)
            self.close_all_positions(self.config.hedge_instrument_id)

        self.unsubscribe_bars(self.config.qqq_bar_type)
        self.unsubscribe_bars(self.config.long_bar_type)
        self.unsubscribe_bars(self.config.hedge_bar_type)

        self.log.info("Strategy stopped", LogColor.RED)
        self.slack.send("Strategy stopped", ":octagonal_sign:")

    def on_reset(self) -> None:
        """Actions to be performed when the strategy is reset."""
        self.sma_long.reset()
        self.sma_short.reset()

    def on_dispose(self) -> None:
        """Cleanup resources."""
        pass
