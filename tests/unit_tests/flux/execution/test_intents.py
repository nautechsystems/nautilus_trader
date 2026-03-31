from __future__ import annotations

import re

import pytest

from nautilus_trader.flux.execution.controller import VenueActivityOrigin
from nautilus_trader.flux.execution.events import ExecutionLifecycleEvent
from nautilus_trader.flux.execution.intents import EXECUTION_LIFECYCLE_STATES
from nautilus_trader.flux.execution.intents import ExecutionClaim
from nautilus_trader.flux.execution.intents import ExecutionIntent
from nautilus_trader.flux.execution.intents import ExecutionLifecycleState
from nautilus_trader.flux.execution.intents import build_client_order_id


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
    expected_client_order_id = build_client_order_id(
        controller_scope_id="acct.execution.main",
        controller_epoch=7,
        controller_seq=42,
        intent_id="intent-001",
    )

    assert claim.lifecycle_state is ExecutionLifecycleState.ACCEPTED
    assert claim.client_order_id == expected_client_order_id
    assert len(claim.client_order_id) <= 36
    assert re.fullmatch(r"[A-Za-z0-9_-]+", claim.client_order_id)
    assert claim.to_dict() == {
        "intent_id": "intent-001",
        "controller_scope_id": "acct.execution.main",
        "strategy_id": "strategy-01",
        "controller_epoch": 7,
        "controller_seq": 42,
        "client_order_id": expected_client_order_id,
        "venue_order_id": None,
        "lifecycle_state": "accepted",
    }
    assert venue_event.to_dict() == {
        "intent_id": "intent-001",
        "controller_scope_id": "acct.execution.main",
        "strategy_id": "strategy-01",
        "controller_epoch": 7,
        "controller_seq": 42,
        "client_order_id": expected_client_order_id,
        "venue_order_id": "venue-9001",
        "lifecycle_state": "sent_to_venue",
        "venue_activity_origin": "controller",
        "reason": None,
    }


def test_public_execution_schemas_normalize_and_validate_enum_backed_fields() -> None:
    intent = ExecutionIntent(
        intent_id="intent-001",
        controller_scope_id="acct.execution.main",
        strategy_id="strategy-01",
        lifecycle_state="published",
    )
    claim = ExecutionClaim(
        intent_id="intent-001",
        controller_scope_id="acct.execution.main",
        strategy_id="strategy-01",
        controller_epoch=7,
        controller_seq=42,
        client_order_id=build_client_order_id(
            controller_scope_id="acct.execution.main",
            controller_epoch=7,
            controller_seq=42,
            intent_id="intent-001",
        ),
        venue_order_id=None,
        lifecycle_state="accepted",
    )
    event = ExecutionLifecycleEvent.from_claim(
        claim=claim,
        lifecycle_state="sent_to_venue",
        venue_activity_origin="controller",
        venue_order_id="venue-9001",
    )

    assert intent.lifecycle_state is ExecutionLifecycleState.PUBLISHED
    assert claim.lifecycle_state is ExecutionLifecycleState.ACCEPTED
    assert event.lifecycle_state is ExecutionLifecycleState.SENT_TO_VENUE
    assert event.venue_activity_origin is VenueActivityOrigin.CONTROLLER
    assert event.to_dict()["venue_activity_origin"] == "controller"

    with pytest.raises(ValueError, match="lifecycle_state"):
        ExecutionClaim(
            intent_id="intent-001",
            controller_scope_id="acct.execution.main",
            strategy_id="strategy-01",
            controller_epoch=7,
            controller_seq=42,
            client_order_id="acct.execution.main:7:42:intent-001",
            venue_order_id=None,
            lifecycle_state="not-a-real-state",
        )

    with pytest.raises(ValueError, match="venue_activity_origin"):
        ExecutionLifecycleEvent.from_claim(
            claim=claim,
            lifecycle_state=ExecutionLifecycleState.SENT_TO_VENUE,
            venue_activity_origin="desk-ghost",
            venue_order_id="venue-9001",
        )


def test_execution_claim_rejects_client_order_ids_that_break_deterministic_chain() -> None:
    with pytest.raises(ValueError, match="client_order_id"):
        ExecutionClaim(
            intent_id="intent-001",
            controller_scope_id="acct.execution.main",
            strategy_id="strategy-01",
            controller_epoch=7,
            controller_seq=42,
            client_order_id="acct.execution.main:7:43:intent-001",
            venue_order_id=None,
            lifecycle_state=ExecutionLifecycleState.ACCEPTED,
        )


def test_build_client_order_id_produces_venue_safe_compact_ids() -> None:
    client_order_id = build_client_order_id(
        controller_scope_id="tokenmm.binance.pm.main",
        controller_epoch=1,
        controller_seq=1,
        intent_id="node-owned-order-id",
    )

    assert len(client_order_id) <= 36
    assert re.fullmatch(r"[A-Za-z0-9_-]+", client_order_id)
