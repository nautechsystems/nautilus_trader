# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from enum import StrEnum
from typing import Any

import msgspec


class SystemHealthState(StrEnum):
    """Trader-facing aggregate health state for the control plane."""

    NORMAL = "normal"
    DEGRADED = "degraded"
    AT_RISK = "atRisk"
    HALTED = "halted"


class TradingPermissionState(StrEnum):
    """Trader-facing permission state for opening or managing trading risk."""

    ALLOWED = "allowed"
    REDUCED_SIGNALS = "reducedSignals"
    CLOSE_ONLY = "closeOnly"
    BLOCKED = "blocked"


class SourceHealthState(msgspec.Struct, frozen=True, kw_only=True):
    provider_id: str
    venue: str = "unknown"
    asset_classes: tuple[str, ...] = ()
    connectivity_state: str = "unknown"
    freshness_state: str = "unknown"
    latency_ms: float | None = None
    last_heartbeat: str | None = None
    last_successful_update: str | None = None
    degradation_reasons: tuple[str, ...] = ()


class RiskCockpitState(msgspec.Struct, frozen=True, kw_only=True):
    permission_state: TradingPermissionState
    exposure_summary: dict[str, Any] = msgspec.field(default_factory=dict)
    drawdown_summary: dict[str, Any] = msgspec.field(default_factory=dict)
    limit_breaches: tuple[str, ...] = ()
    kill_switch_state: str = "unknown"
    recommended_operator_action: str = "Review unavailable risk telemetry before increasing exposure."


class OpsCockpitState(msgspec.Struct, frozen=True, kw_only=True):
    system_health: SystemHealthState
    active_strategies: tuple[str, ...] = ()
    active_venues: tuple[str, ...] = ()
    event_bus_status: str = "unknown"
    execution_status: str = "unknown"
    data_status: str = "unknown"
    persistence_status: str = "unknown"
    warnings: tuple[str, ...] = ()
    incidents: tuple[str, ...] = ()


class AIAssistantContext(msgspec.Struct, frozen=True, kw_only=True):
    current_health_summary: str
    current_risk_summary: str
    active_incidents: tuple[str, ...] = ()
    degraded_sources: tuple[str, ...] = ()
    blocked_or_reduced_trading_reasons: tuple[str, ...] = ()
    recommended_explanation: str


class TraderDashboardSnapshot(msgspec.Struct, frozen=True, kw_only=True):
    timestamp: str
    system_health: SystemHealthState
    trading_permission: TradingPermissionState
    source_health: tuple[SourceHealthState, ...]
    risk_cockpit: RiskCockpitState
    ops_cockpit: OpsCockpitState
    ai_assistant_context: AIAssistantContext
