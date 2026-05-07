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

from collections.abc import Callable
from datetime import UTC
from datetime import datetime
from typing import Any

import msgspec

from nautilus_trader.control_plane.models import AIAssistantContext
from nautilus_trader.control_plane.models import OpsCockpitState
from nautilus_trader.control_plane.models import RiskCockpitState
from nautilus_trader.control_plane.models import SourceHealthState
from nautilus_trader.control_plane.models import SystemHealthState
from nautilus_trader.control_plane.models import TraderDashboardSnapshot
from nautilus_trader.control_plane.models import TradingPermissionState


_HALTED_STATUSES = frozenset({"halted", "stopped", "offline", "down", "failed", "blocked"})
_DEGRADED_STATUSES = frozenset({"degraded", "stale", "unknown", "reconnecting", "delayed", "partial"})
_REDUCED_STATUSES = frozenset({"reduced", "reducedsignals", "reduce_only", "limited"})
_CLOSE_ONLY_STATUSES = frozenset({"closeonly", "close_only", "closing_only", "liquidation_only"})
_BLOCKED_STATUSES = frozenset({"blocked", "halted", "disabled", "kill_switch", "suspended"})


class ControlPlaneRuntimeState(msgspec.Struct, frozen=True, kw_only=True):
    """
    Read-only normalized telemetry supplied by Nautilus runtime components.

    This structure intentionally contains already-observed state only. The control plane service
    consumes it to build trader-facing snapshots and never calls order, strategy, or risk mutation
    methods.
    """

    source_health: tuple[SourceHealthState, ...] = ()
    active_strategies: tuple[str, ...] = ()
    active_venues: tuple[str, ...] = ()
    event_bus_status: str = "unknown"
    execution_status: str = "unknown"
    data_status: str = "unknown"
    persistence_status: str = "unknown"
    exposure_summary: dict[str, Any] = msgspec.field(default_factory=dict)
    drawdown_summary: dict[str, Any] = msgspec.field(default_factory=dict)
    limit_breaches: tuple[str, ...] = ()
    kill_switch_state: str = "unknown"
    permission_state: TradingPermissionState | None = None
    warnings: tuple[str, ...] = ()
    incidents: tuple[str, ...] = ()


class TraderControlPlaneService:
    """Read-only builder for trader dashboard snapshots."""

    def __init__(
        self,
        runtime_state_provider: Callable[[], ControlPlaneRuntimeState | None] | None = None,
        clock: Callable[[], datetime] | None = None,
    ) -> None:
        self._runtime_state_provider = runtime_state_provider
        self._clock = clock or (lambda: datetime.now(UTC))

    def snapshot(self) -> TraderDashboardSnapshot:
        """Build a read-only dashboard snapshot from currently available runtime telemetry."""
        runtime_state = self._read_runtime_state()
        source_health = runtime_state.source_health
        warnings = tuple(runtime_state.warnings)
        incidents = tuple(runtime_state.incidents)

        if not source_health:
            warnings += ("No source-health telemetry is available.",)

        permission_state = runtime_state.permission_state or map_trading_permission_state(
            kill_switch_state=runtime_state.kill_switch_state,
            limit_breaches=runtime_state.limit_breaches,
            data_status=runtime_state.data_status,
            execution_status=runtime_state.execution_status,
        )
        system_health = map_system_health_state(
            sources=source_health,
            permission_state=permission_state,
            event_bus_status=runtime_state.event_bus_status,
            execution_status=runtime_state.execution_status,
            data_status=runtime_state.data_status,
            persistence_status=runtime_state.persistence_status,
            incidents=incidents,
        )
        risk_cockpit = RiskCockpitState(
            permission_state=permission_state,
            exposure_summary=runtime_state.exposure_summary,
            drawdown_summary=runtime_state.drawdown_summary,
            limit_breaches=runtime_state.limit_breaches,
            kill_switch_state=runtime_state.kill_switch_state,
            recommended_operator_action=recommend_operator_action(
                permission_state=permission_state,
                system_health=system_health,
                limit_breaches=runtime_state.limit_breaches,
                kill_switch_state=runtime_state.kill_switch_state,
            ),
        )
        ops_cockpit = OpsCockpitState(
            system_health=system_health,
            active_strategies=runtime_state.active_strategies,
            active_venues=runtime_state.active_venues,
            event_bus_status=runtime_state.event_bus_status,
            execution_status=runtime_state.execution_status,
            data_status=runtime_state.data_status,
            persistence_status=runtime_state.persistence_status,
            warnings=warnings,
            incidents=incidents,
        )
        ai_context = build_ai_assistant_context(
            system_health=system_health,
            risk_cockpit=risk_cockpit,
            ops_cockpit=ops_cockpit,
            source_health=source_health,
        )
        return TraderDashboardSnapshot(
            timestamp=self._clock().isoformat(),
            system_health=system_health,
            trading_permission=permission_state,
            source_health=source_health,
            risk_cockpit=risk_cockpit,
            ops_cockpit=ops_cockpit,
            ai_assistant_context=ai_context,
        )

    def _read_runtime_state(self) -> ControlPlaneRuntimeState:
        if self._runtime_state_provider is None:
            return ControlPlaneRuntimeState(
                warnings=("Runtime telemetry provider is not configured.",),
            )

        runtime_state = self._runtime_state_provider()
        if runtime_state is None:
            return ControlPlaneRuntimeState(
                warnings=("Runtime telemetry provider returned no data.",),
            )

        return runtime_state


