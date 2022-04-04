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

from typing import Any, Dict, FrozenSet, Optional

import pydantic
from frozendict import frozendict
from pydantic import PositiveInt
from pydantic import validator

from nautilus_trader.config.common import resolve_path
from nautilus_trader.core.correctness import PyCondition


class CacheConfig(pydantic.BaseModel):
    """
    Configuration for ``Cache`` instances.

    Parameters
    ----------
    tick_capacity : int
        The maximum length for internal tick deques.
    bar_capacity : int
        The maximum length for internal bar deques.
    """

    tick_capacity: PositiveInt = 1000
    bar_capacity: PositiveInt = 1000


class CacheDatabaseConfig(pydantic.BaseModel):
    """
    Configuration for ``CacheDatabase`` instances.

    Parameters
    ----------
    type : str, {'in-memory', 'redis'}, default 'in-memory'
        The database type.
    host : str, default 'localhost'
        The database host address (default for Redis).
    port : int, default 6379
        The database port (default for Redis).
    flush : bool, default False
        If database should be flushed before start.
    """

    type: str = "in-memory"
    host: str = "localhost"
    port: int = 6379
    flush: bool = False


class ActorConfig(pydantic.BaseModel):
    """
    The base model for all actor configurations.

    Parameters
    ----------
    component_id : str, optional
        The component ID. If ``None`` then the identifier will be taken from
        `type(self).__name__`.

    """

    component_id: Optional[str] = None


class ImportableActorConfig(pydantic.BaseModel):
    """
    Represents an actor configuration for one specific backtest run.

    Parameters
    ----------
    actor_path : str
        The fully qualified name of the Actor class.
    config_path : str
        The fully qualified name of the Actor Config class.
    config : Dict
        The actor configuration
    """

    actor_path: str
    config_path: str
    config: dict


class ActorFactory:
    """
    Provides actor creation from importable configurations.
    """

    @staticmethod
    def create(config: ImportableActorConfig):
        """
        Create an actor from the given configuration.

        Parameters
        ----------
        config : ImportableActorConfig
            The configuration for the building step.

        Returns
        -------
        Actor

        Raises
        ------
        TypeError
            If `config` is not of type `ImportableActorConfig`.

        """
        PyCondition.type(config, ImportableActorConfig, "config")
        strategy_cls = resolve_path(config.actor_path)
        config_cls = resolve_path(config.config_path)
        return strategy_cls(config=config_cls(**config.config))


class TradingStrategyConfig(pydantic.BaseModel):
    """
    The base model for all trading strategy configurations.

    Parameters
    ----------
    strategy_id : str, optional
        The unique ID for the strategy. Will become the strategy ID if not None.
    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    oms_type : OMSType, optional
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs (see docs).

    """

    strategy_id: Optional[str] = None
    order_id_tag: str = "000"
    oms_type: Optional[str] = None


class ImportableStrategyConfig(pydantic.BaseModel):
    """
    Represents a trading strategy configuration for one specific backtest run.

    Parameters
    ----------
    strategy_path : str
        The fully qualified name of the strategy class.
    config_path : str
        The fully qualified name of the config class.
    config : Dict[str, Any]
        The strategy configuration
    """

    strategy_path: str
    config_path: str
    config: Dict[str, Any]


class StrategyFactory:
    """
    Provides strategy creation from importable configurations.
    """

    @staticmethod
    def create(config: ImportableStrategyConfig):
        """
        Create a trading strategy from the given configuration.

        Parameters
        ----------
        config : ImportableStrategyConfig
            The configuration for the building step.

        Returns
        -------
        TradingStrategy

        Raises
        ------
        TypeError
            If `config` is not of type `ImportableStrategyConfig`.

        """
        PyCondition.type(config, ImportableStrategyConfig, "config")
        strategy_cls = resolve_path(config.strategy_path)
        config_cls = resolve_path(config.config_path)
        return strategy_cls(config=config_cls(**config.config))


class InstrumentProviderConfig(pydantic.BaseModel):
    """
    Configuration for ``InstrumentProvider`` instances.

    Parameters
    ----------
    load_all : bool, default False
        If all venue instruments should be loaded on start.
    load_ids : FrozenSet[str], optional
        The list of instrument IDs to be loaded on start (if `load_all_instruments` is False).
    filters : frozendict, optional
        The venue specific instrument loading filters to apply.
    """

    class Config:
        """The base model config"""

        arbitrary_types_allowed = True

    @validator("filters")
    def validate_filters(cls, value):
        return frozendict(value) if value is not None else None

    def __eq__(self, other):
        return (
            self.load_all == other.load_all
            and self.load_ids == other.load_ids
            and self.filters == other.filters
        )

    def __hash__(self):
        return hash((self.load_all, self.load_ids, self.filters))

    load_all: bool = False
    load_ids: Optional[FrozenSet[str]] = None
    filters: Optional[Dict[str, Any]] = None
