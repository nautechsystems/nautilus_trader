from __future__ import annotations

import errno
import fcntl
import json
import sys
import uuid
from contextlib import contextmanager
from dataclasses import asdict
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable


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
    lease_epoch: int
    acquired_at_ms: int
    refreshed_at_ms: int
    lease_ttl_ms: int

    @property
    def expires_at_ms(self) -> int:
        return self.refreshed_at_ms + self.lease_ttl_ms

    def is_stale(self, *, now_ms: int) -> bool:
        return int(now_ms) > self.expires_at_ms


@dataclass(slots=True)
class ControllerIngressClaim:
    controller_scope_id: str
    owner_id: str
    _handle: object

    def release(self) -> None:
        handle = self._handle
        if handle is None:
            return
        try:
            fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
        finally:
            handle.close()
            self._handle = None


class LocalControllerLeaseStore:
    def __init__(
        self,
        *,
        root_dir: str | Path,
        replica_root_dirs: Iterable[str | Path] = (),
    ) -> None:
        self._root_dir = Path(root_dir)
        self._root_dir.mkdir(parents=True, exist_ok=True)
        self._replica_root_dirs = tuple(Path(path) for path in replica_root_dirs)
        for root in self._replica_root_dirs:
            root.mkdir(parents=True, exist_ok=True)

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
        lease_token: str | None = None,
        lease_epoch: int | None = None,
    ) -> ControllerLease:
        scope = _required_text(controller_scope_id, "controller_scope_id")
        owner = _required_text(owner_id, "owner_id")
        timestamp_ms = int(now_ms)
        ttl_ms = max(1, int(lease_ttl_ms))
        with self._locked_scope(scope) as (handle, current):
            replica_currents = tuple(self._replica_currents(scope))
            all_currents = tuple(lease for lease in (current, *replica_currents) if lease is not None)
            if any(not lease.is_stale(now_ms=timestamp_ms) for lease in all_currents):
                raise ControllerLeaseRejectedError(
                    f"controller scope `{scope}` is already owned by `{all_currents[0].owner_id}`",
                )
            next_epoch = (
                int(lease_epoch)
                if lease_epoch is not None
                else max((lease.lease_epoch for lease in all_currents), default=0) + 1
            )
            token = _required_text(lease_token, "lease_token") if lease_token is not None else uuid.uuid4().hex
            acquired_at_ms = timestamp_ms
            lease = ControllerLease(
                controller_scope_id=scope,
                owner_id=owner,
                lease_token=token,
                lease_epoch=max(1, next_epoch),
                acquired_at_ms=acquired_at_ms,
                refreshed_at_ms=timestamp_ms,
                lease_ttl_ms=ttl_ms,
            )
            _write_lease(handle, lease)
            self._write_replicas(scope, lease)
            return lease

    def claim_ingress(
        self,
        *,
        controller_scope_id: str,
        owner_id: str,
    ) -> ControllerIngressClaim:
        scope = _required_text(controller_scope_id, "controller_scope_id")
        owner = _required_text(owner_id, "owner_id")
        path = self._scope_lock_path(scope)
        path.parent.mkdir(parents=True, exist_ok=True)
        handle = path.open("a+", encoding="utf-8")
        try:
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as exc:
            handle.close()
            if exc.errno in (errno.EACCES, errno.EAGAIN):
                raise ControllerLeaseRejectedError(
                    f"controller scope `{scope}` already running on this host",
                ) from exc
            raise
        return ControllerIngressClaim(
            controller_scope_id=scope,
            owner_id=owner,
            _handle=handle,
        )

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
                lease_epoch=lease.lease_epoch,
                acquired_at_ms=lease.acquired_at_ms,
                refreshed_at_ms=int(now_ms),
                lease_ttl_ms=lease.lease_ttl_ms,
            )
            _write_lease(handle, refreshed)
            self._write_replicas(_required_text(controller_scope_id, "controller_scope_id"), refreshed)
            return refreshed

    def assert_can_write(
        self,
        *,
        controller_scope_id: str,
        lease_token: str,
        now_ms: int,
    ) -> ControllerLease:
        scope = _required_text(controller_scope_id, "controller_scope_id")
        current = self.current(scope)
        lease = self._require_current(
            current=current,
            controller_scope_id=scope,
            lease_token=lease_token,
            now_ms=now_ms,
        )
        self._assert_replica_alignment(
            controller_scope_id=scope,
            lease=lease,
            now_ms=now_ms,
        )
        return lease

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
            self._clear_replicas(_required_text(controller_scope_id, "controller_scope_id"), current)

    def current(self, controller_scope_id: str) -> ControllerLease | None:
        scope = _required_text(controller_scope_id, "controller_scope_id")
        return _latest_lease(tuple(self._all_currents(scope)))

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

    def _scope_lock_path(self, controller_scope_id: str) -> Path:
        if "/" in controller_scope_id or "\x00" in controller_scope_id:
            raise ValueError("`controller_scope_id` must be a valid lease path component")
        return self._root_dir / f"{controller_scope_id}.lock"

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

    def _scope_path_for_root(self, root_dir: Path, controller_scope_id: str) -> Path:
        if "/" in controller_scope_id or "\x00" in controller_scope_id:
            raise ValueError("`controller_scope_id` must be a valid lease path component")
        return root_dir / f"{controller_scope_id}.json"

    def _all_currents(self, controller_scope_id: str) -> Iterable[ControllerLease | None]:
        yield self._read_current_from_root(self._root_dir, controller_scope_id)
        for root_dir in self._replica_root_dirs:
            yield self._read_current_from_root(root_dir, controller_scope_id)

    def _replica_currents(self, controller_scope_id: str) -> Iterable[ControllerLease | None]:
        for root_dir in self._replica_root_dirs:
            yield self._read_current_from_root(root_dir, controller_scope_id)

    def _read_current_from_root(
        self,
        root_dir: Path,
        controller_scope_id: str,
    ) -> ControllerLease | None:
        path = self._scope_path_for_root(root_dir, controller_scope_id)
        if not path.exists():
            return None
        with path.open("a+", encoding="utf-8") as handle:
            handle.seek(0)
            return _read_lease(handle.read())

    def _write_replicas(self, controller_scope_id: str, lease: ControllerLease) -> None:
        for root_dir in self._replica_root_dirs:
            path = self._scope_path_for_root(root_dir, controller_scope_id)
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open("a+", encoding="utf-8") as handle:
                _write_lease(handle, lease)

    def _clear_replicas(self, controller_scope_id: str, lease: ControllerLease) -> None:
        for root_dir in self._replica_root_dirs:
            path = self._scope_path_for_root(root_dir, controller_scope_id)
            if not path.exists():
                continue
            with path.open("a+", encoding="utf-8") as handle:
                handle.seek(0)
                current = _read_lease(handle.read())
                if current is None:
                    continue
                if current.lease_token != lease.lease_token or current.lease_epoch != lease.lease_epoch:
                    continue
                handle.seek(0)
                handle.truncate()
                handle.flush()

    def _assert_replica_alignment(
        self,
        *,
        controller_scope_id: str,
        lease: ControllerLease,
        now_ms: int,
    ) -> None:
        for current in self._replica_currents(controller_scope_id):
            if current is None:
                raise StaleControllerWriterError(
                    f"controller scope `{controller_scope_id}` replica state diverged",
                )
            if current.lease_token != lease.lease_token or current.lease_epoch != lease.lease_epoch:
                raise StaleControllerWriterError(
                    f"controller scope `{controller_scope_id}` replica state diverged: different lease token or lease epoch",
                )
            if current.is_stale(now_ms=int(now_ms)):
                raise StaleControllerWriterError(
                    f"controller scope `{controller_scope_id}` replica state diverged: lease is stale",
                )


