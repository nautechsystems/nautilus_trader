from __future__ import annotations

import re


_VENUE_PROTECTION_REASON_PHRASES: tuple[str, ...] = (
    "number of active orders great than limit",
    "number of active orders greater than limit",
    "active order limit",
    "cumulative requests sent",
    "too many visits",
    "api rate limit",
    "too many requests",
)

_HYPERLIQUID_CUMULATIVE_REQUEST_RE = re.compile(
    r"cumulative requests sent \((?P<used>\d+)\s*>\s*(?P<cap>\d+)\) "
    r"for cumulative volume traded \$?(?P<cum_vlm>[0-9]+(?:\.[0-9]+)?)",
    re.IGNORECASE,
)


def normalize_reason_text(reason: object) -> str:
    normalized = f" {str(reason or '').strip().lower()} "
    return " ".join(normalized.split())


def _normalized_reason_tokens(normalized_reason: str) -> set[str]:
    return {
        token
        for token in re.split(r"[^a-z0-9]+", normalized_reason)
        if token
    }


def is_venue_protection_reason(reason: object) -> bool:
    normalized = normalize_reason_text(reason)
    if not normalized:
        return False
    if any(fragment in normalized for fragment in _VENUE_PROTECTION_REASON_PHRASES):
        return True

    tokens = _normalized_reason_tokens(normalized)
    if normalized == "429":
        return True
    if "429" in tokens and tokens.intersection(
        {"api", "code", "http", "limit", "rate", "request", "requests", "status"},
    ):
        return True
    return False


def extract_hyperliquid_request_quota(reason: object) -> dict[str, object]:
    normalized = normalize_reason_text(reason)
    if not normalized:
        return {}

    match = _HYPERLIQUID_CUMULATIVE_REQUEST_RE.search(normalized)
    if match is None:
        return {}

    return {
        "quota_requests_used": int(match.group("used")),
        "quota_requests_cap": int(match.group("cap")),
        "quota_cumulative_volume_traded": match.group("cum_vlm"),
    }


__all__ = [
    "extract_hyperliquid_request_quota",
    "is_venue_protection_reason",
    "normalize_reason_text",
]
