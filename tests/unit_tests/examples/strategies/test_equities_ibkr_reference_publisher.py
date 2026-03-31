from __future__ import annotations

import json
from datetime import datetime
from datetime import timezone
from pathlib import Path

from flux.common.keys import FluxRedisKeys
from flux.runners.shared.ibkr_reference_publisher import (
    IbkrReferencePublisherService,
)
from flux.runners.shared.ibkr_reference_publisher import build_ibkr_reference_publisher_config
from flux.runners.shared.ibkr_reference_publisher import classify_ibkr_session
from flux.runners.shared.ibkr_reference_publisher import compute_next_backoff_ms
from flux.runners.shared.ibkr_reference_publisher import select_reference_feed


class _FakeRedis:
    def __init__(self) -> None:
        self.values: dict[str, bytes] = {}
        self.published: list[tuple[str, str]] = []

    def set(self, key: str, value: str | bytes) -> bool:
        self.values[key] = value.encode() if isinstance(value, str) else value
        return True

    def publish(self, channel: str, message: str) -> int:
        self.published.append((channel, message))
        return 1


def _config(*, instruments: tuple[str, ...] = ("AAPL.NASDAQ", "AMD.NASDAQ")) -> dict:
    return {
        "ibkr_reference_publisher": {
            "enabled": True,
            "profile_id": "equities",
            "account_scope_id": "ibkr.reference.main",
            "service_id": "ibkr_reference_publisher",
            "snapshot_interval_ms": 200,
            "stale_after_ms": 5_000,
            "non_rth_stale_after_ms": 300_000,
            "reconnect_backoff_initial_ms": 1_000,
            "reconnect_backoff_max_ms": 15_000,
        },
        "account_scopes": [
            {
                "scope_id": "ibkr.reference.main",
                "provider": "ibkr",
                "venue": "IBKR",
                "ibg_host": "127.0.0.1",
                "ibg_port": 4001,
                "ibg_fallback_ports": [4002],
                "ibg_client_id": 7,
                "ibg_connection_timeout_secs": 5,
                "ibg_request_timeout_secs": 10,
                "dockerized_gateway": {
                    "trading_mode": "live",
                    "read_only_api": True,
                },
            },
            {
                "scope_id": "ibkr.hedge.main",
                "provider": "ibkr",
                "venue": "IBKR",
                "ibg_host": "127.0.0.1",
                "ibg_port": 4001,
                "ibg_client_id": 8,
            },
        ],
        "strategy_contracts": [
            {
                "strategy_id": "aapl_tradexyz_maker",
                "portfolio_asset_id": "AAPL",
                "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "reference_instrument_id": instruments[0],
                "execution_account_scope_id": "hyperliquid.xyz.main",
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": "ibkr.hedge.main",
            },
            {
                "strategy_id": "aapl_tradexyz_taker",
                "portfolio_asset_id": "AAPL",
                "maker_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "reference_instrument_id": instruments[0],
                "execution_account_scope_id": "hyperliquid.xyz.main",
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": "ibkr.hedge.main",
            },
            {
                "strategy_id": "amd_tradexyz_maker",
                "portfolio_asset_id": "AMD",
                "maker_instrument_id": "xyz:AMD-USD-PERP.HYPERLIQUID",
                "reference_instrument_id": instruments[-1],
                "execution_account_scope_id": "hyperliquid.xyz.main",
                "reference_account_scope_id": "ibkr.reference.main",
                "hedge_account_scope_id": "ibkr.hedge.main",
            },
        ],
    }


def test_build_ibkr_reference_publisher_config_dedupes_universe_and_resolves_scope() -> None:
    config = build_ibkr_reference_publisher_config(_config())

    assert config.profile_id == "equities"
    assert config.account_scope_id == "ibkr.reference.main"
    assert config.service_id == "ibkr_reference_publisher"
    assert config.ibg_host == "127.0.0.1"
    assert config.ibg_port == 4001
    assert config.ibg_fallback_ports == (4002,)
    assert config.ibg_client_id == 7
    assert [instrument.instrument_id for instrument in config.instruments] == [
        "AAPL.NASDAQ",
        "AMD.NASDAQ",
    ]
    assert [instrument.symbol for instrument in config.instruments] == ["AAPL", "AMD"]
    assert [instrument.primary_exchange for instrument in config.instruments] == [
        "NASDAQ",
        "NASDAQ",
    ]


