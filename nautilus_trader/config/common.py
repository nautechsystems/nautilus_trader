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

import importlib
import importlib.util
from typing import Any, Dict, FrozenSet, List, Optional

import fsspec
import pydantic
from frozendict import frozendict
from pydantic import ConstrainedStr
from pydantic import Field
from pydantic import PositiveInt
from pydantic import validator

from nautilus_trader.common import Environment
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog


def resolve_path(path: str):
    module, cls = path.rsplit(":", maxsplit=1)
    mod = importlib.import_module(module)
    cls = getattr(mod, cls)
    return cls


class NautilusConfig(pydantic.BaseModel):
    """
    The base class for all Nautilus configuration objects.
    """

    @classmethod
    def fully_qualified_name(cls) -> str:
        """
        Return the fully qualified name for the `NautilusConfig` class.

        Returns
        -------
        str

        References
        ----------
        https://www.python.org/dev/peps/pep-3155/

        """
        return cls.__module__ + ":" + cls.__qualname__


class CacheConfig(NautilusConfig):
    """
    Configuration for ``Cache`` instances.

    Parameters
    ----------
    tick_capacity : int
        The maximum length for internal tick dequeues.
    bar_capacity : int
        The maximum length for internal bar dequeues.
    """

    tick_capacity: PositiveInt = 1000
    bar_capacity: PositiveInt = 1000


class CacheDatabaseConfig(NautilusConfig):
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


class InstrumentProviderConfig(NautilusConfig):
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
    log_warnings : bool, default True
        If parser warnings should be logged.
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
    log_warnings: bool = True


class DataEngineConfig(NautilusConfig):
    """
    Configuration for ``DataEngine`` instances.

    Parameters
    ----------
    debug : bool
        If debug mode is active (will provide extra debug logging).
    """

    debug: bool = False


class RiskEngineConfig(NautilusConfig):
    """
    Configuration for ``RiskEngine`` instances.

    Parameters
    ----------
    bypass : bool
        If True then all risk checks are bypassed (will still check for duplicate IDs).
    max_order_rate : str, default 100/00:00:01
        The maximum order rate per timedelta.
    max_notional_per_order : Dict[str, str]
        The maximum notional value of an order per instrument ID.
        The value should be a valid decimal format.
    debug : bool
        If debug mode is active (will provide extra debug logging).
    """

    bypass: bool = False
    max_order_rate: ConstrainedStr = ConstrainedStr("100/00:00:01")
    max_notional_per_order: Dict[str, str] = {}
    debug: bool = False


class ExecEngineConfig(NautilusConfig):
    """
    Configuration for ``ExecutionEngine`` instances.

    Parameters
    ----------
    load_cache : bool, default True
        If the cache should be loaded on initialization.
    allow_cash_positions : bool, default True
        If unleveraged spot cash assets should track positions.
    debug : bool
        If debug mode is active (will provide extra debug logging).
    """

    load_cache: bool = True
    allow_cash_positions: bool = True
    debug: bool = False


class StreamingConfig(NautilusConfig):
    """
    Configuration for streaming live or backtest runs to the catalog in feather format.

    Parameters
    ----------
    catalog_path : str
        The path to the data catalog.
    fs_protocol : str, optional
        The `fsspec` filesystem protocol for the catalog.
    fs_storage_options : Dict, optional
        The `fsspec` storage options.
    flush_interval_ms : int, optional
        The flush interval (milliseconds) for writing chunks.
    replace_existing: bool, default False
        If any existing feather files should be replaced.
    """

    catalog_path: str
    fs_protocol: Optional[str] = None
    fs_storage_options: Optional[Dict] = None
    flush_interval_ms: Optional[int] = None
    replace_existing: bool = False
    include_types: Optional[List[str]] = None

    @property
    def fs(self):
        return fsspec.filesystem(protocol=self.fs_protocol, **(self.fs_storage_options or {}))

    @classmethod
    def from_catalog(cls, catalog: ParquetDataCatalog, **kwargs):
        return cls(catalog_path=str(catalog.path), fs_protocol=catalog.fs.protocol, **kwargs)

    def as_catalog(self) -> ParquetDataCatalog:
        return ParquetDataCatalog(
            path=self.catalog_path,
            fs_protocol=self.fs_protocol,
            fs_storage_options=self.fs_storage_options,
        )


class ActorConfig(NautilusConfig):
    """
    The base model for all actor configurations.

    Parameters
    ----------
    component_id : str, optional
        The component ID. If ``None`` then the identifier will be taken from
        `type(self).__name__`.

    """

    component_id: Optional[str] = None


class ImportableActorConfig(NautilusConfig):
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


class StrategyConfig(NautilusConfig):
    """
    The base model for all trading strategy configurations.

    Parameters
    ----------
    strategy_id : str, optional
        The unique ID for the strategy. Will become the strategy ID if not None.
    order_id_tag : str, optional
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    oms_type : OMSType, optional
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs (see docs).

    """

    strategy_id: Optional[str] = None
    order_id_tag: Optional[str] = None
    oms_type: Optional[str] = None


class ImportableStrategyConfig(NautilusConfig):
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
        Strategy

        Raises
        ------
        TypeError
            If `config` is not of type `ImportableStrategyConfig`.

        """
        PyCondition.type(config, ImportableStrategyConfig, "config")
        strategy_cls = resolve_path(config.strategy_path)
        config_cls = resolve_path(config.config_path)
        return strategy_cls(config=config_cls(**config.config))


class NautilusKernelConfig(NautilusConfig):
    """
    Configuration for core system ``NautilusKernel`` instances.

    Parameters
    ----------
    environment : Environment { ``BACKTEST``, ``SANDBOX``, ``LIVE`` }
        The kernel environment context.
    trader_id : str
        The trader ID for the kernel (must be a name and ID tag separated by a hyphen).
    cache : CacheConfig, optional
        The cache configuration.
    cache_database : CacheDatabaseConfig, optional
        The cache database configuration.
    data_engine : DataEngineConfig, optional
        The live data engine configuration.
    risk_engine : RiskEngineConfig, optional
        The live risk engine configuration.
    exec_engine : ExecEngineConfig, optional
        The live execution engine configuration.
    streaming : StreamingConfig, optional
        The configuration for streaming to feather files.
    actors : List[ImportableActorConfig]
        The actor configurations for the kernel.
    strategies : List[ImportableStrategyConfig]
        The strategy configurations for the kernel.
    load_state : bool, default True
        If trading strategy state should be loaded from the database on start.
    save_state : bool, default True
        If trading strategy state should be saved to the database on stop.
    loop_debug : bool, default False
        If the asyncio event loop should be in debug mode.
    log_level : str, default "INFO"
        The stdout log level for the node.
    bypass_logging : bool, default False
        If logging to stdout should be bypassed.
    """

    environment: Environment
    trader_id: str
    cache: Optional[CacheConfig] = None
    cache_database: Optional[CacheDatabaseConfig] = None
    data_engine: DataEngineConfig = None
    risk_engine: RiskEngineConfig = None
    exec_engine: ExecEngineConfig = None
    streaming: Optional[StreamingConfig] = None
    actors: List[ImportableActorConfig] = Field(default_factory=list)
    strategies: List[ImportableStrategyConfig] = Field(default_factory=list)
    load_state: bool = False
    save_state: bool = False
    loop_debug: bool = False
    log_level: str = "INFO"
    bypass_logging: bool = False
