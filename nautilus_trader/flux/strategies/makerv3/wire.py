"""Build structured wire payloads for MakerV3 observability events."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


try:
    from nautilus_trader.serialization import register_serializable_type
except Exception:  # pragma: no cover - fallback test environments
    register_serializable_type = None


if register_serializable_type is not None:

    @dataclass(frozen=True, slots=True)
    class FluxBusPayload:
        """Wrap a JSON payload for transport over Nautilus message bus."""

        topic: str
        payload: str
        ts_event: int = 0
        ts_init: int = 0

        def to_dict(self) -> dict[str, Any]:
            """Return a serializable dictionary representation."""
            return {
                "type": "FluxBusPayload",
                "topic": self.topic,
                "payload": self.payload,
                "ts_event": self.ts_event,
                "ts_init": self.ts_init,
            }

        @classmethod
        def from_dict(cls, data: dict[str, Any]) -> FluxBusPayload:
            """Build a payload object from its dictionary representation."""
            return cls(
                topic=data.get("topic", ""),
                payload=data.get("payload", ""),
                ts_event=int(data.get("ts_event", 0)),
                ts_init=int(data.get("ts_init", 0)),
            )

    register_serializable_type(FluxBusPayload, FluxBusPayload.to_dict, FluxBusPayload.from_dict)
else:  # pragma: no cover - fallback test environments
    FluxBusPayload = None


def build_quote_cycle_id(*, run_id: str, quote_cycle_seq: int) -> str:
    """Return a stable quote-cycle identifier for a run-local sequence."""
    return f"{run_id}:{quote_cycle_seq}"


def build_quote_cycle_envelope(
    *,
    run_id: str,
    quote_cycle_id: str,
    quote_cycle_event: str,
    reason_code: str,
    payload: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Return a standardized quote-cycle event envelope."""
    data: dict[str, Any] = {
        "run_id": run_id,
        "quote_cycle_id": quote_cycle_id,
        "quote_cycle_event": quote_cycle_event,
        "reason_code": reason_code,
    }
    if payload:
        data.update(payload)
    return data
