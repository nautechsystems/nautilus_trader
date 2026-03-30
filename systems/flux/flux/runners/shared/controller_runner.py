from __future__ import annotations

import sys
import time
from dataclasses import dataclass
from typing import Protocol

from flux.execution.controller import ControllerIngressPolicy
from flux.execution.controller import ControllerRunMode
from flux.execution.leases import ControllerIngressClaim
from flux.execution.leases import ControllerLease
from flux.execution.leases import LocalControllerLeaseStore


if __name__ == "flux.runners.shared.controller_runner":
    sys.modules.setdefault("nautilus_trader.flux.runners.shared.controller_runner", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.runners.shared.controller_runner":
    sys.modules.setdefault("flux.runners.shared.controller_runner", sys.modules[__name__])


class ControllerService(Protocol):
    def start(self) -> None: ...

    def stop(self) -> None: ...


@dataclass(frozen=True, slots=True)
class ControllerRunnerConfig:
    controller_scope_id: str
    owner_id: str
    run_mode: ControllerRunMode | str = ControllerRunMode.SHADOW
    ingress_policy: ControllerIngressPolicy | str = ControllerIngressPolicy.SINGLE_HOST_CANARY
    allow_single_host_canary: bool = False
    lease_ttl_ms: int = 250

    def __post_init__(self) -> None:
        object.__setattr__(self, "controller_scope_id", _required_text(self.controller_scope_id, "controller_scope_id"))
        object.__setattr__(self, "owner_id", _required_text(self.owner_id, "owner_id"))
        object.__setattr__(self, "lease_ttl_ms", max(1, int(self.lease_ttl_ms)))
        if not isinstance(self.run_mode, ControllerRunMode):
            object.__setattr__(self, "run_mode", ControllerRunMode(_required_text(self.run_mode, "run_mode")))
        if not isinstance(self.ingress_policy, ControllerIngressPolicy):
            object.__setattr__(
                self,
                "ingress_policy",
                ControllerIngressPolicy(_required_text(self.ingress_policy, "ingress_policy")),
            )


class ShadowControllerRunner:
    def __init__(
        self,
        *,
        config: ControllerRunnerConfig,
        lease_store: LocalControllerLeaseStore,
        controller_service: ControllerService,
    ) -> None:
        self.config = config
        self.lease_store = lease_store
        self._controller_service = controller_service
        self._ingress_claim: ControllerIngressClaim | None = None
        self._lease: ControllerLease | None = None
        self._running = False

    @property
    def running(self) -> bool:
        return self._running

    def start(self, *, now_ms: int | None = None) -> ControllerLease:
        if self._running and self._lease is not None:
            return self._lease
        _validate_single_host_canary_gate(self.config)
        claim = self.lease_store.claim_ingress(
            controller_scope_id=self.config.controller_scope_id,
            owner_id=self.config.owner_id,
        )
        try:
            timestamp_ms = _now_ms(now_ms)
            lease = self.lease_store.acquire(
                controller_scope_id=self.config.controller_scope_id,
                owner_id=self.config.owner_id,
                now_ms=timestamp_ms,
                lease_ttl_ms=self.config.lease_ttl_ms,
            )
            self.lease_store.assert_can_write(
                controller_scope_id=self.config.controller_scope_id,
                lease_token=lease.lease_token,
                now_ms=timestamp_ms,
            )
            self._controller_service.start()
        except Exception:
            if "lease" in locals():
                self.lease_store.release(
                    controller_scope_id=self.config.controller_scope_id,
                    lease_token=lease.lease_token,
                )
            claim.release()
            raise
        self._ingress_claim = claim
        self._lease = lease
        self._running = True
        return lease

    def stop(self) -> None:
        if not self._running:
            return
        try:
            self._controller_service.stop()
        finally:
            if self._lease is not None:
                self.lease_store.release(
                    controller_scope_id=self.config.controller_scope_id,
                    lease_token=self._lease.lease_token,
                )
            if self._ingress_claim is not None:
                self._ingress_claim.release()
                self._ingress_claim = None
            self._lease = None
            self._running = False

    def assert_can_write(self, *, now_ms: int | None = None) -> ControllerLease:
        if self._lease is None:
            raise RuntimeError("controller runner is not started")
        return self.lease_store.assert_can_write(
            controller_scope_id=self.config.controller_scope_id,
            lease_token=self._lease.lease_token,
            now_ms=_now_ms(now_ms),
        )


def _validate_single_host_canary_gate(config: ControllerRunnerConfig) -> None:
    if config.run_mode is not ControllerRunMode.SHADOW:
        raise ValueError("Task 4 controller runner only supports `shadow` mode")
    if config.ingress_policy is not ControllerIngressPolicy.SINGLE_HOST_CANARY:
        raise ValueError("controller runner requires the single-host canary ingress policy")
    if not config.allow_single_host_canary:
        raise ValueError("single-host canary gating must be explicitly enabled")


def _required_text(value: str, field_name: str) -> str:
    text = str(value).strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    return text


def _now_ms(value: int | None) -> int:
    if value is not None:
        return int(value)
    return int(time.time() * 1_000)


__all__ = (
    "ControllerRunnerConfig",
    "ControllerService",
    "ShadowControllerRunner",
)
