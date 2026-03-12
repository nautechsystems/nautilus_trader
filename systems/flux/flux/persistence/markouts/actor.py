from __future__ import annotations

import sqlite3
from collections.abc import Callable
from contextlib import suppress
from dataclasses import dataclass
from dataclasses import replace
from datetime import timedelta
from decimal import Decimal
from typing import Any

from flux.persistence.markouts.common import decimal_text
from flux.persistence.markouts.common import markout_bps
from flux.persistence.markouts.common import signed_markout
from flux.persistence.markouts.common import to_decimal
from flux.persistence.markouts.common import to_optional_int
from flux.persistence.markouts.common import to_optional_text
from flux.persistence.markouts.config import ExecutionMarkoutPersistenceActorConfig
from flux.persistence.markouts.sqlite import ExecutionMarkoutRow
from flux.persistence.markouts.sqlite import connect
from flux.persistence.markouts.sqlite import ensure_schema
from flux.persistence.markouts.sqlite import insert_many
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.persistence._action_intent import PLACE_INTENT_TYPE
from nautilus_trader.persistence._action_intent import ActionIntentCache
from nautilus_trader.persistence._action_intent import ActionIntentRecord
from nautilus_trader.persistence._action_intent import current_ts_ns
from nautilus_trader.persistence._action_intent import iter_json_payload_mappings
from nautilus_trader.persistence._async_sqlite import _AsyncSQLitePersistenceActor


@dataclass(frozen=True, slots=True)
class _PendingMarkout:
    event_id: str
    trade_id: str
    strategy_id: str
    instrument_id: str
    client_order_id: str
    order_side: str
    fill_px: Decimal
    fill_qty: Decimal
    benchmark_name: str
    horizon_s: int
    target_ts_ms: int
    expires_at_ts_ms: int
    run_id: str | None
    quote_cycle_id: str | None
    reason_code: str | None
    level_index: int | None


@dataclass(frozen=True, slots=True)
class _ResolvedMarkout:
    event_id: str
    trade_id: str
    strategy_id: str
    instrument_id: str
    client_order_id: str
    order_side: str
    fill_px: Decimal
    fill_qty: Decimal
    benchmark_name: str
    horizon_s: int
    target_ts_ms: int
    benchmark_ts_ms: int | None
    benchmark_px: Decimal | None
    markout_abs: Decimal | None
    markout_bps: Decimal | None
    resolution_status: str
    run_id: str | None
    quote_cycle_id: str | None
    reason_code: str | None
    level_index: int | None


