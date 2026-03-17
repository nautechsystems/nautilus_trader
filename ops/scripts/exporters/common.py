from __future__ import annotations

import argparse


MIN_POLL_INTERVAL_SECONDS = 0.5


def positive_float_arg(value: str) -> float:
    try:
        parsed = float(value)
    except (TypeError, ValueError) as exc:
        raise argparse.ArgumentTypeError("must be a float") from exc
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be > 0")
    return parsed


def poll_interval_seconds_arg(value: str) -> float:
    parsed = positive_float_arg(value)
    if parsed < MIN_POLL_INTERVAL_SECONDS:
        raise argparse.ArgumentTypeError(f"must be >= {MIN_POLL_INTERVAL_SECONDS}")
    return parsed
