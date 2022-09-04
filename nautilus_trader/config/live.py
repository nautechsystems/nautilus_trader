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

from typing import Dict, FrozenSet, Optional

from pydantic import PositiveFloat
from pydantic import PositiveInt
from pydantic import validator

from nautilus_trader.common import Environment
from nautilus_trader.config.common import DataEngineConfig
from nautilus_trader.config.common import ExecEngineConfig
from nautilus_trader.config.common import InstrumentProviderConfig
from nautilus_trader.config.common import NautilusConfig
from nautilus_trader.config.common import NautilusKernelConfig
from nautilus_trader.config.common import RiskEngineConfig
from nautilus_trader.config.common import resolve_path


class ImportableClientConfig(NautilusConfig):
    """
    Represents a live data or execution client configuration.
    """

    @staticmethod
    def is_importable(data: Dict):
        return set(data) == {"factory_path", "config_path", "config"}

    @staticmethod
    def create(data: Dict, config_type: type):
        assert (
            ":" in data["factory_path"]
        ), "`class_path` variable should be of the form `path.to.module:class`"
        assert (
            ":" in data["config_path"]
        ), "`config_path` variable should be of the form `path.to.module:class`"
        cls = resolve_path(data["config_path"])
        config = cls(**data["config"])
        assert isinstance(config, config_type)
        return config


class LiveDataEngineConfig(DataEngineConfig):
    """
    Configuration for ``LiveDataEngine`` instances.
    """

    qsize: PositiveInt = 10000


class LiveRiskEngineConfig(RiskEngineConfig):
    """
    Configuration for ``LiveRiskEngine`` instances.
    """

    qsize: PositiveInt = 10000


class LiveExecEngineConfig(ExecEngineConfig):
    """
    Configuration for ``LiveExecEngine`` instances.

    Parameters
    ----------
    reconciliation_auto : bool
        If reconciliation should automatically generate events to align state.
    reconciliation_lookback_mins : PositiveInt, optional
        The maximum lookback minutes to reconcile state for. If None then will
        use the maximum lookback available from the venues.
    qsize : PositiveInt
        The queue size for the engines internal queue buffers.
    """

    reconciliation_auto: bool = True
    reconciliation_lookback_mins: Optional[PositiveInt] = None
    qsize: PositiveInt = 10000


class RoutingConfig(NautilusConfig):
    """
    Configuration for live client message routing.

    Parameters
    ----------
    default : bool
        If the client should be registered as the default routing client
        (when a specific venue routing cannot be found).
    venues : List[str], optional
        The venues to register for routing.
    """

    default: bool = False
    venues: Optional[FrozenSet[str]] = None

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
    """

    instrument_provider: InstrumentProviderConfig = InstrumentProviderConfig()
    routing: RoutingConfig = RoutingConfig()


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
    strategies : List[ImportableStrategyConfig]
        The strategy configurations for the node.
    load_strategy_state : bool, default True
        If trading strategy state should be loaded from the database on start.
    save_strategy_state : bool, default True
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
    data_clients: Dict[str, LiveDataClientConfig] = {}
    exec_clients: Dict[str, LiveExecClientConfig] = {}
    timeout_connection: PositiveFloat = 10.0
    timeout_reconciliation: PositiveFloat = 10.0
    timeout_portfolio: PositiveFloat = 10.0
    timeout_disconnection: PositiveFloat = 10.0
    timeout_post_stop: PositiveFloat = 10.0

    @validator("data_clients", pre=True)
    def validate_importable_data_clients(cls, v) -> Dict[str, LiveDataClientConfig]:
        """Resolve any ImportableClientConfig into a LiveDataClientConfig."""

        def resolve(config) -> LiveDataClientConfig:
            if ImportableClientConfig.is_importable(config):
                return ImportableClientConfig.create(config, config_type=LiveDataClientConfig)
            return config

        data_clients = {name: resolve(config) for name, config in v.items()}
        return data_clients

    @validator("exec_clients", pre=True)
    def validate_importable_exec_clients(cls, v) -> Dict[str, LiveExecClientConfig]:
        """Resolve any ImportableClientConfig into a LiveExecClientConfig."""

        def resolve(config) -> LiveExecClientConfig:
            if ImportableClientConfig.is_importable(config):
                return ImportableClientConfig.create(config, config_type=LiveExecClientConfig)
            return config

        exec_clients = {name: resolve(config) for name, config in v.items()}
        return exec_clients
