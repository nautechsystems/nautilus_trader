from __future__ import annotations

import importlib
import json
from pathlib import Path
from types import SimpleNamespace
import time

from flux.execution.intents import ExecutionIntent
from flux.execution.intents import ExecutionLifecycleState
from flux.execution.transport import ControllerIntentRequest
from flux.execution.transport import ControllerIntentCommandPayload
from flux.execution.transport import UdsTransportPaths
from flux.strategies.makerv4.strategy import ControllerStateFeedBridge
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce


class _FakeRedis:
    def __init__(self) -> None:
        self.payloads: dict[str, bytes] = {}

    def get(self, key: str):
        return self.payloads.get(key)

    def set(self, key: str, value: bytes) -> None:
        self.payloads[key] = value


class _RecordingVenueWriter:
    def __init__(self, *, venue_order_id: str = "binance-venue-9001") -> None:
        self.venue_order_id = venue_order_id
        self.claims = []

    async def write_owned_order(self, claim) -> str:
        self.claims.append(claim)
        return self.venue_order_id


class _FailingVenueWriter:
    def __init__(self, *, message: str = "simulated venue failure") -> None:
        self.message = message

    async def write_owned_order(self, claim) -> str:
        raise RuntimeError(self.message)


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _load_run_controller_module():
    path = _repo_root() / "systems/flux/flux/runners/tokenmm/run_controller.py"
    assert path.exists(), "tokenmm controller runner module should exist"
    return importlib.import_module("flux.runners.tokenmm.run_controller")


def _shared_config() -> dict[str, object]:
    return {
        "flux": {"namespace": "flux", "schema_version": "v1"},
        "redis": {"host": "127.0.0.1", "port": 6379, "db": 0},
        "controller": {
            "controller_scope_id": "tokenmm.binance.pm.main",
            "account_scope_id": "binance.pm.main",
            "mode": "active",
            "write_ownership_enabled": True,
            "managed_strategy_ids": [
                "plumeusdt_binance_perp_makerv3",
                "plumeusdt_binance_spot_makerv3",
            ],
        },
        "strategy_contracts": [
            {
                "strategy_id": "plumeusdt_binance_perp_makerv3",
                "portfolio_asset_id": "PLUME",
                "maker_instrument_id": "PLUMEUSDT-PERP.BINANCE_PERP",
                "reference_instrument_id": "PLUMEUSDT.BINANCE_SPOT",
                "execution_account_scope_id": "binance.pm.main",
                "reference_account_scope_id": "binance.pm.main",
                "controller_scope_id": "tokenmm.binance.pm.main",
            },
            {
                "strategy_id": "plumeusdt_binance_spot_makerv3",
                "portfolio_asset_id": "PLUME",
                "maker_instrument_id": "PLUMEUSDT.BINANCE_SPOT",
                "reference_instrument_id": "PLUMEUSDT.BINANCE_SPOT",
                "execution_account_scope_id": "binance.pm.main",
                "reference_account_scope_id": "binance.pm.main",
                "controller_scope_id": "tokenmm.binance.pm.main",
            },
        ],
    }


REQUEST_TIMEOUT_S = 5.0


def _load_transport_module():
    transport = importlib.import_module("flux.execution.transport")
    original_send_request = getattr(
        transport,
        "_codex_original_send_request",
        transport.send_request,
    )
    if original_send_request is transport.send_request:
        def _send_request_with_timeout_floor(*, paths, request, timeout_s=1.0):
            return original_send_request(
                paths=paths,
                request=request,
                timeout_s=max(float(timeout_s), REQUEST_TIMEOUT_S),
            )

        transport._codex_original_send_request = original_send_request
        transport.send_request = _send_request_with_timeout_floor
    return transport


def test_repo_root_resolves_checkout_root_for_packaged_controller_layout() -> None:
    run_controller = _load_run_controller_module()

    assert run_controller._repo_root() == _repo_root()


