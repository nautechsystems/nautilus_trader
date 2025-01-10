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

import inspect

from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.trading.strategy import Strategy


class MockStrategy(Strategy):
    """
    Provides a mock trading strategy for testing.

    Parameters
    ----------
    bar_type : BarType
        The bar type for the strategy.

    """

    def __init__(self, bar_type: BarType) -> None:
        super().__init__()

        self.store: list[object] = []
        self.bar_type = bar_type

        self.ema1 = ExponentialMovingAverage(10)
        self.ema2 = ExponentialMovingAverage(20)

        self.position_id: PositionId | None = None

        self.calls: list[str] = []

    def on_start(self) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        self.register_indicator_for_bars(self.bar_type, self.ema1)
        self.register_indicator_for_bars(self.bar_type, self.ema2)

    def on_instrument(self, instrument) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        self.store.append(instrument)

    def on_ticker(self, ticker):
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        self.store.append(ticker)

    def on_quote_tick(self, tick):
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        self.store.append(tick)

    def on_trade_tick(self, tick) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        self.store.append(tick)

    def on_bar(self, bar) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        self.store.append(bar)

        if bar.bar_type != self.bar_type:
            return

        if self.ema1.value > self.ema2.value:
            buy_order = self.order_factory.market(
                self.bar_type.instrument_id,
                OrderSide.BUY,
                100000,
            )

            self.submit_order(buy_order)
            self.position_id = buy_order.client_order_id
        elif self.ema1.value < self.ema2.value:
            sell_order = self.order_factory.market(
                self.bar_type.instrument_id,
                OrderSide.SELL,
                100000,
            )

            self.submit_order(sell_order)
            self.position_id = sell_order.client_order_id

    def on_data(self, data) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        self.store.append(data)

    def on_signal(self, signal) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        self.store.append(signal)

    def on_strategy_data(self, data) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        self.store.append(data)

    def on_event(self, event) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        self.store.append(event)

    def on_stop(self) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)

    def on_resume(self) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)

    def on_reset(self) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)

    def on_save(self) -> dict[str, bytes]:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        return {"UserState": b"1"}

    def on_load(self, state: dict[str, bytes]) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)
        self.store.append(state)

    def on_dispose(self) -> None:
        current_frame = inspect.currentframe()
        assert current_frame  # Type checking
        self.calls.append(current_frame.f_code.co_name)


class KaboomStrategy(Strategy):
    """
    Provides a mock trading strategy where every called method blows up.
    """

    def __init__(self) -> None:
        super().__init__()

        self._explode_on_start = True
        self._explode_on_stop = True

    def set_explode_on_start(self, setting) -> None:
        self._explode_on_start = setting

    def set_explode_on_stop(self, setting) -> None:
        self._explode_on_stop = setting

    def on_start(self) -> None:
        if self._explode_on_start:
            raise RuntimeError(f"{self} BOOM!")

    def on_stop(self) -> None:
        if self._explode_on_stop:
            raise RuntimeError(f"{self} BOOM!")

    def on_resume(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_reset(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_save(self) -> dict[str, bytes]:
        raise RuntimeError(f"{self} BOOM!")

    def on_load(self, state: dict[str, bytes]) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_dispose(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_instrument(self, instrument) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_quote_tick(self, tick) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_trade_tick(self, tick) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_bar(self, bar) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_data(self, data) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_signal(self, data) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_event(self, event) -> None:
        raise RuntimeError(f"{self} BOOM!")
