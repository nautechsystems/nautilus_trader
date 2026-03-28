from __future__ import annotations

import sys
from dataclasses import dataclass
from enum import Enum


if __name__ == "flux.execution.intents":
    sys.modules.setdefault("nautilus_trader.flux.execution.intents", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.execution.intents":
    sys.modules.setdefault("flux.execution.intents", sys.modules[__name__])


EXECUTION_LIFECYCLE_STATES = (
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


class ExecutionLifecycleState(str, Enum):
    PUBLISHED = "published"
    ACCEPTED = "accepted"
    OWNED_PRE_WRITE = "owned_pre_write"
    REJECTED = "rejected"
    SENT_TO_VENUE = "sent_to_venue"
    WORKING = "working"
    PARTIALLY_FILLED = "partially_filled"
    FILLED = "filled"
    CANCELED = "canceled"
    QUARANTINED = "quarantined"


def _required_text(value: str, field_name: str) -> str:
    text = str(value).strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    return text


def build_client_order_id(
    *,
    controller_scope_id: str,
    controller_epoch: int,
    controller_seq: int,
    intent_id: str,
) -> str:
    return ":".join(
        (
            _required_text(controller_scope_id, "controller_scope_id"),
            str(int(controller_epoch)),
            str(int(controller_seq)),
            _required_text(intent_id, "intent_id"),
        ),
    )


@dataclass(frozen=True, slots=True)
class ExecutionIntent:
    intent_id: str
    controller_scope_id: str
    strategy_id: str
    lifecycle_state: ExecutionLifecycleState = ExecutionLifecycleState.PUBLISHED

    def __post_init__(self) -> None:
        object.__setattr__(self, "intent_id", _required_text(self.intent_id, "intent_id"))
        object.__setattr__(
            self,
            "controller_scope_id",
            _required_text(self.controller_scope_id, "controller_scope_id"),
        )
        object.__setattr__(self, "strategy_id", _required_text(self.strategy_id, "strategy_id"))

    def claim(self, *, controller_epoch: int, controller_seq: int) -> ExecutionClaim:
        return ExecutionClaim(
            intent_id=self.intent_id,
            controller_scope_id=self.controller_scope_id,
            strategy_id=self.strategy_id,
            controller_epoch=int(controller_epoch),
            controller_seq=int(controller_seq),
            client_order_id=build_client_order_id(
                controller_scope_id=self.controller_scope_id,
                controller_epoch=controller_epoch,
                controller_seq=controller_seq,
                intent_id=self.intent_id,
            ),
            venue_order_id=None,
            lifecycle_state=ExecutionLifecycleState.ACCEPTED,
        )


@dataclass(frozen=True, slots=True)
class ExecutionClaim:
    intent_id: str
    controller_scope_id: str
    strategy_id: str
    controller_epoch: int
    controller_seq: int
    client_order_id: str
    venue_order_id: str | None
    lifecycle_state: ExecutionLifecycleState

    def __post_init__(self) -> None:
        object.__setattr__(self, "intent_id", _required_text(self.intent_id, "intent_id"))
        object.__setattr__(
            self,
            "controller_scope_id",
            _required_text(self.controller_scope_id, "controller_scope_id"),
        )
        object.__setattr__(self, "strategy_id", _required_text(self.strategy_id, "strategy_id"))
        object.__setattr__(self, "controller_epoch", int(self.controller_epoch))
        object.__setattr__(self, "controller_seq", int(self.controller_seq))
        object.__setattr__(
            self,
            "client_order_id",
            _required_text(self.client_order_id, "client_order_id"),
        )
        venue_order_id = self.venue_order_id
        if venue_order_id is not None:
            venue_order_id = _required_text(venue_order_id, "venue_order_id")
        object.__setattr__(self, "venue_order_id", venue_order_id)

    def to_dict(self) -> dict[str, str | int | None]:
        return {
            "intent_id": self.intent_id,
            "controller_scope_id": self.controller_scope_id,
            "strategy_id": self.strategy_id,
            "controller_epoch": self.controller_epoch,
            "controller_seq": self.controller_seq,
            "client_order_id": self.client_order_id,
            "venue_order_id": self.venue_order_id,
            "lifecycle_state": self.lifecycle_state.value,
        }


__all__ = (
    "EXECUTION_LIFECYCLE_STATES",
    "ExecutionClaim",
    "ExecutionIntent",
    "ExecutionLifecycleState",
    "build_client_order_id",
)