def test_shared_runtime_root_falls_back_to_checkout_run_dir() -> None:
    run_controller = _load_run_controller_module()
    repo_root = Path("/home/ubuntu/nautilus_trader/.worktrees/account-execution-controller-platform-cut-20260328")

    assert run_controller._shared_runtime_root(repo_root) == repo_root / ".run"


def test_shared_runtime_root_uses_stable_release_lane_root() -> None:
    run_controller = _load_run_controller_module()
    release_root = Path("/home/ubuntu/releases/prod/tokenmm/releases/20260330T031141Z-d3b169d45d")

    assert run_controller._shared_runtime_root(release_root) == Path("/home/ubuntu/releases/prod/tokenmm/runtime")


def test_controller_wal_path_uses_stable_runtime_root_for_release_lane(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()
    release_root = tmp_path / "releases" / "prod" / "tokenmm" / "releases" / "rel-001"
    release_root.mkdir(parents=True)

    wal_path = run_controller._controller_wal_path(
        repo_root=release_root,
        controller_scope_id="tokenmm.binance.pm.main",
    )

    assert wal_path == (
        tmp_path
        / "releases"
        / "prod"
        / "tokenmm"
        / "runtime"
        / "controller-wal"
        / "tokenmm.binance.pm.main.sqlite3"
    )
    assert run_controller._strategy_runtime_config_path(
        repo_root=run_controller._repo_root(),
        strategy_id="plumeusdt_binance_spot_makerv3",
    ) == (_repo_root() / "deploy/tokenmm/strategies/plumeusdt_binance_spot_makerv3.toml")


def test_build_runner_starts_resident_request_reply_controller_service(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-001",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_456,
        ),
        timeout_s=1.0,
    )
    runner.stop()

    assert reply.status == "accepted"
    assert reply.claim is not None
    assert reply.claim.controller_scope_id == "tokenmm.binance.pm.main"
    assert not paths.request_reply_path.exists()


def test_resident_service_publishes_lifecycle_and_canonical_state_to_redis(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()
    fake_redis = _FakeRedis()
    writer = _RecordingVenueWriter()
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
        active_order_writer_factory=lambda payload: writer,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )
    feed = ControllerStateFeedBridge(
        redis_client=fake_redis,
        controller_scope_id="tokenmm.binance.pm.main",
        strategy_id="plumeusdt_binance_spot_makerv3",
        namespace="flux",
        schema_version="v1",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-state-001",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_456,
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                side="BUY",
                quantity="1000",
                limit_price="0.1901",
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

    assert lifecycle_payload["lifecycle_state"] == "sent_to_venue"
    assert canonical_payload["controller_scope_id"] == "tokenmm.binance.pm.main"
    assert canonical_payload["authority_state"] == "authoritative"
    assert canonical_payload["managed_maker_orders"][0]["client_order_id"] == reply.claim.client_order_id


def test_active_writer_path_records_wal_and_sent_to_venue_state(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()
    wal = importlib.import_module("flux.execution.wal")
    fake_redis = _FakeRedis()
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)
    writer = _RecordingVenueWriter()

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
        active_order_writer_factory=lambda payload: writer,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )
    feed = ControllerStateFeedBridge(
        redis_client=fake_redis,
        controller_scope_id="tokenmm.binance.pm.main",
        strategy_id="plumeusdt_binance_perp_makerv3",
        namespace="flux",
        schema_version="v1",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-write-001",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_perp_makerv3",
            ),
            requested_at_ns=123_456,
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="PLUMEUSDT-PERP.BINANCE_PERP",
                side="SELL",
                quantity="1000",
                limit_price="0.1910",
                post_only=True,
                time_in_force="GTC",
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    store = wal.SQLiteOwnershipWal(
        db_path=run_controller._controller_wal_path(
            repo_root=tmp_path,
            controller_scope_id="tokenmm.binance.pm.main",
        ),
    )
    try:
        records = store.list_records()
    finally:
        store.close()

    canonical_payload = json.loads(fake_redis.get(feed.canonical_state_key()).decode("utf-8"))

    assert reply.claim is not None
    assert len(writer.claims) == 1
    assert [record.lifecycle_state for record in records] == [
        ExecutionLifecycleState.OWNED_PRE_WRITE,
        ExecutionLifecycleState.SENT_TO_VENUE,
    ]
    assert records[-1].venue_order_id == "binance-venue-9001"
    assert canonical_payload["managed_maker_orders"][0]["client_order_id"] == reply.claim.client_order_id
    assert canonical_payload["managed_maker_orders"][0]["venue_order_id"] == "binance-venue-9001"


