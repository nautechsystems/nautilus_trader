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
    emitted from the venue may have been lost at some point - leaving an order
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
        If position status reports are filtered from reconciliation.
        This may be applicable when other nodes are trading the same instrument(s), on the same
        account - which could cause conflicts in position status.
    generate_missing_orders : bool, default True
        If MARKET order events will be generated during reconciliation to align discrepancies
        between internal and external positions.
    inflight_check_interval_ms : NonNegativeInt, default 2_000
        The interval (milliseconds) between checking whether in-flight orders
        have exceeded their time-in-flight threshold.
        This should not be set less than the `inflight_check_threshold_ms`.
    inflight_check_threshold_ms : NonNegativeInt, default 5_000
        The threshold (milliseconds) beyond which an in-flight orders status is checked with the venue.
        As a rule of thumb, you shouldn't consider reducing this setting unless you
        are colocated with the venue (to avoid the potential for race conditions).
    inflight_check_retries : NonNegativeInt, default 5
        The number of retry attempts the engine will make to verify the status of an
        in-flight order with the venue, should the initial attempt fail.
    own_books_audit_interval_secs : NonNegativeFloat, optional
        The interval (seconds) between auditing all own books against public order books.
        The audit will ensure all order statuses are in sync and that no closed orders remain in
        an own book. Logs all failures as errors.
    open_check_interval_secs : PositiveFloat, optional
        The interval (seconds) between checks for open orders at the venue.
        If there is a discrepancy then an order status report is generated and reconciled.
        A recommended setting is between 5-10 seconds, consider API rate limits and the additional
        request weights.
        If no value is specified then the open order checking task is not started.
    open_check_open_only : bool, default True
        If True, the **check_open_orders** requests only currently open orders from the venue.
        If False, it requests the entire order history, which can be a heavy API call.
        This parameter only applies if the **check_open_orders** task is running.
    purge_closed_orders_interval_mins : PositiveInt, optional
        The interval (minutes) between purging closed orders from the in-memory cache,
        **will not purge from the database**. If None, closed orders will **not** be automatically purged.
        A recommended setting is 10-15 minutes for HFT.
    purge_closed_orders_buffer_mins : NonNegativeInt, optional
        The time buffer (minutes) from when an order was closed before it can be purged.
        Only orders closed for at least this amount of time will be purged.
        A recommended setting is 60 minutes for HFT.
    purge_closed_positions_interval_mins : PositiveInt, optional
        The interval (minutes) between purging closed positions from the in-memory cache,
        **will not purge from the database**. If None, closed positions will **not** be automatically purged.
        A recommended setting is 10-15 minutes for HFT.
    purge_closed_positions_buffer_mins : NonNegativeInt, optional
        The time buffer (minutes) from when a position was closed before it can be purged.
        Only positions closed for at least this amount of time will be purged.
        A recommended setting is 60 minutes for HFT.
    purge_account_events_interval_mins : PositiveInt, optional
        The interval (minutes) between purging account events from the in-memory cache,
        **will not purge from the database**. If None, closed orders will **not** be automatically purged.
        A recommended setting is 10-15 minutes for HFT.
    purge_account_events_lookback_mins : NonNegativeInt, optional
        The time buffer (minutes) from when an account event occurred before it can be purged.
        Only events outside the lookback window will be purged.
        A recommended setting is 60 minutes for HFT.
    qsize : PositiveInt, default 100_000
        The queue size for the engines internal queue buffers.

    """

    reconciliation: bool = True
    reconciliation_lookback_mins: NonNegativeInt | None = None
    filter_unclaimed_external_orders: bool = False
    filter_position_reports: bool = False
    generate_missing_orders: bool = True
    inflight_check_interval_ms: NonNegativeInt = 2_000
    inflight_check_threshold_ms: NonNegativeInt = 5_000
    inflight_check_retries: NonNegativeInt = 5
    own_books_audit_interval_secs: PositiveFloat | None = None
    open_check_interval_secs: PositiveFloat | None = None
    open_check_open_only: bool = True
    purge_closed_orders_interval_mins: PositiveInt | None = None
    purge_closed_orders_buffer_mins: NonNegativeInt | None = None
    purge_closed_positions_interval_mins: PositiveInt | None = None
    purge_closed_positions_buffer_mins: NonNegativeInt | None = None
    purge_account_events_interval_mins: PositiveInt | None = None
    purge_account_events_lookback_mins: NonNegativeInt | None = None
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

    """

    environment: Environment = Environment.LIVE
    trader_id: TraderId = TraderId("TRADER-001")
    data_engine: LiveDataEngineConfig = LiveDataEngineConfig()
    risk_engine: LiveRiskEngineConfig = LiveRiskEngineConfig()
    exec_engine: LiveExecEngineConfig = LiveExecEngineConfig()
    data_clients: dict[str, LiveDataClientConfig] = {}
    exec_clients: dict[str, LiveExecClientConfig] = {}
