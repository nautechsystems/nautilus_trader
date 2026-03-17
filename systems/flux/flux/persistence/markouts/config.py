from __future__ import annotations

from typing import Literal

from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import PositiveInt

ErrorPolicy = Literal["fail_fast", "log_and_drop", "buffer_until_full_then_fail"]


class ExecutionMarkoutPersistenceActorConfig(ActorConfig, frozen=True):
    """
    Configuration for `ExecutionMarkoutPersistenceActor` instances.
    """

    db_path: str
    topic: str = "events.fills.*"
    fv_topic: str = "flux.makerv3.fv"
    action_intent_topic: str | None = "flux.makerv3.order_intent"
    horizons_s: tuple[int, ...] = (30, 60, 120)
    benchmark_name: str = "fv_market_mid"
    max_pending_ms: PositiveInt = 180_000
    flush_interval_ms: PositiveInt = 250
    max_batch_size: PositiveInt = 1000
    flush_time_budget_ms: PositiveInt | None = 10
    flush_timeout_ms: PositiveInt = 5_000
    max_queue_size: PositiveInt = 10_000
    on_error: ErrorPolicy = "buffer_until_full_then_fail"
    stop_timeout_ms: PositiveInt = 5_000
    strict_stop: bool = False
    propagate_errors_to_bus: bool = False
    action_intent_max_entries: PositiveInt = 50_000
    action_intent_ttl_ms: PositiveInt = 86_400_000