class ExecutionMarkoutPersistenceActor(
    _AsyncSQLitePersistenceActor[_ResolvedMarkout, ExecutionMarkoutRow],
):
    """
    Persist live-forward MakerV3 execution markouts into SQLite.
    """

    EXPIRY_CHECK_INTERVAL = timedelta(seconds=1)

    def __init__(
        self,
        config: ExecutionMarkoutPersistenceActorConfig,
        *,
        connect_fn: Callable[[str], sqlite3.Connection] = connect,
        ensure_schema_fn: Callable[[sqlite3.Connection], None] = ensure_schema,
        insert_many_fn: Callable[
            [sqlite3.Connection, list[ExecutionMarkoutRow]],
            tuple[int, int],
        ] = insert_many,
        run_writer_thread: bool = True,
    ) -> None:
        super().__init__(
            config,
            connect_fn=connect_fn,
            ensure_schema_fn=ensure_schema_fn,
            insert_rows_fn=insert_many_fn,
            run_writer_thread=run_writer_thread,
            thread_name_suffix="markouts",
            writer_name="Execution markout",
            queue_item_name="markout",
        )
        self._horizons_s = self._validate_horizons(config.horizons_s)
        self._pending_by_strategy: dict[str, dict[str, list[_PendingMarkout]]] = {}
        self._action_intent_cache = ActionIntentCache(
            max_entries=config.action_intent_max_entries,
            ttl_ns=config.action_intent_ttl_ms * 1_000_000,
        )
        self._expiry_check_timer_name = f"execution-markout-expiry:{config.component_id}"

    def on_start(self) -> None:
        self._pending_by_strategy.clear()
        self._action_intent_cache.clear()
        super().on_start()
        if self.clock is not None:
            self.clock.set_timer(
                name=self._expiry_check_timer_name,
                interval=self.EXPIRY_CHECK_INTERVAL,
                callback=self._on_expiry_check_timer,
            )
        self.msgbus.subscribe(topic=self.config.topic, handler=self._on_fill_message)
        self.msgbus.subscribe(topic=self.config.fv_topic, handler=self._on_fv_message)
        if self.config.action_intent_topic is not None:
            self.msgbus.subscribe(
                topic=self.config.action_intent_topic,
                handler=self._on_action_intent_message,
            )

    def on_stop(self) -> None:
        self._expire_pending(now_ms=self._now_ms())
        if self.clock is not None:
            with suppress(Exception):
                if self._expiry_check_timer_name in set(self.clock.timer_names):
                    self.clock.cancel_timer(self._expiry_check_timer_name)
        if self.msgbus is not None:
            self.msgbus.unsubscribe(topic=self.config.topic, handler=self._on_fill_message)
            self.msgbus.unsubscribe(topic=self.config.fv_topic, handler=self._on_fv_message)
            if self.config.action_intent_topic is not None:
                self.msgbus.unsubscribe(
                    topic=self.config.action_intent_topic,
                    handler=self._on_action_intent_message,
                )
        self._pending_by_strategy.clear()
        self._action_intent_cache.clear()
        super().on_stop()

    def flush(self) -> None:
        self._expire_pending(now_ms=self._now_ms())
        super().flush()

    def _build_row(self, payload: _ResolvedMarkout) -> ExecutionMarkoutRow:
        trader_id = self.msgbus.trader_id.value if self.msgbus is not None else ""
        return ExecutionMarkoutRow(
            trader_id=trader_id,
            event_id=payload.event_id,
            trade_id=payload.trade_id,
            strategy_id=payload.strategy_id,
            instrument_id=payload.instrument_id,
            client_order_id=payload.client_order_id,
            order_side=payload.order_side,
            fill_px=decimal_text(payload.fill_px) or "0",
            fill_qty=decimal_text(payload.fill_qty) or "0",
            benchmark_name=payload.benchmark_name,
            horizon_s=payload.horizon_s,
            target_ts_ms=payload.target_ts_ms,
            benchmark_ts_ms=payload.benchmark_ts_ms,
            benchmark_px=decimal_text(payload.benchmark_px),
            markout_abs=decimal_text(payload.markout_abs),
            markout_bps=decimal_text(payload.markout_bps),
            resolution_status=payload.resolution_status,
            run_id=payload.run_id,
            quote_cycle_id=payload.quote_cycle_id,
            reason_code=payload.reason_code,
            level_index=payload.level_index,
        )

    def _on_fill_message(self, msg: object) -> None:
        if isinstance(msg, OrderFilled):
            self.on_order_filled(msg)

    def _on_expiry_check_timer(self, event: Any) -> None:
        if getattr(event, "name", "") != self._expiry_check_timer_name:
            return
        self._expire_pending(now_ms=self._now_ms())

    def on_order_filled(self, fill: OrderFilled) -> None:
        now_ns = current_ts_ns(self.clock)
        self._action_intent_cache.prune(now_ns=now_ns)

        fill_px = to_decimal(str(fill.last_px))
        fill_qty = to_decimal(str(fill.last_qty))
        if fill_px is None or fill_qty is None:
            return

        strategy_id = fill.strategy_id.value
        client_order_id = fill.client_order_id.value
        action_intent = self._action_intent_cache.get(
            client_order_id=client_order_id,
            intent_type=PLACE_INTENT_TYPE,
            strategy_id=strategy_id,
            now_ns=now_ns,
        )
        base_fill_ts_ms = fill.ts_event // 1_000_000
        strategy_pending_rows = self._pending_by_strategy.setdefault(strategy_id, {})
        pending_rows = strategy_pending_rows.setdefault(client_order_id, [])
        for horizon_s in self._horizons_s:
            target_ts_ms = base_fill_ts_ms + (horizon_s * 1_000)
            pending_rows.append(
                _PendingMarkout(
                    event_id=fill.id.value,
                    trade_id=fill.trade_id.value,
                    strategy_id=strategy_id,
                    instrument_id=fill.instrument_id.value,
                    client_order_id=client_order_id,
                    order_side=order_side_to_str(fill.order_side),
                    fill_px=fill_px,
                    fill_qty=fill_qty,
                    benchmark_name=self.config.benchmark_name,
                    horizon_s=horizon_s,
                    target_ts_ms=target_ts_ms,
                    expires_at_ts_ms=target_ts_ms + int(self.config.max_pending_ms),
                    run_id=action_intent.run_id if action_intent is not None else None,
                    quote_cycle_id=action_intent.quote_cycle_id
                    if action_intent is not None
                    else None,
                    reason_code=action_intent.reason_code if action_intent is not None else None,
                    level_index=action_intent.level_index if action_intent is not None else None,
                ),
            )

    def _on_fv_message(self, msg: object) -> None:
        for payload in iter_json_payload_mappings(msg):
            strategy_id = to_optional_text(payload.get("strategy_id"))
            fv = to_decimal(payload.get("fv"))
            ts_ms = to_optional_int(payload.get("ts_ms"))
            if strategy_id is None or fv is None or ts_ms is None:
                continue
            self._resolve_pending_for_strategy(strategy_id=strategy_id, fv=fv, ts_ms=ts_ms)

    def _on_action_intent_message(self, msg: object) -> None:
        now_ns = current_ts_ns(self.clock)
        for payload in iter_json_payload_mappings(msg):
            action_intent = ActionIntentRecord.from_payload(payload)
            if action_intent is None:
                continue
            self._action_intent_cache.add(action_intent, now_ns=now_ns)
            self._merge_action_intent(action_intent)

    def _merge_action_intent(self, action_intent: ActionIntentRecord) -> None:
        strategy_pending_rows = self._pending_by_strategy.get(action_intent.strategy_id)
        if not strategy_pending_rows:
            return
        pending_rows = strategy_pending_rows.get(action_intent.client_order_id)
        if not pending_rows:
            return
        strategy_pending_rows[action_intent.client_order_id] = [
            replace(
                row,
                run_id=action_intent.run_id,
                quote_cycle_id=action_intent.quote_cycle_id,
                reason_code=action_intent.reason_code,
                level_index=action_intent.level_index,
            )
            for row in pending_rows
        ]

    def _resolve_pending_for_strategy(self, *, strategy_id: str, fv: Decimal, ts_ms: int) -> None:
        pending_strategy_ids = self._matching_pending_strategy_ids(strategy_id)
        if not pending_strategy_ids:
            return

        for pending_strategy_id in pending_strategy_ids:
            strategy_pending_rows = self._pending_by_strategy.get(pending_strategy_id)
            if not strategy_pending_rows:
                continue

            remaining_by_client_order_id: dict[str, list[_PendingMarkout]] = {}
            for client_order_id, pending_rows in strategy_pending_rows.items():
                remaining_rows: list[_PendingMarkout] = []
                for row in pending_rows:
                    if row.target_ts_ms > ts_ms:
                        remaining_rows.append(row)
                        continue
                    markout_abs = signed_markout(row.order_side, row.fill_px, fv)
                    self._enqueue_payload(
                        _ResolvedMarkout(
                            event_id=row.event_id,
                            trade_id=row.trade_id,
                            strategy_id=row.strategy_id,
                            instrument_id=row.instrument_id,
                            client_order_id=row.client_order_id,
                            order_side=row.order_side,
                            fill_px=row.fill_px,
                            fill_qty=row.fill_qty,
                            benchmark_name=row.benchmark_name,
                            horizon_s=row.horizon_s,
                            target_ts_ms=row.target_ts_ms,
                            benchmark_ts_ms=ts_ms,
                            benchmark_px=fv,
                            markout_abs=markout_abs,
                            markout_bps=markout_bps(markout_abs, row.fill_px),
                            resolution_status="resolved",
                            run_id=row.run_id,
                            quote_cycle_id=row.quote_cycle_id,
                            reason_code=row.reason_code,
                            level_index=row.level_index,
                        ),
                    )
                if remaining_rows:
                    remaining_by_client_order_id[client_order_id] = remaining_rows
            if remaining_by_client_order_id:
                self._pending_by_strategy[pending_strategy_id] = remaining_by_client_order_id
            else:
                self._pending_by_strategy.pop(pending_strategy_id, None)

    def _matching_pending_strategy_ids(self, strategy_id: str) -> tuple[str, ...]:
        return tuple(
            pending_strategy_id
            for pending_strategy_id in self._pending_by_strategy
            if self._strategy_ids_match(pending_strategy_id, strategy_id)
        )

    @staticmethod
    def _strategy_ids_match(left: str, right: str) -> bool:
        right_normalized = ExecutionMarkoutPersistenceActor._normalize_strategy_id(right)
        return left in (
            right,
            right_normalized,
        ) or ExecutionMarkoutPersistenceActor._normalize_strategy_id(left) in {
            right,
            right_normalized,
        }

    @staticmethod
    def _normalize_strategy_id(strategy_id: str) -> str:
        base, sep, suffix = strategy_id.rpartition("-")
        if sep and len(suffix) == 3 and suffix.isdigit():
            return base
        return strategy_id

    def _expire_pending(self, *, now_ms: int) -> None:
        if not self._pending_by_strategy:
            return

        for strategy_id, strategy_pending_rows in list(self._pending_by_strategy.items()):
            remaining_by_client_order_id: dict[str, list[_PendingMarkout]] = {}
            for client_order_id, pending_rows in strategy_pending_rows.items():
                remaining_rows: list[_PendingMarkout] = []
                for row in pending_rows:
                    if row.expires_at_ts_ms > now_ms:
                        remaining_rows.append(row)
                        continue
                    self._enqueue_payload(
                        _ResolvedMarkout(
                            event_id=row.event_id,
                            trade_id=row.trade_id,
                            strategy_id=row.strategy_id,
                            instrument_id=row.instrument_id,
                            client_order_id=row.client_order_id,
                            order_side=row.order_side,
                            fill_px=row.fill_px,
                            fill_qty=row.fill_qty,
                            benchmark_name=row.benchmark_name,
                            horizon_s=row.horizon_s,
                            target_ts_ms=row.target_ts_ms,
                            benchmark_ts_ms=None,
                            benchmark_px=None,
                            markout_abs=None,
                            markout_bps=None,
                            resolution_status="expired",
                            run_id=row.run_id,
                            quote_cycle_id=row.quote_cycle_id,
                            reason_code=row.reason_code,
                            level_index=row.level_index,
                        ),
                    )
                if remaining_rows:
                    remaining_by_client_order_id[client_order_id] = remaining_rows
            if remaining_by_client_order_id:
                self._pending_by_strategy[strategy_id] = remaining_by_client_order_id
            else:
                self._pending_by_strategy.pop(strategy_id, None)

    def _now_ms(self) -> int:
        return current_ts_ns(self.clock) // 1_000_000

    @staticmethod
    def _validate_horizons(raw_horizons: tuple[int, ...]) -> tuple[int, ...]:
        seen: set[int] = set()
        horizons: list[int] = []
        for raw_horizon in raw_horizons:
            horizon_s = int(raw_horizon)
            if horizon_s <= 0 or horizon_s in seen:
                continue
            seen.add(horizon_s)
            horizons.append(horizon_s)
        if not horizons:
            raise ValueError("`horizons_s` must include at least one positive horizon")
        return tuple(horizons)
