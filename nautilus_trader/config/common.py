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

import importlib
import importlib.util
from typing import Any, Optional

import fsspec
import msgspec

from nautilus_trader.common import Environment
from nautilus_trader.config.validation import PositiveFloat
from nautilus_trader.config.validation import PositiveInt
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog


def resolve_path(path: str) -> type:
    module, cls_str = path.rsplit(":", maxsplit=1)
    mod = importlib.import_module(module)
    cls: type = getattr(mod, cls_str)
    return cls


class NautilusConfig(msgspec.Struct, kw_only=True, frozen=True):
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


class CacheConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``Cache`` instances.

    Parameters
    ----------
    tick_capacity : PositiveInt, default 10_000
        The maximum length for internal tick dequeues.
    bar_capacity : PositiveInt, default 10_000
        The maximum length for internal bar dequeues.
    snapshot_orders : bool, default False
        If order state snapshot lists should be persisted.
        Snapshots will be taken at every order state update (when events are applied).
    snapshot_positions : bool, default False
        If position state snapshot lists should be persisted.
        Snapshots will be taken at position opened, changed and closed (when events are applied).
        To include the unrealized PnL in the snapshot then quotes for the positions instrument must
        be available in the cache.
    snapshot_positions_interval : PositiveFloat, optional
        The interval (seconds) at which additional position state snapshots are persisted.
        If ``None`` then no additional snapshots will be taken.
        To include the unrealized PnL in the snapshot then quotes for the positions instrument must
        be available in the cache.

    """

    tick_capacity: PositiveInt = 10_000
    bar_capacity: PositiveInt = 10_000
    snapshot_orders: bool = False
    snapshot_positions: bool = False
    snapshot_positions_interval: Optional[PositiveFloat] = None


class CacheDatabaseConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``CacheDatabase`` instances.

    Parameters
    ----------
    type : str, {'in-memory', 'redis'}, default 'in-memory'
        The database type.
    host : str, default 'localhost'
        The database host address.
    port : int, optional
        The database port.
    username : str, optional
        The account username for the database connection.
    password : str, optional
        The account password for the database connection.
    ssl : bool, default False
        If database should use an SSL enabled connection.
    flush_on_start : bool, default False
        If database should be flushed on start.
    timestamps_as_iso8601, default False
        If timestamps should be persisted as ISO 8601 strings.
        If `False` then will persit as UNIX nanoseconds.

    """

    type: str = "in-memory"
    host: str = "localhost"
    port: Optional[int] = None
    username: Optional[str] = None
    password: Optional[str] = None
    ssl: bool = False
    flush_on_start: bool = False
    timestamps_as_iso8601: bool = False


class InstrumentProviderConfig(NautilusConfig, frozen=True):
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


class DataEngineConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``DataEngine`` instances.

    Parameters
    ----------
    time_bars_build_with_no_updates : bool, default True
        If time bar aggregators will build and emit bars with no new market updates.
    time_bars_timestamp_on_close : bool, default True
        If time bar aggregators will timestamp `ts_event` on bar close.
        If False then will timestamp on bar open.
    validate_data_sequence : bool, default False
        If data objects timestamp sequencing will be validated and handled.
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    time_bars_build_with_no_updates: bool = True
    time_bars_timestamp_on_close: bool = True
    validate_data_sequence: bool = False
    debug: bool = False


class RiskEngineConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``RiskEngine`` instances.

    Parameters
    ----------
    bypass : bool, default False
        If True then will bypass all pre-trade risk checks and rate limits (will still check for duplicate IDs).
    max_order_submit_rate : str, default 100/00:00:01
        The maximum rate of submit order commands per timedelta.
    max_order_modify_rate : str, default 100/00:00:01
        The maximum rate of modify order commands per timedelta.
    max_notional_per_order : dict[str, int], default empty dict
        The maximum notional value of an order per instrument ID.
        The value should be a valid decimal format.
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    bypass: bool = False
    max_order_submit_rate: str = "100/00:00:01"
    max_order_modify_rate: str = "100/00:00:01"
    max_notional_per_order: dict[str, int] = {}
    debug: bool = False


class ExecEngineConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``ExecutionEngine`` instances.

    Parameters
    ----------
    load_cache : bool, default True
        If the cache should be loaded on initialization.
    allow_cash_positions : bool, default True
        If unleveraged spot/cash assets should generate positions.
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    load_cache: bool = True
    allow_cash_positions: bool = True
    debug: bool = False


class OrderEmulatorConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``OrderEmulator`` instances.

    Parameters
    ----------
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    debug: bool = False


class StreamingConfig(NautilusConfig, frozen=True):
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


class DataCatalogConfig(NautilusConfig, frozen=True):
    """
    Configuration for a data catalog.

    Parameters
    ----------
    path : str
        The path to the data catalog.
    fs_protocol : str, optional
        The fsspec file system protocol for the data catalog.
    fs_storage_options : dict, optional
        The fsspec storage options for the data catalog.
    use_rust : bool, default False
        If queries will be for Rust schema versions (when implemented).

    """

    path: str
    fs_protocol: Optional[str] = None
    fs_storage_options: Optional[dict] = None
    use_rust: bool = False


class ActorConfig(NautilusConfig, kw_only=True, frozen=True):
    """
    The base model for all actor configurations.

    Parameters
    ----------
    component_id : str, optional
        The component ID. If ``None`` then the identifier will be taken from
        `type(self).__name__`.

    """

    component_id: Optional[str] = None


class ImportableActorConfig(NautilusConfig, frozen=True):
    """
    Configuration for an actor instance.

    Parameters
    ----------
    actor_path : str
        The fully qualified name of the Actor class.
    config_path : str
        The fully qualified name of the Actor Config class.
    config : dict
        The actor configuration.

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
        actor_cls = resolve_path(config.actor_path)
        config_cls = resolve_path(config.config_path)
        return actor_cls(config=config_cls(**config.config))


class StrategyConfig(NautilusConfig, kw_only=True, frozen=True):
    """
    The base model for all trading strategy configurations.

    Parameters
    ----------
    strategy_id : str, optional
        The unique ID for the strategy. Will become the strategy ID if not None.
    order_id_tag : str, optional
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    oms_type : OmsType, optional
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs (see docs).
    external_order_claims : list[str], optional
        The external order claim instrument IDs.
        External orders for matching instrument IDs will be associated with (claimed by) the strategy.

    """

    strategy_id: Optional[str] = None
    order_id_tag: Optional[str] = None
    oms_type: Optional[str] = None
    external_order_claims: Optional[list[str]] = None


class ImportableStrategyConfig(NautilusConfig, frozen=True):
    """
    Configuration for a trading strategy instance.

    Parameters
    ----------
    strategy_path : str
        The fully qualified name of the strategy class.
    config_path : str
        The fully qualified name of the config class.
    config : dict[str, Any]
        The strategy configuration.

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


class ExecAlgorithmConfig(NautilusConfig, kw_only=True, frozen=True):
    """
    The base model for all execution algorithm configurations.

    Parameters
    ----------
    exec_algorithm_id : str, optional
        The unique ID for the execution algorithm.
        If not ``None`` then will become the execution algorithm ID.

    """

    exec_algorithm_id: Optional[str] = None


class ImportableExecAlgorithmConfig(NautilusConfig, frozen=True):
    """
    Configuration for an execution algorithm instance.

    Parameters
    ----------
    exec_algorithm_path : str
        The fully qualified name of the execution algorithm class.
    config_path : str
        The fully qualified name of the config class.
    config : dict[str, Any]
        The execution algorithm configuration.

    """

    exec_algorithm_path: str
    config_path: str
    config: dict[str, Any]


class ExecAlgorithmFactory:
    """
    Provides execution algorithm creation from importable configurations.
    """

    @staticmethod
    def create(config: ImportableExecAlgorithmConfig):
        """
        Create an execution algorithm from the given configuration.

        Parameters
        ----------
        config : ImportableExecAlgorithmConfig
            The configuration for the building step.

        Returns
        -------
        ExecAlgorithm

        Raises
        ------
        TypeError
            If `config` is not of type `ImportableExecAlgorithmConfig`.

        """
        PyCondition.type(config, ImportableExecAlgorithmConfig, "config")
        exec_algorithm_cls = resolve_path(config.exec_algorithm_path)
        config_cls = resolve_path(config.config_path)
        return exec_algorithm_cls(config=config_cls(**config.config))


