# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.config import ActorConfig
from nautilus_trader.examples.strategies.signal_strategy import SignalStrategy
from nautilus_trader.examples.strategies.signal_strategy import SignalStrategyConfig
from nautilus_trader.trading.controller import Controller


class ControllerConfig(ActorConfig, frozen=True):
    pass


class MyController(Controller):
    def start(self):
        """
        Dynamically add a new strategy after startup.
        """
        instruments = self.cache.instruments()
        strategy_config = SignalStrategyConfig(instrument_id=instruments[0].id.value)
        strategy = SignalStrategy(strategy_config)
        self.create_strategy(strategy)