def test_active_mode_builds_default_runtime_writer_without_injected_factory(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()
    wal = importlib.import_module("flux.execution.wal")
    fake_redis = _FakeRedis()
    spot_calls: list[dict[str, object]] = []

    class _FakeSpotAccountHttpAPI:
        def __init__(self, client, clock, account_type) -> None:
            self.client = client
            self.clock = clock
            self.account_type = account_type

        async def new_order(self, **kwargs):
            spot_calls.append(kwargs)
            return SimpleNamespace(orderId=9_001)

    class _FakeFuturesAccountHttpAPI:
        def __init__(self, client, clock, account_type, private_api_family=None) -> None:
            self.client = client
            self.clock = clock
            self.account_type = account_type
            self.private_api_family = private_api_family

        async def new_order(self, **_kwargs):
            return SimpleNamespace(orderId=8_001)

    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)
    monkeypatch.setattr(
        run_controller,
        "get_cached_binance_http_client",
        lambda **_kwargs: object(),
        raising=False,
    )
    monkeypatch.setattr(
        run_controller,
        "BinanceSpotAccountHttpAPI",
        _FakeSpotAccountHttpAPI,
        raising=False,
    )
    monkeypatch.setattr(
        run_controller,
        "BinanceFuturesAccountHttpAPI",
        _FakeFuturesAccountHttpAPI,
        raising=False,
    )
    monkeypatch.setenv("BINANCE_API_KEY", "test-key")
    monkeypatch.setenv("BINANCE_API_SECRET", "test-secret")
    _write_tokenmm_strategy_configs(tmp_path)

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-runtime-writer-001",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_456,
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                side="BUY",
                quantity="1000",
                limit_price="0.1901",
                post_only=True,
                time_in_force="GTC",
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    store = wal.SQLiteOwnershipWal(
        db_path=run_controller._controller_wal_path(
            repo_root=tmp_path,
            controller_scope_id="tokenmm.binance.pm.main",
        ),
    )
    try:
        records = store.list_records()
    finally:
        store.close()

    assert reply.status == "accepted"
    assert reply.claim is not None
    assert [record.lifecycle_state for record in records] == [
        ExecutionLifecycleState.OWNED_PRE_WRITE,
        ExecutionLifecycleState.SENT_TO_VENUE,
    ]
    assert len(spot_calls) == 1
    assert spot_calls[0]["new_client_order_id"] == reply.claim.client_order_id
    assert spot_calls[0]["side_effect_type"] == "AUTO_BORROW_REPAY"
    assert spot_calls[0]["auto_repay_at_cancel"] == "FALSE"
    assert spot_calls[0]["order_type"].value == "LIMIT_MAKER"


