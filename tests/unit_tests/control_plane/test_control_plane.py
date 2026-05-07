# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from datetime import UTC
from datetime import datetime

import msgspec

from nautilus_trader.control_plane import ControlPlaneRuntimeState
from nautilus_trader.control_plane import SourceHealthState
from nautilus_trader.control_plane import SystemHealthState
from nautilus_trader.control_plane import TraderControlPlaneService
from nautilus_trader.control_plane import TradingPermissionState
from nautilus_trader.control_plane.service import map_system_health_state
from nautilus_trader.control_plane.service import map_trading_permission_state
from nautilus_trader.control_plane.service import snapshot_to_json


def test_health_state_mapping_normal_when_all_dependencies_are_ok() -> None:
    health = map_system_health_state(
        sources=(
            SourceHealthState(
                provider_id="databento",
                connectivity_state="connected",
                freshness_state="fresh",
            ),
        ),
        permission_state=TradingPermissionState.ALLOWED,
        event_bus_status="connected",
        execution_status="connected",
        data_status="connected",
        persistence_status="connected",
        incidents=(),
    )

    assert health == SystemHealthState.NORMAL


def test_health_state_mapping_halted_when_execution_is_halted() -> None:
    health = map_system_health_state(
        sources=(),
        permission_state=TradingPermissionState.BLOCKED,
        event_bus_status="connected",
        execution_status="halted",
        data_status="connected",
        persistence_status="connected",
        incidents=(),
    )

    assert health == SystemHealthState.HALTED


def test_permission_state_mapping_close_only_when_limit_breaches_exist() -> None:
    permission = map_trading_permission_state(
        kill_switch_state="armed",
        limit_breaches=("max-drawdown breached",),
        data_status="connected",
        execution_status="connected",
    )

    assert permission == TradingPermissionState.CLOSE_ONLY


def test_degraded_data_source_behavior_reduces_dashboard_health() -> None:
    service = TraderControlPlaneService(
        runtime_state_provider=lambda: ControlPlaneRuntimeState(
            source_health=(
                SourceHealthState(
                    provider_id="binance",
                    venue="BINANCE",
                    asset_classes=("spot",),
                    connectivity_state="connected",
                    freshness_state="stale",
                    degradation_reasons=("last trade tick exceeded freshness SLA",),
                ),
            ),
            event_bus_status="connected",
            execution_status="connected",
            data_status="connected",
            persistence_status="connected",
            kill_switch_state="armed",
        ),
    )

    snapshot = service.snapshot()

    assert snapshot.system_health == SystemHealthState.DEGRADED
    assert snapshot.ai_assistant_context.degraded_sources == ("binance",)


def test_halted_blocked_risk_state_behavior() -> None:
    service = TraderControlPlaneService(
        runtime_state_provider=lambda: ControlPlaneRuntimeState(
            source_health=(
                SourceHealthState(
                    provider_id="internal",
                    connectivity_state="connected",
                    freshness_state="fresh",
                ),
            ),
            event_bus_status="connected",
            execution_status="connected",
            data_status="connected",
            persistence_status="connected",
            kill_switch_state="kill_switch",
            incidents=("operator kill switch engaged",),
        ),
    )

    snapshot = service.snapshot()

    assert snapshot.system_health == SystemHealthState.HALTED
    assert snapshot.trading_permission == TradingPermissionState.BLOCKED
    assert "Keep trading blocked" in snapshot.risk_cockpit.recommended_operator_action


def test_dashboard_snapshot_serialization() -> None:
    service = TraderControlPlaneService(
        runtime_state_provider=lambda: ControlPlaneRuntimeState(
            source_health=(
                SourceHealthState(
                    provider_id="databento",
                    venue="XNAS",
                    asset_classes=("equity",),
                    connectivity_state="connected",
                    freshness_state="fresh",
                ),
            ),
            event_bus_status="connected",
            execution_status="connected",
            data_status="connected",
            persistence_status="connected",
            kill_switch_state="armed",
        ),
        clock=lambda: datetime(2026, 1, 1, tzinfo=UTC),
    )

    raw = snapshot_to_json(service.snapshot())
    decoded = msgspec.json.decode(raw.encode())

    assert decoded["timestamp"] == "2026-01-01T00:00:00+00:00"
    assert decoded["system_health"] == "normal"
    assert decoded["trading_permission"] == "allowed"
    assert decoded["source_health"][0]["provider_id"] == "databento"


def test_unknown_missing_runtime_data_is_explicitly_degraded() -> None:
    snapshot = TraderControlPlaneService(clock=lambda: datetime(2026, 1, 1, tzinfo=UTC)).snapshot()

    assert snapshot.system_health == SystemHealthState.DEGRADED
    assert snapshot.trading_permission == TradingPermissionState.REDUCED_SIGNALS
    assert "Runtime telemetry provider is not configured." in snapshot.ops_cockpit.warnings
    assert "No source-health telemetry is available." in snapshot.ops_cockpit.warnings
