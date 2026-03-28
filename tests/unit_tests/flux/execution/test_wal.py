from __future__ import annotations

import importlib
from pathlib import Path

import pytest

from nautilus_trader.flux.execution.controller import ControllerSnapshotAuthority
from nautilus_trader.flux.execution.controller import SnapshotAuthorityState
from nautilus_trader.flux.execution.intents import ExecutionIntent
from nautilus_trader.flux.execution.intents import ExecutionLifecycleState


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def _load_wal_module():
    path = _repo_root() / "systems/flux/flux/execution/wal.py"
    assert path.exists(), "ownership wal module should exist"
    return importlib.import_module("flux.execution.wal")


def _authority(*, controller_epoch: int = 7, controller_seq: int = 42) -> ControllerSnapshotAuthority:
    return ControllerSnapshotAuthority(
        controller_scope_id="acct.execution.main",
        controller_epoch=controller_epoch,
        controller_seq=controller_seq,
        snapshot_ts_ms=1_000,
        stale_after_ms=250,
        authority_state=SnapshotAuthorityState.AUTHORITATIVE,
    )


def _claim():
    return ExecutionIntent(
        intent_id="intent-001",
        controller_scope_id="acct.execution.main",
        strategy_id="strategy-01",
    ).claim(controller_epoch=7, controller_seq=42)


def test_sqlite_ownership_wal_uses_full_sync_and_persists_owned_pre_write_claim(
    tmp_path: Path,
) -> None:
    wal = _load_wal_module()
    claim = _claim()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")

    try:
        record = store.append_claim(
            claim=claim,
            authority=_authority(),
            appended_at_ns=111,
        )

        assert record.claim == claim
        assert record.lifecycle_state is ExecutionLifecycleState.OWNED_PRE_WRITE
        assert record.materialized_lifecycle_state is None
        assert store.fetch_by_intent_id(claim.intent_id) == record
        assert store.connection.execute("PRAGMA journal_mode;").fetchone()[0].lower() == "wal"
        assert store.connection.execute("PRAGMA synchronous;").fetchone()[0] == 2
    finally:
        store.close()


def test_append_claim_rejects_controller_epoch_fence_regression(tmp_path: Path) -> None:
    wal = _load_wal_module()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")

    try:
        with pytest.raises(wal.FenceRejectedError, match="before append"):
            store.append_claim(
                claim=_claim(),
                authority=_authority(controller_epoch=8, controller_seq=42),
                appended_at_ns=111,
            )
    finally:
        store.close()


def test_record_venue_write_rechecks_the_fence_and_advances_to_sent_to_venue(
    tmp_path: Path,
) -> None:
    wal = _load_wal_module()
    claim = _claim()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")

    try:
        store.append_claim(
            claim=claim,
            authority=_authority(),
            appended_at_ns=111,
        )

        with pytest.raises(wal.FenceRejectedError, match="before venue write"):
            store.record_venue_write(
                claim=claim,
                authority=_authority(controller_epoch=8, controller_seq=42),
                venue_order_id="venue-9001",
                written_at_ns=222,
            )

        record = store.record_venue_write(
            claim=claim,
            authority=_authority(),
            venue_order_id="venue-9001",
            written_at_ns=222,
        )

        assert record.lifecycle_state is ExecutionLifecycleState.SENT_TO_VENUE
        assert record.venue_order_id == "venue-9001"
        assert store.fetch_by_client_order_id(claim.client_order_id) == record
    finally:
        store.close()