class TracingConfig(NautilusConfig, frozen=True):
    """
    Configuration for standard output and file logging for Rust tracing statements for a
    ``NautilusKernel`` instance.

    Parameters
    ----------
    stdout_level : str, optional
        The minimum log level to write to stdout. Possible options are "debug",
        "info", "warn", "error". Setting it None means no logs are written to
        stdout.
    stderr_level : str, optional
        The minimum log level to write to stderr. Possible options are "debug",
        "info", "warn", "error". Setting it None means no logs are written to
        stderr.
    file_config : tuple[str, str, str], optional
        The minimum log level to write to log file. Possible options are "debug",
        "info", "warn", "error". Setting it None means no logs are written to
        the log file.
        The second str is the prefix name of the log file and the third str is
        the name of the directory.

    """

    stdout_level: Optional[str] = None
    stderr_level: Optional[str] = None
    file_level: Optional[tuple[str, str, str]] = None


class LoggingConfig(NautilusConfig, frozen=True):
    """
    Configuration for standard output and file logging for a ``NautilusKernel``
    instance.

    Parameters
    ----------
    log_level : str, default "INFO"
        The minimum log level to write to stdout.
        Will always write ERROR level logs to stderr (unless `bypass_logging` is True).
    log_level_file : str, optional
        The minimum log level to write to a log file.
        If ``None`` then no file logging will occur.
    log_directory : str, optional
        The path to the log file directory.
        If ``None`` then will write to the current working directory.
    log_file_name : str, optional
        The custom log file name (will use a '.log' suffix for plain text or '.json' for JSON).
        This will override automatic naming, and no daily file rotation will occur.
    log_file_format : str { 'JSON' }, optional
        The log file format. If ``None`` (default) then will log in plain text.
    log_component_levels : dict[str, LogLevel]
        The additional per component log level filters, where keys are component
        IDs (e.g. actor/strategy IDs) and values are log levels.
    bypass_logging : bool, default False
        If all logging should be bypassed.

    """

    log_level: str = "INFO"
    log_level_file: Optional[str] = None
    log_directory: Optional[str] = None
    log_file_name: Optional[str] = None
    log_file_format: Optional[str] = None
    log_component_levels: Optional[dict[str, str]] = None
    bypass_logging: bool = False


class NautilusKernelConfig(NautilusConfig, frozen=True):
    """
    Configuration for a ``NautilusKernel`` core system instance.

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
    emulator : OrderEmulatorConfig, optional
        The order emulator configuration.
    streaming : StreamingConfig, optional
        The configuration for streaming to feather files.
    catalog : DataCatalogConfig, optional
        The data catalog config.
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
    timeout_connection : PositiveFloat (seconds)
        The timeout for all clients to connect and initialize.
    timeout_reconciliation : PositiveFloat (seconds)
        The timeout for execution state to reconcile.
    timeout_portfolio : PositiveFloat (seconds)
        The timeout for portfolio to initialize margins and unrealized PnLs.
    timeout_disconnection : PositiveFloat (seconds)
        The timeout for all engine clients to disconnect.
    timeout_post_stop : PositiveFloat (seconds)
        The timeout after stopping the node to await residual events before final shutdown.

    """

    environment: Environment
    trader_id: str
    instance_id: Optional[str] = None
    cache: Optional[CacheConfig] = None
    cache_database: Optional[CacheDatabaseConfig] = None
    data_engine: Optional[DataEngineConfig] = None
    risk_engine: Optional[RiskEngineConfig] = None
    exec_engine: Optional[ExecEngineConfig] = None
    emulator: Optional[OrderEmulatorConfig] = None
    streaming: Optional[StreamingConfig] = None
    catalog: Optional[DataCatalogConfig] = None
    actors: list[ImportableActorConfig] = []
    strategies: list[ImportableStrategyConfig] = []
    exec_algorithms: list[ImportableExecAlgorithmConfig] = []
    load_state: bool = False
    save_state: bool = False
    loop_debug: bool = False
    logging: Optional[LoggingConfig] = None
    tracing: Optional[TracingConfig] = None
    timeout_connection: PositiveFloat = 10.0
    timeout_reconciliation: PositiveFloat = 10.0
    timeout_portfolio: PositiveFloat = 10.0
    timeout_disconnection: PositiveFloat = 10.0
    timeout_post_stop: PositiveFloat = 10.0


class ImportableFactoryConfig(NautilusConfig, frozen=True):
    """
    Represents an importable (JSON) factory config.
    """

    path: str

    def create(self):
        cls = resolve_path(self.path)
        return cls()


class ImportableConfig(NautilusConfig, frozen=True):
    """
    Represents an importable configuration (typically live data client or live execution
    client).
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
        cfg = msgspec.json.encode(self.config)
        return msgspec.json.decode(cfg, type=cls)
