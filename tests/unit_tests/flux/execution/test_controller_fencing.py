from __future__ import annotations

import importlib
from pathlib import Path

import pytest

from nautilus_trader.flux.execution.controller import ControllerSnapshotAuthority
from nautilus_trader.flux.execution.controller import SnapshotAuthorityState
from nautilus_trader.flux.execution.intents import ExecutionIntent
from nautilus_trader.flux.execution.leases import ControllerLease


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def _load_controller_module():
    path = _repo_root() / "systems/flux/flux/execution/controller.py"
    assert path.exists(), "controller module should exist"
    return importlib.import_module("flux.execution.controller")


def _claim():
    return ExecutionIntent(
        intent_id="intent-001",
        controller_scope_id="ibkr.hedge.main",
        strategy_id="strategy-01",
    ).claim(controller_epoch=7, controller_seq=42)


def _authority(
    *,
    controller_epoch: int = 7,
    controller_seq: int = 42,
    authority_state: SnapshotAuthorityState = SnapshotAuthorityState.AUTHORITATIVE,
) -> ControllerSnapshotAuthority:
    return ControllerSnapshotAuthority(
        controller_scope_id="ibkr.hedge.main",
        controller_epoch=controller_epoch,
        controller_seq=controller_seq,
        snapshot_ts_ms=1_000,
        stale_after_ms=250,
        authority_state=authority_state,
    )


def _lease(*, token: str = "lease-a", refreshed_at_ms: int = 1_000, ttl_ms: int = 250) -> ControllerLease:
    return ControllerLease(
        controller_scope_id="ibkr.hedge.main",
        owner_id="controller-a",
        lease_token=token,
        acquired_at_ms=900,
        refreshed_at_ms=refreshed_at_ms,
        lease_ttl_ms=ttl_ms,
    )


def test_assert_controller_write_fence_rejects_epoch_regression_before_outbound_write() -> None:
    controller = _load_controller_module()

    with pytest.raises(controller.ControllerFenceViolation, match="controller epoch mismatch"):
        controller.assert_controller_write_fence(
            claim=_claim(),
            authority=_authority(controller_epoch=8),
            lease=_lease(),
            now_ms=1_100,
        )


def test_assert_controller_write_fence_rejects_stale_lease_before_outbound_write() -> None:
    controller = _load_controller_module()

    with pytest.raises(controller.ControllerFenceViolation, match="lease is stale"):
        controller.assert_controller_write_fence(
            claim=_claim(),
            authority=_authority(),
            lease=_lease(refreshed_at_ms=1_000, ttl_ms=250),
            now_ms=1_251,
        )


def test_assert_single_writer_rejects_overlapping_live_leases_for_same_scope() -> None:
    controller = _load_controller_module()

    with pytest.raises(controller.ControllerFenceViolation, match="split-brain"):
        controller.assert_single_writer(
            controller_scope_id="ibkr.hedge.main",
            leases=(
                _lease(token="lease-a", refreshed_at_ms=1_000),
                _lease(token="lease-b", refreshed_at_ms=1_050),
            ),
            now_ms=1_100,
        )


def test_assert_stale_writer_stop_budget_rejects_over_threshold_stop_latency() -> None:
    controller = _load_controller_module()

    with pytest.raises(controller.ControllerFenceViolation, match="stop latency"):
        controller.assert_stale_writer_stop_budget(
            lease_lost_at_ms=1_000,
            stopped_at_ms=1_251,
        )
