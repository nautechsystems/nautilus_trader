from __future__ import annotations

import asyncio
import importlib
from pathlib import Path

import pytest

from nautilus_trader.flux.execution.controller import ControllerCrashRecoveryAction
from nautilus_trader.flux.execution.controller import ControllerSnapshotAuthority
from nautilus_trader.flux.execution.controller import SnapshotAuthorityState
from nautilus_trader.flux.execution.controller import VenueActivityOrigin
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


def _load_ledger_module():
    path = _repo_root() / "systems/flux/flux/execution/ledger.py"
    assert path.exists(), "ownership ledger module should exist"
    return importlib.import_module("flux.execution.ledger")


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


class _RecordingMaterializer:
    def __init__(self) -> None:
        self.events = []

    async def materialize(self, event) -> None:
        self.events.append(event)


def test_owned_pre_write_replay_without_venue_evidence_stays_in_pre_write_recovery(
    tmp_path: Path,
) -> None:
    wal = _load_wal_module()
    ledger = _load_ledger_module()
    claim = _claim()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")
    materializer = _RecordingMaterializer()
    owned_ledger = ledger.ExecutionLedger(wal=store, materializer=materializer)

    try:
        store.append_claim(
            claim=claim,
            account_scope_id="ibkr.hedge.main",
            operation_type="submit",
            claim_key="submit:intent-001",
            authority=_authority(),
            appended_at_ns=111,
        )

        plan = asyncio.run(
            owned_ledger.recover(
                client_order_id=claim.client_order_id,
                venue_truth=None,
                recovered_at_ns=500,
            )
        )

        assert plan.classification is ledger.RecoveryClassification.PRE_WRITE_RECOVERY
        assert plan.lifecycle_state is ExecutionLifecycleState.OWNED_PRE_WRITE
        assert plan.crash_recovery_action is ControllerCrashRecoveryAction.RETRY_VENUE_WRITE
        assert plan.requires_fence_revalidation is True
        assert plan.should_send_to_venue is False
        assert plan.should_query_venue is False
        assert materializer.events == []
    finally:
        store.close()


def test_sent_to_venue_replay_without_venue_evidence_stays_pending_recovery(
    tmp_path: Path,
) -> None:
    wal = _load_wal_module()
    ledger = _load_ledger_module()
    claim = _claim()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")
    materializer = _RecordingMaterializer()
    owned_ledger = ledger.ExecutionLedger(wal=store, materializer=materializer)

    try:
        store.append_claim(
            claim=claim,
            account_scope_id="ibkr.hedge.main",
            operation_type="submit",
            claim_key="submit:intent-001",
            authority=_authority(),
            appended_at_ns=111,
        )
        store.record_venue_write(
            claim=claim,
            authority=_authority(),
            venue_order_id="venue-9001",
            written_at_ns=222,
        )

        plan = asyncio.run(
            owned_ledger.recover(
                client_order_id=claim.client_order_id,
                venue_truth=None,
                recovered_at_ns=500,
            )
        )

        assert plan.classification is ledger.RecoveryClassification.PENDING_RECOVERY
        assert plan.lifecycle_state is ExecutionLifecycleState.SENT_TO_VENUE
        assert plan.crash_recovery_action is ControllerCrashRecoveryAction.RECONCILE_BEFORE_RETRY
        assert plan.should_query_venue is True
        assert plan.should_send_to_venue is False
        assert materializer.events == []
    finally:
        store.close()


def test_matching_venue_truth_binds_and_materializes_without_reissuing_ownership(
    tmp_path: Path,
) -> None:
    wal = _load_wal_module()
    ledger = _load_ledger_module()
    claim = _claim()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")
    materializer = _RecordingMaterializer()
    owned_ledger = ledger.ExecutionLedger(wal=store, materializer=materializer)

    try:
        store.append_claim(
            claim=claim,
            account_scope_id="ibkr.hedge.main",
            operation_type="submit",
            claim_key="submit:intent-001",
            authority=_authority(),
            appended_at_ns=111,
        )
        store.record_venue_write(
            claim=claim,
            authority=_authority(),
            venue_order_id="venue-9001",
            written_at_ns=222,
        )

        plan = asyncio.run(
            owned_ledger.recover(
                client_order_id=claim.client_order_id,
                venue_truth=ledger.VenueTruth(
                    client_order_id=claim.client_order_id,
                    venue_order_id="venue-9001",
                    lifecycle_state=ExecutionLifecycleState.WORKING,
                    final_ack=True,
                ),
                recovered_at_ns=500,
            )
        )

        assert plan.classification is ledger.RecoveryClassification.BOUND_TO_VENUE
        assert plan.lifecycle_state is ExecutionLifecycleState.WORKING
        assert plan.should_send_to_venue is False
        assert plan.should_materialize is True
        assert [event.lifecycle_state for event in materializer.events] == [
            ExecutionLifecycleState.WORKING,
        ]
        history = store.list_records()
        assert len(history) == 4
        assert history[-1].lifecycle_state is ExecutionLifecycleState.WORKING
        assert history[-1].materialized_lifecycle_state is ExecutionLifecycleState.WORKING
        assert history[-1].venue_order_id == "venue-9001"
    finally:
        store.close()


