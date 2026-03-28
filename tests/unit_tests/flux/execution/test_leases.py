from __future__ import annotations

import importlib
from pathlib import Path

import pytest


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def _load_leases_module():
    path = _repo_root() / "systems/flux/flux/execution/leases.py"
    assert path.exists(), "controller lease module should exist"
    return importlib.import_module("flux.execution.leases")


def test_local_controller_leases_only_allow_one_active_owner_per_scope(tmp_path: Path) -> None:
    leases = _load_leases_module()
    store = leases.LocalControllerLeaseStore(root_dir=tmp_path / "leases")

    lease = store.acquire(
        controller_scope_id="acct.execution.main",
        owner_id="controller-a",
        now_ms=1_000,
        lease_ttl_ms=250,
    )

    assert lease.owner_id == "controller-a"
    assert store.current("acct.execution.main").lease_token == lease.lease_token

    with pytest.raises(leases.ControllerLeaseRejectedError, match="already owned"):
        store.acquire(
            controller_scope_id="acct.execution.main",
            owner_id="controller-b",
            now_ms=1_100,
            lease_ttl_ms=250,
        )


def test_local_controller_leases_reject_duplicate_fresh_acquire_for_same_owner(
    tmp_path: Path,
) -> None:
    leases = _load_leases_module()
    store = leases.LocalControllerLeaseStore(root_dir=tmp_path / "leases")

    first = store.acquire(
        controller_scope_id="acct.execution.main",
        owner_id="controller-a",
        now_ms=1_000,
        lease_ttl_ms=250,
    )

    with pytest.raises(leases.ControllerLeaseRejectedError, match="already owned"):
        store.acquire(
            controller_scope_id="acct.execution.main",
            owner_id="controller-a",
            now_ms=1_100,
            lease_ttl_ms=250,
        )

    refreshed = store.refresh(
        controller_scope_id="acct.execution.main",
        lease_token=first.lease_token,
        now_ms=1_100,
    )

    assert refreshed.lease_token == first.lease_token
    assert refreshed.refreshed_at_ms == 1_100


def test_local_controller_leases_reject_stale_writer_after_expiry(tmp_path: Path) -> None:
    leases = _load_leases_module()
    store = leases.LocalControllerLeaseStore(root_dir=tmp_path / "leases")

    first = store.acquire(
        controller_scope_id="acct.execution.main",
        owner_id="controller-a",
        now_ms=1_000,
        lease_ttl_ms=250,
    )

    store.assert_can_write(
        controller_scope_id="acct.execution.main",
        lease_token=first.lease_token,
        now_ms=1_200,
    )

    with pytest.raises(leases.StaleControllerWriterError):
        store.assert_can_write(
            controller_scope_id="acct.execution.main",
            lease_token=first.lease_token,
            now_ms=1_251,
        )

    replacement = store.acquire(
        controller_scope_id="acct.execution.main",
        owner_id="controller-b",
        now_ms=1_251,
        lease_ttl_ms=250,
    )

    assert replacement.owner_id == "controller-b"
    assert replacement.lease_token != first.lease_token