class ReplicatedControllerLeaseStore:
    def __init__(self, *, root_dirs: Iterable[str | Path]) -> None:
        self._stores = tuple(LocalControllerLeaseStore(root_dir=path) for path in root_dirs)
        if not self._stores:
            raise ValueError("`root_dirs` must contain at least one lease root")

    def acquire(
        self,
        *,
        controller_scope_id: str,
        owner_id: str,
        now_ms: int,
        lease_ttl_ms: int,
    ) -> ControllerLease:
        active = [
            lease
            for lease in (store.current(controller_scope_id) for store in self._stores)
            if lease is not None
        ]
        if any(not lease.is_stale(now_ms=int(now_ms)) for lease in active):
            raise ControllerLeaseRejectedError(
                f"controller scope `{controller_scope_id}` is already owned by `{active[0].owner_id}`",
            )
        shared_epoch = max((lease.lease_epoch for lease in active), default=0) + 1
        shared_token = uuid.uuid4().hex
        acquired: list[tuple[LocalControllerLeaseStore, ControllerLease]] = []
        try:
            for store in self._stores:
                lease = store.acquire(
                    controller_scope_id=controller_scope_id,
                    owner_id=owner_id,
                    now_ms=now_ms,
                    lease_ttl_ms=lease_ttl_ms,
                    lease_token=shared_token,
                    lease_epoch=shared_epoch,
                )
                acquired.append((store, lease))
        except Exception:
            for store, lease in acquired:
                store.release(
                    controller_scope_id=controller_scope_id,
                    lease_token=lease.lease_token,
                )
            raise
        return acquired[0][1]

    def refresh(
        self,
        *,
        controller_scope_id: str,
        lease_token: str,
        now_ms: int,
    ) -> ControllerLease:
        refreshed: list[ControllerLease] = []
        for store in self._stores:
            refreshed.append(
                store.refresh(
                    controller_scope_id=controller_scope_id,
                    lease_token=lease_token,
                    now_ms=now_ms,
                )
            )
        self._assert_consistent(controller_scope_id=controller_scope_id, leases=refreshed)
        return refreshed[0]

    def assert_can_write(
        self,
        *,
        controller_scope_id: str,
        lease_token: str,
        now_ms: int,
    ) -> ControllerLease:
        current: list[ControllerLease] = []
        for store in self._stores:
            try:
                current.append(
                    store.assert_can_write(
                        controller_scope_id=controller_scope_id,
                        lease_token=lease_token,
                        now_ms=now_ms,
                    )
                )
            except StaleControllerWriterError as exc:
                raise StaleControllerWriterError(
                    f"controller scope `{controller_scope_id}` replica state diverged: {exc}",
                ) from exc
        self._assert_consistent(controller_scope_id=controller_scope_id, leases=current)
        return current[0]

    def release(
        self,
        *,
        controller_scope_id: str,
        lease_token: str,
    ) -> None:
        for store in self._stores:
            store.release(
                controller_scope_id=controller_scope_id,
                lease_token=lease_token,
            )

    def current(self, controller_scope_id: str) -> ControllerLease | None:
        leases = [
            lease
            for lease in (store.current(controller_scope_id) for store in self._stores)
            if lease is not None
        ]
        if not leases:
            return None
        self._assert_consistent(controller_scope_id=controller_scope_id, leases=leases)
        return leases[0]

    @staticmethod
    def _assert_consistent(
        *,
        controller_scope_id: str,
        leases: Iterable[ControllerLease],
    ) -> None:
        lease_list = list(leases)
        if not lease_list:
            raise StaleControllerWriterError(
                f"controller scope `{controller_scope_id}` replica state diverged",
            )
        first = lease_list[0]
        for lease in lease_list[1:]:
            if lease.lease_token != first.lease_token or lease.lease_epoch != first.lease_epoch:
                raise StaleControllerWriterError(
                    f"controller scope `{controller_scope_id}` replica state diverged",
                )

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
        lease_epoch=max(1, int(data.get("lease_epoch", 1))),
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


def _latest_lease(candidates: Iterable[ControllerLease | None]) -> ControllerLease | None:
    known = [lease for lease in candidates if lease is not None]
    if not known:
        return None
    return max(
        known,
        key=lambda lease: (lease.lease_epoch, lease.refreshed_at_ms, lease.acquired_at_ms),
    )


__all__ = (
    "ControllerIngressClaim",
    "ControllerLease",
    "ControllerLeaseError",
    "ControllerLeaseRejectedError",
    "LocalControllerLeaseStore",
    "ReplicatedControllerLeaseStore",
    "StaleControllerWriterError",
)