def test_resident_service_preserves_full_managed_maker_set_across_places(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()
    fake_redis = _FakeRedis()
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
        active_order_writer_factory=lambda payload: _RecordingVenueWriter(),
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )
    feed = ControllerStateFeedBridge(
        redis_client=fake_redis,
        controller_scope_id="tokenmm.binance.pm.main",
        strategy_id="plumeusdt_binance_spot_makerv3",
        namespace="flux",
        schema_version="v1",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    first_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-multi-001",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_456,
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                side="BUY",
                quantity="1000",
                limit_price="0.1901",
                post_only=True,
                time_in_force="GTC",
            ),
        ),
        timeout_s=REQUEST_TIMEOUT_S,
    )
    second_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-multi-002",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_457,
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                side="SELL",
                quantity="900",
                limit_price="0.1902",
                post_only=True,
                time_in_force="GTC",
            ),
        ),
        timeout_s=REQUEST_TIMEOUT_S,
    )
    runner.stop()

    canonical_payload = json.loads(fake_redis.get(feed.canonical_state_key()).decode("utf-8"))
    managed_orders = canonical_payload["managed_maker_orders"]

    assert first_reply.claim is not None
    assert second_reply.claim is not None
    assert len(managed_orders) == 2
    assert {
        row["client_order_id"]
        for row in managed_orders
    } == {first_reply.claim.client_order_id, second_reply.claim.client_order_id}


def test_rejected_cancel_keeps_existing_canonical_state(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()
    fake_redis = _FakeRedis()
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)

    def _writer_factory(payload):
        command = payload["command"]
        if command.command_type == "cancel":
            return _FailingVenueWriter(message="cancel rejected by venue")
        return _RecordingVenueWriter()

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
        active_order_writer_factory=_writer_factory,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )
    feed = ControllerStateFeedBridge(
        redis_client=fake_redis,
        controller_scope_id="tokenmm.binance.pm.main",
        strategy_id="plumeusdt_binance_spot_makerv3",
        namespace="flux",
        schema_version="v1",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    place_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-cancel-reject-place",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_456,
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                side="BUY",
                quantity="1000",
                limit_price="0.1901",
                post_only=True,
                time_in_force="GTC",
            ),
        ),
        timeout_s=REQUEST_TIMEOUT_S,
    )
    assert place_reply.claim is not None

    cancel_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-cancel-reject",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_457,
            command=ControllerIntentCommandPayload(
                command_type="cancel",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                target_client_order_id=place_reply.claim.client_order_id,
            ),
        ),
        timeout_s=REQUEST_TIMEOUT_S,
    )
    runner.stop()

    canonical_payload = json.loads(fake_redis.get(feed.canonical_state_key()).decode("utf-8"))
    managed_orders = canonical_payload["managed_maker_orders"]

    assert cancel_reply.status == "rejected"
    assert len(managed_orders) == 1
    assert managed_orders[0]["client_order_id"] == place_reply.claim.client_order_id


