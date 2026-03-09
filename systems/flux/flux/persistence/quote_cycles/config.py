from __future__ import annotations

from typing import Literal

from nautilus_trader.common.config import ActorConfig
from nautilus_trader.common.config import PositiveInt

ErrorPolicy = Literal["fail_fast", "log_and_drop", "buffer_until_full_then_fail"]


class QuoteCyclePersistenceActorConfig(ActorConfig, frozen=True):
    """
    Configuration for `QuoteCyclePersistenceActor` instances.
    """

    db_path: str
    topic: str = "flux.makerv3.event"
    flush_interval_ms: PositiveInt = 250
    max_batch_size: PositiveInt = 1000
    flush_time_budget_ms: PositiveInt | None = 10
    flush_timeout_ms: PositiveInt = 5_000
    max_queue_size: PositiveInt = 10_000
    on_error: ErrorPolicy = "buffer_until_full_then_fail"
    stop_timeout_ms: PositiveInt = 5_000
    strict_stop: bool = False
    propagate_errors_to_bus: bool = False

