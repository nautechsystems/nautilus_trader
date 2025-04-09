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

from __future__ import annotations

import hashlib
import importlib
from collections.abc import Callable
from decimal import Decimal
from typing import Annotated, Any

import msgspec
import pandas as pd
from msgspec import Meta

from nautilus_trader.common import Environment
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import Identifier
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


# An integer constrained to values > 0
PositiveInt = Annotated[int, Meta(gt=0)]

# An integer constrained to values >= 0
NonNegativeInt = Annotated[int, Meta(ge=0)]

# A float constrained to values > 0
PositiveFloat = Annotated[float, Meta(gt=0.0)]

# A float constrained to values >= 0
NonNegativeFloat = Annotated[float, Meta(ge=0.0)]

CUSTOM_ENCODINGS: dict[type, Callable] = {
    pd.DataFrame: lambda x: x.to_json(),
}


CUSTOM_DECODINGS: dict[type, Callable] = {
    pd.DataFrame: lambda x: pd.read_json(x),
}


class InvalidConfiguration(RuntimeError):
    """
    Raised when there is a violation of a configuration condition, making it invalid.
    """


def resolve_path(path: str) -> type:
    module, cls_str = path.rsplit(":", maxsplit=1)
    mod = importlib.import_module(module)
    cls: type = getattr(mod, cls_str)
    return cls


def resolve_config_path(path: str) -> type[NautilusConfig]:
    config = resolve_path(path)
    if not issubclass(config, NautilusConfig):
        raise TypeError(f"expected a subclass of `NautilusConfig`, was `{type(config)}`")
    return config


def nautilus_schema_hook(type_: type[Any]) -> dict[str, Any]:
    if issubclass(type_, Identifier):
        return {"type": "string"}
    if type_ in (Currency, Price, Quantity, Money, BarType, BarSpecification):
        return {"type": "string"}
    if type_ in (Decimal, UUID4):
        return {"type": "string"}
    if type_ == pd.Timestamp:
        return {"type": "string", "format": "date-time"}
    if type_ == pd.Timedelta:
        return {"type": "string"}
    if type_ == Environment:
        return {"type": "string"}
    if type_ is type:  # Handle <class 'type'>
        return {"type": "string"}  # Represent type objects as strings
    raise TypeError(f"Unsupported type for schema generation: {type_}")


def msgspec_encoding_hook(obj: Any) -> Any:
    if isinstance(obj, Decimal):
        return str(obj)
    if isinstance(obj, UUID4):
        return obj.value
    if isinstance(obj, Identifier):
        return obj.value
    if isinstance(obj, (BarType | BarSpecification)):
        return str(obj)
    if isinstance(obj, (Price | Quantity | Money | Currency)):
        return str(obj)
    if isinstance(obj, (pd.Timestamp | pd.Timedelta)):
        return obj.isoformat()
    if isinstance(obj, Environment):
        return obj.value
    if isinstance(obj, type) and hasattr(obj, "fully_qualified_name"):
        return obj.fully_qualified_name()
    if type(obj) in CUSTOM_ENCODINGS:
        func = CUSTOM_ENCODINGS[type(obj)]
        return func(obj)

    raise TypeError(f"Encoding objects of type {obj.__class__} is unsupported")


def msgspec_decoding_hook(obj_type: type, obj: Any) -> Any:  # noqa: C901 (too complex)
    if obj_type in (Decimal, pd.Timestamp, pd.Timedelta):
        return obj_type(obj)
    if obj_type == UUID4:
        return UUID4.from_str(obj)
    if obj_type == InstrumentId:
        return InstrumentId.from_str(obj)
    if issubclass(obj_type, Identifier):
        return obj_type(obj)
    if obj_type == BarSpecification:
        return BarSpecification.from_str(obj)
    if obj_type == BarType:
        return BarType.from_str(obj)
    if obj_type == Price:
        return Price.from_str(obj)
    if obj_type == Quantity:
        return Quantity.from_str(obj)
    if obj_type == Money:
        return Money.from_str(obj)
    if obj_type == Currency:
        return Currency.from_str(obj)
    if obj_type == Environment:
        return obj_type(obj)
    if obj_type in CUSTOM_DECODINGS:
        func = CUSTOM_DECODINGS[obj_type]
        return func(obj)

    raise TypeError(f"Decoding objects of type {obj_type} is unsupported")


def register_config_encoding(type_: type, encoder: Callable) -> None:
    global CUSTOM_ENCODINGS
    CUSTOM_ENCODINGS[type_] = encoder


