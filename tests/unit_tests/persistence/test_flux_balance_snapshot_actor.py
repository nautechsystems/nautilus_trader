from __future__ import annotations

import sqlite3

from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


TOPIC_BALANCES = "flux.makerv3.balances"


def _fetch_all(db_path: str, sql: str, params: tuple[object, ...] = ()) -> list[sqlite3.Row]:
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    try:
        return conn.execute(sql, params).fetchall()
    finally:
        conn.close()


def _make_actor(tmp_path, *, unchanged_heartbeat_ms: int = 60_000):
    from nautilus_trader.flux.persistence.balance_snapshots.actor import (
        FluxBalanceSnapshotPersistenceActor,
    )
    from nautilus_trader.flux.persistence.balance_snapshots.config import (
        FluxBalanceSnapshotPersistenceActorConfig,
    )

    clock = TestClock()
    msgbus = MessageBus(
        trader_id=TestIdStubs.trader_id(),
        clock=clock,
    )
    cache = TestComponentStubs.cache()
    portfolio = Portfolio(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    db_path = str(tmp_path / "balance_snapshots.sqlite")

    config = FluxBalanceSnapshotPersistenceActorConfig(
        component_id="BALANCE-SNAPSHOT-DB",
        db_path=db_path,
        topic=TOPIC_BALANCES,
        flush_interval_ms=10,
        max_batch_size=1000,
        flush_time_budget_ms=10,
        flush_timeout_ms=5_000,
        max_queue_size=10_000,
        on_error="buffer_until_full_then_fail",
        stop_timeout_ms=5_000,
        strict_stop=False,
        propagate_errors_to_bus=False,
        unchanged_heartbeat_ms=unchanged_heartbeat_ms,
    )

    actor = FluxBalanceSnapshotPersistenceActor(
        config=config,
        run_writer_thread=False,
    )
    actor.register_base(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    return actor, msgbus, db_path


def _balances_payload(*, total: str = "100") -> dict[str, object]:
    return {
        "strategy_id": "maker_v3_01",
        "accounts": [
            {
                "account_id": "BYBIT-001",
                "events": [
                    {
                        "account_id": "BYBIT-001",
                        "balances": [
                            {"currency": "PLUME", "free": "90", "locked": "10", "total": total},
                        ],
                    },
                ],
            },
        ],
        "positions": [
            {
                "position_id": "POS-001",
                "instrument_id": "PLUMEUSDT.BYBIT_SPOT",
                "signed_qty": "50",
                "quantity": "50",
                "side": "LONG",
                "avg_px_open": "0.12",
            },
        ],
        "ts_event": 123_000_000_000,
        "ts_ms": 123_000,
    }


def test_balance_snapshot_actor_persists_snapshot_header_and_rows(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path)

    actor.start()
    msgbus.publish(topic=TOPIC_BALANCES, msg=_balances_payload())
    actor.flush()
    actor.stop()

    header_rows = _fetch_all(
        db_path,
        "SELECT strategy_id, account_count, position_count FROM flux_balance_snapshot",
    )
    body_rows = _fetch_all(
        db_path,
        "SELECT kind, row_key, total, signed_qty FROM flux_balance_snapshot_row ORDER BY row_key ASC",
    )

    assert len(header_rows) == 1
    assert header_rows[0]["strategy_id"] == "maker_v3_01"
    assert header_rows[0]["account_count"] == 1
    assert header_rows[0]["position_count"] == 1

    assert len(body_rows) == 2
    assert {row["kind"] for row in body_rows} == {"cash", "position"}


def test_balance_snapshot_actor_dedupes_unchanged_snapshot_until_heartbeat(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path, unchanged_heartbeat_ms=5_000)

    actor.start()
    msgbus.publish(topic=TOPIC_BALANCES, msg=_balances_payload(total="100"))
    actor.flush()
    msgbus.publish(topic=TOPIC_BALANCES, msg=_balances_payload(total="100"))
    actor.flush()
    msgbus.publish(topic=TOPIC_BALANCES, msg=_balances_payload(total="101"))
    actor.flush()
    actor.stop()

    rows = _fetch_all(
        db_path,
        "SELECT snapshot_hash FROM flux_balance_snapshot ORDER BY created_at ASC",
    )
    assert len(rows) == 2
    assert rows[0]["snapshot_hash"] != rows[1]["snapshot_hash"]
