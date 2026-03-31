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
TOPIC_ORDER_INTENT = "flux.makerv3.order_intent"

BLOCKED_STATE_PREFIX = "blocked_"

QUOTE_CYCLE_EVENT_NAME = "quote_cycle"
QUOTE_CYCLE_EVENT_SKIPPED = "skipped"
QUOTE_CYCLE_EVENT_BLOCKED = "blocked"
QUOTE_CYCLE_EVENT_COMPLETED = "completed"

REASON_SKIPPED_BOT_OFF = "skip_bot_off"
REASON_SKIPPED_REQUOTE_THROTTLED = "skip_requote_throttled"
REASON_SKIPPED_QUOTE_FAIL_CIRCUIT_OPEN = "skip_quote_fail_circuit_open"
REASON_SKIPPED_PENDING_CANCELS = "skip_pending_cancels"
REASON_SKIPPED_CANCEL_REJECT_COOLDOWN = "skip_cancel_reject_cooldown"

REASON_BLOCKED_MAKER_BOOK_UNAVAILABLE = "blocked_maker_book_unavailable"
REASON_BLOCKED_MAKER_MD_STALE = "blocked_maker_md_stale"
REASON_BLOCKED_REFERENCE_MD_STALE = "blocked_reference_md_stale"
REASON_BLOCKED_PORTFOLIO_INVENTORY_UNAVAILABLE = "blocked_portfolio_inventory_unavailable"
REASON_BLOCKED_PRIVATE_PATH_UNAVAILABLE = "blocked_private_path_unavailable"
REASON_BLOCKED_STARTUP_CLEANUP = "blocked_startup_cleanup"
REASON_BLOCKED_PENDING_CANCEL = "pending_cancel_stuck"

REASON_COMPLETED_NO_TARGETS = "completed_no_targets"
REASON_COMPLETED_NO_ACTIONS = "completed_no_actions"
REASON_COMPLETED_REBALANCED = "completed_rebalanced"

REASON_PLACE_MISSING_LEVEL = "place_missing_level"
REASON_PLACE_FRONT_IMPROVE = "place_front_improve"
REASON_PLACE_BACK_BACKFILL = "place_back_backfill"
REASON_PLACE_MISSING_HOLE_REPAIR = "place_missing_hole_repair"

REASON_CANCEL_BOT_OFF = "cancel_bot_off"
REASON_CANCEL_BOT_OFF_FLIP = "cancel_bot_off_flip"
REASON_CANCEL_MAKER_BOOK_UNAVAILABLE = "cancel_maker_book_unavailable"
REASON_CANCEL_MAKER_MD_STALE = "cancel_maker_md_stale"
REASON_CANCEL_REFERENCE_MD_STALE = "cancel_reference_md_stale"
REASON_CANCEL_NO_TARGETS = "cancel_no_targets"
REASON_CANCEL_PRIVATE_PATH_UNAVAILABLE = "cancel_private_path_unavailable"
REASON_CANCEL_ON_STOP = "cancel_on_stop"
REASON_CANCEL_QUOTE_FAIL_CIRCUIT_BREAKER = "cancel_quote_fail_circuit_breaker"
REASON_CANCEL_EXCESS_LEVEL = "cancel_excess_level"
REASON_CANCEL_TOO_AGGRESSIVE = "cancel_too_aggressive"
REASON_CANCEL_BACK_EXCESS = "cancel_back_excess"
REASON_CANCEL_FRONT_VIOLATION = "cancel_front_violation"
REASON_CANCEL_STALE_ORDER = "cancel_stale_order"
REASON_CANCEL_FREE_SLOT_FOR_MISSING_LEVEL = "cancel_free_slot_for_missing_level"

ALERT_KEY_MARKET_DATA_BLOCKED = "market_data_blocked"
ALERT_KEY_PORTFOLIO_INVENTORY_BLOCKED = "portfolio_inventory_blocked"
ALERT_KEY_PRIVATE_PATH_BLOCKED = "private_path_blocked"
ALERT_KEY_QUOTE_LIVENESS_BLOCKED = "quote_liveness_blocked"
ALERT_KEY_RUNTIME_PARAMS_FAILURE = "runtime_params_failure"
ALERT_KEY_QUOTE_FAIL_CIRCUIT_BREAKER = "quote_fail_circuit_breaker"
ALERT_KEY_ORDER_REJECTED_BURST = "order_rejected_burst"
ALERT_KEY_TERMINAL_ORDER_DENIED = "terminal_order_denied"
ALERT_KEY_VENUE_PROTECTION_CIRCUIT_BREAKER = "venue_protection_circuit_breaker"

ALERT_COOLDOWN_BLOCKED_MS = 30_000
ALERT_COOLDOWN_RUNTIME_PARAMS_FAILURE_MS = 60_000
ALERT_COOLDOWN_QUOTE_FAIL_CIRCUIT_BREAKER_MS = 60_000
ALERT_COOLDOWN_ORDER_REJECTED_BURST_MS = 60_000
ALERT_COOLDOWN_TERMINAL_ORDER_DENIED_MS = 60_000
ALERT_COOLDOWN_VENUE_PROTECTION_CIRCUIT_BREAKER_MS = 60_000
