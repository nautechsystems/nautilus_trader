#!/usr/bin/env python3
from __future__ import annotations

import argparse
import asyncio
import socket
import sys
import threading
import time
from dataclasses import dataclass
from decimal import Decimal
from pathlib import Path
from typing import Any
from typing import Callable
from typing import Protocol

import redis
from ibapi.order import Order as IBOrder

from flux.common.account_scopes import AccountScopeConfig
from flux.common.account_scopes import decode_account_scopes
from flux.common.controller_scopes import ControllerScopeConfig
from flux.common.controller_scopes import decode_controller_scopes
from flux.execution.controller import ControllerIngressPolicy
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
from flux.runners.equities.run_node import _repo_root as equities_repo_root
from flux.runners.shared.bootstrap import build_redis_client_kwargs
from flux.runners.shared.bootstrap import load_config as load_shared_config
from flux.runners.shared.bootstrap import load_runtime_config as load_shared_runtime_config
from flux.runners.shared.controller_runner import ControllerRunnerConfig
from flux.runners.shared.controller_runner import ShadowControllerRunner
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.strategies.makerv4.strategy import ControllerStateFeedBridge
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers.config import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers.factories import drop_cached_ib_client
from nautilus_trader.adapters.interactive_brokers.factories import get_cached_ib_client
from nautilus_trader.adapters.interactive_brokers.factories import (
    get_cached_interactive_brokers_instrument_provider,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


if __name__ == "flux.runners.equities.run_controller":
    sys.modules.setdefault("nautilus_trader.flux.runners.equities.run_controller", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.runners.equities.run_controller":
    sys.modules.setdefault("flux.runners.equities.run_controller", sys.modules[__name__])


EQUITIES_DESCRIPTOR = get_strategy_set_descriptor("equities")


@dataclass(frozen=True, slots=True)
class _ResolvedWriterScope:
    controller_scope: ControllerScopeConfig
    writer_account_scope: AccountScopeConfig


class _ActiveWriterGateway(Protocol):
    def start(self) -> None: ...

    def stop(self) -> None: ...

    def place_order(
        self,
        *,
        client_order_id: str,
        instrument_id: str,
        side: str,
        quantity: str,
        limit_price: str,
        time_in_force: str | None,
        route: str | None,
        outside_rth: bool | None,
        include_overnight: bool | None,
    ) -> str: ...

    def cancel_order(self, venue_order_id: str) -> str: ...


class _AsyncLoopThread:
    def __init__(self, *, name: str) -> None:
        self._name = name
        self._loop_ready = threading.Event()
        self._thread: threading.Thread | None = None
        self._loop: asyncio.AbstractEventLoop | None = None

    @property
    def loop(self) -> asyncio.AbstractEventLoop:
        if self._loop is None:
            raise RuntimeError("async loop thread is not running")
        return self._loop

    def start(self) -> None:
        if self._thread is not None and self._thread.is_alive():
            return
        self._loop_ready.clear()
        thread = threading.Thread(target=self._run, name=self._name, daemon=True)
        thread.start()
        self._thread = thread
        self._loop_ready.wait(timeout=5.0)
        if self._loop is None:
            raise RuntimeError("failed to start controller async loop")

    def stop(self) -> None:
        loop = self._loop
        thread = self._thread
        if loop is not None:
            loop.call_soon_threadsafe(loop.stop)
        if thread is not None:
            thread.join(timeout=5.0)
        self._thread = None
        self._loop = None

    def run(self, coroutine):
        future = asyncio.run_coroutine_threadsafe(coroutine, self.loop)
        return future.result()

    def call(self, func: Callable[..., Any], /, *args, **kwargs) -> Any:
        result: list[Any] = []
        error: list[BaseException] = []
        completed = threading.Event()

        def _invoke() -> None:
            try:
                result.append(func(*args, **kwargs))
            except BaseException as exc:  # pragma: no cover - surfaced to caller
                error.append(exc)
            finally:
                completed.set()

        self.loop.call_soon_threadsafe(_invoke)
        completed.wait(timeout=30.0)
        if error:
            raise error[0]
        if not completed.is_set():
            raise TimeoutError("timed out waiting for controller async call")
        return result[0] if result else None

    def _run(self) -> None:
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)
        self._loop = loop
        self._loop_ready.set()
        try:
            loop.run_forever()
        finally:
            pending = asyncio.all_tasks(loop)
            for task in pending:
                task.cancel()
            if pending:
                loop.run_until_complete(asyncio.gather(*pending, return_exceptions=True))
            loop.close()


class _IBKRActiveWriterGateway:
    def __init__(self, *, scope: AccountScopeConfig, controller_scope_id: str) -> None:
        self._scope = scope
        self._controller_scope_id = controller_scope_id
        self._loop_thread = _AsyncLoopThread(
            name=f"ibkr-writer:{controller_scope_id}",
        )
        self._clock = LiveClock()
        self._cache = Cache(database=None)
        self._msgbus = MessageBus(
            trader_id=TraderId("EQUITIES-CONTROLLER-001"),
            clock=self._clock,
        )
        self._client = None
        self._provider = None

    def start(self) -> None:
        if self._client is not None:
            return
        self._loop_thread.start()
        self._client = get_cached_ib_client(
            loop=self._loop_thread.loop,
            msgbus=self._msgbus,
            cache=self._cache,
            clock=self._clock,
            host=self._scope.ibg_host or "127.0.0.1",
            port=self._scope.ibg_port,
            client_id=self._scope.ibg_client_id or 1,
            dockerized_gateway=(
                None
                if self._scope.dockerized_gateway is None
                else DockerizedIBGatewayConfig(**self._scope.dockerized_gateway)
            ),
            request_timeout_secs=60,
        )
        self._provider = get_cached_interactive_brokers_instrument_provider(
            client=self._client,
            clock=self._clock,
            config=InteractiveBrokersInstrumentProviderConfig(),
        )
        self._loop_thread.run(self._bootstrap())

    async def _bootstrap(self) -> None:
        assert self._client is not None
        assert self._provider is not None
        await self._client.wait_until_ready(300)
        await self._provider.initialize()

    def stop(self) -> None:
        if self._client is not None:
            try:
                drop_cached_ib_client(
                    host=self._client._host,
                    port=self._client._port,
                    client_id=self._client._client_id,
                )
            except Exception:
                pass
        self._client = None
        self._provider = None
        self._loop_thread.stop()

    def place_order(
        self,
        *,
        client_order_id: str,
        instrument_id: str,
        side: str,
        quantity: str,
        limit_price: str,
        time_in_force: str | None,
        route: str | None,
        outside_rth: bool | None,
        include_overnight: bool | None,
    ) -> str:
        return self._loop_thread.run(
            self._place_order_async(
                client_order_id=client_order_id,
                instrument_id=instrument_id,
                side=side,
                quantity=quantity,
                limit_price=limit_price,
                time_in_force=time_in_force,
                route=route,
                outside_rth=outside_rth,
                include_overnight=include_overnight,
            ),
        )

    async def _place_order_async(
        self,
        *,
        client_order_id: str,
        instrument_id: str,
        side: str,
        quantity: str,
        limit_price: str,
        time_in_force: str | None,
        route: str | None,
        outside_rth: bool | None,
        include_overnight: bool | None,
    ) -> str:
        assert self._client is not None
        assert self._provider is not None
        contract = await self._provider.instrument_id_to_ib_contract(
            InstrumentId.from_str(instrument_id),
        )
        if contract is None:
            raise ValueError(f"unable to resolve IBKR contract for {instrument_id!r}")

        order = IBOrder()
        order.contract = contract
        order.orderId = self._client.next_order_id()
        order.orderRef = client_order_id
        order.account = self._scope.account_id
        order.clearingAccount = self._scope.account_id
        order.action = str(side).strip().upper()
        order.totalQuantity = float(Decimal(str(quantity)))
        order.orderType = "LMT"
        order.lmtPrice = float(Decimal(str(limit_price)))
        order.tif = str(time_in_force or "DAY").strip().upper()
        if route:
            order.contract.exchange = str(route).strip().upper()
        if outside_rth is not None:
            order.outsideRth = bool(outside_rth)
        if include_overnight is not None:
            order.includeOvernight = bool(include_overnight)
        self._client.place_order(order)
        return str(order.orderId)

    def cancel_order(self, venue_order_id: str) -> str:
        assert self._client is not None
        self._loop_thread.call(self._client.cancel_order, int(venue_order_id))
        return str(venue_order_id)


class _RequestBoundVenueWriter:
    def __init__(
        self,
        *,
        request: ControllerIntentRequest,
        wal: SQLiteOwnershipWal,
        gateway: _ActiveWriterGateway,
        publish_event: Callable[[ExecutionLifecycleState, Any, str | None], None],
    ) -> None:
        if request.command is None:
            raise ValueError("request-bound venue writer requires a controller command")
        self._request = request
        self._command = request.command
        self._wal = wal
        self._gateway = gateway
        self._publish_event = publish_event

    async def write_owned_order(self, claim) -> str:
        record = self._wal.fetch_by_intent_id(claim.intent_id)
        if record is not None and record.lifecycle_state is ExecutionLifecycleState.OWNED_PRE_WRITE:
            self._publish_event(ExecutionLifecycleState.OWNED_PRE_WRITE, claim, None)

        if self._command.command_type == "cancel":
            target_id = _required_text(
                self._command.target_client_order_id,
                "target_client_order_id",
            )
            target = self._wal.fetch_by_client_order_id(target_id)
            if target is None or target.venue_order_id is None:
                raise KeyError(f"missing target venue order for cancel {target_id!r}")
            return self._gateway.cancel_order(target.venue_order_id)

        return self._gateway.place_order(
            client_order_id=claim.client_order_id,
            instrument_id=self._command.instrument_id,
            side=_required_text(self._command.side, "side"),
            quantity=_required_text(self._command.quantity, "quantity"),
            limit_price=_required_text(self._command.limit_price, "limit_price"),
            time_in_force=self._command.time_in_force,
            route=self._command.route,
            outside_rth=self._command.outside_rth,
            include_overnight=self._command.include_overnight,
        )


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
        active_writer_factory: Callable[..., _ActiveWriterGateway] | None = None,
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
        self._run_mode = _resolve_run_mode(_table(self._config, "controller"))
        self._seq_lock = threading.Lock()
        self._stop_event = threading.Event()
        self._server_socket: socket.socket | None = None
        self._thread: threading.Thread | None = None
        self._canonical_state_by_strategy: dict[str, dict[str, Any]] = {}
        self._redis_client = _build_controller_redis_client(self._config)
        self._wal_path = _controller_wal_path(
            repo_root=repo_root,
            controller_scope_id=controller_scope_id,
        )
        self._wal: SQLiteOwnershipWal | None = None
        self._ledger: ExecutionLedger | None = None
        self._writer_scope: _ResolvedWriterScope | None = None
        self._active_order_writer_factory = active_order_writer_factory
        self._active_writer_gateway: _ActiveWriterGateway | None = None
        self._active_writer_factory = active_writer_factory

    def start(self) -> None:
        if self._thread is not None and self._thread.is_alive():
            return
        if self._run_mode is ControllerRunMode.ACTIVE and self._active_order_writer_factory is None:
            self._writer_scope = _resolve_writer_scope_config(
                config=self._config,
                controller_scope_id=self._paths.controller_scope_id,
            )
            self._active_writer_gateway = _build_active_writer_gateway(
                config=self._config,
                controller_scope=self._writer_scope,
                active_writer_factory=self._active_writer_factory,
            )
        if self._active_writer_gateway is not None:
            self._active_writer_gateway.start()
        self._paths.request_reply_path.parent.mkdir(parents=True, exist_ok=True)
        _safe_unlink(self._paths.request_reply_path)
        self._stop_event.clear()
        thread = threading.Thread(
            target=self._serve,
            name=f"controller-rpc:{self._paths.controller_scope_id}",
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
        if self._active_writer_gateway is not None:
            self._active_writer_gateway.stop()

    def _serve(self) -> None:
        if self._run_mode is ControllerRunMode.ACTIVE and (
            self._active_order_writer_factory is not None or self._active_writer_gateway is not None
        ):
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
        self._maybe_execute_active_write(request=request, claim=claim)
        return ControllerIntentReply.accepted(
            claim=claim,
            replied_at_ns=time.time_ns(),
        )

    def _publish_request_state(self, *, request: ControllerIntentRequest, claim) -> None:
        if self._redis_client is None:
            return
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

    def _maybe_execute_active_write(self, *, request: ControllerIntentRequest, claim) -> None:
        command = request.command
        if command is None:
            return
        if self._run_mode is not ControllerRunMode.ACTIVE:
            return
        if self._wal is None or self._ledger is None:
            raise RuntimeError("controller WAL is not initialized")
        writer: ExecutionVenueWriter | None = None
        account_scope_id = self._paths.controller_scope_id
        if self._active_order_writer_factory is not None:
            writer = self._active_order_writer_factory(
                {
                    "claim": claim,
                    "command": command,
                    "controller_scope_id": self._paths.controller_scope_id,
                    "wal_path": self._wal_path,
                },
            )
        else:
            gateway = self._active_writer_gateway
            if gateway is None or self._writer_scope is None or command.order_role != "hedge":
                return
            account_scope_id = self._writer_scope.writer_account_scope.scope_id
            writer = _RequestBoundVenueWriter(
                request=request,
                wal=self._wal,
                gateway=gateway,
                publish_event=self._publish_lifecycle_event,
            )
        if writer is None:
            return

        authority = _authority_for_claim(
            claim=claim,
            snapshot_ts_ms=int(time.time() * 1_000),
        )
        try:
            record = asyncio.run(
                self._ledger.write_owned_order(
                    claim=claim,
                    account_scope_id=account_scope_id,
                    operation_type=_operation_type_for_command(command.command_type),
                    claim_key=_claim_key_for_request(request),
                    append_authority=authority,
                    write_authority=authority,
                    venue_writer=writer,
                    written_at_ns=time.time_ns(),
                ),
            )
        except Exception:
            if self._wal.fetch_by_intent_id(claim.intent_id) is None:
                raise
            return
        self._publish_lifecycle_event(
            ExecutionLifecycleState.SENT_TO_VENUE,
            claim,
            record.venue_order_id,
        )

    def _publish_lifecycle_event(
        self,
        lifecycle_state: ExecutionLifecycleState,
        claim,
        venue_order_id: str | None,
    ) -> None:
        if self._redis_client is None:
            return
        feed = _feed_bridge_for_claim(
            redis_client=self._redis_client,
            config=self._config,
            claim=claim,
        )
        feed.publish_lifecycle_event(
            ExecutionLifecycleEvent.from_claim(
                claim=claim,
                lifecycle_state=lifecycle_state,
                venue_activity_origin="controller",
                venue_order_id=venue_order_id,
            ),
        )

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


def _repo_root() -> Path:
    return equities_repo_root()


def _load_config(path: Path) -> dict[str, Any]:
    return load_shared_config(path, env_prefix=EQUITIES_DESCRIPTOR.env_prefix)


def _load_runtime_config(path: Path, *, shared_config_path: Path | None = None) -> dict[str, Any]:
    return load_shared_runtime_config(
        path,
        shared_config_path=shared_config_path,
        load_config=_load_config,
        table_names=("redis", "strategy_contracts", "account_scopes", "controller_scopes", "controller"),
    )


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the Equities shadow controller scaffold.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--shared-config", type=Path, default=None)
    parser.add_argument("--owner-id", default=None)
    parser.add_argument("--allow-single-host-canary", action="store_true")
    return parser.parse_args()


class EquitiesControllerRunner(ShadowControllerRunner):
    def start(self, *, now_ms: int | None = None):
        if self._running and self._lease is not None:
            return self._lease
        _validate_equities_single_host_canary_gate(self.config)
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


def build_runner(
    config: dict[str, Any],
    *,
    owner_id: str | None = None,
    repo_root: Path | None = None,
    lease_store: LocalControllerLeaseStore | None = None,
    controller_service_factory: Callable[[dict[str, Any]], Any] | None = None,
    active_writer_factory: Callable[..., _ActiveWriterGateway] | None = None,
    active_order_writer_factory: Callable[[dict[str, Any]], ExecutionVenueWriter] | None = None,
) -> ShadowControllerRunner:
    controller_cfg = _table(config, "controller")
    scope_id = _required_text(controller_cfg.get("controller_scope_id"), "controller_scope_id")
    allow_single_host_canary = bool(controller_cfg.get("allow_single_host_canary", False))
    run_mode = _resolve_run_mode(controller_cfg)
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
    service_factory = controller_service_factory or (
        lambda _config: _ResidentRequestReplyControllerService(
            controller_scope_id=scope_id,
            transport_root_dir=root / ".run",
            repo_root=root,
            config=_config,
            active_writer_factory=active_writer_factory,
            active_order_writer_factory=active_order_writer_factory,
        )
    )
    return EquitiesControllerRunner(
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
    try:
        while True:
            time.sleep(1.0)
    except KeyboardInterrupt:
        pass
    finally:
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


def _resolve_run_mode(controller_cfg: dict[str, Any]) -> ControllerRunMode:
    requested_run_mode = ControllerRunMode(
        str(controller_cfg.get("mode", ControllerRunMode.SHADOW.value)).strip(),
    )
    write_ownership_enabled = bool(controller_cfg.get("write_ownership_enabled", True))
    if requested_run_mode is ControllerRunMode.ACTIVE and not write_ownership_enabled:
        return ControllerRunMode.SHADOW
    return requested_run_mode


def _validate_equities_single_host_canary_gate(config: ControllerRunnerConfig) -> None:
    if config.run_mode not in (ControllerRunMode.SHADOW, ControllerRunMode.ACTIVE):
        raise ValueError("controller runner requires `shadow` or `active` mode")
    if config.ingress_policy is not ControllerIngressPolicy.SINGLE_HOST_CANARY:
        raise ValueError("controller runner requires the single-host canary ingress policy")
    if not config.allow_single_host_canary:
        raise ValueError("single-host canary gating must be explicitly enabled")


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


def _resolve_writer_scope_config(
    *,
    config: dict[str, Any],
    controller_scope_id: str,
) -> _ResolvedWriterScope:
    controller_scopes = {
        scope.controller_scope_id: scope
        for scope in decode_controller_scopes(config.get("controller_scopes") or [])
    }
    controller_scope = controller_scopes.get(controller_scope_id)
    if controller_scope is None:
        raise ValueError(f"missing controller scope config for {controller_scope_id!r}")
    account_scopes = {
        scope.scope_id: scope
        for scope in decode_account_scopes(config.get("account_scopes") or [])
    }
    writer_account_scope = account_scopes.get(controller_scope.writer_account_scope_id)
    if writer_account_scope is None:
        raise ValueError(
            f"missing writer account scope {controller_scope.writer_account_scope_id!r}",
        )
    return _ResolvedWriterScope(
        controller_scope=controller_scope,
        writer_account_scope=writer_account_scope,
    )


def _build_active_writer_gateway(
    *,
    config: dict[str, Any],
    controller_scope: _ResolvedWriterScope,
    active_writer_factory: Callable[..., _ActiveWriterGateway] | None,
) -> _ActiveWriterGateway | None:
    controller_cfg = _table(config, "controller")
    if _resolve_run_mode(controller_cfg) is not ControllerRunMode.ACTIVE:
        return None
    if controller_scope.writer_account_scope.provider.lower() != "ibkr":
        raise ValueError("equities canary active writer currently supports only IBKR scopes")
    if active_writer_factory is not None:
        return active_writer_factory(
            config=config,
            controller_scope=controller_scope.controller_scope,
            writer_account_scope=controller_scope.writer_account_scope,
        )
    return _IBKRActiveWriterGateway(
        scope=controller_scope.writer_account_scope,
        controller_scope_id=controller_scope.controller_scope.controller_scope_id,
    )


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


def _build_controller_redis_client(config: dict[str, Any]) -> Any | None:
    redis_cfg = config.get("redis")
    if not isinstance(redis_cfg, dict) or not redis_cfg:
        return None
    return redis.Redis(**build_redis_client_kwargs(redis_cfg))


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
            "snapshot_ts_ms": int(time.time() * 1_000),
            "stale_after_ms": 1_000,
            "stale": False,
        },
    )
    command = request.command
    if command is not None and command.command_type == "place" and command.order_role == "maker":
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


__all__ = (
    "EquitiesControllerRunner",
    "ControllerRunMode",
    "build_runner",
    "main",
)


if __name__ == "__main__":
    main()
