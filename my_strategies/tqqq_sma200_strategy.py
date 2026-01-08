"""
TQQQ + SPY SMA200 Strategy for NautilusTrader
- SPY SMA 200 기반 매매 결정
- SMA 위: 100% TQQQ
- SMA 아래: 현금 (전량 청산)
- 손절 15%, 익절 50%
"""

from decimal import Decimal

import pandas as pd

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.indicators import SimpleMovingAverage
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.events import PositionChanged
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.trading.strategy import Strategy


class TQQQSMA200Config(StrategyConfig, frozen=True):
    """
    Configuration for TQQQ SMA200 Strategy.

    Parameters
    ----------
    spy_instrument_id : InstrumentId
        SPY instrument ID for SMA calculation.
    tqqq_instrument_id : InstrumentId
        TQQQ instrument ID for trading.
    spy_bar_type : BarType
        The bar type for SPY (daily bars recommended).
    tqqq_bar_type : BarType
        The bar type for TQQQ.
    sma_period : int, default 200
        SMA period for SPY.
    allocation_pct : float, default 0.80
        Percentage of account to allocate (0.0 to 1.0).
    stop_loss_pct : float, default 0.15
        Stop loss percentage (0.15 = 15%).
    take_profit_pct : float, default 0.50
        Take profit percentage (0.50 = 50%).
    close_positions_on_stop : bool, default True
        If all open positions should be closed on strategy stop.
    """

    spy_instrument_id: InstrumentId
    tqqq_instrument_id: InstrumentId
    spy_bar_type: BarType
    tqqq_bar_type: BarType
    sma_period: PositiveInt = 200
    allocation_pct: PositiveFloat = 0.80
    stop_loss_pct: PositiveFloat = 0.15
    take_profit_pct: PositiveFloat = 0.50
    close_positions_on_stop: bool = True


