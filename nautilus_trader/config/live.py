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

from typing import Annotated, Optional

from msgspec import Meta

from nautilus_trader.common import Environment
from nautilus_trader.config.common import DataEngineConfig
from nautilus_trader.config.common import ExecEngineConfig
from nautilus_trader.config.common import InstrumentProviderConfig
from nautilus_trader.config.common import NautilusConfig
from nautilus_trader.config.common import NautilusKernelConfig
from nautilus_trader.config.common import RiskEngineConfig
from nautilus_trader.config.common import resolve_path
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


# A float constrained to values > 0
PositiveFloat = Annotated[float, Meta(gt=0)]


class ImportableClientConfig(NautilusConfig):
    """
    Represents a live data or execution client configuration.
    """

    @staticmethod
    def is_importable(data: dict):
        return set(data) == {"factory_path", "config_path", "config"}

    @staticmethod
    def create(data: dict, config_type: type):
        assert (
            ":" in data["factory_path"]
        ), "`class_path` variable should be of the form `path.to.module:class`"
        assert (
            ":" in data["config_path"]
        ), "`config_path` variable should be of the form `path.to.module:class`"
        factory = resolve_path(data["factory_path"])
        cls = resolve_path(data["config_path"])
        config = cls(**data["config"], factory=factory)
        assert isinstance(config, config_type)
        return config


class LiveDataEngineConfig(DataEngineConfig):
    """
    Configuration for ``LiveDataEngine`` instances.
    """

    qsize: int = 10000


class LiveRiskEngineConfig(RiskEngineConfig):
    """
    Configuration for ``LiveRiskEngine`` instances.
    """

    qsize: int = 10000


class LiveExecEngineConfig(ExecEngineConfig):
    """
    Configuration for ``LiveExecEngine`` instances.

    Parameters
    ----------
    reconciliation : bool, default True
        If reconciliation is active at start-up.
    reconciliation_lookback_mins : NonNegativeInt, optional
        The maximum lookback minutes to reconcile state for.
        If ``None`` or 0 then will use the maximum lookback available from the venues.
    inflight_check_interval_ms : NonNegativeInt, default 5000
        The interval (milliseconds) between checking whether in-flight orders
        have exceeded their time-in-flight threshold.
    inflight_check_threshold_ms : NonNegativeInt, default 1000
        The threshold (milliseconds) beyond which an in-flight orders status
        is checked with the venue.
    qsize : PositiveInt, default 10000
        The queue size for the engines internal queue buffers.
    """

    reconciliation: bool = True
    reconciliation_lookback_mins: Optional[int] = None
    inflight_check_interval_ms: int = 5000
    inflight_check_threshold_ms: int = 1000
    qsize: int = 10000


class RoutingConfig(NautilusConfig):
    """
    Configuration for live client message routing.

    Parameters
    ----------
    default : bool
        If the client should be registered as the default routing client
        (when a specific venue routing cannot be found).
    venues : list[str], optional
        The venues to register for routing.
    """

    default: bool = False
    venues: Optional[frozenset[str]] = None

    def __hash__(self):  # make hashable BaseModel subclass
        return hash((type(self),) + tuple(self.__dict__.values()))


class LiveDataClientConfig(NautilusConfig):
    """
    Configuration for ``LiveDataClient`` instances.

    Parameters
    ----------
    instrument_provider : InstrumentProviderConfig
        The clients instrument provider configuration.
    routing : RoutingConfig
        The clients message routing config.
    factory :
    """

    instrument_provider: InstrumentProviderConfig = InstrumentProviderConfig()
    routing: RoutingConfig = RoutingConfig()
    factory: Optional[type[LiveDataClientFactory]] = None


class LiveExecClientConfig(NautilusConfig):
    """
    Configuration for ``LiveExecutionClient`` instances.

    Parameters
    ----------
    instrument_provider : InstrumentProviderConfig
        The clients instrument provider configuration.
    routing : RoutingConfig
        The clients message routing config.
    """

    instrument_provider: InstrumentProviderConfig = InstrumentProviderConfig()
    routing: RoutingConfig = RoutingConfig()
    factory: Optional[type[LiveExecClientFactory]] = None


class TradingNodeConfig(NautilusKernelConfig):
    """
    Configuration for ``TradingNode`` instances.

    Parameters
    ----------
    trader_id : str, default "TRADER-000"
        The trader ID for the node (must be a name and ID tag separated by a hyphen).
    cache : CacheConfig, optional
        The cache configuration.
    cache_database : CacheDatabaseConfig, optional
        The cache database configuration.
    data_engine : LiveDataEngineConfig, optional
        The live data engine configuration.
    risk_engine : LiveRiskEngineConfig, optional
        The live risk engine configuration.
    exec_engine : LiveExecEngineConfig, optional
        The live execution engine configuration.
    streaming : StreamingConfig, optional
        The configuration for streaming to feather files.
    data_clients : dict[str, LiveDataClientConfig], optional
        The data client configurations.
    exec_clients : dict[str, LiveExecClientConfig], optional
        The execution client configurations.
    strategies : list[ImportableStrategyConfig]
        The strategy configurations for the node.
    load_state : bool, default True
        If trading strategy state should be loaded from the database on start.
    save_state : bool, default True
        If trading strategy state should be saved to the database on stop.
    log_level : str, default "INFO"
        The stdout log level for the node.
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

    environment: Environment = Environment.LIVE
    trader_id: str = "TRADER-001"
    data_engine: LiveDataEngineConfig = LiveDataEngineConfig()
    risk_engine: LiveRiskEngineConfig = LiveRiskEngineConfig()
    exec_engine: LiveExecEngineConfig = LiveExecEngineConfig()
    data_clients: dict[str, LiveDataClientConfig] = {}
    exec_clients: dict[str, LiveExecClientConfig] = {}
    timeout_connection: float = 10.0
    timeout_reconciliation: float = 10.0
    timeout_portfolio: float = 10.0
    timeout_disconnection: PositiveFloat = 10.0
    timeout_post_stop: PositiveFloat = 10.0

    # @validator("data_clients", pre=True)
    # def validate_importable_data_clients(cls, v) -> dict[str, LiveDataClientConfig]:
    #     """Resolve any ImportableClientConfig into a LiveDataClientConfig."""
    #
    #     def resolve(config) -> LiveDataClientConfig:
    #         if ImportableClientConfig.is_importable(config):
    #             return ImportableClientConfig.create(config, config_type=LiveDataClientConfig)
    #         return config
    #
    #     data_clients = {name: resolve(config) for name, config in v.items()}
    #     return data_clients
    #
    # @validator("exec_clients", pre=True)
    # def validate_importable_exec_clients(cls, v) -> dict[str, LiveExecClientConfig]:
    #     """Resolve any ImportableClientConfig into a LiveExecClientConfig."""
    #
    #     def resolve(config) -> LiveExecClientConfig:
    #         if ImportableClientConfig.is_importable(config):
    #             return ImportableClientConfig.create(config, config_type=LiveExecClientConfig)
    #         return config
    #
    #     exec_clients = {name: resolve(config) for name, config in v.items()}
    #     return exec_clients
