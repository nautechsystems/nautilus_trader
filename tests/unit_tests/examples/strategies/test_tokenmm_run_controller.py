from __future__ import annotations

import importlib
import json
from pathlib import Path
import time

from flux.execution.intents import ExecutionIntent
from flux.execution.intents import ExecutionLifecycleState
from flux.execution.transport import ControllerIntentRequest
from flux.execution.transport import ControllerIntentCommandPayload
from flux.execution.transport import UdsTransportPaths
from flux.strategies.makerv4.strategy import ControllerStateFeedBridge


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


def test_build_runner_starts_resident_request_reply_controller_service(tmp_path: Path) -> None:
    run_controller = _load_run_controller_module()
    transport = importlib.import_module("flux.execution.transport")

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
    transport = importlib.import_module("flux.execution.transport")
    fake_redis = _FakeRedis()
    monkeypatch.setattr(run_controller.redis, "Redis", lambda **_kwargs: fake_redis)

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

    assert lifecycle_payload["lifecycle_state"] == "accepted"
    assert canonical_payload["controller_scope_id"] == "tokenmm.binance.pm.main"
    assert canonical_payload["authority_state"] == "authoritative"
    assert canonical_payload["managed_maker_orders"][0]["client_order_id"] == reply.claim.client_order_id


def test_active_writer_path_records_wal_and_sent_to_venue_state(
    tmp_path: Path,
    monkeypatch,
) -> None:
    run_controller = _load_run_controller_module()
    transport = importlib.import_module("flux.execution.transport")
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


def _wait_for_socket(path: Path, *, timeout_s: float = 1.0) -> None:
    deadline = time.monotonic() + timeout_s
    while time.monotonic() < deadline:
        if path.exists():
            return
        time.sleep(0.01)
    raise AssertionError(f"timed out waiting for socket: {path}")
