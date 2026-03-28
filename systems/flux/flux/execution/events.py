from __future__ import annotations

import sys
from dataclasses import dataclass

from .controller import VenueActivityOrigin
from .controller import _coerce_venue_activity_origin
from .intents import ExecutionClaim
from .intents import ExecutionLifecycleState
from .intents import _coerce_lifecycle_state


if __name__ == "flux.execution.events":
    sys.modules.setdefault("nautilus_trader.flux.execution.events", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.execution.events":
    sys.modules.setdefault("flux.execution.events", sys.modules[__name__])


@dataclass(frozen=True, slots=True)
class ExecutionLifecycleEvent:
    intent_id: str
    controller_scope_id: str
    strategy_id: str
    controller_epoch: int
    controller_seq: int
    client_order_id: str
    venue_order_id: str | None
    lifecycle_state: ExecutionLifecycleState
    venue_activity_origin: VenueActivityOrigin
    reason: str | None = None

    def __post_init__(self) -> None:
        object.__setattr__(
            self,
            "lifecycle_state",
            _coerce_lifecycle_state(self.lifecycle_state, field_name="lifecycle_state"),
        )
        object.__setattr__(
            self,
            "venue_activity_origin",
            _coerce_venue_activity_origin(
                self.venue_activity_origin,
                field_name="venue_activity_origin",
            ),
        )

    @classmethod
    def from_claim(
        cls,
        *,
        claim: ExecutionClaim,
        lifecycle_state: ExecutionLifecycleState | str,
        venue_activity_origin: VenueActivityOrigin | str,
        venue_order_id: str | None = None,
        reason: str | None = None,
    ) -> ExecutionLifecycleEvent:
        return cls(
            intent_id=claim.intent_id,
            controller_scope_id=claim.controller_scope_id,
            strategy_id=claim.strategy_id,
            controller_epoch=claim.controller_epoch,
            controller_seq=claim.controller_seq,
            client_order_id=claim.client_order_id,
            venue_order_id=venue_order_id,
            lifecycle_state=lifecycle_state,
            venue_activity_origin=venue_activity_origin,
            reason=reason,
        )

    @classmethod
    def sent_to_venue(
        cls,
        *,
        claim: ExecutionClaim,
        venue_order_id: str,
    ) -> ExecutionLifecycleEvent:
        return cls.from_claim(
            claim=claim,
            lifecycle_state=ExecutionLifecycleState.SENT_TO_VENUE,
            venue_activity_origin=VenueActivityOrigin.CONTROLLER,
            venue_order_id=venue_order_id,
            reason=None,
        )

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
            "venue_activity_origin": self.venue_activity_origin.value,
            "reason": self.reason,
        }


__all__ = ("ExecutionLifecycleEvent",)
