from __future__ import annotations

import sqlite3
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

from flux.persistence.balance_snapshots.config import FluxBalanceSnapshotPersistenceActorConfig
from flux.persistence.balance_snapshots.normalize import FluxBalanceSnapshotRecord
from flux.persistence.balance_snapshots.normalize import normalize_balance_snapshot
from flux.persistence.balance_snapshots.sqlite import connect
from flux.persistence.balance_snapshots.sqlite import ensure_schema
from flux.persistence.balance_snapshots.sqlite import insert_many
from nautilus_trader.persistence._action_intent import iter_json_payload_mappings
from nautilus_trader.persistence._async_sqlite import _AsyncSQLitePersistenceActor


@dataclass(frozen=True, slots=True)
class _BalanceSnapshotEnvelope:
    payload: dict[str, Any]
    ts_ingest_ns: int


class FluxBalanceSnapshotPersistenceActor(
    _AsyncSQLitePersistenceActor[_BalanceSnapshotEnvelope, FluxBalanceSnapshotRecord]
):
    """
    Persist Flux balance snapshots into SQLite for historical reconciliation.
    """

    def __init__(
        self,
        config: FluxBalanceSnapshotPersistenceActorConfig,
        *,
        connect_fn: Callable[[str], sqlite3.Connection] = connect,
        ensure_schema_fn: Callable[[sqlite3.Connection], None] = ensure_schema,
        insert_many_fn: Callable[[sqlite3.Connection, list[FluxBalanceSnapshotRecord]], tuple[int, int]] = insert_many,
        run_writer_thread: bool = True,
    ) -> None:
        super().__init__(
            config,
            connect_fn=connect_fn,
            ensure_schema_fn=ensure_schema_fn,
            insert_rows_fn=insert_many_fn,
            run_writer_thread=run_writer_thread,
            thread_name_suffix="balance-snapshots",
            writer_name="Balance snapshot",
            queue_item_name="balance_snapshot",
        )
        self.filtered = 0
        self._last_snapshot_hash_by_strategy: dict[str, str] = {}
        self._last_persisted_ts_ms_by_strategy: dict[str, int] = {}

    def on_start(self) -> None:
        self._last_snapshot_hash_by_strategy.clear()
        self._last_persisted_ts_ms_by_strategy.clear()
        super().on_start()
        self.msgbus.subscribe(topic=self.config.topic, handler=self._on_event_message)

    def on_stop(self) -> None:
        if self.msgbus is not None:
            self.msgbus.unsubscribe(topic=self.config.topic, handler=self._on_event_message)
        super().on_stop()

    def _on_event_message(self, msg: object) -> None:
        matched = False
        ts_ingest_ns = int(self.clock.timestamp_ns()) if self.clock is not None else 0
        for payload in iter_json_payload_mappings(msg):
            strategy_id = str(payload.get("strategy_id", "")).strip()
            if not strategy_id:
                continue
            matched = True
            self._enqueue_payload(
                _BalanceSnapshotEnvelope(
                    payload=payload,
                    ts_ingest_ns=ts_ingest_ns,
                ),
            )
        if not matched:
            self.filtered += 1

    def _build_row(self, payload: _BalanceSnapshotEnvelope) -> FluxBalanceSnapshotRecord | None:
        trader_id = self.msgbus.trader_id.value if self.msgbus is not None else ""
        record = normalize_balance_snapshot(
            trader_id=trader_id,
            topic=self.config.topic,
            payload=payload.payload,
            ts_ingest_ns=payload.ts_ingest_ns,
        )
        if record is None:
            return None

        strategy_id = record.snapshot.strategy_id
        last_hash = self._last_snapshot_hash_by_strategy.get(strategy_id)
        last_ts_ms = self._last_persisted_ts_ms_by_strategy.get(strategy_id)
        unchanged = last_hash == record.snapshot.snapshot_hash
        heartbeat_due = (
            last_ts_ms is None
            or record.snapshot.ts_ms - last_ts_ms >= self.config.unchanged_heartbeat_ms
        )
        if unchanged and not heartbeat_due:
            return None

        self._last_snapshot_hash_by_strategy[strategy_id] = record.snapshot.snapshot_hash
        self._last_persisted_ts_ms_by_strategy[strategy_id] = record.snapshot.ts_ms
        return record