def test_terminal_place_reject_rolls_back_canonical_state_and_publishes_rejected_lifecycle(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()
    wal = importlib.import_module("flux.execution.wal")
    fake_redis = _FakeRedis()
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
        active_order_writer_factory=lambda _payload: _FailingVenueWriter(
            message="{'code': -2010, 'msg': 'Order would immediately match and take.'}",
        ),
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )
    feed = ControllerStateFeedBridge(
        redis_client=fake_redis,
        controller_scope_id="tokenmm.binance.pm.main",
        strategy_id="plumeusdt_binance_spot_makerv3",
        namespace="flux",
        schema_version="v1",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-place-reject-terminal",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_456,
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                side="BUY",
                quantity="1000",
                limit_price="0.1901",
                post_only=True,
                time_in_force="GTC",
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    store = wal.SQLiteOwnershipWal(
        db_path=run_controller._controller_wal_path(
            repo_root=tmp_path,
            controller_scope_id="tokenmm.binance.pm.main",
        ),
    )
    try:
        records = store.list_records()
    finally:
        store.close()

    lifecycle_payload = json.loads(fake_redis.get(feed.lifecycle_event_key()).decode("utf-8"))
    canonical_payload = json.loads(fake_redis.get(feed.canonical_state_key()).decode("utf-8"))

    assert reply.status == "accepted"
    assert reply.claim is not None
    assert [record.lifecycle_state for record in records] == [
        ExecutionLifecycleState.OWNED_PRE_WRITE,
        ExecutionLifecycleState.REJECTED,
    ]
    assert lifecycle_payload["lifecycle_state"] == "rejected"
    assert "immediately match and take" in str(lifecycle_payload["reason"]).lower()
    assert canonical_payload["managed_maker_orders"] == []


def test_successful_cancel_prunes_canonical_state_after_write(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()
    fake_redis = _FakeRedis()
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
        active_order_writer_factory=lambda payload: _RecordingVenueWriter(),
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )
    feed = ControllerStateFeedBridge(
        redis_client=fake_redis,
        controller_scope_id="tokenmm.binance.pm.main",
        strategy_id="plumeusdt_binance_spot_makerv3",
        namespace="flux",
        schema_version="v1",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    place_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-cancel-success-place",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_456,
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                side="BUY",
                quantity="1000",
                limit_price="0.1901",
                post_only=True,
                time_in_force="GTC",
            ),
        ),
        timeout_s=1.0,
    )
    assert place_reply.claim is not None

    cancel_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-cancel-success",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_457,
            command=ControllerIntentCommandPayload(
                command_type="cancel",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                target_client_order_id=place_reply.claim.client_order_id,
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    canonical_payload = json.loads(fake_redis.get(feed.canonical_state_key()).decode("utf-8"))

    assert cancel_reply.status == "accepted"
    assert canonical_payload["managed_maker_orders"] == []


def test_successful_cancel_prunes_internal_state_even_if_lifecycle_publish_fails(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()
    fake_redis = _FakeRedis()
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)

    class _LifecycleFailingFeed:
        def publish_lifecycle_event(self, event) -> None:
            if event.lifecycle_state == ExecutionLifecycleState.SENT_TO_VENUE:
                raise RuntimeError("redis unavailable during lifecycle publish")

        def publish_canonical_state(self, state: dict[str, object]) -> None:
            return None

    monkeypatch.setattr(
        run_controller,
        "_feed_bridge_for_claim",
        lambda **_kwargs: _LifecycleFailingFeed(),
    )

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
        active_order_writer_factory=lambda payload: _RecordingVenueWriter(),
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    place_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-cancel-lifecycle-fail-place",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_456,
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                side="BUY",
                quantity="1000",
                limit_price="0.1901",
                post_only=True,
                time_in_force="GTC",
            ),
        ),
        timeout_s=1.0,
    )
    assert place_reply.claim is not None

    cancel_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-cancel-lifecycle-fail",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_457,
            command=ControllerIntentCommandPayload(
                command_type="cancel",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                target_client_order_id=place_reply.claim.client_order_id,
            ),
        ),
        timeout_s=1.0,
    )

    controller_service = getattr(runner, "_controller_service")
    strategy_state = controller_service._canonical_state_by_strategy["plumeusdt_binance_spot_makerv3"]
    runner.stop()

    assert cancel_reply.status == "accepted"
    assert strategy_state["managed_maker_orders"] == []


