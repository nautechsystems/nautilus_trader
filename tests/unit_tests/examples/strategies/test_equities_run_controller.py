from __future__ import annotations

import asyncio
import importlib
import json
from pathlib import Path
import time

import pytest

from flux.execution.intents import ExecutionIntent
from flux.execution.transport import ControllerIntentRequest
from flux.execution.transport import UdsTransportPaths
from flux.strategies.makerv4.strategy import ControllerStateFeedBridge


class _RecordingControllerService:
    def __init__(self) -> None:
        self.started = 0
        self.stopped = 0

    def start(self) -> None:
        self.started += 1

    def stop(self) -> None:
        self.stopped += 1


class _FakeRedis:
    def __init__(self) -> None:
        self.payloads: dict[str, bytes] = {}

    def get(self, key: str):
        return self.payloads.get(key)

    def set(self, key: str, value: bytes) -> None:
        self.payloads[key] = value


class _RecordingActiveWriterGateway:
    def __init__(self, *, venue_order_id: str = "ibkr-venue-9001", fail_on_place: bool = False) -> None:
        self.venue_order_id = venue_order_id
        self.fail_on_place = fail_on_place
        self.started = 0
        self.stopped = 0
        self.place_calls: list[dict[str, object]] = []
        self.cancel_calls: list[str] = []

    def start(self) -> None:
        self.started += 1

    def stop(self) -> None:
        self.stopped += 1

    def place_order(self, **payload):
        self.place_calls.append(dict(payload))
        if self.fail_on_place:
            raise RuntimeError("simulated venue write failure")
        return self.venue_order_id

    def cancel_order(self, venue_order_id: str) -> str:
        self.cancel_calls.append(str(venue_order_id))
        return str(venue_order_id)


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _load_run_controller_module():
    path = _repo_root() / "systems/flux/flux/runners/equities/run_controller.py"
    assert path.exists(), "equities controller runner module should exist"
    return importlib.import_module("flux.runners.equities.run_controller")


def test_build_runner_requires_explicit_single_host_canary_gate(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()

    with pytest.raises(ValueError, match="single-host canary"):
        run_controller.build_runner(
            {
                "controller": {
                    "controller_scope_id": "acct.execution.main",
                },
            },
            owner_id="controller-a",
            repo_root=tmp_path,
        )


def test_build_runner_defaults_shadow_mode_and_lease_root(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()
    service = _RecordingControllerService()
    runner = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
            },
        },
        owner_id="controller-a",
        repo_root=tmp_path,
        controller_service_factory=lambda _config: service,
    )

    assert runner.config.run_mode is run_controller.ControllerRunMode.SHADOW
    assert runner.config.controller_scope_id == "acct.execution.main"
    assert runner.lease_store.root_dir == tmp_path / ".run" / "equities-controller-leases"

    runner.start(now_ms=1_000)
    runner.stop()

    assert service.started == 1
    assert service.stopped == 1


def test_build_runner_accepts_active_single_host_canary_mode(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()

    runner = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
                "mode": "active",
            },
        },
        owner_id="controller-a",
        repo_root=tmp_path,
    )

    assert runner.config.run_mode is run_controller.ControllerRunMode.ACTIVE
    assert runner.config.controller_scope_id == "acct.execution.main"


def test_build_runner_rolls_active_canary_back_to_shadow_when_write_ownership_disabled(
    tmp_path: Path,
) -> None:
    run_controller = _load_run_controller_module()

    runner = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
                "mode": "active",
                "write_ownership_enabled": False,
            },
        },
        owner_id="controller-a",
        repo_root=tmp_path,
    )

    assert runner.config.run_mode is run_controller.ControllerRunMode.SHADOW
    assert runner.config.controller_scope_id == "acct.execution.main"
    assert runner.lease_store.root_dir == tmp_path / ".run" / "equities-controller-leases"


