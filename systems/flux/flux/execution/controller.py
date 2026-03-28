from __future__ import annotations

import sys
from dataclasses import dataclass
from enum import Enum

from .intents import ExecutionLifecycleState


if __name__ == "flux.execution.controller":
    sys.modules.setdefault("nautilus_trader.flux.execution.controller", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.execution.controller":
    sys.modules.setdefault("flux.execution.controller", sys.modules[__name__])


class ControllerOwnershipState(str, Enum):
    UNOWNED = "unowned"
    CLAIMED = "claimed"
    OWNED = "owned"
    TERMINAL = "terminal"
    QUARANTINED = "quarantined"


class ControllerCrashRecoveryAction(str, Enum):
    NOOP = "noop"
    RETRY_VENUE_WRITE = "retry_venue_write"
    RECONCILE_BEFORE_RETRY = "reconcile_before_retry"
    RELEASE_CLAIM = "release_claim"
    MANUAL_INTERVENTION = "manual_intervention"


class VenueActivityOrigin(str, Enum):
    CONTROLLER = "controller"
    EXTERNAL = "external"
    MANUAL = "manual"
    ORPHAN = "orphan"


class SnapshotAuthorityState(str, Enum):
    AUTHORITATIVE = "authoritative"
    OBSERVER = "observer"
    STALE = "stale"


class ControllerRunMode(str, Enum):
    SHADOW = "shadow"
    ACTIVE = "active"


class ControllerIngressPolicy(str, Enum):
    SINGLE_HOST_CANARY = "single_host_canary"


def _coerce_venue_activity_origin(
    value: VenueActivityOrigin | str,
    *,
    field_name: str,
) -> VenueActivityOrigin:
    if isinstance(value, VenueActivityOrigin):
        return value
    text = str(value).strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    try:
        return VenueActivityOrigin(text)
    except ValueError as exc:
        raise ValueError(
            f"`{field_name}` must be one of {tuple(origin.value for origin in VenueActivityOrigin)}",
        ) from exc


def _coerce_authority_state(
    value: SnapshotAuthorityState | str,
    *,
    field_name: str,
) -> SnapshotAuthorityState:
    if isinstance(value, SnapshotAuthorityState):
        return value
    text = str(value).strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    try:
        return SnapshotAuthorityState(text)
    except ValueError as exc:
        raise ValueError(
            f"`{field_name}` must be one of {tuple(state.value for state in SnapshotAuthorityState)}",
        ) from exc


@dataclass(frozen=True, slots=True)
class ExecutionLifecycleSemantics:
    lifecycle_state: ExecutionLifecycleState
    ownership_state: ControllerOwnershipState
    controller_owns_claim: bool
    venue_write_attempted: bool
    crash_recovery_action: ControllerCrashRecoveryAction


_LIFECYCLE_SEMANTICS = {
    ExecutionLifecycleState.PUBLISHED: ExecutionLifecycleSemantics(
        lifecycle_state=ExecutionLifecycleState.PUBLISHED,
        ownership_state=ControllerOwnershipState.UNOWNED,
        controller_owns_claim=False,
        venue_write_attempted=False,
        crash_recovery_action=ControllerCrashRecoveryAction.NOOP,
    ),
    ExecutionLifecycleState.ACCEPTED: ExecutionLifecycleSemantics(
        lifecycle_state=ExecutionLifecycleState.ACCEPTED,
        ownership_state=ControllerOwnershipState.CLAIMED,
        controller_owns_claim=True,
        venue_write_attempted=False,
        crash_recovery_action=ControllerCrashRecoveryAction.NOOP,
    ),
    ExecutionLifecycleState.OWNED_PRE_WRITE: ExecutionLifecycleSemantics(
        lifecycle_state=ExecutionLifecycleState.OWNED_PRE_WRITE,
        ownership_state=ControllerOwnershipState.OWNED,
        controller_owns_claim=True,
        venue_write_attempted=False,
        crash_recovery_action=ControllerCrashRecoveryAction.RETRY_VENUE_WRITE,
    ),
    ExecutionLifecycleState.REJECTED: ExecutionLifecycleSemantics(
        lifecycle_state=ExecutionLifecycleState.REJECTED,
        ownership_state=ControllerOwnershipState.TERMINAL,
        controller_owns_claim=False,
        venue_write_attempted=False,
        crash_recovery_action=ControllerCrashRecoveryAction.RELEASE_CLAIM,
    ),
    ExecutionLifecycleState.SENT_TO_VENUE: ExecutionLifecycleSemantics(
        lifecycle_state=ExecutionLifecycleState.SENT_TO_VENUE,
        ownership_state=ControllerOwnershipState.OWNED,
        controller_owns_claim=True,
        venue_write_attempted=True,
        crash_recovery_action=ControllerCrashRecoveryAction.RECONCILE_BEFORE_RETRY,
    ),
    ExecutionLifecycleState.WORKING: ExecutionLifecycleSemantics(
        lifecycle_state=ExecutionLifecycleState.WORKING,
        ownership_state=ControllerOwnershipState.OWNED,
        controller_owns_claim=True,
        venue_write_attempted=True,
        crash_recovery_action=ControllerCrashRecoveryAction.RECONCILE_BEFORE_RETRY,
    ),
    ExecutionLifecycleState.PARTIALLY_FILLED: ExecutionLifecycleSemantics(
        lifecycle_state=ExecutionLifecycleState.PARTIALLY_FILLED,
        ownership_state=ControllerOwnershipState.OWNED,
        controller_owns_claim=True,
        venue_write_attempted=True,
        crash_recovery_action=ControllerCrashRecoveryAction.RECONCILE_BEFORE_RETRY,
    ),
    ExecutionLifecycleState.FILLED: ExecutionLifecycleSemantics(
        lifecycle_state=ExecutionLifecycleState.FILLED,
        ownership_state=ControllerOwnershipState.TERMINAL,
        controller_owns_claim=False,
        venue_write_attempted=True,
        crash_recovery_action=ControllerCrashRecoveryAction.RELEASE_CLAIM,
    ),
    ExecutionLifecycleState.CANCELED: ExecutionLifecycleSemantics(
        lifecycle_state=ExecutionLifecycleState.CANCELED,
        ownership_state=ControllerOwnershipState.TERMINAL,
        controller_owns_claim=False,
        venue_write_attempted=True,
        crash_recovery_action=ControllerCrashRecoveryAction.RELEASE_CLAIM,
    ),
    ExecutionLifecycleState.QUARANTINED: ExecutionLifecycleSemantics(
        lifecycle_state=ExecutionLifecycleState.QUARANTINED,
        ownership_state=ControllerOwnershipState.QUARANTINED,
        controller_owns_claim=False,
        venue_write_attempted=True,
        crash_recovery_action=ControllerCrashRecoveryAction.MANUAL_INTERVENTION,
    ),
}


def lifecycle_semantics(lifecycle_state: ExecutionLifecycleState) -> ExecutionLifecycleSemantics:
    return _LIFECYCLE_SEMANTICS[lifecycle_state]


def ownership_state_for_lifecycle(lifecycle_state: ExecutionLifecycleState) -> ControllerOwnershipState:
    return lifecycle_semantics(lifecycle_state).ownership_state


def default_lifecycle_state_for_venue_activity(
    venue_activity_origin: VenueActivityOrigin | str,
) -> ExecutionLifecycleState:
    venue_activity_origin = _coerce_venue_activity_origin(
        venue_activity_origin,
        field_name="venue_activity_origin",
    )
    if venue_activity_origin is VenueActivityOrigin.CONTROLLER:
        return ExecutionLifecycleState.WORKING
    return ExecutionLifecycleState.QUARANTINED


@dataclass(frozen=True, slots=True)
class ControllerSnapshotAuthority:
    controller_scope_id: str
    controller_epoch: int
    controller_seq: int
    snapshot_ts_ms: int
    stale_after_ms: int
    authority_state: SnapshotAuthorityState

    def __post_init__(self) -> None:
        object.__setattr__(self, "controller_scope_id", str(self.controller_scope_id).strip())
        object.__setattr__(self, "controller_epoch", int(self.controller_epoch))
        object.__setattr__(self, "controller_seq", int(self.controller_seq))
        object.__setattr__(self, "snapshot_ts_ms", int(self.snapshot_ts_ms))
        object.__setattr__(self, "stale_after_ms", max(0, int(self.stale_after_ms)))
        if not self.controller_scope_id:
            raise ValueError("`controller_scope_id` must be a non-empty string")
        object.__setattr__(
            self,
            "authority_state",
            _coerce_authority_state(self.authority_state, field_name="authority_state"),
        )

    def is_stale(self, *, now_ms: int) -> bool:
        return int(now_ms) > self.snapshot_ts_ms + self.stale_after_ms

    def to_snapshot_fields(self, *, now_ms: int) -> dict[str, str | int | bool]:
        return {
            "controller_scope_id": self.controller_scope_id,
            "controller_epoch": self.controller_epoch,
            "controller_seq": self.controller_seq,
            "authority_state": self.authority_state.value,
            "snapshot_ts_ms": self.snapshot_ts_ms,
            "stale_after_ms": self.stale_after_ms,
            "stale": self.is_stale(now_ms=now_ms),
        }

    def assert_can_follow(self, previous: ControllerSnapshotAuthority) -> None:
        if self.controller_scope_id != previous.controller_scope_id:
            raise ValueError("controller_scope_id must match previous authority snapshot")
        if self.controller_epoch < previous.controller_epoch:
            raise ValueError("controller sequencing must remain monotonic")
        if (
            self.controller_epoch == previous.controller_epoch
            and self.controller_seq <= previous.controller_seq
        ):
            raise ValueError("controller sequencing must remain monotonic")


__all__ = (
    "ControllerCrashRecoveryAction",
    "ControllerIngressPolicy",
    "ControllerOwnershipState",
    "ControllerRunMode",
    "ControllerSnapshotAuthority",
    "ExecutionLifecycleSemantics",
    "SnapshotAuthorityState",
    "VenueActivityOrigin",
    "default_lifecycle_state_for_venue_activity",
    "lifecycle_semantics",
    "ownership_state_for_lifecycle",
)
