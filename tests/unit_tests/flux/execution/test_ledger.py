from __future__ import annotations

import asyncio
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


def test_write_owned_order_appends_durably_before_the_owned_venue_send(tmp_path: Path) -> None:
    wal = _load_wal_module()
    ledger = _load_ledger_module()
    claim = _claim()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")
    materializer = _RecordingMaterializer()
    owned_ledger = ledger.ExecutionLedger(wal=store, materializer=materializer)
    seen_lifecycle_states = []

    class _VenueWriter:
        async def write_owned_order(self, claim_to_send):
            persisted = store.fetch_by_intent_id(claim_to_send.intent_id)
            seen_lifecycle_states.append(None if persisted is None else persisted.lifecycle_state.value)
            return "venue-9001"

    try:
        record = asyncio.run(
            owned_ledger.write_owned_order(
                claim=claim,
                account_scope_id="ibkr.hedge.main",
                operation_type="submit",
                claim_key="submit:intent-001",
                append_authority=_authority(),
                write_authority=_authority(),
                venue_writer=_VenueWriter(),
                written_at_ns=222,
            )
        )

        assert seen_lifecycle_states == ["owned_pre_write"]
        assert record.lifecycle_state is ExecutionLifecycleState.SENT_TO_VENUE
        assert record.venue_order_id == "venue-9001"
        assert record.account_scope_id == "ibkr.hedge.main"
        assert record.operation_type == "submit"
        assert record.claim_key == "submit:intent-001"
        history = store.list_records()
        assert len(history) == 2
        assert history[0].lifecycle_state is ExecutionLifecycleState.OWNED_PRE_WRITE
        assert history[1].previous_wal_seq == history[0].wal_seq
        assert materializer.events == []
    finally:
        store.close()


def test_write_owned_order_revalidates_the_controller_epoch_before_the_send(
    tmp_path: Path,
) -> None:
    wal = _load_wal_module()
    ledger = _load_ledger_module()
    claim = _claim()
    store = wal.SQLiteOwnershipWal(db_path=tmp_path / "ownership.db")
    owned_ledger = ledger.ExecutionLedger(
        wal=store,
        materializer=_RecordingMaterializer(),
    )

    class _ShouldNotSendWriter:
        async def write_owned_order(self, claim_to_send):
            raise AssertionError(f"writer should not run for {claim_to_send.intent_id}")

    try:
        with pytest.raises(wal.FenceRejectedError, match="before venue write"):
            asyncio.run(
                owned_ledger.write_owned_order(
                    claim=claim,
                    account_scope_id="ibkr.hedge.main",
                    operation_type="submit",
                    claim_key="submit:intent-001",
                    append_authority=_authority(),
                    write_authority=_authority(controller_epoch=8, controller_seq=42),
                    venue_writer=_ShouldNotSendWriter(),
                    written_at_ns=222,
                )
            )

        persisted = store.fetch_by_intent_id(claim.intent_id)
        assert persisted is not None
        assert persisted.lifecycle_state is ExecutionLifecycleState.OWNED_PRE_WRITE
        assert persisted.venue_order_id is None
    finally:
        store.close()


def test_materialization_is_async_and_replay_safe_for_duplicate_state_replays(
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

        first_event = asyncio.run(
            owned_ledger.materialize_intent(
                intent_id=claim.intent_id,
                lifecycle_state=ExecutionLifecycleState.SENT_TO_VENUE,
                venue_order_id="venue-9001",
                materialized_at_ns=333,
            )
        )
        second_event = asyncio.run(
            owned_ledger.materialize_intent(
                intent_id=claim.intent_id,
                lifecycle_state=ExecutionLifecycleState.SENT_TO_VENUE,
                venue_order_id="venue-9001",
                materialized_at_ns=444,
            )
        )

        assert first_event is not None
        assert second_event is None
        assert [event.lifecycle_state for event in materializer.events] == [
            ExecutionLifecycleState.SENT_TO_VENUE,
        ]
        history = store.list_records()
        assert len(history) == 3
        assert history[-1].materialized_lifecycle_state is ExecutionLifecycleState.SENT_TO_VENUE
        assert store.fetch_by_intent_id(claim.intent_id).materialized_lifecycle_state is (
            ExecutionLifecycleState.SENT_TO_VENUE
        )
    finally:
        store.close()
