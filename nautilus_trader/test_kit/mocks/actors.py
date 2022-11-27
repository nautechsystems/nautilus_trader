# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.actor import Actor
from nautilus_trader.config import ActorConfig
from nautilus_trader.test_kit.mocks.object_storer import ObjectStorer


class MockActorConfig(ActorConfig):
    """
    Provides a mock actor config for testing.
    """

    component_id: str = "ACTOR-001"


class MockActor(Actor):
    """
    Provides a mock actor for testing.
    """

    def __init__(self, config: ActorConfig = None):
        super().__init__(config)

        self.object_storer = ObjectStorer()

        self.calls: list[str] = []

    def on_start(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_stop(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_resume(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_reset(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_dispose(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_degrade(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_fault(self) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)

    def on_instrument(self, instrument) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(instrument)

    def on_instruments(self, instruments) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(instruments)

    def on_ticker(self, ticker):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(ticker)

    def on_quote_tick(self, tick):
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(tick)

    def on_trade_tick(self, tick) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(tick)

    def on_bar(self, bar) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(bar)

    def on_data(self, data) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(data)

    def on_strategy_data(self, data) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(data)

    def on_event(self, event) -> None:
        self.calls.append(inspect.currentframe().f_code.co_name)
        self.object_storer.store(event)


class KaboomActor(Actor):
    """
    Provides a mock actor where every called method blows up.
    """

    def __init__(self):
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

    def on_dispose(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_degrade(self) -> None:
        raise RuntimeError(f"{self} BOOM!")

    def on_fault(self) -> None:
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

    def on_event(self, event) -> None:
        raise RuntimeError(f"{self} BOOM!")
