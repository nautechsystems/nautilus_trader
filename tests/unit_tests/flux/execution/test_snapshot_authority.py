from __future__ import annotations

import pytest

from nautilus_trader.flux.execution.controller import ControllerSnapshotAuthority
from nautilus_trader.flux.execution.controller import SnapshotAuthorityState


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def test_snapshot_authority_emits_scope_epoch_seq_freshness_and_authority_state() -> None:
    authority = ControllerSnapshotAuthority(
        controller_scope_id="acct.execution.main",
        controller_epoch=7,
        controller_seq=42,
        snapshot_ts_ms=1_000,
        stale_after_ms=250,
        authority_state=SnapshotAuthorityState.AUTHORITATIVE,
    )

    assert authority.to_snapshot_fields(now_ms=1_200) == {
        "controller_scope_id": "acct.execution.main",
        "controller_epoch": 7,
        "controller_seq": 42,
        "authority_state": "authoritative",
        "snapshot_ts_ms": 1_000,
        "stale_after_ms": 250,
        "stale": False,
    }
    assert authority.to_snapshot_fields(now_ms=1_251)["stale"] is True


def test_snapshot_authority_requires_monotonic_epoch_then_sequence_progression() -> None:
    baseline = ControllerSnapshotAuthority(
        controller_scope_id="acct.execution.main",
        controller_epoch=7,
        controller_seq=42,
        snapshot_ts_ms=1_000,
        stale_after_ms=250,
        authority_state=SnapshotAuthorityState.AUTHORITATIVE,
    )
    next_seq = ControllerSnapshotAuthority(
        controller_scope_id="acct.execution.main",
        controller_epoch=7,
        controller_seq=43,
        snapshot_ts_ms=1_001,
        stale_after_ms=250,
        authority_state=SnapshotAuthorityState.AUTHORITATIVE,
    )
    next_epoch = ControllerSnapshotAuthority(
        controller_scope_id="acct.execution.main",
        controller_epoch=8,
        controller_seq=1,
        snapshot_ts_ms=1_002,
        stale_after_ms=250,
        authority_state=SnapshotAuthorityState.AUTHORITATIVE,
    )
    regressed = ControllerSnapshotAuthority(
        controller_scope_id="acct.execution.main",
        controller_epoch=7,
        controller_seq=41,
        snapshot_ts_ms=999,
        stale_after_ms=250,
        authority_state=SnapshotAuthorityState.AUTHORITATIVE,
    )
    wrong_scope = ControllerSnapshotAuthority(
        controller_scope_id="acct.execution.secondary",
        controller_epoch=7,
        controller_seq=44,
        snapshot_ts_ms=1_003,
        stale_after_ms=250,
        authority_state=SnapshotAuthorityState.AUTHORITATIVE,
    )

    next_seq.assert_can_follow(baseline)
    next_epoch.assert_can_follow(next_seq)

    with pytest.raises(ValueError, match="monotonic"):
        regressed.assert_can_follow(next_seq)

    with pytest.raises(ValueError, match="controller_scope_id"):
        wrong_scope.assert_can_follow(baseline)
