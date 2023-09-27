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

from __future__ import annotations

from nautilus_trader.common.actor import Actor
from nautilus_trader.config.common import ActorConfig
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.trader import Trader


class Controller(Actor):
    """
    The base class for all trader controllers.

    Parameters
    ----------
    trader : Trader
        The reference to the trader instance to control.
    config : ActorConfig, optional
        The configuratuon for the controller

    """

    def __init__(
        self,
        trader: Trader,
        config: ActorConfig | None = None,
    ) -> None:
        if config is None:
            config = ActorConfig()
        super().__init__(config=config)

        self.trader = trader

    def create_actor(self, actor: Actor) -> None:
        self.trader.add_actor(actor)
        actor.start()

    def create_strategy(self, strategy: Strategy) -> None:
        self.trader.add_strategy(strategy)
        strategy.start()
