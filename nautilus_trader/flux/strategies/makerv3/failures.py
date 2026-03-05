"""
Handle MakerV3 quote refresh failures and circuit-breaker behavior.
"""

from __future__ import annotations

from collections.abc import Callable
from contextlib import suppress
from decimal import Decimal
from typing import TYPE_CHECKING

from nautilus_trader.flux.strategies.makerv3.constants import (
    ALERT_COOLDOWN_QUOTE_FAIL_CIRCUIT_BREAKER_MS,
)
from nautilus_trader.flux.strategies.makerv3.constants import ALERT_KEY_QUOTE_FAIL_CIRCUIT_BREAKER


if TYPE_CHECKING:
    from nautilus_trader.flux.strategies.makerv3.strategy import MakerV3Strategy


def handle_quote_failure(
    strategy: MakerV3Strategy,
    *,
    now_ns: int,
    exc: Exception,
    context: str,
) -> None:
    """
    Record a quote-cycle failure and stop after circuit-breaker thresholds are exceeded.
    """
    if not hasattr(strategy, "_quote_failure_circuit_open"):
        strategy._quote_failure_circuit_open = False
    if not hasattr(strategy, "_quote_failures_ns"):
        strategy._quote_failures_ns = []

    def _safe(effect: Callable[[], None]) -> None:
        with suppress(Exception):
            effect()

    count_threshold = max(0, int(strategy._runtime_int("quote_fail_critical_after_count")))
    window_seconds = max(Decimal(0), strategy._runtime_decimal("quote_fail_critical_after_s"))
    window_ns = int(window_seconds * Decimal(1_000_000_000))
    strategy._quote_failures_ns.append(now_ns)
    if window_ns > 0:
        cutoff_ns = now_ns - window_ns
        strategy._quote_failures_ns = [
            ts_ns for ts_ns in strategy._quote_failures_ns if ts_ns >= cutoff_ns
        ]
    elif count_threshold > 0:
        strategy._quote_failures_ns = strategy._quote_failures_ns[-count_threshold:]

    failure_count = len(strategy._quote_failures_ns)

    def _emit_quote_refresh_failed() -> None:
        strategy._publish_event(
            "quote_refresh_failed",
            context=context,
            failure_count=failure_count,
            threshold=count_threshold,
            error_type=type(exc).__name__,
            error_message=str(exc),
        )

    _safe(
        _emit_quote_refresh_failed,
    )
    _safe(
        lambda: strategy.log.error(
            f"Quote refresh failure strategy_id={strategy._external_strategy_id} context={context} "
            f"count={failure_count} threshold={count_threshold} err={type(exc).__name__}: {exc}",
        ),
    )
    strategy._last_requote_ns = now_ns
    if count_threshold <= 0 or failure_count < count_threshold:
        return

    strategy._quote_failure_circuit_open = True
    try:
        _safe(lambda: strategy._cancel_managed_quotes("quote_fail_circuit_breaker", force=True))
        _safe(lambda: strategy._publish_state("blocked_quote_failures"))

        def _emit_quote_fail_circuit_breaker_alert() -> None:
            strategy._publish_actionable_alert(
                alert_key=ALERT_KEY_QUOTE_FAIL_CIRCUIT_BREAKER,
                message=(
                    "quote_fail_circuit_breaker triggered "
                    f"count={failure_count} threshold={count_threshold} window_s={window_seconds}"
                ),
                level="error",
                reason_code=ALERT_KEY_QUOTE_FAIL_CIRCUIT_BREAKER,
                cooldown_ms=ALERT_COOLDOWN_QUOTE_FAIL_CIRCUIT_BREAKER_MS,
                transition="circuit_breaker_closed->open",
                now_ns=now_ns,
            )

        _safe(
            _emit_quote_fail_circuit_breaker_alert,
        )
        _safe(
            lambda: strategy._publish_event(
                "quote_fail_circuit_breaker",
                failure_count=failure_count,
                threshold=count_threshold,
                window_s=str(window_seconds),
            ),
        )
        _safe(
            lambda: strategy.log.error(
                f"Quote failure circuit breaker triggered strategy_id={strategy._external_strategy_id}",
            ),
        )
    finally:
        _safe(strategy.stop)


__all__ = ["handle_quote_failure"]
