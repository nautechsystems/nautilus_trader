from __future__ import annotations

import pytest

from nautilus_trader.flux.execution.controller import ControllerCrashRecoveryAction
from nautilus_trader.flux.execution.controller import ControllerOwnershipState
from nautilus_trader.flux.execution.controller import VenueActivityOrigin
from nautilus_trader.flux.execution.controller import default_lifecycle_state_for_venue_activity
from nautilus_trader.flux.execution.controller import lifecycle_semantics
from nautilus_trader.flux.execution.controller import ownership_state_for_lifecycle
from nautilus_trader.flux.execution.events import ExecutionLifecycleEvent
from nautilus_trader.flux.execution.intents import ExecutionIntent
from nautilus_trader.flux.execution.intents import ExecutionLifecycleState


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def test_owned_pre_write_and_sent_to_venue_have_distinct_crash_window_semantics() -> None:
    owned_pre_write = lifecycle_semantics(ExecutionLifecycleState.OWNED_PRE_WRITE)
    sent_to_venue = lifecycle_semantics(ExecutionLifecycleState.SENT_TO_VENUE)

    assert owned_pre_write.controller_owns_claim is True
    assert owned_pre_write.venue_write_attempted is False
    assert owned_pre_write.crash_recovery_action is ControllerCrashRecoveryAction.RETRY_VENUE_WRITE
    assert ownership_state_for_lifecycle(ExecutionLifecycleState.OWNED_PRE_WRITE) is ControllerOwnershipState.OWNED

    assert sent_to_venue.controller_owns_claim is True
    assert sent_to_venue.venue_write_attempted is True
    assert (
        sent_to_venue.crash_recovery_action
        is ControllerCrashRecoveryAction.RECONCILE_BEFORE_RETRY
    )
    assert ownership_state_for_lifecycle(ExecutionLifecycleState.SENT_TO_VENUE) is ControllerOwnershipState.OWNED


def test_external_manual_and_orphan_venue_activity_are_quarantined_first() -> None:
    assert (
        default_lifecycle_state_for_venue_activity(VenueActivityOrigin.EXTERNAL)
        is ExecutionLifecycleState.QUARANTINED
    )
    assert (
        default_lifecycle_state_for_venue_activity(VenueActivityOrigin.MANUAL)
        is ExecutionLifecycleState.QUARANTINED
    )
    assert (
        default_lifecycle_state_for_venue_activity(VenueActivityOrigin.ORPHAN)
        is ExecutionLifecycleState.QUARANTINED
    )
    assert ownership_state_for_lifecycle(ExecutionLifecycleState.QUARANTINED) is ControllerOwnershipState.QUARANTINED


def test_controller_origin_round_trips_through_serialized_event_payloads() -> None:
    claim = ExecutionIntent(
        intent_id="intent-001",
        controller_scope_id="acct.execution.main",
        strategy_id="strategy-01",
    ).claim(controller_epoch=7, controller_seq=42)
    payload = ExecutionLifecycleEvent.sent_to_venue(
        claim=claim,
        venue_order_id="venue-9001",
    ).to_dict()

    assert payload["venue_activity_origin"] == "controller"
    assert default_lifecycle_state_for_venue_activity(payload["venue_activity_origin"]) is ExecutionLifecycleState.WORKING
