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

import pandas as pd

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.core.message import Event
from nautilus_trader.core.rust.common import LogColor
from nautilus_trader.core.rust.model import TimeInForce
from nautilus_trader.core.rust.model import TriggerType
from nautilus_trader.indicators.macd import MovingAverageConvergenceDivergence
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Position
from nautilus_trader.model import TradeTick
from nautilus_trader.model import Venue
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import PositionChanged
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.strategy import StrategyConfig


_BINANCE_VENUE = Venue("BINANCE")


class MACDStrategyConfig(StrategyConfig):
    instrument_id: InstrumentId
    fast_period: int = 12
    slow_period: int = 26
    trade_size: float = 0.05
    entry_threshold: float = 0.00010


class MACDStrategy(Strategy):
    def __init__(self, config: MACDStrategyConfig) -> None:
        super().__init__(config=config)
        self.macd = MovingAverageConvergenceDivergence(
            fast_period=config.fast_period,
            slow_period=config.slow_period,
            price_type=PriceType.MID,
        )

        self._position: Position | None = None
        self._closing = False

        self.events: list[Event] = []

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.config.instrument_id)
        self.account = self.portfolio.account(self.instrument.venue)
        self.trade_size = self.instrument.make_qty(self.config.trade_size)

        self.subscribe_trade_ticks(instrument_id=self.config.instrument_id)

        self.msgbus.subscribe(f"events.account.{self.account.id}", self.events.append)
        self.msgbus.subscribe(f"events.order.{self.id}", self.events.append)
        self.msgbus.subscribe(f"events.position.{self.id}", self.events.append)

    def on_stop(self) -> None:
        self.cancel_all_orders(self.config.instrument_id)
        self.close_all_positions(self.config.instrument_id)
        self.unsubscribe_trade_ticks(instrument_id=self.config.instrument_id)

    def on_order_accepted(self, event: OrderAccepted) -> None:
        if self._limit_order is not None:
            if self._limit_order.client_order_id == event.client_order_id:
                if self.account.is_margin_account:
                    self.log.info(
                        f"After limit order accepted with qty {self._limit_order.quantity} balances locked: "
                        + f"{self.account.balances_locked()[self.account.base_currency].as_double()}",
                        LogColor.MAGENTA,
                    )
                    return

        if self.account.is_margin_account:
            self.log.info(
                "After unidentified order accepted balances locked: "
                + f"{self.account.balances_locked()[self.account.base_currency].as_double()}",
                LogColor.MAGENTA,
            )

    def on_order_filled(self, event: OrderFilled) -> None:
        if self.account.is_margin_account:
            self.log.info(
                f"After filled qty {event.last_qty} balances locked: "
                + f"{self.account.balances_locked()[self.account.base_currency].as_double()}",
                LogColor.CYAN,
            )

    def on_trade_tick(self, tick: TradeTick) -> None:
        # You can register indicators to receive quote tick updates automatically,
        # here we manually update the indicator
        self.macd.handle_trade_tick(tick)

        if not self.macd.initialized:
            return  # Wait for indicator to warm up

        # self._log.info(f"{self.macd.value=}:%5d")
        self.check_for_entry(tick)
        self.check_for_exit()

    def check_for_entry(self, tick: TradeTick) -> None:
        if self._closing:
            return

        quantity = self.instrument.make_qty(
            self.config.trade_size,
        )

        # If MACD line is above our entry threshold, we should be LONG
        if self.macd.value > self.config.entry_threshold:
            if self._position and self._position.side == PositionSide.LONG:
                return  # Already LONG

            order = self.order_factory.market(
                instrument_id=self.config.instrument_id,
                order_side=OrderSide.BUY,
                quantity=quantity,
            )
            self.position_open_price = tick.price
            self.submit_order(order)

        # If MACD line is below our entry threshold, we should be SHORT
        elif self.macd.value < -self.config.entry_threshold:
            if self._position and self._position.side == PositionSide.SHORT:
                return  # Already SHORT

            order = self.order_factory.market(
                instrument_id=self.config.instrument_id,
                order_side=OrderSide.SELL,
                quantity=quantity,
            )
            self.position_open_price = tick.price
            self.submit_order(order)

    def check_for_exit(self) -> None:
        if not self._position:
            return

        exit_now = (self._position.side == PositionSide.SHORT and self.macd.value >= 0.0) or (
            self._position.side == PositionSide.LONG and self.macd.value < 0.0
        )

        if exit_now and not self._closing:
            self._closing = True
            self.cancel_all_orders(self.config.instrument_id)

            order = self.order_factory.market(
                instrument_id=self.config.instrument_id,
                order_side=self._position.closing_order_side(),
                quantity=self._position.quantity,
            )
            self.submit_order(order)

    def on_position_opened(self, event: PositionOpened) -> None:
        self._position = self.cache.position(event.position_id)
        assert self._position is not None  # Type checking

        if self._position.side == PositionSide.LONG:
            order_side = OrderSide.BUY
            limit_price = self.instrument.make_price(
                self.position_open_price * (1 - 0.001),
            )

        else:
            order_side = OrderSide.SELL
            limit_price = self.instrument.make_price(
                self.position_open_price * (1 + 0.001),
            )

        quantity = self.instrument.make_qty(self.config.trade_size)

        self._limit_order = self.order_factory.limit(
            self.config.instrument_id,
            order_side,
            quantity,
            limit_price,
            reduce_only=False,
            post_only=False,
            time_in_force=TimeInForce.GTC,
            expire_time=None,
            quote_quantity=False,
            emulation_trigger=TriggerType.NO_TRIGGER,
            trigger_instrument_id=None,
            exec_algorithm_id=None,
            exec_algorithm_params=None,
            display_qty=None,
        )
        self.submit_order(self._limit_order)

    def on_position_changed(self, event: PositionChanged) -> None:
        assert self._position is not None  # Type checking

        if self.account.is_margin_account:
            self.log.info(
                f"After position changed to amount {self._position.quantity} balances locked: "
                + f"{self.account.balances_locked()[self.account.base_currency].as_double()}",
                LogColor.CYAN,
            )

    def on_position_closed(self, event: PositionClosed) -> None:
        self._position = None
        self._closing = False

    def on_dispose(self) -> None:
        pass  # Do nothing else


