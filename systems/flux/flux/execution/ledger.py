from __future__ import annotations

import sys
from dataclasses import dataclass
from enum import Enum
from typing import Protocol

from .controller import ControllerCrashRecoveryAction
from .controller import ControllerSnapshotAuthority
from .controller import VenueActivityOrigin
from .events import ExecutionLifecycleEvent
from .intents import ExecutionClaim
from .intents import ExecutionLifecycleState
from .wal import SQLiteOwnershipWal
from .wal import assert_controller_epoch_fence


if __name__ == "flux.execution.ledger":
    sys.modules.setdefault("nautilus_trader.flux.execution.ledger", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.execution.ledger":
    sys.modules.setdefault("flux.execution.ledger", sys.modules[__name__])


TERMINAL_LIFECYCLE_STATES = frozenset(
    {
        ExecutionLifecycleState.FILLED,
        ExecutionLifecycleState.CANCELED,
        ExecutionLifecycleState.REJECTED,
        ExecutionLifecycleState.QUARANTINED,
    }
)


class RecoveryClassification(str, Enum):
    PRE_WRITE_RECOVERY = "pre_write_recovery"
    PENDING_RECOVERY = "pending_recovery"
    BOUND_TO_VENUE = "bound_to_venue"
    MATERIALIZED_FROM_VENUE = "materialized_from_venue"
    QUARANTINED_ORPHAN = "quarantined_orphan"


class ExecutionVenueWriter(Protocol):
    async def write_owned_order(self, claim: ExecutionClaim) -> str: ...


class ExecutionMaterializer(Protocol):
    async def materialize(self, event: ExecutionLifecycleEvent) -> None: ...


@dataclass(frozen=True, slots=True)
class VenueTruth:
    client_order_id: str | None
    venue_order_id: str | None
    lifecycle_state: ExecutionLifecycleState
    final_ack: bool = True
    venue_activity_origin: VenueActivityOrigin = VenueActivityOrigin.CONTROLLER
    reason: str | None = None

    def __post_init__(self) -> None:
        if self.client_order_id is not None:
            object.__setattr__(
                self,
                "client_order_id",
                _required_text(self.client_order_id, "client_order_id"),
            )
        if self.venue_order_id is not None:
            object.__setattr__(
                self,
                "venue_order_id",
                _required_text(self.venue_order_id, "venue_order_id"),
            )
        if self.client_order_id is None and self.venue_order_id is None:
            raise ValueError("venue truth requires `client_order_id` or `venue_order_id`")
        object.__setattr__(
            self,
            "lifecycle_state",
            _coerce_lifecycle_state(self.lifecycle_state),
        )
        object.__setattr__(
            self,
            "venue_activity_origin",
            _coerce_venue_activity_origin(self.venue_activity_origin),
        )
        if self.reason is not None:
            object.__setattr__(self, "reason", _required_text(self.reason, "reason"))


@dataclass(frozen=True, slots=True)
class RecoveryPlan:
    classification: RecoveryClassification
    lifecycle_state: ExecutionLifecycleState
    crash_recovery_action: ControllerCrashRecoveryAction
    venue_activity_origin: VenueActivityOrigin
    should_send_to_venue: bool
    should_query_venue: bool
    should_materialize: bool
    requires_fence_revalidation: bool
    claim: ExecutionClaim | None
    venue_order_id: str | None
    reason: str | None = None


class _NullMaterializer:
    async def materialize(self, event: ExecutionLifecycleEvent) -> None:
        return None


class ExecutionLedger:
    def __init__(
        self,
        *,
        wal: SQLiteOwnershipWal,
        materializer: ExecutionMaterializer | None = None,
    ) -> None:
        self._wal = wal
        self._materializer = materializer or _NullMaterializer()

    async def write_owned_order(
        self,
        *,
        claim: ExecutionClaim,
        account_scope_id: str,
        operation_type: str,
        claim_key: str,
        append_authority: ControllerSnapshotAuthority,
        write_authority: ControllerSnapshotAuthority,
        venue_writer: ExecutionVenueWriter,
        written_at_ns: int,
    ):
        self._wal.append_claim(
            claim=claim,
            account_scope_id=account_scope_id,
            operation_type=operation_type,
            claim_key=claim_key,
            authority=append_authority,
            appended_at_ns=written_at_ns,
        )
        assert_controller_epoch_fence(
            claim=claim,
            authority=write_authority,
            phase="before venue write",
        )
        venue_order_id = _required_text(
            await venue_writer.write_owned_order(claim),
            "venue_order_id",
        )
        return self._wal.record_venue_write(
            claim=claim,
            authority=write_authority,
            venue_order_id=venue_order_id,
            written_at_ns=written_at_ns,
        )

    async def materialize_intent(
        self,
        *,
        intent_id: str,
        lifecycle_state: ExecutionLifecycleState | str,
        materialized_at_ns: int,
        venue_order_id: str | None = None,
        venue_activity_origin: VenueActivityOrigin | str = VenueActivityOrigin.CONTROLLER,
        reason: str | None = None,
    ) -> ExecutionLifecycleEvent | None:
        record = self._wal.fetch_by_intent_id(intent_id)
        if record is None:
            raise KeyError(f"missing ownership record for intent_id={intent_id}")
        target_state = _coerce_lifecycle_state(lifecycle_state)
        target_origin = _coerce_venue_activity_origin(venue_activity_origin)
        effective_venue_order_id = venue_order_id or record.venue_order_id
        if (
            record.materialized_lifecycle_state is target_state
            and effective_venue_order_id == record.venue_order_id
        ):
            return None
        event = ExecutionLifecycleEvent.from_claim(
            claim=record.claim,
            lifecycle_state=target_state,
            venue_activity_origin=target_origin,
            venue_order_id=effective_venue_order_id,
            reason=reason,
        )
        await self._materializer.materialize(event)
        self._wal.mark_materialized(
            intent_id=record.claim.intent_id,
            lifecycle_state=target_state,
            materialized_at_ns=materialized_at_ns,
            venue_order_id=effective_venue_order_id,
        )
        return event

    async def recover(
        self,
        *,
        client_order_id: str | None,
        venue_truth: VenueTruth | None,
        recovered_at_ns: int,
    ) -> RecoveryPlan:
        record = self._resolve_record(
            client_order_id=client_order_id,
            venue_truth=venue_truth,
        )
        if record is None:
            if venue_truth is None:
                raise KeyError("missing ownership record and no venue truth was supplied")
            return RecoveryPlan(
                classification=RecoveryClassification.QUARANTINED_ORPHAN,
                lifecycle_state=ExecutionLifecycleState.QUARANTINED,
                crash_recovery_action=ControllerCrashRecoveryAction.MANUAL_INTERVENTION,
                venue_activity_origin=VenueActivityOrigin.ORPHAN,
                should_send_to_venue=False,
                should_query_venue=False,
                should_materialize=False,
                requires_fence_revalidation=False,
                claim=None,
                venue_order_id=venue_truth.venue_order_id,
                reason="venue truth exists with no matching claim tuple",
            )

        if venue_truth is None:
            if record.lifecycle_state is ExecutionLifecycleState.OWNED_PRE_WRITE:
                return RecoveryPlan(
                    classification=RecoveryClassification.PRE_WRITE_RECOVERY,
                    lifecycle_state=ExecutionLifecycleState.OWNED_PRE_WRITE,
                    crash_recovery_action=ControllerCrashRecoveryAction.RETRY_VENUE_WRITE,
                    venue_activity_origin=VenueActivityOrigin.CONTROLLER,
                    should_send_to_venue=False,
                    should_query_venue=False,
                    should_materialize=False,
                    requires_fence_revalidation=True,
                    claim=record.claim,
                    venue_order_id=record.venue_order_id,
                    reason=None,
                )
            return RecoveryPlan(
                classification=RecoveryClassification.PENDING_RECOVERY,
                lifecycle_state=record.lifecycle_state,
                crash_recovery_action=ControllerCrashRecoveryAction.RECONCILE_BEFORE_RETRY,
                venue_activity_origin=VenueActivityOrigin.CONTROLLER,
                should_send_to_venue=False,
                should_query_venue=True,
                should_materialize=False,
                requires_fence_revalidation=False,
                claim=record.claim,
                venue_order_id=record.venue_order_id,
                reason=None,
            )

        if not _venue_truth_matches_record(record.claim, venue_truth):
            return RecoveryPlan(
                classification=RecoveryClassification.QUARANTINED_ORPHAN,
                lifecycle_state=ExecutionLifecycleState.QUARANTINED,
                crash_recovery_action=ControllerCrashRecoveryAction.MANUAL_INTERVENTION,
                venue_activity_origin=VenueActivityOrigin.ORPHAN,
                should_send_to_venue=False,
                should_query_venue=False,
                should_materialize=False,
                requires_fence_revalidation=False,
                claim=None,
                venue_order_id=venue_truth.venue_order_id,
                reason="venue truth exists with no matching claim tuple",
            )

        updated_record = self._wal.update_lifecycle(
            intent_id=record.claim.intent_id,
            lifecycle_state=venue_truth.lifecycle_state,
            updated_at_ns=recovered_at_ns,
            venue_order_id=venue_truth.venue_order_id,
        )

        should_query_venue = False
        should_materialize = True
        classification = RecoveryClassification.BOUND_TO_VENUE
        recovery_action = ControllerCrashRecoveryAction.RECONCILE_BEFORE_RETRY

        if venue_truth.lifecycle_state in TERMINAL_LIFECYCLE_STATES and not venue_truth.final_ack:
            classification = RecoveryClassification.MATERIALIZED_FROM_VENUE
            recovery_action = ControllerCrashRecoveryAction.RELEASE_CLAIM
        elif not venue_truth.final_ack:
            classification = RecoveryClassification.PENDING_RECOVERY
            should_query_venue = True

        await self.materialize_intent(
            intent_id=updated_record.claim.intent_id,
            lifecycle_state=venue_truth.lifecycle_state,
            venue_order_id=venue_truth.venue_order_id,
            venue_activity_origin=venue_truth.venue_activity_origin,
            reason=venue_truth.reason,
            materialized_at_ns=recovered_at_ns,
        )

        return RecoveryPlan(
            classification=classification,
            lifecycle_state=venue_truth.lifecycle_state,
            crash_recovery_action=recovery_action,
            venue_activity_origin=venue_truth.venue_activity_origin,
            should_send_to_venue=False,
            should_query_venue=should_query_venue,
            should_materialize=should_materialize,
            requires_fence_revalidation=False,
            claim=updated_record.claim,
            venue_order_id=venue_truth.venue_order_id,
            reason=venue_truth.reason,
        )

    def _resolve_record(
        self,
        *,
        client_order_id: str | None,
        venue_truth: VenueTruth | None,
    ):
        if client_order_id:
            record = self._wal.fetch_by_client_order_id(client_order_id)
            if record is not None:
                return record
        if venue_truth is None:
            return None
        if venue_truth.client_order_id is not None:
            record = self._wal.fetch_by_client_order_id(venue_truth.client_order_id)
            if record is not None:
                return record
        if venue_truth.venue_order_id is not None:
            return self._wal.fetch_by_venue_order_id(venue_truth.venue_order_id)
        return None


def _coerce_lifecycle_state(
    value: ExecutionLifecycleState | str,
) -> ExecutionLifecycleState:
    if isinstance(value, ExecutionLifecycleState):
        return value
    return ExecutionLifecycleState(_required_text(value, "lifecycle_state"))


def _coerce_venue_activity_origin(
    value: VenueActivityOrigin | str,
) -> VenueActivityOrigin:
    if isinstance(value, VenueActivityOrigin):
        return value
    return VenueActivityOrigin(_required_text(value, "venue_activity_origin"))


def _required_text(value: str, field_name: str) -> str:
    text = str(value).strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    return text


def _venue_truth_matches_record(claim: ExecutionClaim, venue_truth: VenueTruth) -> bool:
    if venue_truth.client_order_id is not None and venue_truth.client_order_id == claim.client_order_id:
        return True
    if (
        venue_truth.venue_order_id is not None
        and claim.venue_order_id is not None
        and venue_truth.venue_order_id == claim.venue_order_id
    ):
        return True
    return False


__all__ = (
    "ExecutionLedger",
    "ExecutionMaterializer",
    "ExecutionVenueWriter",
    "RecoveryClassification",
    "RecoveryPlan",
    "TERMINAL_LIFECYCLE_STATES",
    "VenueTruth",
)
