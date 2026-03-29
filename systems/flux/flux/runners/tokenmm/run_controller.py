#!/usr/bin/env python3

from __future__ import annotations

import argparse
import asyncio
import signal
import socket
import sys
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any
from typing import Callable

import redis

from flux.execution.controller import ControllerRunMode
from flux.execution.controller import ControllerSnapshotAuthority
from flux.execution.controller import SnapshotAuthorityState
from flux.execution.events import ExecutionLifecycleEvent
from flux.execution.intents import ExecutionLifecycleState
from flux.execution.ledger import ExecutionLedger
from flux.execution.ledger import ExecutionVenueWriter
from flux.execution.leases import LocalControllerLeaseStore
from flux.execution.transport import ControllerIntentReply
from flux.execution.transport import ControllerIntentRequest
from flux.execution.transport import UdsTransportPaths
from flux.execution.transport import decode_request_frame
from flux.execution.transport import encode_reply_frame
from flux.execution.wal import SQLiteOwnershipWal
from flux.runners.shared.bootstrap import build_redis_client_kwargs
from flux.runners.shared.bootstrap import load_config as load_shared_config
from flux.runners.shared.bootstrap import table as shared_table
from flux.runners.shared.controller_runner import ControllerRunnerConfig
from flux.runners.shared.controller_runner import ShadowControllerRunner
from flux.runners.shared.logging import configure_python_logging
from flux.runners.shared.logging import emit_startup_banner
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.strategies.makerv4.strategy import ControllerStateFeedBridge


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


class _NullControllerService:
    def start(self) -> None:
        return None

    def stop(self) -> None:
        return None


