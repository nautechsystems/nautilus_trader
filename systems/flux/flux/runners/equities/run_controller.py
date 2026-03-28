#!/usr/bin/env python3
from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import Any
from typing import Callable

from flux.execution.controller import ControllerRunMode
from flux.execution.leases import LocalControllerLeaseStore
from flux.runners.equities.run_node import _repo_root as equities_repo_root
from flux.runners.shared.bootstrap import load_config as load_shared_config
from flux.runners.shared.bootstrap import load_runtime_config as load_shared_runtime_config
from flux.runners.shared.controller_runner import ControllerRunnerConfig
from flux.runners.shared.controller_runner import ShadowControllerRunner
from flux.runners.shared.strategy_set import get_strategy_set_descriptor


if __name__ == "flux.runners.equities.run_controller":
    sys.modules.setdefault("nautilus_trader.flux.runners.equities.run_controller", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.runners.equities.run_controller":
    sys.modules.setdefault("flux.runners.equities.run_controller", sys.modules[__name__])


EQUITIES_DESCRIPTOR = get_strategy_set_descriptor("equities")


class _NullControllerService:
    def start(self) -> None:
        return None

    def stop(self) -> None:
        return None


def _repo_root() -> Path:
    return equities_repo_root()


def _load_config(path: Path) -> dict[str, Any]:
    return load_shared_config(path, env_prefix=EQUITIES_DESCRIPTOR.env_prefix)


def _load_runtime_config(path: Path, *, shared_config_path: Path | None = None) -> dict[str, Any]:
    return load_shared_runtime_config(
        path,
        shared_config_path=shared_config_path,
        load_config=_load_config,
        table_names=("redis", "strategy_contracts", "account_scopes", "controller"),
    )


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the Equities shadow controller scaffold.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--shared-config", type=Path, default=None)
    parser.add_argument("--owner-id", default=None)
    parser.add_argument("--allow-single-host-canary", action="store_true")
    return parser.parse_args()


def build_runner(
    config: dict[str, Any],
    *,
    owner_id: str | None = None,
    repo_root: Path | None = None,
    lease_store: LocalControllerLeaseStore | None = None,
    controller_service_factory: Callable[[dict[str, Any]], Any] | None = None,
) -> ShadowControllerRunner:
    controller_cfg = _table(config, "controller")
    scope_id = _required_text(controller_cfg.get("controller_scope_id"), "controller_scope_id")
    allow_single_host_canary = bool(controller_cfg.get("allow_single_host_canary", False))
    run_mode = ControllerRunMode(str(controller_cfg.get("mode", ControllerRunMode.SHADOW.value)).strip())
    if run_mode is not ControllerRunMode.SHADOW:
        raise ValueError("Task 4 controller runner only supports `shadow` mode")
    if not allow_single_host_canary:
        raise ValueError("single-host canary gating must be explicitly enabled")
    root = repo_root or _repo_root()
    effective_owner_id = _required_text(
        owner_id or controller_cfg.get("owner_id") or f"equities-controller:{scope_id}",
        "owner_id",
    )
    store = lease_store or LocalControllerLeaseStore(
        root_dir=root / ".run" / "equities-controller-leases",
    )
    service_factory = controller_service_factory or (lambda _config: _NullControllerService())
    return ShadowControllerRunner(
        config=ControllerRunnerConfig(
            controller_scope_id=scope_id,
            owner_id=effective_owner_id,
            run_mode=run_mode,
            allow_single_host_canary=allow_single_host_canary,
            lease_ttl_ms=int(controller_cfg.get("lease_ttl_ms", 250)),
        ),
        lease_store=store,
        controller_service=service_factory(config),
    )


def main() -> None:
    args = _parse_args()
    config = _load_runtime_config(args.config, shared_config_path=args.shared_config)
    controller_cfg = _table(config, "controller")
    if args.allow_single_host_canary:
        controller_cfg["allow_single_host_canary"] = True
    runner = build_runner(
        config,
        owner_id=_optional_text(args.owner_id),
    )
    runner.start()
    runner.stop()


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    value = data.get(name, {})
    if not isinstance(value, dict):
        raise ValueError(f"[{name}] must be a TOML table")
    return value


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _required_text(value: Any, field_name: str) -> str:
    text = _optional_text(value)
    if text is None:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    return text


__all__ = (
    "ControllerRunMode",
    "build_runner",
    "main",
)


if __name__ == "__main__":
    main()
