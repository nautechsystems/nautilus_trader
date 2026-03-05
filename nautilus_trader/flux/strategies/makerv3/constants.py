"""
Define constants for the canonical MakerV3 strategy surface.
"""

from __future__ import annotations


TOPIC_STATE = "flux.makerv3.state"
TOPIC_EVENT = "flux.makerv3.event"
TOPIC_TRADE = "flux.makerv3.trade"
TOPIC_ALERT = "flux.makerv3.alert"
TOPIC_MARKET_BBO = "flux.makerv3.market_bbo"
TOPIC_FV = "flux.makerv3.fv"
TOPIC_BALANCES = "flux.makerv3.balances"

BLOCKED_STATE_PREFIX = "blocked_"

QUOTE_CYCLE_EVENT_NAME = "quote_cycle"
QUOTE_CYCLE_EVENT_SKIPPED = "skipped"
QUOTE_CYCLE_EVENT_BLOCKED = "blocked"
QUOTE_CYCLE_EVENT_COMPLETED = "completed"

REASON_SKIPPED_BOT_OFF = "skip_bot_off"
REASON_SKIPPED_REQUOTE_THROTTLED = "skip_requote_throttled"
REASON_SKIPPED_QUOTE_FAIL_CIRCUIT_OPEN = "skip_quote_fail_circuit_open"

REASON_BLOCKED_MAKER_BOOK_UNAVAILABLE = "blocked_maker_book_unavailable"
REASON_BLOCKED_MAKER_MD_STALE = "blocked_maker_md_stale"
REASON_BLOCKED_REFERENCE_MD_STALE = "blocked_reference_md_stale"

REASON_COMPLETED_NO_TARGETS = "completed_no_targets"
REASON_COMPLETED_NO_ACTIONS = "completed_no_actions"
REASON_COMPLETED_REBALANCED = "completed_rebalanced"

ALERT_KEY_MARKET_DATA_BLOCKED = "market_data_blocked"
ALERT_KEY_RUNTIME_PARAMS_FAILURE = "runtime_params_failure"
ALERT_KEY_QUOTE_FAIL_CIRCUIT_BREAKER = "quote_fail_circuit_breaker"

ALERT_COOLDOWN_BLOCKED_MS = 30_000
ALERT_COOLDOWN_RUNTIME_PARAMS_FAILURE_MS = 60_000
ALERT_COOLDOWN_QUOTE_FAIL_CIRCUIT_BREAKER_MS = 60_000
