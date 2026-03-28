from __future__ import annotations

import fcntl
import json
import sys
import uuid
from contextlib import contextmanager
from dataclasses import asdict
from dataclasses import dataclass
from pathlib import Path


if __name__ == "flux.execution.leases":
    sys.modules.setdefault("nautilus_trader.flux.execution.leases", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.execution.leases":
    sys.modules.setdefault("flux.execution.leases", sys.modules[__name__])


class ControllerLeaseError(RuntimeError):
    pass


class ControllerLeaseRejectedError(ControllerLeaseError):
    pass


class StaleControllerWriterError(ControllerLeaseError):
    pass


@dataclass(frozen=True, slots=True)
class ControllerLease:
    controller_scope_id: str
    owner_id: str
    lease_token: str
    acquired_at_ms: int
    refreshed_at_ms: int
    lease_ttl_ms: int

    @property
    def expires_at_ms(self) -> int:
        return self.refreshed_at_ms + self.lease_ttl_ms

    def is_stale(self, *, now_ms: int) -> bool:
        return int(now_ms) > self.expires_at_ms


class LocalControllerLeaseStore:
    def __init__(self, *, root_dir: str | Path) -> None:
        self._root_dir = Path(root_dir)
        self._root_dir.mkdir(parents=True, exist_ok=True)

    @property
    def root_dir(self) -> Path:
        return self._root_dir

    def acquire(
        self,
        *,
        controller_scope_id: str,
        owner_id: str,
        now_ms: int,
        lease_ttl_ms: int,
    ) -> ControllerLease:
        scope = _required_text(controller_scope_id, "controller_scope_id")
        owner = _required_text(owner_id, "owner_id")
        timestamp_ms = int(now_ms)
        ttl_ms = max(1, int(lease_ttl_ms))
        with self._locked_scope(scope) as (handle, current):
            if current is not None and not current.is_stale(now_ms=timestamp_ms):
                if current.owner_id != owner:
                    raise ControllerLeaseRejectedError(
                        f"controller scope `{scope}` is already owned by `{current.owner_id}`",
                    )
                lease_token = current.lease_token
                acquired_at_ms = current.acquired_at_ms
            else:
                lease_token = uuid.uuid4().hex
                acquired_at_ms = timestamp_ms
            lease = ControllerLease(
                controller_scope_id=scope,
                owner_id=owner,
                lease_token=lease_token,
                acquired_at_ms=acquired_at_ms,
                refreshed_at_ms=timestamp_ms,
                lease_ttl_ms=ttl_ms,
            )
            _write_lease(handle, lease)
            return lease

    def refresh(
        self,
        *,
        controller_scope_id: str,
        lease_token: str,
        now_ms: int,
    ) -> ControllerLease:
        with self._locked_scope(controller_scope_id) as (handle, current):
            lease = self._require_current(
                current=current,
                controller_scope_id=controller_scope_id,
                lease_token=lease_token,
                now_ms=now_ms,
            )
            refreshed = ControllerLease(
                controller_scope_id=lease.controller_scope_id,
                owner_id=lease.owner_id,
                lease_token=lease.lease_token,
                acquired_at_ms=lease.acquired_at_ms,
                refreshed_at_ms=int(now_ms),
                lease_ttl_ms=lease.lease_ttl_ms,
            )
            _write_lease(handle, refreshed)
            return refreshed

    def assert_can_write(
        self,
        *,
        controller_scope_id: str,
        lease_token: str,
        now_ms: int,
    ) -> ControllerLease:
        current = self.current(controller_scope_id)
        return self._require_current(
            current=current,
            controller_scope_id=controller_scope_id,
            lease_token=lease_token,
            now_ms=now_ms,
        )

    def release(
        self,
        *,
        controller_scope_id: str,
        lease_token: str,
    ) -> None:
        with self._locked_scope(controller_scope_id) as (handle, current):
            if current is None or current.lease_token != _required_text(lease_token, "lease_token"):
                return
            handle.seek(0)
            handle.truncate()
            handle.flush()

    def current(self, controller_scope_id: str) -> ControllerLease | None:
        scope = _required_text(controller_scope_id, "controller_scope_id")
        path = self._scope_path(scope)
        if not path.exists():
            return None
        with path.open("a+", encoding="utf-8") as handle:
            handle.seek(0)
            return _read_lease(handle.read())

    @contextmanager
    def _locked_scope(self, controller_scope_id: str):
        scope = _required_text(controller_scope_id, "controller_scope_id")
        path = self._scope_path(scope)
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("a+", encoding="utf-8") as handle:
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
            try:
                handle.seek(0)
                yield handle, _read_lease(handle.read())
            finally:
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)

    def _scope_path(self, controller_scope_id: str) -> Path:
        if "/" in controller_scope_id or "\x00" in controller_scope_id:
            raise ValueError("`controller_scope_id` must be a valid lease path component")
        return self._root_dir / f"{controller_scope_id}.json"

    @staticmethod
    def _require_current(
        *,
        current: ControllerLease | None,
        controller_scope_id: str,
        lease_token: str,
        now_ms: int,
    ) -> ControllerLease:
        scope = _required_text(controller_scope_id, "controller_scope_id")
        token = _required_text(lease_token, "lease_token")
        if current is None:
            raise StaleControllerWriterError(f"controller scope `{scope}` has no active lease")
        if current.lease_token != token:
            raise StaleControllerWriterError(
                f"controller scope `{scope}` is owned by a different lease token",
            )
        if current.is_stale(now_ms=int(now_ms)):
            raise StaleControllerWriterError(f"controller scope `{scope}` lease is stale")
        return current


def _read_lease(payload: str) -> ControllerLease | None:
    raw = payload.strip()
    if not raw:
        return None
    data = json.loads(raw)
    return ControllerLease(
        controller_scope_id=_required_text(data["controller_scope_id"], "controller_scope_id"),
        owner_id=_required_text(data["owner_id"], "owner_id"),
        lease_token=_required_text(data["lease_token"], "lease_token"),
        acquired_at_ms=int(data["acquired_at_ms"]),
        refreshed_at_ms=int(data["refreshed_at_ms"]),
        lease_ttl_ms=max(1, int(data["lease_ttl_ms"])),
    )


def _write_lease(handle, lease: ControllerLease) -> None:
    handle.seek(0)
    handle.truncate()
    handle.write(json.dumps(asdict(lease), sort_keys=True, separators=(",", ":")))
    handle.flush()


def _required_text(value: str, field_name: str) -> str:
    text = str(value).strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    return text


__all__ = (
    "ControllerLease",
    "ControllerLeaseError",
    "ControllerLeaseRejectedError",
    "LocalControllerLeaseStore",
    "StaleControllerWriterError",
)