def test_build_ibkr_reference_publisher_config_allows_publisher_client_id_override() -> None:
    raw = _config()
    raw["ibkr_reference_publisher"]["ibg_client_id"] = 109

    config = build_ibkr_reference_publisher_config(raw)

    assert config.account_scope_id == "ibkr.reference.main"
    assert config.ibg_client_id == 109


def test_build_ibkr_reference_publisher_config_defaults_to_less_aggressive_rth_staleness() -> None:
    raw = _config()
    raw["ibkr_reference_publisher"].pop("stale_after_ms")

    config = build_ibkr_reference_publisher_config(raw)

    assert config.stale_after_ms == 5_000


def test_classify_ibkr_session_preserves_regular_and_overnight_windows() -> None:
    assert classify_ibkr_session(
        datetime(2026, 3, 30, 12, 0, tzinfo=timezone.utc),
    ) == "PRE"
    assert classify_ibkr_session(
        datetime(2026, 3, 31, 1, 30, tzinfo=timezone.utc),
    ) == "OVERNIGHT"


def test_select_reference_feed_prefers_session_route_and_falls_back_to_fresh_alternative() -> None:
    now_ms = 10_000
    smart_md = {"bid": 100.0, "ask": 100.5, "bid_size": 10.0, "ask_size": 11.0, "ts_event_ms": 9_500}
    overnight_md = {
        "bid": 99.9,
        "ask": 100.4,
        "bid_size": 9.0,
        "ask_size": 10.0,
        "ts_event_ms": 9_800,
    }

    route, selected = select_reference_feed(
        session="RTH",
        smart_md=smart_md,
        overnight_md=overnight_md,
        now_ms=now_ms,
        stale_after_ms=5_000,
    )
    assert route == "SMART"
    assert selected == smart_md

    route, selected = select_reference_feed(
        session="OVERNIGHT",
        smart_md=smart_md,
        overnight_md={**overnight_md, "ts_event_ms": 4_000},
        now_ms=now_ms,
        stale_after_ms=5_000,
    )
    assert route == "SMART"
    assert selected == smart_md


def test_publish_from_snapshot_map_writes_shared_quote_and_status_keys() -> None:
    redis_client = _FakeRedis()
    config = build_ibkr_reference_publisher_config(_config(instruments=("AAPL.NASDAQ",)))
    service = IbkrReferencePublisherService(config=config, redis_client=redis_client)

    status_payload = service.publish_from_snapshot_map(
        {
            "AAPL.NASDAQ": {
                "SMART": {
                    "bid": 190.25,
                    "ask": 190.5,
                    "bid_size": 7.0,
                    "ask_size": 9.0,
                    "ts_event_ms": 9_900,
                },
                "OVERNIGHT": {
                    "bid": 189.0,
                    "ask": 189.5,
                    "bid_size": 2.0,
                    "ask_size": 3.0,
                    "ts_event_ms": 8_000,
                },
            },
        },
        session="RTH",
        now_ms=10_000,
    )

    market_key = FluxRedisKeys.profile_market_last(
        profile_id="equities",
        account_scope_id="ibkr.reference.main",
        exchange="ibkr",
        instrument_id="AAPL.NASDAQ",
    )
    payload = json.loads(redis_client.values[market_key].decode())
    assert payload["instrument_id"] == "AAPL.NASDAQ"
    assert payload["route"] == "SMART"
    assert payload["bid"] == 190.25
    assert payload["ask"] == 190.5
    assert payload["session"] == "RTH"
    assert payload["ts_event_ms"] == 9_900

    status_key = FluxRedisKeys.profile_market_data_status(
        profile_id="equities",
        account_scope_id="ibkr.reference.main",
        service_id="ibkr_reference_publisher",
    )
    stored_status = json.loads(redis_client.values[status_key].decode())
    assert stored_status["state"] == "publishing"
    assert stored_status["connected"] is True
    assert stored_status["instrument_status"]["AAPL.NASDAQ"]["state"] == "healthy"
    assert status_payload == stored_status
    assert (
        FluxRedisKeys.profile_market_last_channel(
            profile_id="equities",
            account_scope_id="ibkr.reference.main",
            exchange="ibkr",
            instrument_id="AAPL.NASDAQ",
        ),
        json.dumps(payload, sort_keys=True),
    ) in redis_client.published