def register_config_decoding(type_: type, decoder: Callable) -> None:
    global CUSTOM_DECODINGS
    CUSTOM_DECODINGS[type_] = decoder


def tokenize_config(obj: NautilusConfig) -> str:
    return hashlib.sha256(obj.json()).hexdigest()


class NautilusConfig(msgspec.Struct, kw_only=True, frozen=True):
    """
    The base class for all Nautilus configuration objects.
    """

    @property
    def id(self) -> str:
        """
        Return the hashed identifier for the configuration.

        Returns
        -------
        str

        """
        return tokenize_config(self)

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

    @classmethod
    def json_schema(cls) -> dict[str, Any]:
        """
        Generate a JSON schema for this configuration class.

        Returns
        -------
        dict[str, Any]

        """
        return msgspec.json.schema(cls, schema_hook=nautilus_schema_hook)

    @classmethod
    def parse(cls, raw: bytes | str) -> Any:
        """
        Return a decoded object of the given `cls`.

        Parameters
        ----------
        cls : type
            The type to decode to.
        raw : bytes or str
            The raw bytes or JSON string to decode.

        Returns
        -------
        Any

        """
        return msgspec.json.decode(raw, type=cls, dec_hook=msgspec_decoding_hook)

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
        return msgspec.json.encode(self, enc_hook=msgspec_encoding_hook)

    def json_primitives(self) -> dict[str, Any]:  # type: ignore [valid-type]
        """
        Return a dictionary representation of the configuration with JSON primitive
        types as values.

        Returns
        -------
        dict[str, Any]

        """
        return msgspec.json.decode(self.json())

    def validate(self) -> bool:
        """
        Return whether the configuration can be represented as valid JSON.

        Returns
        -------
        bool

        """
        return bool(self.parse(self.json()))


class DatabaseConfig(NautilusConfig, frozen=True):
    """
    Configuration for database connections.

    Parameters
    ----------
    type : str, {'redis'}, default 'redis'
        The database type.
    host : str, optional
        The database host address. If `None` then should use the typical default.
    port : int, optional
        The database port. If `None` then should use the typical default.
    username : str, optional
        The account username for the database connection.
    password : str, optional
        The account password for the database connection.
        If a value is provided then it will be redacted in the string repr for this object.
    ssl : bool, default False
        If socket should use an SSL (TLS encryption) enabled connection.
    timeout : int, default 20
        The timeout (seconds) to wait for a new connection.

    Notes
    -----
    If `type` is 'redis' then requires Redis version 6.2 or higher for correct operation (required for streams functionality).

    """

    type: str = "redis"
    host: str | None = None
    port: int | None = None
    username: str | None = None
    password: str | None = None
    ssl: bool = False
    timeout: int | None = 20

    def __repr__(self) -> str:
        redacted_password = "None"
        if self.password:
            if len(self.password) >= 4:
                redacted_password = f"{self.password[:2]}...{self.password[-2:]}"
            else:
                redacted_password = self.password
        return (
            f"{type(self).__name__}("
            f"type={self.type}, "
            f"host={self.host}, "
            f"port={self.port}, "
            f"username={self.username}, "
            f"password={redacted_password}, "
            f"ssl={self.ssl}, "
            f"timeout={self.timeout})"
        )


class MessageBusConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``MessageBus`` instances.

    Parameters
    ----------
    database : DatabaseConfig, optional
        The configuration for the message bus backing database.
    encoding : str, {'msgpack', 'json'}, default 'msgpack'
        The encoding for database operations, controls the type of serializer used.
    timestamps_as_iso8601, default False
        If timestamps should be persisted as ISO 8601 strings.
        If `False` then will persit as UNIX nanoseconds.
    buffer_interval_ms : PositiveInt, optional
        The buffer interval (milliseconds) between pipelined/batched transactions.
        The recommended range if using buffered pipeling is [10, 1000] milliseconds,
        with a good compromise being 100 milliseconds.
    autotrim_mins : int, optional
        The lookback window in minutes for automatic stream trimming.
        The actual window may extend up to one minute beyond the specified value since streams are
        trimmed at most once every minute.
        Note that this feature requires Redis version 6.2 or higher; otherwise it will result
        in a command syntax error.
    use_trader_prefix : bool, default True
        If a 'trader-' prefix is used for stream names.
    use_trader_id : bool, default True
        If the traders ID is used for stream names.
    use_instance_id : bool, default False
        If the traders instance ID is used for stream names.
    streams_prefix : str, default 'stream'
        The prefix for externally published stream names (must have a `database` config).
        If `use_trader_id` and `use_instance_id` are *both* false, then it becomes possible for
        many traders to be configured to write to the same streams.
    stream_per_topic : bool, default True
        If True, messages will be written to separate streams per topic.
        If False, all messages will be written to the same stream.
    external_streams : list[str], optional
        The external stream keys the node will listen to for publishing deserialized message
        payloads on the internal message bus.
    types_filter : list[type], optional
        A list of serializable types **not** to publish externally.
    heartbeat_interval_secs : PositiveInt, optional
        The heartbeat interval (seconds) to use for trading node health.

    """

    database: DatabaseConfig | None = None
    encoding: str = "msgpack"
    timestamps_as_iso8601: bool = False
    buffer_interval_ms: PositiveInt | None = None
    autotrim_mins: int | None = None
    use_trader_prefix: bool = True
    use_trader_id: bool = True
    use_instance_id: bool = False
    streams_prefix: str = "stream"
    stream_per_topic: bool = True
    external_streams: list[str] | None = None
    types_filter: list[type] | None = None
    heartbeat_interval_secs: PositiveInt | None = None


class InstrumentProviderConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``InstrumentProvider`` instances.

    Parameters
    ----------
    load_all : bool, default False
        If all venue instruments should be loaded on start.
    load_ids : frozenset[InstrumentId], optional
        The list of instrument IDs to be loaded on start (if `load_all` is False).
    filters : frozendict or dict[str, Any], optional
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
        filters = frozenset(self.filters.items()) if self.filters else None
        return hash((self.load_all, self.load_ids, filters))

    load_all: bool = False
    load_ids: frozenset[InstrumentId] | None = None
    filters: dict[str, Any] | None = None
    filter_callable: str | None = None
    log_warnings: bool = True


class OrderEmulatorConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``OrderEmulator`` instances.

    Parameters
    ----------
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    debug: bool = False


class ActorConfig(NautilusConfig, kw_only=True, frozen=True):
    """
    The base model for all actor configurations.

    Parameters
    ----------
    component_id : ComponentId, optional
        The component ID. If ``None`` then the identifier will be taken from
        `type(self).__name__`.
    log_events : bool, default True
        If events should be logged by the actor.
        If False, then only warning events and above are logged.
    log_commands : bool, default True
        If commands should be logged by the actor.

    """

    component_id: ComponentId | None = None
    log_events: bool = True
    log_commands: bool = True


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
        config_cls = resolve_config_path(config.config_path)
        json = msgspec.json.encode(config.config, enc_hook=msgspec_encoding_hook)
        config = config_cls.parse(json)
        return actor_cls(config)


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
    log_file_max_size : PositiveInt, optional
        The maximum size of log files in bytes before rotation occurs.
    log_file_max_backup_count : NonNegativeInt, default 5
        The maximum number of backup log files to keep when rotating.
    log_colors : bool, default True
        If ANSI codes should be used to produce colored log lines.
    log_component_levels : dict[str, LogLevel]
        The additional per component log level filters, where keys are component
        IDs (e.g. actor/strategy IDs) and values are log levels.
    bypass_logging : bool, default False
        If all logging should be bypassed.
    print_config : bool, default False
        If the core logging configuration should be printed to stdout at initialization.
    use_pyo3: bool, default False
        If the logging system should be initialized via pyo3,
        this isn't recommended for backtesting as the performance is much lower
        but can be useful for seeing logs originating from Rust.
    clear_log_file : bool, default False
        If the log file name should be cleared before being used (e.g. for testing).
        Only applies if `log_file_name` is not ``None``.

    """

    log_level: str = "INFO"
    log_level_file: str | None = None
    log_directory: str | None = None
    log_file_name: str | None = None
    log_file_format: str | None = None
    log_file_max_size: PositiveInt | None = None
    log_file_max_backup_count: NonNegativeInt = 5
    log_colors: bool = True
    log_component_levels: dict[str, str] | None = None
    bypass_logging: bool = False
    print_config: bool = False
    use_pyo3: bool = False
    clear_log_file: bool = False


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
    factory: ImportableFactoryConfig | None = None

    @staticmethod
    def is_importable(data: dict) -> bool:
        return set(data) == {"path", "config"}

    def create(self):
        assert ":" in self.path, "`path` variable should be of the form `path.to.module:class`"
        cls = resolve_path(self.path)
        cfg = msgspec.json.encode(self.config, enc_hook=msgspec_encoding_hook)
        return msgspec.json.decode(cfg, type=cls)
