# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.actor import Actor
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.core.correctness import PyCondition
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
        The configuration for the controller

    Raises
    ------
    TypeError
        If `config` is not of type `ActorConfig`.

    """

    def __init__(
        self,
        trader: Trader,
        config: ActorConfig | None = None,
    ) -> None:
        if config is None:
            config = ActorConfig()
        PyCondition.type(config, ActorConfig, "config")
        super().__init__(config=config)

        self._trader = trader

    def create_actor(self, actor: Actor, start: bool = True) -> None:
        """
        Add the given actor to the controlled trader.

        Parameters
        ----------
        actor : Actor
            The actor to add.
        start : bool, default True
            If the actor should be started immediately.

        Raises
        ------
        ValueError
            If `actor.state` is ``RUNNING`` or ``DISPOSED``.
        RuntimeError
            If `actor` is already registered with the trader.

        """
        self._trader.add_actor(actor)
        if start:
            actor.start()

    def create_strategy(self, strategy: Strategy, start: bool = True) -> None:
        """
        Add the given strategy to the controlled trader.

        Parameters
        ----------
        strategy : Strategy
            The strategy to add.
        start : bool, default True
            If the strategy should be started immediately.

        Raises
        ------
        ValueError
            If `strategy.state` is ``RUNNING`` or ``DISPOSED``.
        RuntimeError
            If `strategy` is already registered with the trader.

        """
        self._trader.add_strategy(strategy)
        if start:
            strategy.start()

    def start_actor(self, actor: Actor) -> None:
        """
        Start the given `actor`.

        Will log a warning if the actor is already ``RUNNING``.

        Raises
        ------
        ValueError
            If `actor` is not already registered with the trader.

        """
        self._trader.start_actor(actor.id)

    def start_strategy(self, strategy: Strategy) -> None:
        """
        Start the given `strategy`.

        Will log a warning if the strategy is already ``RUNNING``.

        Raises
        ------
        ValueError
            If `strategy` is not already registered with the trader.

        """
        self._trader.start_strategy(strategy.id)

    def stop_actor(self, actor: Actor) -> None:
        """
        Stop the given `actor`.

        Will log a warning if the actor is not ``RUNNING``.

        Parameters
        ----------
        actor : Actor
            The actor to stop.

        Raises
        ------
        ValueError
            If `actor` is not already registered with the trader.

        """
        self._trader.stop_actor(actor.id)

    def stop_strategy(self, strategy: Strategy) -> None:
        """
        Stop the given `strategy`.

        Will log a warning if the strategy is not ``RUNNING``.

        Parameters
        ----------
        strategy : Strategy
            The strategy to stop.

        Raises
        ------
        ValueError
            If `strategy` is not already registered with the trader.

        """
        self._trader.stop_strategy(strategy.id)

    def remove_actor(self, actor: Actor) -> None:
        """
        Remove the given `actor`.

        Will stop the actor first if state is ``RUNNING``.

        Parameters
        ----------
        actor : Actor
            The actor to remove.

        Raises
        ------
        ValueError
            If `actor` is not already registered with the trader.

        """
        self._trader.remove_actor(actor.id)

    def remove_strategy(self, strategy: Strategy) -> None:
        """
        Remove the given `strategy`.

        Will stop the strategy first if state is ``RUNNING``.

        Parameters
        ----------
        strategy : Strategy
            The strategy to remove.

        Raises
        ------
        ValueError
            If `strategy` is not already registered with the trader.

        """
        self._trader.remove_strategy(strategy.id)