def create_engine() -> BacktestEngine:
    config = BacktestEngineConfig(
        logging=LoggingConfig(bypass_logging=True),
        run_analysis=False,
    )
    return BacktestEngine(config=config)


def test_cash_account_trades_macd_event_sequencing() -> None:
    # Arrange
    engine = create_engine()

    # Add venue
    engine.add_venue(
        venue=_BINANCE_VENUE,
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        starting_balances=[
            Money(10, ETH),
            Money(100_000, USDT),
        ],
    )

    # Add data
    ethusdt = TestInstrumentProvider.ethusdt_binance()
    engine.add_instrument(ethusdt)

    provider = TestDataProvider()
    wrangler = TradeTickDataWrangler(instrument=ethusdt)
    trades = wrangler.process(provider.read_csv_ticks("binance/ethusdt-trades.csv"))
    engine.add_data(trades)

    # Add strategy
    config = MACDStrategyConfig(instrument_id=ethusdt.id)
    strategy = MACDStrategy(config=config)
    engine.add_strategy(strategy)

    # Act
    engine.run(end=pd.Timestamp("2020-08-14 10:10:00", tz="UTC"))

    # Assert
    assert engine.iteration == 2_123
    assert engine.cache.orders_open_count() == 0
    assert engine.cache.orders_closed_count() == 138
    assert engine.cache.orders_total_count() == 138
    assert engine.cache.positions_open_count() == 0
    assert engine.cache.positions_closed_count() == 1  # Netting
    assert engine.cache.positions_total_count() == 1  # Netting

    assert len(strategy.events) == 769

    # -- First entry sequence
    assert isinstance(strategy.events[0], OrderInitialized)
    assert isinstance(strategy.events[1], OrderSubmitted)
    assert isinstance(strategy.events[2], AccountState)
    assert isinstance(strategy.events[3], OrderFilled)
    assert isinstance(strategy.events[4], OrderInitialized)  # Follow-up order
    assert isinstance(strategy.events[5], OrderSubmitted)
    assert isinstance(strategy.events[6], PositionOpened)
    assert isinstance(strategy.events[7], AccountState)
    assert isinstance(strategy.events[8], OrderAccepted)

    # -- Closing sequence
    assert isinstance(strategy.events[9], OrderInitialized)
    assert isinstance(strategy.events[10], OrderSubmitted)
    assert isinstance(strategy.events[11], AccountState)
    assert isinstance(strategy.events[12], OrderCanceled)
    assert isinstance(strategy.events[13], AccountState)
    assert isinstance(strategy.events[14], OrderFilled)
    assert isinstance(strategy.events[15], PositionClosed)

    # -- Second entry sequence
    assert isinstance(strategy.events[16], OrderInitialized)
    assert isinstance(strategy.events[17], OrderSubmitted)
    assert isinstance(strategy.events[18], AccountState)
    assert isinstance(strategy.events[19], OrderFilled)
    assert isinstance(strategy.events[20], OrderInitialized)  # Follow-up order
    assert isinstance(strategy.events[21], OrderSubmitted)
    assert isinstance(strategy.events[22], PositionOpened)
    assert isinstance(strategy.events[23], AccountState)
    assert isinstance(strategy.events[24], OrderAccepted)
