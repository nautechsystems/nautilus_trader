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

from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.core.message import Command
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.trading.config import ImportableStrategyConfig


class CreateActor(Command):
    """
    Represents a command to create an actor.

    Parameters
    ----------
    actor_config : ImportableActorConfig
        The configuration for the actor.
    start: bool, optional
        If True, start the actor after creation, by default True.
    command_id : UUID4
        The command ID.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        actor_config: ImportableActorConfig,
        start: bool = True,
        command_id: UUID4 | None = None,
        ts_init: int = 0,
    ) -> None:

        super().__init__(command_id or UUID4(), ts_init)

        self.actor_config = actor_config
        self.start = start


class CreateStrategy(Command):
    """
    Represents a command to create a strategy.

    Parameters
    ----------
    strategy_config : ImportableStrategyConfig
        The configuration for the strategy.
    start: bool, optional
        If True, start the strategy after creation, by default True.
    command_id : UUID4
        The command ID.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        strategy_config: ImportableStrategyConfig,
        start: bool = True,
        command_id: UUID4 | None = None,
        ts_init: int = 0,
    ) -> None:

        super().__init__(command_id or UUID4(), ts_init)

        self.strategy_config = strategy_config
        self.start = start


class StartActor(Command):
    """
    Represents a command to start an actor.

    Parameters
    ----------
    actor_id : ComponentId
        The ID of the actor to start.
    command_id : UUID4
        The command ID.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        actor_id: ComponentId,
        command_id: UUID4 | None = None,
        ts_init: int = 0,
    ) -> None:
        super().__init__(command_id or UUID4(), ts_init)

        self.actor_id = actor_id


class StartStrategy(Command):
    """
    Represents a command to start a strategy.

    Parameters
    ----------
    strategy_id : StrategyId
        The ID of the strategy to start.
    command_id : UUID4
        The command ID.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        strategy_id: StrategyId,
        command_id: UUID4 | None = None,
        ts_init: int = 0,
    ) -> None:
        super().__init__(command_id or UUID4(), ts_init)

        self.strategy_id = strategy_id


class StopActor(Command):
    """
    Represents a command to strop an actor.

    Parameters
    ----------
    actor_id : ComponentId
        The ID of the actor to start.
    command_id : UUID4
        The command ID.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        actor_id: ComponentId,
        command_id: UUID4 | None = None,
        ts_init: int = 0,
    ) -> None:
        super().__init__(command_id or UUID4(), ts_init)

        self.actor_id = actor_id


class StopStrategy(Command):
    """
    Represents a command to stop a strategy.

    Parameters
    ----------
    strategy_id : StrategyId
        The ID of the strategy to start.
    command_id : UUID4
        The command ID.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        strategy_id: StrategyId,
        command_id: UUID4 | None = None,
        ts_init: int = 0,
    ) -> None:
        super().__init__(command_id or UUID4(), ts_init)

        self.strategy_id = strategy_id


class RemoveActor(Command):
    """
    Represents a command to remove an actor.

    Parameters
    ----------
    actor_id : ComponentId
        The ID of the actor to start.
    command_id : UUID4
        The command ID.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        actor_id: ComponentId,
        command_id: UUID4 | None = None,
        ts_init: int = 0,
    ) -> None:
        super().__init__(command_id or UUID4(), ts_init)

        self.actor_id = actor_id


class RemoveStrategy(Command):
    """
    Represents a command to remove a strategy.

    Parameters
    ----------
    strategy_id : StrategyId
        The ID of the strategy to start.
    command_id : UUID4
        The command ID.
    ts_init : int
        UNIX timestamp (nanoseconds) when the object was initialized.

    """

    def __init__(
        self,
        strategy_id: StrategyId,
        command_id: UUID4 | None = None,
        ts_init: int = 0,
    ) -> None:
        super().__init__(command_id or UUID4(), ts_init)

        self.strategy_id = strategy_id
