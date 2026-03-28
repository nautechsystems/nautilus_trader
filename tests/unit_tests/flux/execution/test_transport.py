from __future__ import annotations

import importlib
from pathlib import Path

from flux.execution.events import ExecutionLifecycleEvent
from flux.execution.intents import ExecutionIntent
import pytest


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _load_transport_module():
    path = _repo_root() / "systems/flux/flux/execution/transport.py"
    assert path.exists(), "transport contract module should exist"
    return importlib.import_module("flux.execution.transport")


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def test_v1_uds_paths_are_scope_specific_and_versioned() -> None:
    transport = _load_transport_module()

    paths = transport.UdsTransportPaths.for_controller_scope(
        controller_scope_id="acct.execution.main",
        root_dir=Path("/run/flux"),
    )

    assert paths.to_dict() == {
        "schema_version": "v1",
        "transport": "uds",
        "controller_scope_id": "acct.execution.main",
        "request_reply_path": "/run/flux/flux-execution-v1/acct.execution.main.intent-rpc.sock",
        "event_stream_path": "/run/flux/flux-execution-v1/acct.execution.main.lifecycle-events.sock",
    }


def test_v1_uds_paths_reject_linux_af_unix_pathnames_longer_than_limit() -> None:
    transport = _load_transport_module()

    with pytest.raises(ValueError, match="AF_UNIX"):
        transport.UdsTransportPaths.for_controller_scope(
            controller_scope_id="a" * 90,
            root_dir=Path("/run/flux"),
        )


def test_v1_uds_request_reply_contract_round_trips_intents_and_claims() -> None:
    transport = _load_transport_module()
    intent = ExecutionIntent(
        intent_id="intent-001",
        controller_scope_id="acct.execution.main",
        strategy_id="strategy-01",
    )
    claim = intent.claim(controller_epoch=7, controller_seq=42)

    request = transport.ControllerIntentRequest(intent=intent, requested_at_ns=123_456)
    accepted = transport.ControllerIntentReply.accepted(claim=claim, replied_at_ns=123_999)
    rejected = transport.ControllerIntentReply.rejected(
        intent=intent,
        reason="intent rejected by controller guardrail",
        replied_at_ns=124_001,
    )

    assert request.to_dict() == {
        "schema_version": "v1",
        "transport": "uds",
        "channel": "request_reply",
        "requested_at_ns": 123_456,
        "intent": {
            "intent_id": "intent-001",
            "controller_scope_id": "acct.execution.main",
            "strategy_id": "strategy-01",
            "lifecycle_state": "published",
        },
    }
    assert transport.ControllerIntentRequest.from_dict(request.to_dict()) == request

    assert accepted.to_dict() == {
        "schema_version": "v1",
        "transport": "uds",
        "channel": "request_reply",
        "status": "accepted",
        "replied_at_ns": 123_999,
        "intent_id": "intent-001",
        "controller_scope_id": "acct.execution.main",
        "strategy_id": "strategy-01",
        "claim": {
            "intent_id": "intent-001",
            "controller_scope_id": "acct.execution.main",
            "strategy_id": "strategy-01",
            "controller_epoch": 7,
            "controller_seq": 42,
            "client_order_id": "acct.execution.main:7:42:intent-001",
            "venue_order_id": None,
            "lifecycle_state": "accepted",
        },
        "reason": None,
    }
    assert transport.ControllerIntentReply.from_dict(accepted.to_dict()) == accepted

    assert rejected.to_dict() == {
        "schema_version": "v1",
        "transport": "uds",
        "channel": "request_reply",
        "status": "rejected",
        "replied_at_ns": 124_001,
        "intent_id": "intent-001",
        "controller_scope_id": "acct.execution.main",
        "strategy_id": "strategy-01",
        "claim": None,
        "reason": "intent rejected by controller guardrail",
    }
    assert transport.ControllerIntentReply.from_dict(rejected.to_dict()) == rejected


def test_v1_accepted_reply_rejects_top_level_identity_mismatch_with_claim() -> None:
    transport = _load_transport_module()

    with pytest.raises(ValueError, match="claim identity"):
        transport.ControllerIntentReply.from_dict(
            {
                "schema_version": "v1",
                "transport": "uds",
                "channel": "request_reply",
                "status": "accepted",
                "replied_at_ns": 123_999,
                "intent_id": "intent-a",
                "controller_scope_id": "acct.execution.main",
                "strategy_id": "strategy-01",
                "claim": {
                    "intent_id": "intent-b",
                    "controller_scope_id": "acct.execution.main",
                    "strategy_id": "strategy-01",
                    "controller_epoch": 7,
                    "controller_seq": 42,
                    "client_order_id": "acct.execution.main:7:42:intent-b",
                    "venue_order_id": None,
                    "lifecycle_state": "accepted",
                },
                "reason": None,
            }
        )


def test_v1_uds_event_stream_contract_round_trips_lifecycle_events() -> None:
    transport = _load_transport_module()
    claim = ExecutionIntent(
        intent_id="intent-001",
        controller_scope_id="acct.execution.main",
        strategy_id="strategy-01",
    ).claim(controller_epoch=7, controller_seq=42)
    event = ExecutionLifecycleEvent.sent_to_venue(
        claim=claim,
        venue_order_id="venue-9001",
    )

    envelope = transport.ControllerLifecycleEventEnvelope(
        event=event,
        event_seq=43,
        emitted_at_ns=125_000,
    )

    assert envelope.to_dict() == {
        "schema_version": "v1",
        "transport": "uds",
        "channel": "event_stream",
        "stream": "execution_lifecycle",
        "event_seq": 43,
        "emitted_at_ns": 125_000,
        "event": {
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
        },
    }
    assert transport.ControllerLifecycleEventEnvelope.from_dict(envelope.to_dict()) == envelope