def test_partial_venue_truth_without_final_ack_stays_pending_recovery_but_materializes_truth(
    tmp_path: Path,
) -> None:
    wal = _load_wal_module()
    ledger = _load_ledger_module()
    claim = _claim()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")
    materializer = _RecordingMaterializer()
    owned_ledger = ledger.ExecutionLedger(wal=store, materializer=materializer)

    try:
        store.append_claim(
            claim=claim,
            account_scope_id="ibkr.hedge.main",
            operation_type="submit",
            claim_key="submit:intent-001",
            authority=_authority(),
            appended_at_ns=111,
        )
        store.record_venue_write(
            claim=claim,
            authority=_authority(),
            venue_order_id="venue-9001",
            written_at_ns=222,
        )

        plan = asyncio.run(
            owned_ledger.recover(
                client_order_id=claim.client_order_id,
                venue_truth=ledger.VenueTruth(
                    client_order_id=claim.client_order_id,
                    venue_order_id="venue-9001",
                    lifecycle_state=ExecutionLifecycleState.PARTIALLY_FILLED,
                    final_ack=False,
                ),
                recovered_at_ns=500,
            )
        )

        assert plan.classification is ledger.RecoveryClassification.PENDING_RECOVERY
        assert plan.lifecycle_state is ExecutionLifecycleState.PARTIALLY_FILLED
        assert plan.should_query_venue is True
        assert plan.should_send_to_venue is False
        assert plan.should_materialize is True
        assert [event.lifecycle_state for event in materializer.events] == [
            ExecutionLifecycleState.PARTIALLY_FILLED,
        ]
        assert len(store.list_records()) == 4
    finally:
        store.close()


def test_terminal_venue_truth_materializes_the_missed_ack_from_venue_truth(
    tmp_path: Path,
) -> None:
    wal = _load_wal_module()
    ledger = _load_ledger_module()
    claim = _claim()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")
    materializer = _RecordingMaterializer()
    owned_ledger = ledger.ExecutionLedger(wal=store, materializer=materializer)

    try:
        store.append_claim(
            claim=claim,
            account_scope_id="ibkr.hedge.main",
            operation_type="submit",
            claim_key="submit:intent-001",
            authority=_authority(),
            appended_at_ns=111,
        )
        store.record_venue_write(
            claim=claim,
            authority=_authority(),
            venue_order_id="venue-9001",
            written_at_ns=222,
        )

        plan = asyncio.run(
            owned_ledger.recover(
                client_order_id=claim.client_order_id,
                venue_truth=ledger.VenueTruth(
                    client_order_id=claim.client_order_id,
                    venue_order_id="venue-9001",
                    lifecycle_state=ExecutionLifecycleState.FILLED,
                    final_ack=False,
                ),
                recovered_at_ns=500,
            )
        )

        assert plan.classification is ledger.RecoveryClassification.MATERIALIZED_FROM_VENUE
        assert plan.lifecycle_state is ExecutionLifecycleState.FILLED
        assert plan.should_send_to_venue is False
        assert plan.should_materialize is True
        assert [event.lifecycle_state for event in materializer.events] == [
            ExecutionLifecycleState.FILLED,
        ]
        history = store.list_records()
        assert len(history) == 4
        assert history[-1].materialized_lifecycle_state is ExecutionLifecycleState.FILLED
    finally:
        store.close()


def test_terminal_venue_truth_with_final_ack_releases_ownership(
    tmp_path: Path,
) -> None:
    wal = _load_wal_module()
    ledger = _load_ledger_module()
    claim = _claim()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")
    materializer = _RecordingMaterializer()
    owned_ledger = ledger.ExecutionLedger(wal=store, materializer=materializer)

    try:
        store.append_claim(
            claim=claim,
            account_scope_id="ibkr.hedge.main",
            operation_type="submit",
            claim_key="submit:intent-001",
            authority=_authority(),
            appended_at_ns=111,
        )
        store.record_venue_write(
            claim=claim,
            authority=_authority(),
            venue_order_id="venue-9001",
            written_at_ns=222,
        )

        plan = asyncio.run(
            owned_ledger.recover(
                client_order_id=claim.client_order_id,
                venue_truth=ledger.VenueTruth(
                    client_order_id=claim.client_order_id,
                    venue_order_id="venue-9001",
                    lifecycle_state=ExecutionLifecycleState.FILLED,
                    final_ack=True,
                ),
                recovered_at_ns=500,
            )
        )

        assert plan.classification is ledger.RecoveryClassification.BOUND_TO_VENUE
        assert plan.crash_recovery_action is ControllerCrashRecoveryAction.RELEASE_CLAIM
        assert plan.lifecycle_state is ExecutionLifecycleState.FILLED
        assert plan.should_query_venue is False
        assert plan.should_send_to_venue is False
        assert plan.should_materialize is True
        assert [event.lifecycle_state for event in materializer.events] == [
            ExecutionLifecycleState.FILLED,
        ]
    finally:
        store.close()


def test_venue_truth_without_matching_claim_tuple_is_quarantined_as_an_orphan(
    tmp_path: Path,
) -> None:
    wal = _load_wal_module()
    ledger = _load_ledger_module()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")
    materializer = _RecordingMaterializer()
    owned_ledger = ledger.ExecutionLedger(wal=store, materializer=materializer)

    try:
        plan = asyncio.run(
            owned_ledger.recover(
                client_order_id="acct.execution.main:7:42:intent-orphan",
                venue_truth=ledger.VenueTruth(
                    client_order_id="acct.execution.main:7:42:intent-orphan",
                    venue_order_id="venue-orphan",
                    lifecycle_state=ExecutionLifecycleState.WORKING,
                    final_ack=True,
                ),
                recovered_at_ns=500,
            )
        )

        assert plan.classification is ledger.RecoveryClassification.QUARANTINED_ORPHAN
        assert plan.lifecycle_state is ExecutionLifecycleState.QUARANTINED
        assert plan.venue_activity_origin is VenueActivityOrigin.ORPHAN
        assert "claim tuple" in (plan.reason or "")
        assert materializer.events == []
    finally:
        store.close()
