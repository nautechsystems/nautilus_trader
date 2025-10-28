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
from typing import Any

from nautilus_trader.common.actor import Actor
from nautilus_trader.config import ActorConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.instruments import Instrument


class MockActorConfig(ActorConfig, frozen=True):
    """
    Provides a mock actor config for testing.
    """

    component_id: str = "ACTOR-001"


class MockActor(Actor):
    """
    Provides a mock actor for testing.
    """

    def __init__(self, config: ActorConfig | None = None):
        super().__init__(config)

        self.store: list[object] = []

        self.calls: list[str] = []
        self._user_state: dict[str, Any] = {}

    def on_save(self) -> dict:
        self._user_state["A"] = 1
        return self._user_state

    def on_start(self) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)

    def on_stop(self) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)

    def on_resume(self) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)

    def on_reset(self) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)

    def on_dispose(self) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)

    def on_degrade(self) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)

    def on_fault(self) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)

    def on_instrument(self, instrument: Instrument) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)
        self.store.append(instrument)

    def on_instruments(self, instruments: list[Instrument]) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)
        self.store.append(instruments)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)
        self.store.append(tick)

    def on_trade_tick(self, tick: TradeTick) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)
        self.store.append(tick)

    def on_bar(self, bar: Bar) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)
        self.store.append(bar)

    def on_data(self, data: Data) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)
        self.store.append(data)

    def on_signal(self, signal: Data) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)
        self.store.append(signal)

    def on_strategy_data(self, data: Data) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)
        self.store.append(data)

    def on_event(self, event: Event) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)
        self.store.append(event)

    def on_order_filled(self, event: OrderFilled) -> None:
        current_frame = inspect.currentframe()
        if current_frame:
            self.calls.append(current_frame.f_code.co_name)
        self.store.append(event)


class KaboomActor(Actor):
    """
    Provides a mock actor where every called method blows up.
    """

    def __init__(self):
        super().__init__()

        self._explode_on_start = True
        self._explode_on_stop = True

    def set_explode_on_start(self, setting: bool) -> None:
        self._explode_on_start = setting

    def set_explode_on_stop(self, setting: bool) -> None:
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

    def on_dispose(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_degrade(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_fault(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_instrument(self, instrument: Instrument) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_quote_tick(self, tick: QuoteTick) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_trade_tick(self, tick: TradeTick) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_bar(self, bar: Bar) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_data(self, data: Data) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_signal(self, signal: Data) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_event(self, event: Event) -> None:
        raise RuntimeError(f"{self} BOOM!")
