"""
Build structured wire payloads for MakerV3 observability events.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from flux.events import FluxBusPayload


@dataclass(frozen=True, slots=True)
class QuoteCycleContext:
    run_id: str
    quote_cycle_id: str
    quote_cycle_seq: int
    instrument_id: str
    trigger_source: str | None
    trigger_instrument_id: str | None
    trigger_md_ts_event_ns: int | None
    trigger_md_ts_init_ns: int | None
    ts_cycle_start_ns: int


def build_quote_cycle_id(*, run_id: str, quote_cycle_seq: int) -> str:
    """
    Return a stable quote-cycle identifier for a run-local sequence.
    """
    return f"{run_id}:{quote_cycle_seq}"


def build_quote_cycle_envelope(
    *,
    context: QuoteCycleContext,
    quote_cycle_event: str,
    reason_code: str,
    ts_cycle_end_ns: int,
    payload: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """
    Return a standardized quote-cycle event envelope.
    """
    data: dict[str, Any] = {
        "run_id": context.run_id,
        "quote_cycle_id": context.quote_cycle_id,
        "quote_cycle_seq": context.quote_cycle_seq,
        "instrument_id": context.instrument_id,
        "quote_cycle_event": quote_cycle_event,
        "reason_code": reason_code,
        "trigger_source": context.trigger_source,
        "trigger_instrument_id": context.trigger_instrument_id,
        "trigger_md_ts_event_ns": context.trigger_md_ts_event_ns,
        "trigger_md_ts_init_ns": context.trigger_md_ts_init_ns,
        "ts_cycle_start_ns": context.ts_cycle_start_ns,
        "ts_cycle_end_ns": ts_cycle_end_ns,
    }
    if payload:
        data.update(payload)
    return data
