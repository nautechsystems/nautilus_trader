from __future__ import annotations

import pytest

from nautilus_trader.flux.execution.events import ExecutionLifecycleEvent
from nautilus_trader.flux.execution.intents import EXECUTION_LIFECYCLE_STATES
from nautilus_trader.flux.execution.intents import ExecutionIntent
from nautilus_trader.flux.execution.intents import ExecutionLifecycleState


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def test_execution_lifecycle_states_are_frozen_in_contract_order() -> None:
    assert EXECUTION_LIFECYCLE_STATES == (
        "published",
        "accepted",
        "owned_pre_write",
        "rejected",
        "sent_to_venue",
        "working",
        "partially_filled",
        "filled",
        "canceled",
        "quarantined",
    )
    assert tuple(state.value for state in ExecutionLifecycleState) == EXECUTION_LIFECYCLE_STATES


def test_intent_claim_and_event_schema_preserve_deterministic_id_chain() -> None:
    intent = ExecutionIntent(
        intent_id="intent-001",
        controller_scope_id="acct.execution.main",
        strategy_id="strategy-01",
    )

    claim = intent.claim(controller_epoch=7, controller_seq=42)
    venue_event = ExecutionLifecycleEvent.sent_to_venue(
        claim=claim,
        venue_order_id="venue-9001",
    )

    assert claim.lifecycle_state is ExecutionLifecycleState.ACCEPTED
    assert claim.client_order_id == "acct.execution.main:7:42:intent-001"
    assert claim.to_dict() == {
        "intent_id": "intent-001",
        "controller_scope_id": "acct.execution.main",
        "strategy_id": "strategy-01",
        "controller_epoch": 7,
        "controller_seq": 42,
        "client_order_id": "acct.execution.main:7:42:intent-001",
        "venue_order_id": None,
        "lifecycle_state": "accepted",
    }
    assert venue_event.to_dict() == {
        "intent_id": "intent-001",
        "controller_scope_id": "acct.execution.main",
        "strategy_id": "strategy-01",
        "controller_epoch": 7,
        "controller_seq": 42,
        "client_order_id": "acct.execution.main:7:42:intent-001",
        "venue_order_id": "venue-9001",
        "lifecycle_state": "sent_to_venue",
        "venue_activity_origin": "controller",
        "reason": None,
    }