class TQQQSMA200Strategy(Strategy):
    """
    TQQQ + SPY SMA200 전략.

    SPY가 200일 SMA 위에 있으면 TQQQ 매수,
    아래로 내려가면 전량 청산.

    Parameters
    ----------
    config : TQQQSMA200Config
        The configuration for the instance.
    """

    def __init__(self, config: TQQQSMA200Config) -> None:
        super().__init__(config)

        self.spy_instrument: Instrument = None
        self.tqqq_instrument: Instrument = None

        # SMA indicator for SPY
        self.sma = SimpleMovingAverage(config.sma_period)

        # State tracking
        self.last_spy_price: float = 0.0
        self.last_sma_value: float = 0.0
        self.above_sma: bool | None = None  # None = unknown state
        self.entry_price: float | None = None

    def on_start(self) -> None:
        """Actions to be performed on strategy start."""
        # Load instruments
        self.spy_instrument = self.cache.instrument(self.config.spy_instrument_id)
        self.tqqq_instrument = self.cache.instrument(self.config.tqqq_instrument_id)

        if self.spy_instrument is None:
            self.log.error(f"Could not find SPY instrument: {self.config.spy_instrument_id}")
            self.stop()
            return

        if self.tqqq_instrument is None:
            self.log.error(f"Could not find TQQQ instrument: {self.config.tqqq_instrument_id}")
            self.stop()
            return

        # Register SMA indicator for SPY bars
        self.register_indicator_for_bars(self.config.spy_bar_type, self.sma)

        # Request historical bars for SMA warmup (need 200+ days)
        self.request_bars(
            self.config.spy_bar_type,
            start=self._clock.utc_now() - pd.Timedelta(days=250),
        )

        # Subscribe to live bars
        self.subscribe_bars(self.config.spy_bar_type)
        self.subscribe_bars(self.config.tqqq_bar_type)

        self.log.info(
            f"Strategy started: SPY SMA{self.config.sma_period} -> TQQQ",
            LogColor.GREEN,
        )
        self.log.info(
            f"Allocation: {self.config.allocation_pct * 100:.0f}%, "
            f"Stop Loss: {self.config.stop_loss_pct * 100:.0f}%, "
            f"Take Profit: {self.config.take_profit_pct * 100:.0f}%",
            LogColor.GREEN,
        )

    def on_bar(self, bar: Bar) -> None:
        """Actions to be performed when receiving a bar."""
        # Check if this is a SPY bar for signal generation
        if bar.bar_type == self.config.spy_bar_type:
            self._handle_spy_bar(bar)
        # Check if this is a TQQQ bar for risk management
        elif bar.bar_type == self.config.tqqq_bar_type:
            self._handle_tqqq_bar(bar)

    def _handle_spy_bar(self, bar: Bar) -> None:
        """Process SPY bar for SMA signal."""
        # Check if SMA is ready
        if not self.sma.initialized:
            self.log.info(
                f"Waiting for SMA warmup... "
                f"[{self.cache.bar_count(self.config.spy_bar_type)}/{self.config.sma_period}]",
                color=LogColor.BLUE,
            )
            return

        self.last_spy_price = float(bar.close)
        self.last_sma_value = self.sma.value

        # Calculate distance from SMA
        distance_pct = (self.last_spy_price - self.last_sma_value) / self.last_sma_value * 100
        was_above = self.above_sma
        self.above_sma = self.last_spy_price >= self.last_sma_value

        status = "🟢 SMA 위" if self.above_sma else "🔴 SMA 아래"
        self.log.info(
            f"SPY ${self.last_spy_price:.2f} | "
            f"SMA200 ${self.last_sma_value:.2f} ({distance_pct:+.2f}%) | {status}",
            LogColor.CYAN,
        )

        # Trading logic
        is_flat = self.portfolio.is_flat(self.config.tqqq_instrument_id)
        is_long = self.portfolio.is_net_long(self.config.tqqq_instrument_id)

        # BUY: SMA 위에서 포지션 없을 때
        if self.above_sma and is_flat:
            self.log.info("Signal: BUY TQQQ (SPY above SMA200)", LogColor.GREEN)
            self._buy_tqqq()

        # SELL: SMA 아래로 내려갔을 때
        elif not self.above_sma and is_long:
            self.log.info("Signal: SELL TQQQ (SPY below SMA200)", LogColor.RED)
            self._close_tqqq()

    def _handle_tqqq_bar(self, bar: Bar) -> None:
        """Process TQQQ bar for risk management."""
        if self.entry_price is None:
            return

        current_price = float(bar.close)
        pnl_pct = (current_price - self.entry_price) / self.entry_price

        # Stop Loss check
        if pnl_pct <= -self.config.stop_loss_pct:
            self.log.warning(
                f"STOP LOSS triggered! PnL: {pnl_pct * 100:.1f}%",
                LogColor.RED,
            )
            self._close_tqqq()

        # Take Profit check
        elif pnl_pct >= self.config.take_profit_pct:
            self.log.info(
                f"TAKE PROFIT triggered! PnL: {pnl_pct * 100:.1f}%",
                LogColor.GREEN,
            )
            self._close_tqqq()

    def _buy_tqqq(self) -> None:
        """Execute TQQQ buy order based on allocation."""
        account = self.portfolio.account(self.tqqq_instrument.id.venue)
        if account is None:
            self.log.error("No account found for TQQQ venue")
            return

        # Calculate position size based on allocation
        balance = float(account.balance_total())
        available = balance * self.config.allocation_pct

        # Get current TQQQ price from last quote or bar
        last_bar = self.cache.bar(self.config.tqqq_bar_type)
        if last_bar is None:
            self.log.error("No TQQQ price data available")
            return

        tqqq_price = float(last_bar.close)
        if tqqq_price <= 0:
            self.log.error("Invalid TQQQ price")
            return

        # Calculate quantity (use 95% to leave buffer for commissions)
        quantity = int((available * 0.95) / tqqq_price)
        if quantity <= 0:
            self.log.warning("Insufficient funds for TQQQ purchase")
            return

        self.log.info(
            f"Buying TQQQ: {quantity} shares @ ${tqqq_price:.2f} "
            f"(${quantity * tqqq_price:,.0f})",
            LogColor.GREEN,
        )

        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.tqqq_instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.tqqq_instrument.make_qty(Decimal(quantity)),
            time_in_force=TimeInForce.DAY,
        )

        self.submit_order(order)
        self.entry_price = tqqq_price

    def _close_tqqq(self) -> None:
        """Close all TQQQ positions."""
        self.close_all_positions(self.config.tqqq_instrument_id)
        self.entry_price = None

    def on_event(self, event: Event) -> None:
        """Handle position events for tracking entry price."""
        if isinstance(event, PositionOpened):
            if event.instrument_id == self.config.tqqq_instrument_id:
                self.entry_price = float(event.avg_px_open)
                self.log.info(
                    f"Position opened at ${self.entry_price:.2f}",
                    LogColor.GREEN,
                )

    def on_stop(self) -> None:
        """Actions to be performed when the strategy is stopped."""
        self.cancel_all_orders(self.config.tqqq_instrument_id)
        if self.config.close_positions_on_stop:
            self.close_all_positions(self.config.tqqq_instrument_id)

        self.unsubscribe_bars(self.config.spy_bar_type)
        self.unsubscribe_bars(self.config.tqqq_bar_type)

    def on_reset(self) -> None:
        """Actions to be performed when the strategy is reset."""
        self.sma.reset()
        self.last_spy_price = 0.0
        self.last_sma_value = 0.0
        self.above_sma = None
        self.entry_price = None

    def on_dispose(self) -> None:
        """Cleanup resources."""
        pass
