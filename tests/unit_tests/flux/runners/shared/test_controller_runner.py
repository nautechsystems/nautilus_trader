from __future__ import annotations

import importlib
from pathlib import Path

import pytest


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[5]


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def _load_leases_module():
    path = _repo_root() / "systems/flux/flux/execution/leases.py"
    assert path.exists(), "controller lease module should exist"
    return importlib.import_module("flux.execution.leases")


def _load_runner_module():
    path = _repo_root() / "systems/flux/flux/runners/shared/controller_runner.py"
    assert path.exists(), "controller runner module should exist"
    return importlib.import_module("flux.runners.shared.controller_runner")


class _RecordingControllerService:
    def __init__(self) -> None:
        self.started = 0
        self.stopped = 0

    def start(self) -> None:
        self.started += 1

    def stop(self) -> None:
        self.stopped += 1


def test_shadow_controller_runner_starts_and_stops_with_a_local_lease(tmp_path: Path) -> None:
    leases = _load_leases_module()
    runner_module = _load_runner_module()
    service = _RecordingControllerService()
    lease_store = leases.LocalControllerLeaseStore(root_dir=tmp_path / "leases")
    runner = runner_module.ShadowControllerRunner(
        config=runner_module.ControllerRunnerConfig(
            controller_scope_id="acct.execution.main",
            owner_id="controller-a",
            allow_single_host_canary=True,
            lease_ttl_ms=250,
        ),
        lease_store=lease_store,
        controller_service=service,
    )

    runner.start(now_ms=1_000)

    assert runner.running is True
    assert service.started == 1
    assert lease_store.current("acct.execution.main").owner_id == "controller-a"

    runner.stop()

    assert runner.running is False
    assert service.stopped == 1
    assert lease_store.current("acct.execution.main") is None


def test_shadow_controller_runner_rejects_a_stale_local_writer(tmp_path: Path) -> None:
    leases = _load_leases_module()
    runner_module = _load_runner_module()
    runner = runner_module.ShadowControllerRunner(
        config=runner_module.ControllerRunnerConfig(
            controller_scope_id="acct.execution.main",
            owner_id="controller-a",
            allow_single_host_canary=True,
            lease_ttl_ms=250,
        ),
        lease_store=leases.LocalControllerLeaseStore(root_dir=tmp_path / "leases"),
        controller_service=_RecordingControllerService(),
    )

    runner.start(now_ms=1_000)
    runner.assert_can_write(now_ms=1_200)

    with pytest.raises(leases.StaleControllerWriterError):
        runner.assert_can_write(now_ms=1_251)


def test_shadow_controller_runner_blocks_duplicate_start_after_ttl_while_running(tmp_path: Path) -> None:
    leases = _load_leases_module()
    runner_module = _load_runner_module()
    first_service = _RecordingControllerService()
    second_service = _RecordingControllerService()
    lease_store = leases.LocalControllerLeaseStore(root_dir=tmp_path / "leases")
    first = runner_module.ShadowControllerRunner(
        config=runner_module.ControllerRunnerConfig(
            controller_scope_id="acct.execution.main",
            owner_id="controller-a",
            allow_single_host_canary=True,
            lease_ttl_ms=250,
        ),
        lease_store=lease_store,
        controller_service=first_service,
    )
    second = runner_module.ShadowControllerRunner(
        config=runner_module.ControllerRunnerConfig(
            controller_scope_id="acct.execution.main",
            owner_id="controller-b",
            allow_single_host_canary=True,
            lease_ttl_ms=250,
        ),
        lease_store=lease_store,
        controller_service=second_service,
    )

    first.start(now_ms=1_000)

    with pytest.raises(leases.ControllerLeaseRejectedError, match="already running"):
        second.start(now_ms=1_251)

    assert first.running is True
    assert second.running is False
    assert first_service.started == 1
    assert second_service.started == 0

    first.stop()
