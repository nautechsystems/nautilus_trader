from __future__ import annotations

import os
import sys
from dataclasses import dataclass
from pathlib import Path

from .events import ExecutionLifecycleEvent
from .intents import ExecutionClaim
from .intents import ExecutionIntent


if __name__ == "flux.execution.transport":
    sys.modules.setdefault("nautilus_trader.flux.execution.transport", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.execution.transport":
    sys.modules.setdefault("flux.execution.transport", sys.modules[__name__])


TRANSPORT_SCHEMA_VERSION = "v1"
TRANSPORT_KIND = "uds"
REQUEST_REPLY_CHANNEL = "request_reply"
EVENT_STREAM_CHANNEL = "event_stream"
EVENT_STREAM_NAME = "execution_lifecycle"
SOCKET_NAMESPACE = "flux-execution-v1"
REQUEST_REPLY_SUFFIX = ".intent-rpc.sock"
EVENT_STREAM_SUFFIX = ".lifecycle-events.sock"
REPLY_STATUS_ACCEPTED = "accepted"
REPLY_STATUS_REJECTED = "rejected"
AF_UNIX_MAX_PATH_BYTES = 107


def _required_text(value: str, field_name: str) -> str:
    text = str(value).strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    return text


def _scope_socket_name(controller_scope_id: str, suffix: str) -> str:
    scope = _required_text(controller_scope_id, "controller_scope_id")
    if "/" in scope or "\x00" in scope:
        raise ValueError("`controller_scope_id` must be a valid socket path component")
    return f"{scope}{suffix}"


def _assert_af_unix_path_length(path: Path) -> None:
    path_bytes = os.fsencode(str(path))
    if len(path_bytes) > AF_UNIX_MAX_PATH_BYTES:
        raise ValueError(
            f"AF_UNIX pathname exceeds {AF_UNIX_MAX_PATH_BYTES} bytes: {path} ({len(path_bytes)} bytes)",
        )


def _intent_to_dict(intent: ExecutionIntent) -> dict[str, str]:
    return {
        "intent_id": intent.intent_id,
        "controller_scope_id": intent.controller_scope_id,
        "strategy_id": intent.strategy_id,
        "lifecycle_state": intent.lifecycle_state.value,
    }


def _intent_from_dict(payload: dict[str, object]) -> ExecutionIntent:
    return ExecutionIntent(
        intent_id=str(payload["intent_id"]),
        controller_scope_id=str(payload["controller_scope_id"]),
        strategy_id=str(payload["strategy_id"]),
        lifecycle_state=str(payload["lifecycle_state"]),
    )


@dataclass(frozen=True, slots=True)
class UdsTransportPaths:
    controller_scope_id: str
    root_dir: Path
    request_reply_path: Path
    event_stream_path: Path
    schema_version: str = TRANSPORT_SCHEMA_VERSION
    transport: str = TRANSPORT_KIND

    @classmethod
    def for_controller_scope(
        cls,
        *,
        controller_scope_id: str,
        root_dir: str | Path,
    ) -> UdsTransportPaths:
        scope = _required_text(controller_scope_id, "controller_scope_id")
        root = Path(root_dir)
        base_dir = root / SOCKET_NAMESPACE
        request_reply_path = base_dir / _scope_socket_name(scope, REQUEST_REPLY_SUFFIX)
        event_stream_path = base_dir / _scope_socket_name(scope, EVENT_STREAM_SUFFIX)
        _assert_af_unix_path_length(request_reply_path)
        _assert_af_unix_path_length(event_stream_path)
        return cls(
            controller_scope_id=scope,
            root_dir=root,
            request_reply_path=request_reply_path,
            event_stream_path=event_stream_path,
        )

    def to_dict(self) -> dict[str, str]:
        return {
            "schema_version": self.schema_version,
            "transport": self.transport,
            "controller_scope_id": self.controller_scope_id,
            "request_reply_path": str(self.request_reply_path),
            "event_stream_path": str(self.event_stream_path),
        }


@dataclass(frozen=True, slots=True)
class ControllerIntentRequest:
    intent: ExecutionIntent
    requested_at_ns: int
    schema_version: str = TRANSPORT_SCHEMA_VERSION
    transport: str = TRANSPORT_KIND
    channel: str = REQUEST_REPLY_CHANNEL

    def __post_init__(self) -> None:
        object.__setattr__(self, "requested_at_ns", int(self.requested_at_ns))

    def to_dict(self) -> dict[str, object]:
        return {
            "schema_version": self.schema_version,
            "transport": self.transport,
            "channel": self.channel,
            "requested_at_ns": self.requested_at_ns,
            "intent": _intent_to_dict(self.intent),
        }

    @classmethod
    def from_dict(cls, payload: dict[str, object]) -> ControllerIntentRequest:
        intent_payload = dict(payload["intent"])
        return cls(
            intent=_intent_from_dict(intent_payload),
            requested_at_ns=int(payload["requested_at_ns"]),
            schema_version=str(payload["schema_version"]),
            transport=str(payload["transport"]),
            channel=str(payload["channel"]),
        )


@dataclass(frozen=True, slots=True)
class ControllerIntentReply:
    status: str
    replied_at_ns: int
    intent_id: str
    controller_scope_id: str
    strategy_id: str
    claim: ExecutionClaim | None
    reason: str | None = None
    schema_version: str = TRANSPORT_SCHEMA_VERSION
    transport: str = TRANSPORT_KIND
    channel: str = REQUEST_REPLY_CHANNEL

    def __post_init__(self) -> None:
        object.__setattr__(self, "status", _required_text(self.status, "status"))
        object.__setattr__(self, "replied_at_ns", int(self.replied_at_ns))
        object.__setattr__(self, "intent_id", _required_text(self.intent_id, "intent_id"))
        object.__setattr__(
            self,
            "controller_scope_id",
            _required_text(self.controller_scope_id, "controller_scope_id"),
        )
        object.__setattr__(self, "strategy_id", _required_text(self.strategy_id, "strategy_id"))
        if self.status == REPLY_STATUS_ACCEPTED:
            if self.claim is None:
                raise ValueError("accepted replies require `claim`")
            if self.reason is not None:
                raise ValueError("accepted replies cannot carry `reason`")
            if (
                self.intent_id != self.claim.intent_id
                or self.controller_scope_id != self.claim.controller_scope_id
                or self.strategy_id != self.claim.strategy_id
            ):
                raise ValueError("accepted replies must preserve claim identity")
        elif self.status == REPLY_STATUS_REJECTED:
            if self.claim is not None:
                raise ValueError("rejected replies cannot carry `claim`")
            object.__setattr__(self, "reason", _required_text(self.reason or "", "reason"))
        else:
            raise ValueError("`status` must be 'accepted' or 'rejected'")

    @classmethod
    def accepted(cls, *, claim: ExecutionClaim, replied_at_ns: int) -> ControllerIntentReply:
        return cls(
            status=REPLY_STATUS_ACCEPTED,
            replied_at_ns=replied_at_ns,
            intent_id=claim.intent_id,
            controller_scope_id=claim.controller_scope_id,
            strategy_id=claim.strategy_id,
            claim=claim,
            reason=None,
        )

    @classmethod
    def rejected(
        cls,
        *,
        intent: ExecutionIntent,
        reason: str,
        replied_at_ns: int,
    ) -> ControllerIntentReply:
        return cls(
            status=REPLY_STATUS_REJECTED,
            replied_at_ns=replied_at_ns,
            intent_id=intent.intent_id,
            controller_scope_id=intent.controller_scope_id,
            strategy_id=intent.strategy_id,
            claim=None,
            reason=reason,
        )

    def to_dict(self) -> dict[str, object]:
        return {
            "schema_version": self.schema_version,
            "transport": self.transport,
            "channel": self.channel,
            "status": self.status,
            "replied_at_ns": self.replied_at_ns,
            "intent_id": self.intent_id,
            "controller_scope_id": self.controller_scope_id,
            "strategy_id": self.strategy_id,
            "claim": None if self.claim is None else self.claim.to_dict(),
            "reason": self.reason,
        }

    @classmethod
    def from_dict(cls, payload: dict[str, object]) -> ControllerIntentReply:
        claim_payload = payload.get("claim")
        claim = None
        if isinstance(claim_payload, dict):
            claim = ExecutionClaim(**claim_payload)
        return cls(
            status=str(payload["status"]),
            replied_at_ns=int(payload["replied_at_ns"]),
            intent_id=str(payload["intent_id"]),
            controller_scope_id=str(payload["controller_scope_id"]),
            strategy_id=str(payload["strategy_id"]),
            claim=claim,
            reason=None if payload.get("reason") is None else str(payload["reason"]),
            schema_version=str(payload["schema_version"]),
            transport=str(payload["transport"]),
            channel=str(payload["channel"]),
        )


@dataclass(frozen=True, slots=True)
class ControllerLifecycleEventEnvelope:
    event: ExecutionLifecycleEvent
    event_seq: int
    emitted_at_ns: int
    schema_version: str = TRANSPORT_SCHEMA_VERSION
    transport: str = TRANSPORT_KIND
    channel: str = EVENT_STREAM_CHANNEL
    stream: str = EVENT_STREAM_NAME

    def __post_init__(self) -> None:
        object.__setattr__(self, "event_seq", int(self.event_seq))
        object.__setattr__(self, "emitted_at_ns", int(self.emitted_at_ns))

    def to_dict(self) -> dict[str, object]:
        return {
            "schema_version": self.schema_version,
            "transport": self.transport,
            "channel": self.channel,
            "stream": self.stream,
            "event_seq": self.event_seq,
            "emitted_at_ns": self.emitted_at_ns,
            "event": self.event.to_dict(),
        }

    @classmethod
    def from_dict(cls, payload: dict[str, object]) -> ControllerLifecycleEventEnvelope:
        return cls(
            event=ExecutionLifecycleEvent(**dict(payload["event"])),
            event_seq=int(payload["event_seq"]),
            emitted_at_ns=int(payload["emitted_at_ns"]),
            schema_version=str(payload["schema_version"]),
            transport=str(payload["transport"]),
            channel=str(payload["channel"]),
            stream=str(payload["stream"]),
        )


__all__ = (
    "ControllerIntentReply",
    "ControllerIntentRequest",
    "ControllerLifecycleEventEnvelope",
    "EVENT_STREAM_CHANNEL",
    "EVENT_STREAM_NAME",
    "REPLY_STATUS_ACCEPTED",
    "REPLY_STATUS_REJECTED",
    "REQUEST_REPLY_CHANNEL",
    "TRANSPORT_KIND",
    "TRANSPORT_SCHEMA_VERSION",
    "UdsTransportPaths",
)
