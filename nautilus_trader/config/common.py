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
from typing import Any, Optional

import fsspec
import msgspec

from nautilus_trader.common import Environment
from nautilus_trader.config.validation import PositiveInt
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog


def resolve_path(path: str):
    module, cls = path.rsplit(":", maxsplit=1)
    mod = importlib.import_module(module)
    cls = getattr(mod, cls)
    return cls


class NautilusConfig(msgspec.Struct):
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

    def dict(self) -> dict[str, Any]:
        """
        Return a dictionary representation of the configuration.

        Returns
        -------
        dict[str, Any]

        """
        return {k: getattr(self, k) for k in self.__struct_fields__}

    def json(self) -> bytes:
        """
        Return serialized JSON encoded bytes.

        Returns
        -------
        bytes

        """
        return msgspec.json.encode(self)

    @classmethod
    def parse(cls, raw: bytes) -> Any:
        """
        Return a decoded object of the given `cls`.

        Parameters
        ----------
        cls : type
            The type to decode to.
        raw : bytes
            The raw bytes to decode.

        Returns
        -------
        Any

        """
        return msgspec.json.decode(raw, type=cls)

    def validate(self) -> bool:
        """
        Return whether the configuration can be represented as valid JSON.

        Returns
        -------
        bool

        """
        return bool(msgspec.json.decode(self.json(), type=self.__class__))


class CacheConfig(NautilusConfig):
    """
    Configuration for ``Cache`` instances.

    Parameters
    ----------
    tick_capacity : PositiveInt
        The maximum length for internal tick dequeues.
    bar_capacity : PositiveInt
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
    filter_callable: str, optional
        A fully qualified path to a callable that takes a single argument, `instrument` and returns a bool, indicating
        whether the instrument should be loaded
    log_warnings : bool, default True
        If parser warnings should be logged.
    """

    def __eq__(self, other):
        return (
            self.load_all == other.load_all
            and self.load_ids == other.load_ids
            and self.filters == other.filters
        )

    def __hash__(self):
        return hash((self.load_all, self.load_ids, self.filters))

    load_all: bool = False
    load_ids: Optional[frozenset[str]] = None
    filters: Optional[dict[str, Any]] = None
    filter_callable: Optional[str] = None
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
    bypass : bool, default False
        If True then will bypass all pre-trade risk checks and rate limits (will still check for duplicate IDs).
    deny_modify_pending_update : bool, default True
        If deny `ModifyOrder` commands when an order is in a `PENDING_UPDATE` state.
    max_order_submit_rate : str, default 100/00:00:01
        The maximum rate of submit order commands per timedelta.
    max_order_modify_rate : str, default 100/00:00:01
        The maximum rate of modify order commands per timedelta.
    max_notional_per_order : dict[str, int]
        The maximum notional value of an order per instrument ID.
        The value should be a valid decimal format.
    debug : bool
        If debug mode is active (will provide extra debug logging).
    """

    bypass: bool = False
    deny_modify_pending_update: bool = True
    max_order_submit_rate: str = "100/00:00:01"
    max_order_modify_rate: str = "100/00:00:01"
    max_notional_per_order: dict[str, int] = {}
    debug: bool = False


class ExecEngineConfig(NautilusConfig):
    """
    Configuration for ``ExecutionEngine`` instances.

    Parameters
    ----------
    load_cache : bool, default True
        If the cache should be loaded on initialization.
    allow_cash_positions : bool, default True
        If unleveraged spot/cash assets should generate positions.
    debug : bool
        If debug mode is active (will provide extra debug logging).
    """

    load_cache: bool = True
    allow_cash_positions: bool = True
    debug: bool = False


class OrderEmulatorConfig(NautilusConfig):
    """
    Configuration for ``OrderEmulator`` instances.
    """


class StreamingConfig(NautilusConfig):
    """
    Configuration for streaming live or backtest runs to the catalog in feather format.

    Parameters
    ----------
    catalog_path : str
        The path to the data catalog.
    fs_protocol : str, optional
        The `fsspec` filesystem protocol for the catalog.
    fs_storage_options : dict, optional
        The `fsspec` storage options.
    flush_interval_ms : int, optional
        The flush interval (milliseconds) for writing chunks.
    replace_existing: bool, default False
        If any existing feather files should be replaced.
    """

    catalog_path: str
    fs_protocol: Optional[str] = None
    fs_storage_options: Optional[dict] = None
    flush_interval_ms: Optional[int] = None
    replace_existing: bool = False
    include_types: Optional[list[str]] = None

    @property
    def fs(self):
        return fsspec.filesystem(protocol=self.fs_protocol, **(self.fs_storage_options or {}))

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
    config : dict
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
    config : dict[str, Any]
        The strategy configuration
    """

    strategy_path: str
    config_path: str
    config: dict[str, Any]


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
    actors : list[ImportableActorConfig]
        The actor configurations for the kernel.
    strategies : list[ImportableStrategyConfig]
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
    instance_id: Optional[str] = None
    cache: Optional[CacheConfig] = None
    cache_database: Optional[CacheDatabaseConfig] = None
    data_engine: Optional[DataEngineConfig] = None
    risk_engine: Optional[RiskEngineConfig] = None
    exec_engine: Optional[ExecEngineConfig] = None
    streaming: Optional[StreamingConfig] = None
    actors: list[ImportableActorConfig] = []
    strategies: list[ImportableStrategyConfig] = []
    load_state: bool = False
    save_state: bool = False
    loop_debug: bool = False
    log_level: str = "INFO"
    bypass_logging: bool = False


class ImportableFactoryConfig(NautilusConfig):
    """
    Represents an importable (json) Factory config.
    """

    path: str

    def create(self):
        cls = resolve_path(self.path)
        return cls()


class ImportableConfig(NautilusConfig):
    """
    Represents an importable (typically live data or execution) client configuration.
    """

    path: str
    config: dict = {}
    factory: Optional[ImportableFactoryConfig] = None

    @staticmethod
    def is_importable(data: dict):
        return set(data) == {"path", "config"}

    def create(self):
        assert ":" in self.path, "`path` variable should be of the form `path.to.module:class`"
        cls = resolve_path(self.path)
        return msgspec.json.decode(msgspec.json.encode(self.config), type=cls)
