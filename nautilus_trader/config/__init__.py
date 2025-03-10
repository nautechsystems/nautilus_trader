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
"""
The `config` subpackage groups all configuration classes and factories.

All configurations inherit from :class:`NautilusConfig` which in turn inherits from :class:`msgspec.Struct`.

"""

from nautilus_trader.backtest.config import BacktestDataConfig
from nautilus_trader.backtest.config import BacktestEngineConfig
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.backtest.config import BacktestVenueConfig
from nautilus_trader.backtest.config import FXRolloverInterestConfig
from nautilus_trader.backtest.config import SimulationModuleConfig
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import ActorFactory
from nautilus_trader.common.config import DatabaseConfig
from nautilus_trader.common.config import ImportableActorConfig
from nautilus_trader.common.config import ImportableConfig
from nautilus_trader.common.config import InstrumentProviderConfig
from nautilus_trader.common.config import InvalidConfiguration
from nautilus_trader.common.config import LoggingConfig
from nautilus_trader.common.config import MessageBusConfig
from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import NonNegativeFloat
from nautilus_trader.common.config import NonNegativeInt
from nautilus_trader.common.config import OrderEmulatorConfig
from nautilus_trader.common.config import PositiveFloat
from nautilus_trader.common.config import PositiveInt
from nautilus_trader.common.config import msgspec_decoding_hook
from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.common.config import register_config_decoding
from nautilus_trader.common.config import register_config_encoding
from nautilus_trader.common.config import resolve_config_path
from nautilus_trader.common.config import resolve_path
from nautilus_trader.common.config import tokenize_config
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.execution.config import ExecAlgorithmConfig
from nautilus_trader.execution.config import ExecAlgorithmFactory
from nautilus_trader.execution.config import ExecEngineConfig
from nautilus_trader.execution.config import ImportableExecAlgorithmConfig
from nautilus_trader.live.config import ControllerConfig
from nautilus_trader.live.config import ControllerFactory
from nautilus_trader.live.config import ImportableControllerConfig
from nautilus_trader.live.config import LiveDataClientConfig
from nautilus_trader.live.config import LiveDataEngineConfig
from nautilus_trader.live.config import LiveExecClientConfig
from nautilus_trader.live.config import LiveExecEngineConfig
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.live.config import RoutingConfig
from nautilus_trader.live.config import TradingNodeConfig
from nautilus_trader.persistence.config import DataCatalogConfig
from nautilus_trader.persistence.config import StreamingConfig
from nautilus_trader.portfolio.config import PortfolioConfig
from nautilus_trader.risk.config import RiskEngineConfig
from nautilus_trader.system.config import NautilusKernelConfig
from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.trading.config import StrategyConfig
from nautilus_trader.trading.config import StrategyFactory


__all__ = [
    "ActorConfig",
    "ActorFactory",
    "BacktestDataConfig",
    "BacktestEngineConfig",
    "BacktestRunConfig",
    "BacktestVenueConfig",
    "CacheConfig",
    "ControllerConfig",
    "ControllerFactory",
    "DataCatalogConfig",
    "DataEngineConfig",
    "DatabaseConfig",
    "ExecAlgorithmConfig",
    "ExecAlgorithmFactory",
    "ExecEngineConfig",
    "FXRolloverInterestConfig",
    "ImportableActorConfig",
    "ImportableConfig",
    "ImportableControllerConfig",
    "ImportableExecAlgorithmConfig",
    "ImportableStrategyConfig",
    "InstrumentProviderConfig",
    "InvalidConfiguration",
    "LiveDataClientConfig",
    "LiveDataEngineConfig",
    "LiveExecClientConfig",
    "LiveExecEngineConfig",
    "LiveRiskEngineConfig",
    "LoggingConfig",
    "MessageBusConfig",
    "NautilusConfig",
    "NautilusKernelConfig",
    "NonNegativeFloat",
    "NonNegativeInt",
    "OrderEmulatorConfig",
    "PortfolioConfig",
    "PositiveFloat",
    "PositiveInt",
    "RiskEngineConfig",
    "RoutingConfig",
    "SimulationModuleConfig",
    "StrategyConfig",
    "StrategyFactory",
    "StreamingConfig",
    "TradingNodeConfig",
    "msgspec_decoding_hook",
    "msgspec_encoding_hook",
    "register_config_decoding",
    "register_config_encoding",
    "resolve_config_path",
    "resolve_path",
    "tokenize_config",
]
