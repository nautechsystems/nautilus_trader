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

import pydantic
from pydantic import PositiveInt

from nautilus_trader.config.common import resolve_path
from nautilus_trader.config.components import InstrumentProviderConfig
from nautilus_trader.config.engines import DataEngineConfig
from nautilus_trader.config.engines import ExecEngineConfig
from nautilus_trader.config.engines import RiskEngineConfig


class ImportableClientConfig(pydantic.BaseModel):
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
    reconciliation_lookback_mins : int, optional
        The maximum lookback minutes to reconcile state for. If None then will
        use the maximum lookback available from the venues.
    qsize : PositiveInt
        The queue size for the engines internal queue buffers.
    """

    reconciliation_auto: bool = True
    reconciliation_lookback_mins: Optional[PositiveInt] = None
    qsize: PositiveInt = 10000


class RoutingConfig(pydantic.BaseModel):
    """
    Configuration for live client message routing.

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


class LiveDataClientConfig(pydantic.BaseModel):
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


class LiveExecClientConfig(pydantic.BaseModel):
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
