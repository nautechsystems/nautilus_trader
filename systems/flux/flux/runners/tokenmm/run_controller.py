#!/usr/bin/env python3

from __future__ import annotations

import argparse
import signal
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from flux.execution.controller import ControllerRunMode
from flux.execution.leases import LocalControllerLeaseStore
from flux.runners.shared.bootstrap import load_config as load_shared_config
from flux.runners.shared.bootstrap import table as shared_table
from flux.runners.shared.controller_runner import ControllerRunnerConfig
from flux.runners.shared.controller_runner import ShadowControllerRunner
from flux.runners.shared.logging import configure_python_logging
from flux.runners.shared.logging import emit_startup_banner
from flux.runners.shared.strategy_set import get_strategy_set_descriptor


if __name__ == "flux.runners.tokenmm.run_controller":
    sys.modules.setdefault("nautilus_trader.flux.runners.tokenmm.run_controller", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.runners.tokenmm.run_controller":
    sys.modules.setdefault("flux.runners.tokenmm.run_controller", sys.modules[__name__])


SAFE_MODES = frozenset({"paper", "testnet", "live"})
TOKENMM_DESCRIPTOR = get_strategy_set_descriptor("tokenmm")


@dataclass(frozen=True, slots=True)
class TokenmmControllerContract:
    controller_scope_id: str
    account_scope_id: str
    managed_strategy_ids: tuple[str, ...]
    mode: ControllerRunMode
    write_ownership_enabled: bool


class _TokenmmControllerService:
    def __init__(self, *, contract: TokenmmControllerContract) -> None:
        self._contract = contract
        self._running = False

    def start(self) -> None:
        if self._running:
            return
        self._running = True

    def stop(self) -> None:
        self._running = False


class TokenmmControllerRunner(ShadowControllerRunner):
    def start(self, *, now_ms: int | None = None):
        if self._running and self._lease is not None:
            return self._lease
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

    def refresh(self, *, now_ms: int | None = None):
        if self._lease is None:
            raise RuntimeError("controller runner is not started")
        timestamp_ms = _now_ms(now_ms)
        refreshed = self.lease_store.refresh(
            controller_scope_id=self.config.controller_scope_id,
            lease_token=self._lease.lease_token,
            now_ms=timestamp_ms,
        )
        self._lease = refreshed
        return refreshed


def _load_config(path: Path) -> dict[str, Any]:
    return load_shared_config(path, env_prefix=TOKENMM_DESCRIPTOR.env_prefix)


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run the TokenMM shared-Binance controller lane.",
    )
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--owner-id", default=None)
    parser.add_argument("--log-level", default=None)
    return parser.parse_args()


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    return shared_table(data, name)


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


def _resolve_mode(config: dict[str, Any], args: argparse.Namespace) -> str:
    flux = _table(config, "flux")
    mode = str(args.mode or flux.get("mode", "paper")).strip().lower()
    if mode not in SAFE_MODES:
        raise ValueError(f"Invalid mode {mode!r}; expected one of {sorted(SAFE_MODES)}")
    if mode == "live" and not args.confirm_live:
        raise ValueError("Live mode requires explicit --confirm-live")
    return mode


def _coerce_strategy_ids(raw_value: Any) -> tuple[str, ...]:
    if isinstance(raw_value, str) or not isinstance(raw_value, list | tuple):
        raise ValueError("`managed_strategy_ids` must be a list of non-empty strategy IDs")
    out: list[str] = []
    seen: set[str] = set()
    for item in raw_value:
        strategy_id = _required_text(item, "managed_strategy_ids")
        if strategy_id in seen:
            continue
        seen.add(strategy_id)
        out.append(strategy_id)
    if not out:
        raise ValueError("`managed_strategy_ids` must contain at least one strategy ID")
    return tuple(out)


