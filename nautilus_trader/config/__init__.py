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
"""
The `config` subpackage groups all configuration classes and factories.

All configurations inherit from :class:`msgspec.Struct`.

"""

from nautilus_trader.config.backtest import BacktestDataConfig
from nautilus_trader.config.backtest import BacktestEngineConfig
from nautilus_trader.config.backtest import BacktestRunConfig
from nautilus_trader.config.backtest import BacktestVenueConfig
from nautilus_trader.config.common import ActorConfig
from nautilus_trader.config.common import ActorFactory
from nautilus_trader.config.common import CacheConfig
from nautilus_trader.config.common import DataCatalogConfig
from nautilus_trader.config.common import DataEngineConfig
from nautilus_trader.config.common import ExecAlgorithmConfig
from nautilus_trader.config.common import ExecAlgorithmFactory
from nautilus_trader.config.common import ExecEngineConfig
from nautilus_trader.config.common import ImportableActorConfig
from nautilus_trader.config.common import ImportableConfig
from nautilus_trader.config.common import ImportableExecAlgorithmConfig
from nautilus_trader.config.common import ImportableStrategyConfig
from nautilus_trader.config.common import InstrumentProviderConfig
from nautilus_trader.config.common import LoggingConfig
from nautilus_trader.config.common import NautilusKernelConfig
from nautilus_trader.config.common import OrderEmulatorConfig
from nautilus_trader.config.common import RiskEngineConfig
from nautilus_trader.config.common import StrategyConfig
from nautilus_trader.config.common import StrategyFactory
from nautilus_trader.config.common import StreamingConfig
from nautilus_trader.config.live import LiveDataClientConfig
from nautilus_trader.config.live import LiveDataEngineConfig
from nautilus_trader.config.live import LiveExecClientConfig
from nautilus_trader.config.live import LiveExecEngineConfig
from nautilus_trader.config.live import LiveRiskEngineConfig
from nautilus_trader.config.live import RoutingConfig
from nautilus_trader.config.live import TradingNodeConfig


__all__ = [
    "BacktestDataConfig",
    "BacktestEngineConfig",
    "BacktestRunConfig",
    "BacktestVenueConfig",
    "ActorConfig",
    "ActorFactory",
    "CacheConfig",
    "DataCatalogConfig",
    "DataEngineConfig",
    "ExecAlgorithmConfig",
    "ExecAlgorithmFactory",
    "ExecEngineConfig",
    "ImportableActorConfig",
    "ImportableExecAlgorithmConfig",
    "ImportableStrategyConfig",
    "InstrumentProviderConfig",
    "LoggingConfig",
    "NautilusKernelConfig",
    "OrderEmulatorConfig",
    "RiskEngineConfig",
    "StrategyConfig",
    "StrategyFactory",
    "StreamingConfig",
    "ImportableConfig",
    "LiveDataClientConfig",
    "LiveDataEngineConfig",
    "LiveExecClientConfig",
    "LiveExecEngineConfig",
    "LiveRiskEngineConfig",
    "RoutingConfig",
    "TradingNodeConfig",
]
