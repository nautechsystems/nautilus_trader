"""
Shared Flux message-bus envelopes and topics.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any


try:
    from nautilus_trader.serialization import register_serializable_type
except Exception:  # pragma: no cover - fallback test environments
    register_serializable_type = None


TOPIC_EXECUTION_ALERT = "flux.execution.alert"


@dataclass(frozen=True, slots=True)
class FluxBusPayload:
    """
    Wrap a JSON payload for transport over the Nautilus message bus.
    """

    topic: str
    payload: str
    ts_event: int = 0
    ts_init: int = 0

    def to_dict(self) -> dict[str, Any]:
        return {
            "type": "FluxBusPayload",
            "topic": self.topic,
            "payload": self.payload,
            "ts_event": self.ts_event,
            "ts_init": self.ts_init,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> FluxBusPayload:
        return cls(
            topic=str(data.get("topic", "")),
            payload=str(data.get("payload", "")),
            ts_event=int(data.get("ts_event", 0)),
            ts_init=int(data.get("ts_init", 0)),
        )


if register_serializable_type is not None:
    try:
        register_serializable_type(FluxBusPayload, FluxBusPayload.to_dict, FluxBusPayload.from_dict)
    except KeyError:  # pragma: no cover - already registered in long-lived test processes
        pass