class _ResidentRequestReplyControllerService:
    def __init__(
        self,
        *,
        controller_scope_id: str,
        transport_root_dir: Path,
        repo_root: Path,
        config: dict[str, Any],
        active_order_writer_factory: Callable[[dict[str, Any]], ExecutionVenueWriter] | None = None,
    ) -> None:
        self._paths = UdsTransportPaths.for_controller_scope(
            controller_scope_id=controller_scope_id,
            root_dir=transport_root_dir,
        )
        self._config = dict(config)
        self._repo_root = repo_root
        self._controller_epoch = 1
        self._controller_seq = 0
        self._run_mode = load_controller_contract(config).mode
        self._seq_lock = threading.Lock()
        self._stop_event = threading.Event()
        self._server_socket: socket.socket | None = None
        self._thread: threading.Thread | None = None
        self._canonical_state_by_strategy: dict[str, dict[str, Any]] = {}
        self._redis_client = _build_controller_redis_client(self._config)
        self._active_order_writer_factory = active_order_writer_factory
        self._wal_path = _controller_wal_path(
            repo_root=repo_root,
            controller_scope_id=controller_scope_id,
        )
        self._wal: SQLiteOwnershipWal | None = None
        self._ledger: ExecutionLedger | None = None

    def start(self) -> None:
        if self._thread is not None and self._thread.is_alive():
            return
        self._paths.request_reply_path.parent.mkdir(parents=True, exist_ok=True)
        _safe_unlink(self._paths.request_reply_path)
        self._stop_event.clear()
        thread = threading.Thread(
            target=self._serve,
            name=f"tokenmm-controller-rpc:{self._paths.controller_scope_id}",
            daemon=True,
        )
        thread.start()
        self._thread = thread

    def stop(self) -> None:
        self._stop_event.set()
        self._poke_server()
        thread = self._thread
        if thread is not None:
            thread.join(timeout=1.0)
        self._thread = None
        sock = self._server_socket
        self._server_socket = None
        if sock is not None:
            sock.close()
        _safe_unlink(self._paths.request_reply_path)

    def _serve(self) -> None:
        if self._run_mode is ControllerRunMode.ACTIVE and self._active_order_writer_factory is not None:
            self._wal = _build_controller_wal(
                repo_root=self._repo_root,
                controller_scope_id=self._paths.controller_scope_id,
            )
            self._ledger = ExecutionLedger(wal=self._wal)

        server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._server_socket = server
        server.bind(str(self._paths.request_reply_path))
        server.listen()
        server.settimeout(0.1)
        try:
            while not self._stop_event.is_set():
                try:
                    conn, _ = server.accept()
                except socket.timeout:
                    continue
                except OSError:
                    break
                with conn:
                    try:
                        request_frame = _recv_request_frame(conn)
                        if not request_frame:
                            continue
                        request = decode_request_frame(request_frame)
                        reply = self._handle_request(request)
                        conn.sendall(encode_reply_frame(reply))
                    except Exception:
                        continue
        finally:
            if self._wal is not None:
                self._wal.close()
                self._wal = None
                self._ledger = None
            server.close()
            self._server_socket = None

    def _handle_request(self, request: ControllerIntentRequest) -> ControllerIntentReply:
        with self._seq_lock:
            self._controller_seq += 1
            claim = request.intent.claim(
                controller_epoch=self._controller_epoch,
                controller_seq=self._controller_seq,
            )
        self._publish_request_state(request=request, claim=claim)
        try:
            self._maybe_execute_active_write(request=request, claim=claim)
        except Exception as exc:
            return ControllerIntentReply.rejected(
                intent=request.intent,
                reason=str(exc),
                replied_at_ns=time.time_ns(),
            )
        return ControllerIntentReply.accepted(
            claim=claim,
            replied_at_ns=time.time_ns(),
        )

    def _publish_request_state(self, *, request: ControllerIntentRequest, claim) -> None:
        if self._redis_client is None:
            return
        try:
            feed = _feed_bridge_for_claim(
                redis_client=self._redis_client,
                config=self._config,
                claim=claim,
            )
            feed.publish_lifecycle_event(
                ExecutionLifecycleEvent.from_claim(
                    claim=claim,
                    lifecycle_state=claim.lifecycle_state,
                    venue_activity_origin="controller",
                ),
            )
            state = _canonical_state_payload(
                request=request,
                claim=claim,
                existing_state=self._canonical_state_by_strategy.get(claim.strategy_id),
            )
            self._canonical_state_by_strategy[claim.strategy_id] = state
            feed.publish_canonical_state(state)
        except Exception:
            return

    def _maybe_execute_active_write(self, *, request: ControllerIntentRequest, claim) -> None:
        command = request.command
        if command is None:
            return
        if self._run_mode is not ControllerRunMode.ACTIVE:
            return
        if self._active_order_writer_factory is None:
            raise RuntimeError("TokenMM active controller requires an active order writer")
        if self._wal is None or self._ledger is None:
            raise RuntimeError("controller WAL is not initialized")

        authority = _authority_for_claim(claim=claim, snapshot_ts_ms=_now_ms(None))
        writer = self._active_order_writer_factory(
            {
                "claim": claim,
                "command": command,
                "controller_scope_id": self._paths.controller_scope_id,
                "wal_path": self._wal_path,
            },
        )
        asyncio.run(
            self._ledger.write_owned_order(
                claim=claim,
                account_scope_id=self._paths.controller_scope_id,
                operation_type=_operation_type_for_command(command.command_type),
                claim_key=_claim_key_for_request(request),
                append_authority=authority,
                write_authority=authority,
                venue_writer=writer,
                written_at_ns=time.time_ns(),
            ),
        )
        if self._redis_client is None:
            return
        try:
            feed = _feed_bridge_for_claim(
                redis_client=self._redis_client,
                config=self._config,
                claim=claim,
            )
            feed.publish_lifecycle_event(
                ExecutionLifecycleEvent.sent_to_venue(
                    claim=claim,
                    venue_order_id=_latest_venue_order_id(self._wal, claim.intent_id),
                ),
            )
        except Exception:
            return

    def _poke_server(self) -> None:
        if not self._paths.request_reply_path.exists():
            return
        try:
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            try:
                sock.connect(str(self._paths.request_reply_path))
            finally:
                sock.close()
        except OSError:
            return


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
    controller_service_factory: Callable[[ControllerRunnerConfig], Any] | None = None,
    active_order_writer_factory: Callable[[dict[str, Any]], ExecutionVenueWriter] | None = None,
) -> TokenmmControllerRunner:
    contract = load_controller_contract(config)
    root = repo_root or _repo_root()
    controller_cfg = _table(config, "controller")
    runner_config = ControllerRunnerConfig(
        controller_scope_id=contract.controller_scope_id,
        owner_id=_required_text(
            owner_id or controller_cfg.get("owner_id") or f"tokenmm-controller:{contract.controller_scope_id}",
            "owner_id",
        ),
        run_mode=contract.mode,
        lease_ttl_ms=int(controller_cfg.get("lease_ttl_ms", 5_000)),
    )
    store = lease_store or LocalControllerLeaseStore(
        root_dir=root / ".run" / "tokenmm-controller-leases",
    )
    controller_service = (
        controller_service_factory(runner_config)
        if controller_service_factory is not None
        else _ResidentRequestReplyControllerService(
            controller_scope_id=contract.controller_scope_id,
            transport_root_dir=root / ".run",
            repo_root=root,
            config=config,
            active_order_writer_factory=active_order_writer_factory,
        )
    )
    return TokenmmControllerRunner(
        config=runner_config,
        lease_store=store,
        controller_service=controller_service,
    )


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _now_ms(value: int | None) -> int:
    if value is not None:
        return int(value)
    return int(time.time() * 1_000)


