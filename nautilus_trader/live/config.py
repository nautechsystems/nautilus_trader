# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec

from nautilus_trader.common import Environment
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import InstrumentProviderConfig
from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import NonNegativeInt
from nautilus_trader.common.config import PositiveFloat
from nautilus_trader.common.config import PositiveInt
from nautilus_trader.common.config import resolve_config_path
from nautilus_trader.common.config import resolve_path
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.data.config import DataEngineConfig
from nautilus_trader.execution.config import ExecEngineConfig
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.risk.config import RiskEngineConfig
from nautilus_trader.system.config import NautilusKernelConfig
from nautilus_trader.trading.config import ImportableControllerConfig


class LiveDataEngineConfig(DataEngineConfig, frozen=True):
    """
    Configuration for ``LiveDataEngine`` instances.

    Parameters
    ----------
    qsize : PositiveInt, default 100_000
        The queue size for the engines internal queue buffers.

    """

    qsize: PositiveInt = 100_000


class LiveRiskEngineConfig(RiskEngineConfig, frozen=True):
    """
    Configuration for ``LiveRiskEngine`` instances.

    Parameters
    ----------
    qsize : PositiveInt, default 100_000
        The queue size for the engines internal queue buffers.

    """

    qsize: PositiveInt = 100_000


class LiveExecEngineConfig(ExecEngineConfig, frozen=True):
    """
    Configuration for ``LiveExecEngine`` instances.

    The purpose of the in-flight order check is for live reconciliation, events
    emitted from the exchange may have been lost at some point - leaving an order
    in an intermediate state, the check can recover these events via status reports.

    Parameters
    ----------
    reconciliation : bool, default True
        If reconciliation is active at start-up.
    reconciliation_lookback_mins : NonNegativeInt, optional
        The maximum lookback minutes to reconcile state for.
        If ``None`` or 0 then will use the maximum lookback available from the venues.
    filter_unclaimed_external_orders : bool, default False
        If unclaimed order events with an EXTERNAL strategy ID should be filtered/dropped.
    filter_position_reports : bool, default False
        If `PositionStatusReport`s are filtered from reconciliation.
        This may be applicable when other nodes are trading the same instrument(s), on the same
        account - which could cause conflicts in position status.
    inflight_check_interval_ms : NonNegativeInt, default 2_000
        The interval (milliseconds) between checking whether in-flight orders
        have exceeded their time-in-flight threshold.
        This should not be set less than the `inflight_check_interval_ms`.
    inflight_check_threshold_ms : NonNegativeInt, default 5_000
        The threshold (milliseconds) beyond which an in-flight orders status
        is checked with the venue.
        As a rule of thumb, you shouldn't consider reducing this setting unless you
        are colocated with the venue (to avoid the potential for race conditions).
    qsize : PositiveInt, default 100_000
        The queue size for the engines internal queue buffers.

    """

    reconciliation: bool = True
    reconciliation_lookback_mins: NonNegativeInt | None = None
    filter_unclaimed_external_orders: bool = False
    filter_position_reports: bool = False
    inflight_check_interval_ms: NonNegativeInt = 2_000
    inflight_check_threshold_ms: NonNegativeInt = 5_000
    qsize: PositiveInt = 100_000


class RoutingConfig(NautilusConfig, frozen=True):
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
    venues: frozenset[str] | None = None


class LiveDataClientConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``LiveDataClient`` instances.

    Parameters
    ----------
    handle_revised_bars : bool
        If DataClient will emit bar updates when a new bar opens.
    instrument_provider : InstrumentProviderConfig
        The clients instrument provider configuration.
    routing : RoutingConfig
        The clients message routing config.

    """

    handle_revised_bars: bool = False
    instrument_provider: InstrumentProviderConfig = InstrumentProviderConfig()
    routing: RoutingConfig = RoutingConfig()


class LiveExecClientConfig(NautilusConfig, frozen=True):
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


class ControllerConfig(ActorConfig, kw_only=True, frozen=True):
    """
    The base model for all controller configurations.
    """


class ControllerFactory:
    """
    Provides controller creation from importable configurations.
    """

    @staticmethod
    def create(
        config: ImportableControllerConfig,
        trader,
    ):
        from nautilus_trader.trading.trader import Trader

        PyCondition.type(trader, Trader, "trader")
        controller_cls = resolve_path(config.controller_path)
        config_cls = resolve_config_path(config.config_path)
        config = config_cls.parse(msgspec.json.encode(config.config))
        return controller_cls(config=config, trader=trader)


class TradingNodeConfig(NautilusKernelConfig, frozen=True):
    """
    Configuration for ``TradingNode`` instances.

    Parameters
    ----------
    trader_id : TraderId, default "TRADER-000"
        The trader ID for the node (must be a name and ID tag separated by a hyphen).
    cache : CacheConfig, optional
        The cache configuration.
    data_engine : LiveDataEngineConfig, optional
        The live data engine configuration.
    risk_engine : LiveRiskEngineConfig, optional
        The live risk engine configuration.
    exec_engine : LiveExecEngineConfig, optional
        The live execution engine configuration.
    data_clients : dict[str, ImportableConfig | LiveDataClientConfig], optional
        The data client configurations.
    exec_clients : dict[str, ImportableConfig | LiveExecClientConfig], optional
        The execution client configurations.
    heartbeat_interval : PositiveFloat, optional
        The heartbeat interval (seconds) to use for trading node health.

    """

    environment: Environment = Environment.LIVE
    trader_id: TraderId = TraderId("TRADER-001")
    data_engine: LiveDataEngineConfig = LiveDataEngineConfig()
    risk_engine: LiveRiskEngineConfig = LiveRiskEngineConfig()
    exec_engine: LiveExecEngineConfig = LiveExecEngineConfig()
    data_clients: dict[str, LiveDataClientConfig] = {}
    exec_clients: dict[str, LiveExecClientConfig] = {}
    heartbeat_interval: PositiveFloat | None = None
