from __future__ import annotations

import hashlib
import json
import os
import socket
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

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
SOCKET_CHUNK_SIZE = 65_536


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


def _short_socket_root_dir(root_dir: Path) -> Path:
    digest = hashlib.sha1(os.fsencode(str(root_dir))).hexdigest()[:12]
    # systemd PrivateTmp isolates /tmp per service, so shortened socket paths
    # must live under a shared per-user root to remain visible across units.
    short_root = Path.home() / ".flux-uds" / digest
    short_root.mkdir(parents=True, exist_ok=True)
    return short_root


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
class ControllerIntentCommandPayload:
    command_type: str
    order_role: str
    instrument_id: str
    side: str | None = None
    quantity: str | None = None
    limit_price: str | None = None
    post_only: bool | None = None
    time_in_force: str | None = None
    target_client_order_id: str | None = None
    route: str | None = None
    outside_rth: bool | None = None
    include_overnight: bool | None = None
    cancel_after_ms: int | None = None

    def __post_init__(self) -> None:
        object.__setattr__(self, "command_type", _required_text(self.command_type, "command_type"))
        object.__setattr__(self, "order_role", _required_text(self.order_role, "order_role"))
        object.__setattr__(self, "instrument_id", _required_text(self.instrument_id, "instrument_id"))
        if self.side is not None:
            object.__setattr__(self, "side", _required_text(self.side, "side"))
        if self.quantity is not None:
            object.__setattr__(self, "quantity", _required_text(self.quantity, "quantity"))
        if self.limit_price is not None:
            object.__setattr__(self, "limit_price", _required_text(self.limit_price, "limit_price"))
        if self.time_in_force is not None:
            object.__setattr__(
                self,
                "time_in_force",
                _required_text(self.time_in_force, "time_in_force"),
            )
        if self.target_client_order_id is not None:
            object.__setattr__(
                self,
                "target_client_order_id",
                _required_text(self.target_client_order_id, "target_client_order_id"),
            )
        if self.route is not None:
            object.__setattr__(self, "route", _required_text(self.route, "route"))
        if self.cancel_after_ms is not None:
            object.__setattr__(self, "cancel_after_ms", int(self.cancel_after_ms))

    def to_dict(self) -> dict[str, object]:
        return {
            "command_type": self.command_type,
            "order_role": self.order_role,
            "instrument_id": self.instrument_id,
            "side": self.side,
            "quantity": self.quantity,
            "limit_price": self.limit_price,
            "post_only": self.post_only,
            "time_in_force": self.time_in_force,
            "target_client_order_id": self.target_client_order_id,
            "route": self.route,
            "outside_rth": self.outside_rth,
            "include_overnight": self.include_overnight,
            "cancel_after_ms": self.cancel_after_ms,
        }

    @classmethod
    def from_dict(cls, payload: dict[str, object]) -> ControllerIntentCommandPayload:
        return cls(
            command_type=str(payload["command_type"]),
            order_role=str(payload["order_role"]),
            instrument_id=str(payload["instrument_id"]),
            side=None if payload.get("side") is None else str(payload["side"]),
            quantity=None if payload.get("quantity") is None else str(payload["quantity"]),
            limit_price=None if payload.get("limit_price") is None else str(payload["limit_price"]),
            post_only=None if payload.get("post_only") is None else bool(payload["post_only"]),
            time_in_force=(
                None if payload.get("time_in_force") is None else str(payload["time_in_force"])
            ),
            target_client_order_id=(
                None
                if payload.get("target_client_order_id") is None
                else str(payload["target_client_order_id"])
            ),
            route=None if payload.get("route") is None else str(payload["route"]),
            outside_rth=None if payload.get("outside_rth") is None else bool(payload["outside_rth"]),
            include_overnight=(
                None if payload.get("include_overnight") is None else bool(payload["include_overnight"])
            ),
            cancel_after_ms=(
                None if payload.get("cancel_after_ms") is None else int(payload["cancel_after_ms"])
            ),
        )

    @classmethod
    def from_command(cls, command: Any) -> ControllerIntentCommandPayload:
        return cls(
            command_type=str(getattr(command, "command_type")),
            order_role=str(getattr(command, "order_role")),
            instrument_id=str(getattr(command, "instrument_id")),
            side=None if getattr(command, "side", None) is None else str(getattr(command, "side")),
            quantity=(
                None if getattr(command, "quantity", None) is None else str(getattr(command, "quantity"))
            ),
            limit_price=(
                None
                if getattr(command, "limit_price", None) is None
                else str(getattr(command, "limit_price"))
            ),
            post_only=getattr(command, "post_only", None),
            time_in_force=(
                None
                if getattr(command, "time_in_force", None) is None
                else str(getattr(command, "time_in_force"))
            ),
            target_client_order_id=(
                None
                if getattr(command, "target_client_order_id", None) is None
                else str(getattr(command, "target_client_order_id"))
            ),
            route=None if getattr(command, "route", None) is None else str(getattr(command, "route")),
            outside_rth=getattr(command, "outside_rth", None),
            include_overnight=getattr(command, "include_overnight", None),
            cancel_after_ms=getattr(command, "cancel_after_ms", None),
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
        try:
            _assert_af_unix_path_length(request_reply_path)
            _assert_af_unix_path_length(event_stream_path)
        except ValueError:
            root = _short_socket_root_dir(root)
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
    command: ControllerIntentCommandPayload | None = None
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
            "command": None if self.command is None else self.command.to_dict(),
        }

    @classmethod
    def from_dict(cls, payload: dict[str, object]) -> ControllerIntentRequest:
        intent_payload = dict(payload["intent"])
        command_payload = payload.get("command")
        return cls(
            intent=_intent_from_dict(intent_payload),
            requested_at_ns=int(payload["requested_at_ns"]),
            command=(
                None
                if not isinstance(command_payload, dict)
                else ControllerIntentCommandPayload.from_dict(command_payload)
            ),
            schema_version=str(payload["schema_version"]),
            transport=str(payload["transport"]),
            channel=str(payload["channel"]),
        )

    @classmethod
    def from_command(
        cls,
        *,
        command: Any,
        requested_at_ns: int,
    ) -> ControllerIntentRequest:
        return cls(
            intent=getattr(command, "intent"),
            requested_at_ns=requested_at_ns,
            command=ControllerIntentCommandPayload.from_command(command),
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


def encode_request_frame(request: ControllerIntentRequest) -> bytes:
    return _encode_frame(request.to_dict())


def decode_request_frame(frame: bytes) -> ControllerIntentRequest:
    return ControllerIntentRequest.from_dict(_decode_frame(frame))


def encode_reply_frame(reply: ControllerIntentReply) -> bytes:
    return _encode_frame(reply.to_dict())


def decode_reply_frame(frame: bytes) -> ControllerIntentReply:
    return ControllerIntentReply.from_dict(_decode_frame(frame))


def send_request(
    *,
    paths: UdsTransportPaths,
    request: ControllerIntentRequest,
    timeout_s: float,
) -> ControllerIntentReply:
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        sock.settimeout(float(timeout_s))
        sock.connect(str(paths.request_reply_path))
        sock.sendall(encode_request_frame(request))
        sock.shutdown(socket.SHUT_WR)
        return decode_reply_frame(_recv_frame(sock))
    finally:
        sock.close()


def _encode_frame(payload: dict[str, object]) -> bytes:
    return (json.dumps(payload, separators=(",", ":"), sort_keys=True) + "\n").encode("utf-8")


def _decode_frame(frame: bytes) -> dict[str, object]:
    text = frame.decode("utf-8").strip()
    if not text:
        raise ValueError("transport frame must not be empty")
    decoded = json.loads(text)
    if not isinstance(decoded, dict):
        raise ValueError("transport frame must decode to a mapping")
    return decoded


def _recv_frame(sock: socket.socket) -> bytes:
    chunks: list[bytes] = []
    while True:
        chunk = sock.recv(SOCKET_CHUNK_SIZE)
        if not chunk:
            break
        chunks.append(chunk)
        if chunk.endswith(b"\n"):
            break
    return b"".join(chunks)


__all__ = (
    "ControllerIntentCommandPayload",
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
    "decode_reply_frame",
    "decode_request_frame",
    "encode_reply_frame",
    "encode_request_frame",
    "send_request",
)