def _recv_request_frame(conn: socket.socket) -> bytes:
    chunks: list[bytes] = []
    while True:
        chunk = conn.recv(65_536)
        if not chunk:
            break
        chunks.append(chunk)
        if chunk.endswith(b"\n"):
            break
    return b"".join(chunks)


def _safe_unlink(path: Path) -> None:
    try:
        path.unlink()
    except FileNotFoundError:
        return


def _build_controller_wal(*, repo_root: Path, controller_scope_id: str) -> SQLiteOwnershipWal:
    return SQLiteOwnershipWal(
        db_path=_controller_wal_path(
            repo_root=repo_root,
            controller_scope_id=controller_scope_id,
        ),
    )


def _controller_wal_path(
    *,
    repo_root: Path | None = None,
    root_dir: Path | None = None,
    controller_scope_id: str,
) -> Path:
    base_root = repo_root or root_dir
    if base_root is None:
        raise ValueError("`repo_root` or `root_dir` is required")
    wal_dir = base_root / ".run" / "controller-wal"
    wal_dir.mkdir(parents=True, exist_ok=True)
    return wal_dir / f"{controller_scope_id}.sqlite3"


def _build_controller_redis_client(config: dict[str, Any]) -> Any | None:
    redis_cfg = config.get("redis")
    if not isinstance(redis_cfg, dict) or not redis_cfg:
        return None
    return redis.Redis(**build_redis_client_kwargs(redis_cfg))


def _authority_for_claim(*, claim, snapshot_ts_ms: int) -> ControllerSnapshotAuthority:
    return ControllerSnapshotAuthority(
        controller_scope_id=claim.controller_scope_id,
        controller_epoch=claim.controller_epoch,
        controller_seq=claim.controller_seq,
        snapshot_ts_ms=int(snapshot_ts_ms),
        stale_after_ms=1_000,
        authority_state=SnapshotAuthorityState.AUTHORITATIVE,
    )


def _operation_type_for_command(command_type: str) -> str:
    normalized = _required_text(command_type, "command_type").lower()
    if normalized == "place":
        return "submit"
    if normalized == "cancel":
        return "cancel"
    return normalized


def _claim_key_for_request(request: ControllerIntentRequest) -> str:
    command = request.command
    if command is None:
        return f"intent:{request.intent.intent_id}"
    if command.command_type == "cancel" and command.target_client_order_id:
        return f"{command.order_role}:cancel:{command.target_client_order_id}"
    return f"{command.order_role}:{command.command_type}:{request.intent.intent_id}"


def _canonical_state_payload(
    *,
    request: ControllerIntentRequest,
    claim,
    existing_state: dict[str, Any] | None,
) -> dict[str, Any]:
    state = dict(existing_state or {})
    state.update(
        {
            "controller_scope_id": claim.controller_scope_id,
            "controller_epoch": claim.controller_epoch,
            "controller_seq": claim.controller_seq,
            "authority_state": "authoritative",
            "snapshot_ts_ms": _now_ms(None),
            "stale_after_ms": 1_000,
            "stale": False,
        },
    )
    command = request.command
    if command is None:
        return state
    managed_orders = [
        dict(row)
        for row in state.get("managed_maker_orders", [])
        if isinstance(row, dict)
    ]
    if command.command_type == "place" and command.order_role == "maker":
        managed_orders = [
            {
                "client_order_id": claim.client_order_id,
                "instrument_id": command.instrument_id,
                "side": command.side,
                "quantity": command.quantity,
                "price": command.limit_price,
                "post_only": bool(command.post_only),
                "pending_cancel": False,
            },
        ]
    elif command.command_type == "cancel" and command.target_client_order_id:
        managed_orders = [
            row
            for row in managed_orders
            if str(row.get("client_order_id", "")).strip() != command.target_client_order_id
        ]
    state["managed_maker_orders"] = managed_orders
    return state


def _feed_bridge_for_claim(
    *,
    redis_client: Any,
    config: dict[str, Any],
    claim,
) -> ControllerStateFeedBridge:
    flux_cfg = config.get("flux")
    namespace = "flux"
    schema_version = "v1"
    if isinstance(flux_cfg, dict):
        namespace = str(flux_cfg.get("namespace", namespace))
        schema_version = str(flux_cfg.get("schema_version", schema_version))
    return ControllerStateFeedBridge(
        redis_client=redis_client,
        controller_scope_id=claim.controller_scope_id,
        strategy_id=claim.strategy_id,
        namespace=namespace,
        schema_version=schema_version,
    )


def _latest_venue_order_id(wal: SQLiteOwnershipWal | None, intent_id: str) -> str:
    if wal is None:
        raise RuntimeError("controller WAL is not initialized")
    record = wal.fetch_by_intent_id(intent_id)
    if record is None or record.venue_order_id is None:
        raise RuntimeError(f"missing venue order id for intent_id={intent_id}")
    return record.venue_order_id


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