def test_build_runner_starts_resident_request_reply_controller_service(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()
    transport = importlib.import_module("flux.execution.transport")

    runner = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
            },
        },
        owner_id="controller-a",
        repo_root=tmp_path,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="acct.execution.main",
        root_dir=tmp_path / ".run",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-001",
                controller_scope_id="acct.execution.main",
                strategy_id="strategy-01",
            ),
            requested_at_ns=123_456,
        ),
        timeout_s=1.0,
    )
    runner.stop()

    assert reply.status == "accepted"
    assert reply.claim is not None
    assert reply.claim.controller_epoch == 1
    assert reply.claim.controller_seq == 1
    assert not paths.request_reply_path.exists()


def test_resident_service_publishes_accepted_lifecycle_and_canonical_state_to_redis(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = importlib.import_module("flux.execution.transport")
    fake_redis = _FakeRedis()
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)

    runner = run_controller.build_runner(
        {
            "flux": {"namespace": "flux", "schema_version": "v1"},
            "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
            },
        },
        owner_id="controller-a",
        repo_root=tmp_path,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="acct.execution.main",
        root_dir=tmp_path / ".run",
    )
    feed = ControllerStateFeedBridge(
        redis_client=fake_redis,
        controller_scope_id="acct.execution.main",
        strategy_id="strategy-01",
        namespace="flux",
        schema_version="v1",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-redis-001",
                controller_scope_id="acct.execution.main",
                strategy_id="strategy-01",
            ),
            requested_at_ns=123_456,
            command=transport.ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
                side="BUY",
                quantity="1",
                limit_price="190.01",
                post_only=True,
                time_in_force="GTC",
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    assert reply.claim is not None
    lifecycle_payload = json.loads(fake_redis.get(feed.lifecycle_event_key()).decode("utf-8"))
    canonical_payload = json.loads(fake_redis.get(feed.canonical_state_key()).decode("utf-8"))

    assert lifecycle_payload["lifecycle_state"] == "accepted"
    assert lifecycle_payload["client_order_id"] == reply.claim.client_order_id
    assert canonical_payload["controller_scope_id"] == "acct.execution.main"
    assert canonical_payload["authority_state"] == "authoritative"
    assert canonical_payload["managed_maker_orders"][0]["client_order_id"] == reply.claim.client_order_id