def test_publish_from_snapshot_map_marks_service_degraded_when_any_required_instrument_is_missing() -> None:
    redis_client = _FakeRedis()
    config = build_ibkr_reference_publisher_config(_config())
    service = IbkrReferencePublisherService(config=config, redis_client=redis_client)

    status_payload = service.publish_from_snapshot_map(
        {
            "AAPL.NASDAQ": {
                "SMART": {
                    "bid": 190.25,
                    "ask": 190.5,
                    "bid_size": 7.0,
                    "ask_size": 9.0,
                    "ts_event_ms": 9_900,
                },
            },
        },
        session="RTH",
        now_ms=10_000,
    )

    assert status_payload["state"] == "degraded"
    assert status_payload["instrument_status"]["AAPL.NASDAQ"]["state"] == "healthy"
    assert status_payload["instrument_status"]["AMD.NASDAQ"]["state"] == "missing"


def test_publish_from_snapshot_map_uses_non_rth_freshness_budget_outside_regular_hours() -> None:
    redis_client = _FakeRedis()
    config = build_ibkr_reference_publisher_config(_config(instruments=("AAPL.NASDAQ",)))
    service = IbkrReferencePublisherService(config=config, redis_client=redis_client)

    status_payload = service.publish_from_snapshot_map(
        {
            "AAPL.NASDAQ": {
                "SMART": {
                    "bid": 190.25,
                    "ask": 190.5,
                    "bid_size": 7.0,
                    "ask_size": 9.0,
                    "ts_event_ms": 20_000,
                },
            },
        },
        session="POST",
        now_ms=50_000,
    )

    assert status_payload["state"] == "publishing"
    assert status_payload["stale_after_ms"] == 300_000
    assert status_payload["instrument_status"]["AAPL.NASDAQ"]["state"] == "healthy"
    assert status_payload["instrument_status"]["AAPL.NASDAQ"]["age_ms"] == 30_000


def test_compute_next_backoff_ms_is_explicit_and_bounded() -> None:
    assert compute_next_backoff_ms(
        current_backoff_ms=None,
        initial_backoff_ms=1_000,
        max_backoff_ms=15_000,
    ) == 1_000
    assert compute_next_backoff_ms(
        current_backoff_ms=1_000,
        initial_backoff_ms=1_000,
        max_backoff_ms=15_000,
    ) == 2_000
    assert compute_next_backoff_ms(
        current_backoff_ms=10_000,
        initial_backoff_ms=1_000,
        max_backoff_ms=15_000,
    ) == 15_000


def test_runtime_close_waits_for_client_stop_before_stopping_loop(monkeypatch) -> None:
    from flux.runners.shared import ibkr_reference_publisher as publisher_mod

    class _FakeFuture:
        def __init__(self) -> None:
            self.result_timeout: float | None = None

        def result(self, timeout: float | None = None) -> None:
            self.result_timeout = timeout

    class _FakeLoop:
        def __init__(self) -> None:
            self.closed = False
            self.calls: list[tuple[object, tuple[object, ...]]] = []

        def is_closed(self) -> bool:
            return self.closed

        def call_soon_threadsafe(self, callback, *args) -> None:
            self.calls.append((callback, args))

        def stop(self) -> None:
            return None

    class _FakeThread:
        def is_alive(self) -> bool:
            return False

    class _FakeClient:
        async def _stop_async(self) -> None:
            return None

    fake_future = _FakeFuture()
    close_calls: list[tuple[object, object]] = []

    def _fake_run_coroutine_threadsafe(coro, loop):
        close_calls.append((coro, loop))
        coro.close()
        return fake_future

    monkeypatch.setattr(
        publisher_mod.asyncio,
        "run_coroutine_threadsafe",
        _fake_run_coroutine_threadsafe,
    )

    runtime = publisher_mod._ThreadedIbkrReferenceRuntime.__new__(
        publisher_mod._ThreadedIbkrReferenceRuntime,
    )
    runtime._client = _FakeClient()
    runtime._loop = _FakeLoop()
    runtime._thread = _FakeThread()

    runtime.close()

    assert len(close_calls) == 1
    assert fake_future.result_timeout == 5
    assert runtime._loop is None


def test_module_source_does_not_depend_on_chainsaw_or_configparser() -> None:
    import flux.runners.shared.ibkr_reference_publisher as publisher_mod

    source = Path(publisher_mod.__file__).read_text(encoding="utf-8")

    assert "configparser" not in source
    assert "chainsaw" not in source