def map_system_health_state(
    *,
    sources: tuple[SourceHealthState, ...],
    permission_state: TradingPermissionState,
    event_bus_status: str,
    execution_status: str,
    data_status: str,
    persistence_status: str,
    incidents: tuple[str, ...],
) -> SystemHealthState:
    statuses = {event_bus_status, execution_status, data_status, persistence_status}
    normalized_statuses = {_normalize_status(status) for status in statuses}
    if permission_state == TradingPermissionState.BLOCKED or normalized_statuses & _HALTED_STATUSES:
        return SystemHealthState.HALTED
    if incidents or permission_state == TradingPermissionState.CLOSE_ONLY:
        return SystemHealthState.AT_RISK
    if permission_state == TradingPermissionState.REDUCED_SIGNALS:
        return SystemHealthState.DEGRADED
    if not sources or normalized_statuses & _DEGRADED_STATUSES:
        return SystemHealthState.DEGRADED
    if any(is_source_degraded(source) for source in sources):
        return SystemHealthState.DEGRADED
    return SystemHealthState.NORMAL


def map_trading_permission_state(
    *,
    kill_switch_state: str,
    limit_breaches: tuple[str, ...],
    data_status: str,
    execution_status: str,
) -> TradingPermissionState:
    normalized_kill_switch = _normalize_status(kill_switch_state)
    normalized_data = _normalize_status(data_status)
    normalized_execution = _normalize_status(execution_status)

    if normalized_kill_switch in _BLOCKED_STATUSES or normalized_execution in _BLOCKED_STATUSES:
        return TradingPermissionState.BLOCKED
    if normalized_kill_switch in _CLOSE_ONLY_STATUSES or limit_breaches:
        return TradingPermissionState.CLOSE_ONLY
    if normalized_data in _HALTED_STATUSES:
        return TradingPermissionState.CLOSE_ONLY
    if (
        normalized_kill_switch in _REDUCED_STATUSES
        or normalized_data in _DEGRADED_STATUSES
        or normalized_execution in _DEGRADED_STATUSES
    ):
        return TradingPermissionState.REDUCED_SIGNALS
    return TradingPermissionState.ALLOWED


def is_source_degraded(source: SourceHealthState) -> bool:
    if source.degradation_reasons:
        return True

    connectivity_state = _normalize_status(source.connectivity_state)
    freshness_state = _normalize_status(source.freshness_state)
    return connectivity_state in _HALTED_STATUSES | _DEGRADED_STATUSES or freshness_state in _DEGRADED_STATUSES


def recommend_operator_action(
    *,
    permission_state: TradingPermissionState,
    system_health: SystemHealthState,
    limit_breaches: tuple[str, ...],
    kill_switch_state: str,
) -> str:
    if permission_state == TradingPermissionState.BLOCKED:
        return "Keep trading blocked; verify kill switch, execution connectivity, and active incidents."
    if permission_state == TradingPermissionState.CLOSE_ONLY:
        if limit_breaches:
            return "Operate close-only and resolve breached limits before allowing new risk."
        return "Operate close-only until halted data or execution dependencies recover."
    if permission_state == TradingPermissionState.REDUCED_SIGNALS:
        return "Reduce signal intake and monitor degraded dependencies before adding exposure."
    if system_health == SystemHealthState.NORMAL and _normalize_status(kill_switch_state) in {"armed", "ok", "normal"}:
        return "Trading may continue; monitor standard health and risk controls."
    return "Trading may continue with operator review of unknown or degraded telemetry."


def build_ai_assistant_context(
    *,
    system_health: SystemHealthState,
    risk_cockpit: RiskCockpitState,
    ops_cockpit: OpsCockpitState,
    source_health: tuple[SourceHealthState, ...],
) -> AIAssistantContext:
    degraded_sources = tuple(
        source.provider_id for source in source_health if is_source_degraded(source)
    )
    blocked_or_reduced_reasons = tuple(risk_cockpit.limit_breaches) + tuple(
        warning for warning in ops_cockpit.warnings if "telemetry" in warning.lower()
    )
    return AIAssistantContext(
        current_health_summary=f"System health is {system_health.value}.",
        current_risk_summary=f"Trading permission is {risk_cockpit.permission_state.value}; kill switch is {risk_cockpit.kill_switch_state}.",
        active_incidents=ops_cockpit.incidents,
        degraded_sources=degraded_sources,
        blocked_or_reduced_trading_reasons=blocked_or_reduced_reasons,
        recommended_explanation=(
            f"{risk_cockpit.recommended_operator_action} "
            "This context is deterministic and does not call an LLM provider."
        ),
    )


def snapshot_to_json(snapshot: TraderDashboardSnapshot) -> str:
    return msgspec.json.encode(snapshot).decode()


def _normalize_status(status: str | None) -> str:
    return (status or "unknown").strip().lower()