def test_active_canary_hedge_place_routes_through_wal_backed_writer_path(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()
    transport = importlib.import_module("flux.execution.transport")
    intents = importlib.import_module("flux.execution.intents")
    wal = importlib.import_module("flux.execution.wal")
    gateway = _RecordingActiveWriterGateway()
    service = run_controller._ResidentRequestReplyControllerService(
        controller_scope_id="equities.ibkr.hedge.main",
        transport_root_dir=tmp_path / ".run",
        repo_root=tmp_path,
        config={
            "controller": {
                "controller_scope_id": "equities.ibkr.hedge.main",
                "allow_single_host_canary": True,
                "mode": "active",
                "write_ownership_enabled": True,
            },
            "account_scopes": [
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4002,
                    "ibg_client_id": 208,
                    "account_id": "U10015777",
                    "controller_scope_id": "equities.ibkr.hedge.main",
                },
            ],
            "controller_scopes": [
                {
                    "controller_scope_id": "equities.ibkr.hedge.main",
                    "profile_id": "equities",
                    "writer_account_scope_id": "ibkr.hedge.main",
                    "account_scope_ids": ["ibkr.hedge.main"],
                    "canary": True,
                },
            ],
        },
        active_writer_factory=lambda **_kwargs: gateway,
    )
    runner = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "equities.ibkr.hedge.main",
                "allow_single_host_canary": True,
                "mode": "active",
                "write_ownership_enabled": True,
            },
            "account_scopes": [
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4002,
                    "ibg_client_id": 208,
                    "account_id": "U10015777",
                    "controller_scope_id": "equities.ibkr.hedge.main",
                },
            ],
            "controller_scopes": [
                {
                    "controller_scope_id": "equities.ibkr.hedge.main",
                    "profile_id": "equities",
                    "writer_account_scope_id": "ibkr.hedge.main",
                    "account_scope_ids": ["ibkr.hedge.main"],
                    "canary": True,
                },
            ],
        },
        owner_id="controller-a",
        repo_root=tmp_path,
        controller_service_factory=lambda _config: service,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="equities.ibkr.hedge.main",
        root_dir=tmp_path / ".run",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-hedge-001",
                controller_scope_id="equities.ibkr.hedge.main",
                strategy_id="strategy-01",
            ),
            requested_at_ns=123_456,
            command=transport.ControllerIntentCommandPayload(
                command_type="place",
                order_role="hedge",
                instrument_id="AAPL.NASDAQ",
                side="BUY",
                quantity="10",
                limit_price="190.01",
                time_in_force="IOC",
                route="SMART",
                outside_rth=False,
                include_overnight=False,
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    store = wal.SQLiteOwnershipWal(db_path=service._wal_path)
    try:
        records = store.list_records()
    finally:
        store.close()

    assert reply.status == "accepted"
    assert reply.claim is not None
    assert gateway.started == 1
    assert gateway.stopped == 1
    assert len(gateway.place_calls) == 1
    assert gateway.place_calls[0]["client_order_id"] == reply.claim.client_order_id
    assert [record.lifecycle_state for record in records] == [
        intents.ExecutionLifecycleState.OWNED_PRE_WRITE,
        intents.ExecutionLifecycleState.SENT_TO_VENUE,
    ]
    assert records[-1].venue_order_id == "ibkr-venue-9001"


def test_active_canary_hedge_cancel_routes_through_wal_bound_venue_id(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()
    transport = importlib.import_module("flux.execution.transport")
    wal = importlib.import_module("flux.execution.wal")
    gateway = _RecordingActiveWriterGateway()
    service = run_controller._ResidentRequestReplyControllerService(
        controller_scope_id="equities.ibkr.hedge.main",
        transport_root_dir=tmp_path / ".run",
        repo_root=tmp_path,
        config={
            "controller": {
                "controller_scope_id": "equities.ibkr.hedge.main",
                "allow_single_host_canary": True,
                "mode": "active",
                "write_ownership_enabled": True,
            },
            "account_scopes": [
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4002,
                    "ibg_client_id": 208,
                    "account_id": "U10015777",
                    "controller_scope_id": "equities.ibkr.hedge.main",
                },
            ],
            "controller_scopes": [
                {
                    "controller_scope_id": "equities.ibkr.hedge.main",
                    "profile_id": "equities",
                    "writer_account_scope_id": "ibkr.hedge.main",
                    "account_scope_ids": ["ibkr.hedge.main"],
                    "canary": True,
                },
            ],
        },
        active_writer_factory=lambda **_kwargs: gateway,
    )
    runner = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "equities.ibkr.hedge.main",
                "allow_single_host_canary": True,
                "mode": "active",
                "write_ownership_enabled": True,
            },
            "account_scopes": [
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4002,
                    "ibg_client_id": 208,
                    "account_id": "U10015777",
                    "controller_scope_id": "equities.ibkr.hedge.main",
                },
            ],
            "controller_scopes": [
                {
                    "controller_scope_id": "equities.ibkr.hedge.main",
                    "profile_id": "equities",
                    "writer_account_scope_id": "ibkr.hedge.main",
                    "account_scope_ids": ["ibkr.hedge.main"],
                    "canary": True,
                },
            ],
        },
        owner_id="controller-a",
        repo_root=tmp_path,
        controller_service_factory=lambda _config: service,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="equities.ibkr.hedge.main",
        root_dir=tmp_path / ".run",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    place_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-hedge-place-001",
                controller_scope_id="equities.ibkr.hedge.main",
                strategy_id="strategy-01",
            ),
            requested_at_ns=123_456,
            command=transport.ControllerIntentCommandPayload(
                command_type="place",
                order_role="hedge",
                instrument_id="AAPL.NASDAQ",
                side="BUY",
                quantity="10",
                limit_price="190.01",
                time_in_force="IOC",
                route="SMART",
            ),
        ),
        timeout_s=1.0,
    )
    assert place_reply.claim is not None

    cancel_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-hedge-cancel-001",
                controller_scope_id="equities.ibkr.hedge.main",
                strategy_id="strategy-01",
            ),
            requested_at_ns=123_789,
            command=transport.ControllerIntentCommandPayload(
                command_type="cancel",
                order_role="hedge",
                instrument_id="AAPL.NASDAQ",
                target_client_order_id=place_reply.claim.client_order_id,
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    store = wal.SQLiteOwnershipWal(db_path=service._wal_path)
    try:
        records = store.list_records()
    finally:
        store.close()

    cancel_records = [
        record
        for record in records
        if record.claim.intent_id == "intent-hedge-cancel-001"
    ]

    assert cancel_reply.status == "accepted"
    assert cancel_reply.claim is not None
    assert len(gateway.place_calls) == 1
    assert gateway.cancel_calls == ["ibkr-venue-9001"]
    assert [record.lifecycle_state.value for record in cancel_records] == [
        "owned_pre_write",
        "sent_to_venue",
    ]
    assert cancel_records[-1].venue_order_id == "ibkr-venue-9001"


def test_active_canary_writer_failures_leave_distinct_owned_pre_write_record(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()
    transport = importlib.import_module("flux.execution.transport")
    intents = importlib.import_module("flux.execution.intents")
    wal = importlib.import_module("flux.execution.wal")
    gateway = _RecordingActiveWriterGateway(fail_on_place=True)
    service = run_controller._ResidentRequestReplyControllerService(
        controller_scope_id="equities.ibkr.hedge.main",
        transport_root_dir=tmp_path / ".run",
        repo_root=tmp_path,
        config={
            "controller": {
                "controller_scope_id": "equities.ibkr.hedge.main",
                "allow_single_host_canary": True,
                "mode": "active",
                "write_ownership_enabled": True,
            },
            "account_scopes": [
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4002,
                    "ibg_client_id": 208,
                    "account_id": "U10015777",
                    "controller_scope_id": "equities.ibkr.hedge.main",
                },
            ],
            "controller_scopes": [
                {
                    "controller_scope_id": "equities.ibkr.hedge.main",
                    "profile_id": "equities",
                    "writer_account_scope_id": "ibkr.hedge.main",
                    "account_scope_ids": ["ibkr.hedge.main"],
                    "canary": True,
                },
            ],
        },
        active_writer_factory=lambda **_kwargs: gateway,
    )
    runner = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "equities.ibkr.hedge.main",
                "allow_single_host_canary": True,
                "mode": "active",
                "write_ownership_enabled": True,
            },
            "account_scopes": [
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "ibg_host": "127.0.0.1",
                    "ibg_port": 4002,
                    "ibg_client_id": 208,
                    "account_id": "U10015777",
                    "controller_scope_id": "equities.ibkr.hedge.main",
                },
            ],
            "controller_scopes": [
                {
                    "controller_scope_id": "equities.ibkr.hedge.main",
                    "profile_id": "equities",
                    "writer_account_scope_id": "ibkr.hedge.main",
                    "account_scope_ids": ["ibkr.hedge.main"],
                    "canary": True,
                },
            ],
        },
        owner_id="controller-a",
        repo_root=tmp_path,
        controller_service_factory=lambda _config: service,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="equities.ibkr.hedge.main",
        root_dir=tmp_path / ".run",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-hedge-fail-001",
                controller_scope_id="equities.ibkr.hedge.main",
                strategy_id="strategy-01",
            ),
            requested_at_ns=123_456,
            command=transport.ControllerIntentCommandPayload(
                command_type="place",
                order_role="hedge",
                instrument_id="AAPL.NASDAQ",
                side="BUY",
                quantity="10",
                limit_price="190.01",
                time_in_force="IOC",
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    store = wal.SQLiteOwnershipWal(db_path=service._wal_path)
    try:
        records = store.list_records()
    finally:
        store.close()

    assert reply.status == "accepted"
    assert reply.claim is not None
    assert len(gateway.place_calls) == 1
    assert len(records) == 1
    assert records[0].lifecycle_state is intents.ExecutionLifecycleState.OWNED_PRE_WRITE
    assert records[0].venue_order_id is None


def test_ibkr_active_writer_gateway_preserves_include_overnight_tags() -> None:
    run_controller = _load_run_controller_module()
    account_scopes = importlib.import_module("flux.common.account_scopes")
    contract_module = importlib.import_module("ibapi.contract")

    class _FakeProvider:
        async def instrument_id_to_ib_contract(self, _instrument_id):
            contract = contract_module.Contract()
            contract.symbol = "AAPL"
            contract.secType = "STK"
            contract.exchange = "SMART"
            contract.currency = "USD"
            return contract

    class _FakeClient:
        def __init__(self) -> None:
            self.orders = []

        def next_order_id(self) -> int:
            return 42

        def place_order(self, order) -> None:
            self.orders.append(order)

    gateway = run_controller._IBKRActiveWriterGateway(
        scope=account_scopes.AccountScopeConfig(
            scope_id="ibkr.hedge.main",
            provider="ibkr",
            venue="IBKR",
            ibg_host="127.0.0.1",
            ibg_port=4002,
            ibg_client_id=208,
            account_id="U10015777",
        ),
        controller_scope_id="equities.ibkr.hedge.main",
    )
    client = _FakeClient()
    gateway._client = client
    gateway._provider = _FakeProvider()

    venue_order_id = asyncio.run(
        gateway._place_order_async(
            client_order_id="client-001",
            instrument_id="AAPL.NASDAQ",
            side="BUY",
            quantity="10",
            limit_price="190.01",
            time_in_force="DAY",
            route="SMART",
            outside_rth=True,
            include_overnight=True,
        ),
    )

    assert venue_order_id == "42"
    assert len(client.orders) == 1
    assert client.orders[0].outsideRth is True
    assert client.orders[0].includeOvernight is True


def test_active_canary_routes_place_intents_through_wal_backed_writer(
    tmp_path: Path,
) -> None:
    run_controller = _load_run_controller_module()
    transport = importlib.import_module("flux.execution.transport")
    wal = importlib.import_module("flux.execution.wal")
    seen_pre_write_states: list[str | None] = []

    class _RecordingGateway:
        def start(self) -> None:
            return None

        def stop(self) -> None:
            return None

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
            del instrument_id, side, quantity, limit_price, time_in_force, route, outside_rth, include_overnight
            store = wal.SQLiteOwnershipWal(
                db_path=run_controller._controller_wal_path(
                    repo_root=tmp_path,
                    controller_scope_id="acct.execution.main",
                ),
            )
            try:
                record = store.fetch_by_client_order_id(client_order_id)
                seen_pre_write_states.append(
                    None if record is None else record.lifecycle_state.value,
                )
            finally:
                store.close()
            return "venue-9001"

        def cancel_order(self, venue_order_id: str) -> str:
            return venue_order_id

    runner = run_controller.build_runner(
        {
            "account_scopes": [
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "account_id": "DU123456",
                },
            ],
            "controller_scopes": [
                {
                    "controller_scope_id": "acct.execution.main",
                    "profile_id": "equities",
                    "writer_account_scope_id": "ibkr.hedge.main",
                    "account_scope_ids": ["ibkr.hedge.main"],
                    "canary": True,
                },
            ],
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
                "mode": "active",
            },
        },
        owner_id="controller-a",
        repo_root=tmp_path,
        active_writer_factory=lambda **_kwargs: _RecordingGateway(),
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="acct.execution.main",
        root_dir=tmp_path / ".run",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-active-001",
                controller_scope_id="acct.execution.main",
                strategy_id="strategy-01",
            ),
            requested_at_ns=123_456,
            command=transport.ControllerIntentCommandPayload(
                command_type="place",
                order_role="hedge",
                instrument_id="AAPL.NASDAQ",
                side="BUY",
                quantity="10",
                limit_price="190.01",
                time_in_force="DAY",
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    assert reply.status == "accepted"
    assert seen_pre_write_states == ["owned_pre_write"]

    store = wal.SQLiteOwnershipWal(
        db_path=run_controller._controller_wal_path(
            repo_root=tmp_path,
            controller_scope_id="acct.execution.main",
        ),
    )
    try:
        history = store.list_records()
    finally:
        store.close()

    assert [record.lifecycle_state.value for record in history] == [
        "owned_pre_write",
        "sent_to_venue",
    ]
    assert history[-1].venue_order_id == "venue-9001"
    assert history[-1].claim.client_order_id == reply.claim.client_order_id


def test_active_canary_rollback_to_shadow_keeps_request_path_without_invoking_writer(
    tmp_path: Path,
) -> None:
    run_controller = _load_run_controller_module()
    transport = importlib.import_module("flux.execution.transport")
    writer_calls: list[str] = []

    class _ShouldNotRunGateway:
        def start(self) -> None:
            return None

        def stop(self) -> None:
            return None

        def place_order(self, **kwargs) -> str:
            writer_calls.append(str(kwargs["client_order_id"]))
            return "venue-should-not-exist"

        def cancel_order(self, venue_order_id: str) -> str:
            writer_calls.append(venue_order_id)
            return venue_order_id

    runner = run_controller.build_runner(
        {
            "account_scopes": [
                {
                    "scope_id": "ibkr.hedge.main",
                    "provider": "ibkr",
                    "venue": "IBKR",
                    "account_id": "DU123456",
                },
            ],
            "controller_scopes": [
                {
                    "controller_scope_id": "acct.execution.main",
                    "profile_id": "equities",
                    "writer_account_scope_id": "ibkr.hedge.main",
                    "account_scope_ids": ["ibkr.hedge.main"],
                    "canary": True,
                },
            ],
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
                "mode": "active",
                "write_ownership_enabled": False,
            },
        },
        owner_id="controller-a",
        repo_root=tmp_path,
        active_writer_factory=lambda **_kwargs: _ShouldNotRunGateway(),
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="acct.execution.main",
        root_dir=tmp_path / ".run",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-shadow-001",
                controller_scope_id="acct.execution.main",
                strategy_id="strategy-01",
            ),
            requested_at_ns=123_456,
            command=transport.ControllerIntentCommandPayload(
                command_type="place",
                order_role="hedge",
                instrument_id="AAPL.NASDAQ",
                side="BUY",
                quantity="10",
                limit_price="190.01",
                time_in_force="DAY",
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    assert runner.config.run_mode is run_controller.ControllerRunMode.SHADOW
    assert reply.status == "accepted"
    assert writer_calls == []
    assert not run_controller._controller_wal_path(
        repo_root=tmp_path,
        controller_scope_id="acct.execution.main",
    ).exists()


def test_build_runner_rejects_duplicate_default_startup_for_same_scope(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()
    leases = importlib.import_module("flux.execution.leases")
    first = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
            },
        },
        repo_root=tmp_path,
    )
    second = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
            },
        },
        repo_root=tmp_path,
    )

    first.start(now_ms=1_000)

    with pytest.raises(leases.ControllerLeaseRejectedError, match="already (owned|running)"):
        second.start(now_ms=1_000)

    first.stop()


def test_build_runner_rejects_duplicate_default_startup_after_ttl_while_running(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()
    leases = importlib.import_module("flux.execution.leases")
    first = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
            },
        },
        repo_root=tmp_path,
    )
    second = run_controller.build_runner(
        {
            "controller": {
                "controller_scope_id": "acct.execution.main",
                "allow_single_host_canary": True,
            },
        },
        repo_root=tmp_path,
    )

    first.start(now_ms=1_000)

    with pytest.raises(leases.ControllerLeaseRejectedError, match="already running"):
        second.start(now_ms=1_251)

    assert first.running is True
    assert second.running is False

    first.stop()


def _wait_for_socket(path: Path, *, timeout_s: float = 1.0) -> None:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.01)
    raise AssertionError(f"timed out waiting for socket: {path}")
