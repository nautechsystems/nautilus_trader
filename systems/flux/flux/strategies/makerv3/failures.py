"""
Handle MakerV3 quote refresh failures and circuit-breaker behavior.
"""

from __future__ import annotations

from collections.abc import Callable
from contextlib import suppress
from decimal import Decimal
import re
from typing import TYPE_CHECKING

from flux.strategies.makerv3.constants import (
    ALERT_COOLDOWN_VENUE_PROTECTION_CIRCUIT_BREAKER_MS,
    ALERT_COOLDOWN_QUOTE_FAIL_CIRCUIT_BREAKER_MS,
)
from flux.strategies.makerv3.constants import (
    ALERT_KEY_VENUE_PROTECTION_CIRCUIT_BREAKER,
)
from flux.strategies.makerv3.constants import ALERT_KEY_QUOTE_FAIL_CIRCUIT_BREAKER
from flux.strategies.shared.venue_protection import extract_hyperliquid_request_quota
from flux.strategies.shared.venue_protection import is_venue_protection_reason
from flux.strategies.shared.venue_protection import normalize_reason_text


if TYPE_CHECKING:
    from flux.strategies.makerv3.strategy import MakerV3Strategy


def is_terminal_order_denial_reason(reason: object) -> bool:
    """
    Return True when `reason` indicates a persistent incompatibility which
    should stop further quote attempts until manually re-enabled.
    """
    normalized = normalize_reason_text(reason)
    if not normalized:
        return False
    return normalized.startswith("unsupported_account_mode")


_EXPLICIT_EXCHANGE_CODE_RE = re.compile(
    r"(?<![A-Za-z0-9_])exchange_code(?![A-Za-z0-9_])\s*(?:=|:)\s*['\"]?(?P<code>[A-Za-z0-9_-]+)",
    re.IGNORECASE,
)
_EXCHANGE_ERROR_CODE_RE = re.compile(
    r"(?<![A-Za-z0-9_])['\"]?code['\"]?(?![A-Za-z0-9_])\s*(?:=|:)\s*['\"]?(?P<code>[A-Za-z0-9_-]+)",
    re.IGNORECASE,
)


def extract_exchange_error_code(reason: object) -> str | None:
    normalized = normalize_reason_text(reason)
    if not normalized:
        return None
    match = _EXPLICIT_EXCHANGE_CODE_RE.search(normalized)
    if match is None:
        match = _EXCHANGE_ERROR_CODE_RE.search(normalized)
    if match is None:
        return None
    return match.group("code")


def is_spot_borrow_block_reason(reason: object) -> bool:
    normalized = normalize_reason_text(reason)
    if not normalized:
        return False
    return (
        "code=51006" in normalized
        or "maximum borrowable amount" in normalized
        or "maximum borrowable" in normalized
    )

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
        _safe(lambda: strategy.request_immediate_stop(True))
        _safe(strategy.stop_immediately)


def handle_venue_protection(
    strategy: MakerV3Strategy,
    *,
    now_ns: int,
    reason: object,
    source_event: str,
    client_order_id: object | None = None,
) -> None:
    """
    Hard-stop the strategy when the venue signals order-limit or rate-limit protection.
    """
    if not hasattr(strategy, "_venue_protection_circuit_open"):
        strategy._venue_protection_circuit_open = False
    if strategy._venue_protection_circuit_open:
        return

    strategy._venue_protection_circuit_open = True
    normalized_reason = normalize_reason_text(reason) or "unknown"
    raw_reason = str(reason or "")
    client_order_id_text = str(client_order_id or "")
    quota_fields = extract_hyperliquid_request_quota(reason)

    def _safe(effect: Callable[[], None]) -> None:
        with suppress(Exception):
            effect()

    _safe(lambda: strategy._set_managed_only_stop_safety(True))
    try:
        _safe(
            lambda: strategy._cancel_managed_quotes(
                "venue_protection_circuit_breaker",
                force=True,
                allow_instrument_cancel=False,
            ),
        )
        _safe(lambda: strategy._publish_state("blocked_venue_protection"))
        _safe(
            lambda: strategy._publish_actionable_alert(
                alert_key=ALERT_KEY_VENUE_PROTECTION_CIRCUIT_BREAKER,
                message=(
                    "venue_protection_circuit_breaker triggered "
                    f"source_event={source_event} reason={normalized_reason!r}"
                ),
                level="error",
                reason_code=ALERT_KEY_VENUE_PROTECTION_CIRCUIT_BREAKER,
                cooldown_ms=ALERT_COOLDOWN_VENUE_PROTECTION_CIRCUIT_BREAKER_MS,
                transition=f"{source_event}:{normalized_reason}",
                now_ns=now_ns,
                source_event=source_event,
                raw_reason=raw_reason,
                client_order_id=client_order_id_text,
                **quota_fields,
            ),
        )
        _safe(
            lambda: strategy._publish_event(
                "venue_protection_circuit_breaker",
                source_event=source_event,
                reason=normalized_reason,
                raw_reason=raw_reason,
                client_order_id=client_order_id_text,
                **quota_fields,
            ),
        )
        _safe(
            lambda: strategy.log.error(
                "Venue protection circuit breaker triggered "
                f"strategy_id={strategy._external_strategy_id} "
                f"source_event={source_event} client_order_id={client_order_id_text or 'unknown'} "
                f"reason={raw_reason}"
                + (
                    " "
                    f"quota_requests_used={quota_fields['quota_requests_used']} "
                    f"quota_requests_cap={quota_fields['quota_requests_cap']} "
                    f"quota_cumulative_volume_traded={quota_fields['quota_cumulative_volume_traded']}"
                    if quota_fields
                    else ""
                ),
            ),
        )
    finally:
        _safe(strategy.stop_immediately)


__all__ = [
    "extract_exchange_error_code",
    "handle_quote_failure",
    "handle_venue_protection",
    "is_spot_borrow_block_reason",
    "is_venue_protection_reason",
    "normalize_reason_text",
]