def load_controller_contract(config: dict[str, Any]) -> TokenmmControllerContract:
    controller_cfg = _table(config, "controller")
    contract = TokenmmControllerContract(
        controller_scope_id=_required_text(
            controller_cfg.get("controller_scope_id"),
            "controller_scope_id",
        ),
        account_scope_id=_required_text(
            controller_cfg.get("account_scope_id"),
            "account_scope_id",
        ),
        managed_strategy_ids=_coerce_strategy_ids(controller_cfg.get("managed_strategy_ids")),
        mode=ControllerRunMode(
            _required_text(controller_cfg.get("mode", ControllerRunMode.SHADOW.value), "mode"),
        ),
        write_ownership_enabled=bool(controller_cfg.get("write_ownership_enabled", True)),
    )
    if contract.mode is not ControllerRunMode.ACTIVE:
        raise ValueError("TokenMM controller migration requires `mode = \"active\"`")
    if not contract.write_ownership_enabled:
        raise ValueError("TokenMM controller migration requires `write_ownership_enabled = true`")

    strategy_contracts = {
        _required_text(row.get("strategy_id"), "strategy_id"): row
        for row in config.get("strategy_contracts") or ()
        if isinstance(row, dict)
    }
    for strategy_id in contract.managed_strategy_ids:
        row = strategy_contracts.get(strategy_id)
        if row is None:
            raise ValueError(f"managed strategy `{strategy_id}` is missing from [[strategy_contracts]]")
        controller_scope_id = _required_text(
            row.get("controller_scope_id"),
            "controller_scope_id",
        )
        if controller_scope_id != contract.controller_scope_id:
            raise ValueError(
                f"managed strategy `{strategy_id}` must bind controller_scope_id `{contract.controller_scope_id}`",
            )
        execution_account_scope_id = _required_text(
            row.get("execution_account_scope_id"),
            "execution_account_scope_id",
        )
        if execution_account_scope_id != contract.account_scope_id:
            raise ValueError(
                f"managed strategy `{strategy_id}` must use account_scope_id `{contract.account_scope_id}`",
            )
    return contract


def build_runner(
    config: dict[str, Any],
    *,
    owner_id: str | None = None,
    repo_root: Path | None = None,
    lease_store: LocalControllerLeaseStore | None = None,
) -> TokenmmControllerRunner:
    contract = load_controller_contract(config)
    root = repo_root or _repo_root()
    controller_cfg = _table(config, "controller")
    store = lease_store or LocalControllerLeaseStore(
        root_dir=root / ".run" / "tokenmm-controller-leases",
    )
    return TokenmmControllerRunner(
        config=ControllerRunnerConfig(
            controller_scope_id=contract.controller_scope_id,
            owner_id=_required_text(
                owner_id or controller_cfg.get("owner_id") or f"tokenmm-controller:{contract.controller_scope_id}",
                "owner_id",
            ),
            run_mode=contract.mode,
            lease_ttl_ms=int(controller_cfg.get("lease_ttl_ms", 5_000)),
        ),
        lease_store=store,
        controller_service=_TokenmmControllerService(contract=contract),
    )


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _now_ms(value: int | None) -> int:
    if value is not None:
        return int(value)
    return int(time.time() * 1_000)


def main() -> None:
    args = _parse_args()
    config = _load_config(args.config)
    _resolve_mode(config, args)
    contract = load_controller_contract(config)
    controller_cfg = _table(config, "controller")
    configure_python_logging(
        cli_level=args.log_level,
        config_level=controller_cfg.get("log_level", "INFO"),
        service_env_var="FLUX_CONTROLLER_LOG_LEVEL",
    )
    emit_startup_banner(
        prefix="tokenmm-run-controller",
        message=(
            f"controller_scope_id={contract.controller_scope_id} "
            f"account_scope_id={contract.account_scope_id} "
            f"managed_strategy_ids={list(contract.managed_strategy_ids)}"
        ),
    )
    runner = build_runner(config, owner_id=_optional_text(args.owner_id))
    runner.start()

    stop_requested = False

    def _request_stop(_signum: int, _frame: object) -> None:
        nonlocal stop_requested
        stop_requested = True

    signal.signal(signal.SIGTERM, _request_stop)
    signal.signal(signal.SIGINT, _request_stop)

    refresh_interval_secs = max(float(runner.config.lease_ttl_ms) / 2_000.0, 0.5)
    try:
        while not stop_requested:
            time.sleep(refresh_interval_secs)
            runner.refresh()
    finally:
        runner.stop()


if __name__ == "__main__":
    main()
