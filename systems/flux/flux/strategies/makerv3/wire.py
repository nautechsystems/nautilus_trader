"""
Build structured wire payloads for MakerV3 observability events.
"""

from __future__ import annotations

from typing import Any

from flux.events import FluxBusPayload


def build_quote_cycle_id(*, run_id: str, quote_cycle_seq: int) -> str:
    """
    Return a stable quote-cycle identifier for a run-local sequence.
    """
    return f"{run_id}:{quote_cycle_seq}"


def build_quote_cycle_envelope(
    *,
    run_id: str,
    quote_cycle_id: str,
    quote_cycle_event: str,
    reason_code: str,
    payload: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """
    Return a standardized quote-cycle event envelope.
    """
    data: dict[str, Any] = {
        "run_id": run_id,
        "quote_cycle_id": quote_cycle_id,
        "quote_cycle_event": quote_cycle_event,
        "reason_code": reason_code,
    }
    if payload:
        data.update(payload)
    return data