def test_successful_cancel_does_not_require_latest_venue_order_id_lookup(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()
    fake_redis = _FakeRedis()
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)

    original_lookup = run_controller._latest_venue_order_id

    def _guarded_latest_venue_order_id(wal, intent_id: str) -> str:
        if intent_id == "intent-cancel-no-lookup":
            raise AssertionError("cancel path should not require venue_order_id lookup")
        return original_lookup(wal, intent_id)

    monkeypatch.setattr(
        run_controller,
        "_latest_venue_order_id",
        _guarded_latest_venue_order_id,
    )

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
        active_order_writer_factory=lambda payload: _RecordingVenueWriter(),
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    place_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-cancel-no-lookup-place",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_456,
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                side="BUY",
                quantity="1000",
                limit_price="0.1901",
                post_only=True,
                time_in_force="GTC",
            ),
        ),
        timeout_s=1.0,
    )
    assert place_reply.claim is not None

    cancel_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-cancel-no-lookup",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_457,
            command=ControllerIntentCommandPayload(
                command_type="cancel",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                target_client_order_id=place_reply.claim.client_order_id,
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    assert cancel_reply.status == "accepted"


def test_default_runtime_writer_treats_unknown_order_cancel_as_terminal_success(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()
    wal = importlib.import_module("flux.execution.wal")
    fake_redis = _FakeRedis()
    spot_cancel_calls: list[dict[str, object]] = []
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)

    class _FakeSpotAccountHttpAPI:
        def __init__(self, client, clock, account_type) -> None:
            self.client = client
            self.clock = clock
            self.account_type = account_type

        async def new_order(self, **_kwargs):
            return SimpleNamespace(orderId=9_001)

        async def cancel_order(self, **kwargs):
            spot_cancel_calls.append(kwargs)
            raise RuntimeError({"code": -2011, "msg": "Unknown order sent."})

    class _FakeFuturesAccountHttpAPI:
        def __init__(self, client, clock, account_type, private_api_family=None) -> None:
            self.client = client
            self.clock = clock
            self.account_type = account_type
            self.private_api_family = private_api_family

    monkeypatch.setattr(
        run_controller,
        "get_cached_binance_http_client",
        lambda **_kwargs: object(),
        raising=False,
    )
    monkeypatch.setattr(
        run_controller,
        "BinanceSpotAccountHttpAPI",
        _FakeSpotAccountHttpAPI,
        raising=False,
    )
    monkeypatch.setattr(
        run_controller,
        "BinanceFuturesAccountHttpAPI",
        _FakeFuturesAccountHttpAPI,
        raising=False,
    )
    monkeypatch.setenv("BINANCE_API_KEY", "test-key")
    monkeypatch.setenv("BINANCE_API_SECRET", "test-secret")
    _write_tokenmm_strategy_configs(tmp_path)

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )
    feed = ControllerStateFeedBridge(
        redis_client=fake_redis,
        controller_scope_id="tokenmm.binance.pm.main",
        strategy_id="plumeusdt_binance_spot_makerv3",
        namespace="flux",
        schema_version="v1",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    place_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-cancel-unknown-place",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_456,
            command=ControllerIntentCommandPayload(
                command_type="place",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                side="BUY",
                quantity="1000",
                limit_price="0.1901",
                post_only=True,
                time_in_force="GTC",
            ),
        ),
        timeout_s=1.0,
    )
    assert place_reply.claim is not None

    cancel_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-cancel-unknown",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_457,
            command=ControllerIntentCommandPayload(
                command_type="cancel",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                target_client_order_id=place_reply.claim.client_order_id,
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    store = wal.SQLiteOwnershipWal(
        db_path=run_controller._controller_wal_path(
            repo_root=tmp_path,
            controller_scope_id="tokenmm.binance.pm.main",
        ),
    )
    try:
        records = store.list_records()
    finally:
        store.close()

    canonical_payload = json.loads(fake_redis.get(feed.canonical_state_key()).decode("utf-8"))

    assert cancel_reply.status == "accepted"
    assert spot_cancel_calls == [
        {
            "symbol": "PLUMEUSDT",
            "orig_client_order_id": place_reply.claim.client_order_id,
            "recv_window": None,
        },
    ]
    assert [record.lifecycle_state for record in records] == [
        ExecutionLifecycleState.OWNED_PRE_WRITE,
        ExecutionLifecycleState.SENT_TO_VENUE,
        ExecutionLifecycleState.OWNED_PRE_WRITE,
        ExecutionLifecycleState.SENT_TO_VENUE,
    ]
    assert canonical_payload["managed_maker_orders"] == []


def test_unknown_cancel_for_carryover_order_without_wal_record_is_quarantined_and_pruned(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = _load_transport_module()
    wal = importlib.import_module("flux.execution.wal")
    fake_redis = _FakeRedis()
    spot_cancel_calls: list[dict[str, object]] = []
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)

    class _FakeSpotAccountHttpAPI:
        def __init__(self, client, clock, account_type) -> None:
            self.client = client
            self.clock = clock
            self.account_type = account_type

        async def new_order(self, **_kwargs):
            return SimpleNamespace(orderId=9_001)

        async def cancel_order(self, **kwargs):
            spot_cancel_calls.append(kwargs)
            raise RuntimeError({"code": -2011, "msg": "Unknown order sent."})

    class _FakeFuturesAccountHttpAPI:
        def __init__(self, client, clock, account_type, private_api_family=None) -> None:
            self.client = client
            self.clock = clock
            self.account_type = account_type
            self.private_api_family = private_api_family

    monkeypatch.setattr(
        run_controller,
        "get_cached_binance_http_client",
        lambda **_kwargs: object(),
        raising=False,
    )
    monkeypatch.setattr(
        run_controller,
        "BinanceSpotAccountHttpAPI",
        _FakeSpotAccountHttpAPI,
        raising=False,
    )
    monkeypatch.setattr(
        run_controller,
        "BinanceFuturesAccountHttpAPI",
        _FakeFuturesAccountHttpAPI,
        raising=False,
    )
    monkeypatch.setenv("BINANCE_API_KEY", "test-key")
    monkeypatch.setenv("BINANCE_API_SECRET", "test-secret")
    _write_tokenmm_strategy_configs(tmp_path)

    runner = run_controller.build_runner(
        _shared_config(),
        owner_id="controller-a",
        repo_root=tmp_path,
    )
    paths = UdsTransportPaths.for_controller_scope(
        controller_scope_id="tokenmm.binance.pm.main",
        root_dir=tmp_path / ".run",
    )
    feed = ControllerStateFeedBridge(
        redis_client=fake_redis,
        controller_scope_id="tokenmm.binance.pm.main",
        strategy_id="plumeusdt_binance_spot_makerv3",
        namespace="flux",
        schema_version="v1",
    )

    runner.start(now_ms=1_000)
    _wait_for_socket(paths.request_reply_path)
    stale_client_order_id = "ctl_carryover_startup_cleanup_target"
    controller_service = getattr(runner, "_controller_service")
    controller_service._canonical_state_by_strategy["plumeusdt_binance_spot_makerv3"] = {
        "managed_maker_orders": [
            {
                "client_order_id": stale_client_order_id,
                "instrument_id": "PLUMEUSDT.BINANCE_SPOT",
                "side": "BUY",
                "quantity": "1000",
                "price": "0.1901",
                "post_only": True,
                "pending_cancel": False,
            },
        ],
    }

    cancel_reply = transport.send_request(
        paths=paths,
        request=ControllerIntentRequest(
            intent=ExecutionIntent(
                intent_id="intent-cancel-carryover-stale",
                controller_scope_id="tokenmm.binance.pm.main",
                strategy_id="plumeusdt_binance_spot_makerv3",
            ),
            requested_at_ns=123_457,
            command=ControllerIntentCommandPayload(
                command_type="cancel",
                order_role="maker",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
                target_client_order_id=stale_client_order_id,
            ),
        ),
        timeout_s=1.0,
    )
    runner.stop()

    store = wal.SQLiteOwnershipWal(
        db_path=run_controller._controller_wal_path(
            repo_root=tmp_path,
            controller_scope_id="tokenmm.binance.pm.main",
        ),
    )
    try:
        records = store.list_records()
    finally:
        store.close()

    lifecycle_payload = json.loads(fake_redis.get(feed.lifecycle_event_key()).decode("utf-8"))
    canonical_payload = json.loads(fake_redis.get(feed.canonical_state_key()).decode("utf-8"))

    assert cancel_reply.status == "accepted"
    assert spot_cancel_calls == [
        {
            "symbol": "PLUMEUSDT",
            "orig_client_order_id": stale_client_order_id,
            "recv_window": None,
        },
    ]
    assert [record.lifecycle_state for record in records] == [
        ExecutionLifecycleState.OWNED_PRE_WRITE,
        ExecutionLifecycleState.QUARANTINED,
    ]
    assert lifecycle_payload["lifecycle_state"] == "quarantined"
    assert lifecycle_payload["venue_activity_origin"] == "orphan"
    assert "missing venue order id" in str(lifecycle_payload["reason"]).lower()
    assert canonical_payload["managed_maker_orders"] == []


def test_controller_normalizes_nautilus_enum_side_and_tif_values() -> None:
    run_controller = _load_run_controller_module()

    assert run_controller._coerce_binance_order_side(str(OrderSide.BUY)).value == "BUY"
    assert run_controller._coerce_binance_time_in_force(str(TimeInForce.GTC)).value == "GTC"


def test_canonical_state_payload_normalizes_numeric_side_codes() -> None:
    run_controller = _load_run_controller_module()
    request = ControllerIntentRequest(
        intent=ExecutionIntent(
            intent_id="intent-state-normalize-001",
            controller_scope_id="tokenmm.binance.pm.main",
            strategy_id="plumeusdt_binance_spot_makerv3",
        ),
        requested_at_ns=123_456,
        command=ControllerIntentCommandPayload(
            command_type="place",
            order_role="maker",
            instrument_id="PLUMEUSDT.BINANCE_SPOT",
            side=str(OrderSide.SELL),
            quantity="1000",
            limit_price="0.1901",
            post_only=True,
            time_in_force=str(TimeInForce.GTC),
        ),
    )
    claim = request.intent.claim(controller_epoch=4, controller_seq=17)

    state = run_controller._canonical_state_payload(
        request=request,
        claim=claim,
        existing_state=None,
    )

    assert state["managed_maker_orders"][0]["side"] == "SELL"


def _write_tokenmm_strategy_configs(repo_root: Path) -> None:
    strategies_dir = repo_root / "deploy" / "tokenmm" / "strategies"
    strategies_dir.mkdir(parents=True, exist_ok=True)
    (strategies_dir / "plumeusdt_binance_spot_makerv3.toml").write_text(
        """
[flux]
mode = "live"
confirm_live = true

[identity]
strategy_id = "plumeusdt_binance_spot_makerv3"

[venues]
execution_venue = "BINANCE_SPOT"
execution_symbol = "PLUMEUSDT"

[node.venues.BINANCE_SPOT]
adapter = "binance"
execution = true
api_key_env = "BINANCE_API_KEY"
api_secret_env = "BINANCE_API_SECRET"
account_type = "PORTFOLIO_MARGIN"
allow_cash_borrowing = true

[strategy]
strategy_id = "plumeusdt_binance_spot_makerv3"
spot_cash_borrowing_policy = "both_sides"
""".strip(),
        encoding="utf-8",
    )
    (strategies_dir / "plumeusdt_binance_perp_makerv3.toml").write_text(
        """
[flux]
mode = "live"
confirm_live = true

[identity]
strategy_id = "plumeusdt_binance_perp_makerv3"

[venues]
execution_venue = "BINANCE_PERP"
execution_symbol = "PLUMEUSDT"

[node.venues.BINANCE_PERP]
adapter = "binance"
execution = true
api_key_env = "BINANCE_API_KEY"
api_secret_env = "BINANCE_API_SECRET"
account_type = "USDT_FUTURES"
private_api_family = "PORTFOLIO_MARGIN"

[strategy]
strategy_id = "plumeusdt_binance_perp_makerv3"
""".strip(),
        encoding="utf-8",
    )


def _wait_for_socket(path: Path, *, timeout_s: float = 20.0) -> None:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.01)
    raise AssertionError(f"timed out waiting for socket: {path}")
