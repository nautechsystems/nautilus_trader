from __future__ import annotations

import importlib
from pathlib import Path

import pytest


class _RecordingControllerService:
    def __init__(self) -> None:
        self.started = 0
        self.stopped = 0

    def start(self) -> None:
        self.started += 1

    def stop(self) -> None:
        self.stopped += 1


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _load_run_controller_module():
    path = _repo_root() / "systems/flux/flux/runners/equities/run_controller.py"
    assert path.exists(), "equities controller runner module should exist"
    return importlib.import_module("flux.runners.equities.run_controller")


def test_build_runner_requires_explicit_single_host_canary_gate(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()

    with pytest.raises(ValueError, match="single-host canary"):
        run_controller.build_runner(
            {
                "controller": {
                    "controller_scope_id": "acct.execution.main",
                },
            },
            owner_id="controller-a",
            repo_root=tmp_path,
        )


def test_build_runner_defaults_shadow_mode_and_lease_root(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()
    service = _RecordingControllerService()
    runner = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
            },
        },
        owner_id="controller-a",
        repo_root=tmp_path,
        controller_service_factory=lambda _config: service,
    )

    assert runner.config.run_mode is run_controller.ControllerRunMode.SHADOW
    assert runner.config.controller_scope_id == "acct.execution.main"
    assert runner.lease_store.root_dir == tmp_path / ".run" / "equities-controller-leases"

    runner.start(now_ms=1_000)
    runner.stop()

    assert service.started == 1
    assert service.stopped == 1
