#!/usr/bin/env python3
"""Run a minimal PMXT event replay and simple strategy backtest.

The first target is the curated Shanghai temperature event:

    highest-temperature-in-shanghai-on-june-9-2026 / 25C / YES

This is intentionally a small research harness, not a production engine:
- reads the event_index + gamma raw metadata;
- reads the curated PMXT orderbook parquet;
- rebuilds one selected token's L2 book from book + price_change events;
- checks snapshot-to-snapshot replay alignment and trade-vs-book sanity;
- simulates one simple strategy/fill model;
- writes small JSON/CSV outputs for inspection.
"""

from __future__ import annotations

import argparse
import csv
import json
import re
from dataclasses import dataclass
from datetime import timezone
from decimal import Decimal
from pathlib import Path
from typing import Any

import pandas as pd
import pyarrow.parquet as pq


ROOT = Path(__file__).resolve().parents[3]
DEFAULT_EVENT_SLUG = "highest-temperature-in-shanghai-on-june-9-2026"
DEFAULT_MARKET_LABEL = "25\u00b0C"
DEFAULT_TOKEN_SIDE = "YES"
LOCAL_CURATED_ROOT = ROOT / "data" / "curated" / "polymarket" / "events"
POLYREAPER_CURATED_ROOT = Path("C:/Projects/PolyReaper/data/curated/polymarket/events")
DEFAULT_CURATED_ROOT = LOCAL_CURATED_ROOT if LOCAL_CURATED_ROOT.exists() else POLYREAPER_CURATED_ROOT
DEFAULT_OUT_DIR = ROOT / "research" / "2026-06-24-polymarket-shanghai-event-backtest" / "data"
BBO_MISMATCH_WARN_THRESHOLD = 0.01
SNAPSHOT_BBO_MISMATCH_WARN_THRESHOLD = 0.01
TRADE_OFF_BOOK_WARN_THRESHOLD = 0.20
PRICE_EPSILON = 1e-9


@dataclass(frozen=True)
class SelectedMarket:
    event_slug: str
    event_title: str
    market_label: str
    market_id: str
    condition_id: str
    question: str
    token_side: str
    token_id: str
    settlement_value: float | None


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--event-slug", default=DEFAULT_EVENT_SLUG)
    parser.add_argument("--market-label", default=DEFAULT_MARKET_LABEL)
    parser.add_argument("--token-side", choices=["YES", "NO"], default=DEFAULT_TOKEN_SIDE)
    parser.add_argument("--curated-root", type=Path, default=DEFAULT_CURATED_ROOT)
    parser.add_argument("--out-dir", type=Path, default=DEFAULT_OUT_DIR)
    parser.add_argument(
        "--strategy",
        choices=["maker_bbo", "buy_hold_first_ask", "momentum_taker", "contrarian_taker"],
        default="maker_bbo",
        help=(
            "maker_bbo quotes both sides at current BBO and fills from trade prints; "
            "buy_hold_first_ask buys once at the first ask; "
            "momentum_taker/contrarian_taker trade at BBO on sampled mid-price signals."
        ),
    )
    parser.add_argument("--quote-size", type=float, default=10.0)
    parser.add_argument("--max-inventory", type=float, default=100.0)
    parser.add_argument(
        "--decision-frequency",
        default="5min",
        help="Decision cadence for taker signal strategies.",
    )
    parser.add_argument(
        "--signal-threshold",
        type=float,
        default=0.03,
        help="Absolute mid-price change threshold for momentum/contrarian taker signals.",
    )
    parser.add_argument(
        "--fill-model",
        choices=["conservative", "touch"],
        default="conservative",
        help=(
            "conservative fills only when taker side is compatible with our quote; "
            "touch also fills whenever trade price crosses our quote."
        ),
    )
    parser.add_argument(
        "--timeseries-frequency",
        default="5min",
        help="Pandas frequency for output BBO/equity time series.",
    )
    parser.add_argument(
        "--replay-order",
        choices=["source_time", "received_time"],
        default="source_time",
        help=(
            "Row order used for replay. source_time preserves the original "
            "research harness behavior; received_time replays PMXT rows in "
            "collector receive-time order for no-receive-inversion samples."
        ),
    )
    return parser.parse_args()


def repo_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


def display_input_path(path: Path, curated_root: Path) -> str:
    resolved = path.resolve()
    try:
        return resolved.relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        pass
    try:
        return "EXTERNAL_CURATED_ROOT/" + resolved.relative_to(curated_root.resolve()).as_posix()
    except ValueError:
        return "EXTERNAL_INPUT/" + path.name


def slugify(value: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]+", "_", value).strip("._") or "unknown"


def slugify_number(value: float) -> str:
    return slugify(f"{value:g}".replace("-", "neg").replace(".", "p"))


def child_path(parent: Path, filename: str) -> Path:
    parent_resolved = parent.resolve()
    path = (parent / filename).resolve()
    if parent_resolved != path.parent and parent_resolved not in path.parents:
        raise SystemExit(f"refusing to write outside output dir: {path}")
    return path


def read_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8"))


def parse_json_field(value: Any) -> Any:
    if isinstance(value, str):
        try:
            return json.loads(value)
        except json.JSONDecodeError:
            return value
    return value


def as_float(value: Any) -> float | None:
    if value is None or pd.isna(value):
        return None
    if isinstance(value, Decimal):
        return float(value)
    return float(value)


def as_decimal(value: Any) -> Decimal | None:
    if value is None or pd.isna(value):
        return None
    if isinstance(value, Decimal):
        return value
    return Decimal(str(value))


def decimal_to_json(value: Decimal | None) -> str | None:
    if value is None:
        return None
    return format(value.normalize(), "f")


def as_utc_iso(value: Any) -> str | None:
    if value is None or pd.isna(value):
        return None
    ts = pd.Timestamp(value)
    if ts.tzinfo is None:
        ts = ts.tz_localize(timezone.utc)
    return ts.tz_convert(timezone.utc).isoformat().replace("+00:00", "Z")


def load_selection(event_dir: Path, market_label: str, token_side: str) -> SelectedMarket:
    event_index = read_json(event_dir / "event_index.json")
    gamma_payload = read_json(event_dir / "gamma_event.raw.json")
    gamma_event = gamma_payload[0] if isinstance(gamma_payload, list) else gamma_payload
    markets = event_index["markets"]
    matches = [m for m in markets if m["label"] == market_label]
    if not matches:
        labels = ", ".join(m["label"] for m in markets)
        raise SystemExit(f"market label {market_label!r} not found. Available: {labels}")
    market = matches[0]
    token_id = market["yesToken"] if token_side == "YES" else market["noToken"]

    settlement_value: float | None = None
    for gm in gamma_event.get("markets", []):
        if str(gm.get("id")) != str(market["marketId"]):
            continue
        outcomes = parse_json_field(gm.get("outcomes")) or []
        prices = parse_json_field(gm.get("outcomePrices")) or []
        for outcome, price in zip(outcomes, prices, strict=False):
            if str(outcome).upper() == token_side:
                settlement_value = float(price)
                break
        break

    return SelectedMarket(
        event_slug=str(event_index["eventSlug"]),
        event_title=str(event_index["title"]),
        market_label=str(market["label"]),
        market_id=str(market["marketId"]),
        condition_id=str(market["conditionId"]),
        question=str(market["question"]),
        token_side=token_side,
        token_id=str(token_id),
        settlement_value=settlement_value,
    )


def parse_levels(raw: Any) -> dict[float, float]:
    if raw is None or pd.isna(raw):
        return {}
    parsed = json.loads(str(raw))
    return {float(price): float(size) for price, size in parsed if float(size) > 0}


def best_bid_ask(bids: dict[float, float], asks: dict[float, float]) -> tuple[float | None, float | None]:
    best_bid = max(bids) if bids else None
    best_ask = min(asks) if asks else None
    return best_bid, best_ask


def update_level(levels: dict[float, float], price: float, size: float) -> None:
    if size <= 0:
        levels.pop(price, None)
    else:
        levels[price] = size


def books_equal(left: dict[float, float], right: dict[float, float]) -> bool:
    if left.keys() != right.keys():
        return False
    return all(abs(left[price] - right[price]) <= PRICE_EPSILON for price in left)


def bbo_equal(left: tuple[float | None, float | None], right: tuple[float | None, float | None]) -> bool:
    return all(
        (a is None and b is None) or (a is not None and b is not None and abs(a - b) <= PRICE_EPSILON)
        for a, b in zip(left, right, strict=True)
    )


def normalize_pmxt_bbo(best_bid: float | None, best_ask: float | None) -> tuple[float | None, float | None]:
    """Normalize PMXT diagnostic BBO sentinels without changing the replayed book."""
    if best_bid is not None and best_bid <= PRICE_EPSILON:
        best_bid = None
    if best_ask is not None and best_ask >= 1.0 - PRICE_EPSILON:
        best_ask = None
    return best_bid, best_ask


def mark_price_for_inventory(
    best_bid: float | None,
    best_ask: float | None,
    inventory: float,
) -> float | None:
    """Return a conservative display mark for inventory.

    Mid is preferred when both sides are present. Near resolution Polymarket
    books can become one-sided (for example bid=0.999 with no ask). Marking
    those rows at zero creates false tail cliffs in report equity curves, so
    fall back to the available liquidation side when possible.
    """
    if best_bid is not None and best_ask is not None:
        return (best_bid + best_ask) / 2
    if inventory > 0 and best_bid is not None:
        return best_bid
    if inventory < 0 and best_ask is not None:
        return best_ask
    if best_bid is not None:
        return best_bid
    if best_ask is not None:
        return best_ask
    return None


def price_on_tick(price: float, tick_size: Decimal | None) -> bool:
    if tick_size is None or tick_size <= 0:
        return True
    price_decimal = Decimal(str(price))
    return price_decimal % tick_size == 0


def diagnostic_bbo_equal(
    local_bbo: tuple[float | None, float | None],
    pmxt_bbo: tuple[float | None, float | None],
) -> bool:
    return bbo_equal(local_bbo, normalize_pmxt_bbo(*pmxt_bbo))


def update_book_from_price_change(
    bids: dict[float, float],
    asks: dict[float, float],
    *,
    price: float,
    size: float,
    side: str,
) -> None:
    if side == "BUY":
        update_level(bids, price, size)
    elif side == "SELL":
        update_level(asks, price, size)


def append_fill(
    fills: list[dict[str, Any]],
    *,
    timestamp: Any,
    strategy: str,
    reason: str,
    fill_side: str,
    quantity: float,
    price: float,
    inventory_after: float,
    cash_after: float,
    trade_price: float | None = None,
    trade_size: float | None = None,
    trade_side: str | None = None,
    transaction_hash: str | None = None,
) -> None:
    fills.append(
        {
            "timestamp": as_utc_iso(timestamp),
            "strategy": strategy,
            "reason": reason,
            "fill_side": fill_side,
            "quantity": quantity,
            "price": price,
            "trade_price": trade_price,
            "trade_size": trade_size,
            "trade_side": trade_side,
            "inventory_after": inventory_after,
            "cash_after": cash_after,
            "transaction_hash": transaction_hash,
        }
    )


def fill_decision(
    *,
    trade_side: str | None,
    trade_price: float,
    trade_size: float,
    quote_bid: float | None,
    quote_ask: float | None,
    quote_size: float,
    inventory: float,
    max_inventory: float,
    fill_model: str,
) -> tuple[str | None, float, float]:
    """Return (fill_side, fill_qty, fill_price).

    fill_side is from our perspective: BUY means we bought token at bid,
    SELL means we sold token at ask.
    """
    side = (trade_side or "").upper()
    can_buy = quote_bid is not None and inventory < max_inventory
    can_sell = quote_ask is not None and inventory > -max_inventory

    if fill_model == "conservative":
        if side == "SELL" and can_buy and trade_price <= quote_bid:
            return "BUY", min(quote_size, trade_size, max_inventory - inventory), quote_bid
        if side == "BUY" and can_sell and trade_price >= quote_ask:
            return "SELL", min(quote_size, trade_size, inventory + max_inventory), quote_ask
        return None, 0.0, 0.0

    if can_buy and trade_price <= quote_bid:
        return "BUY", min(quote_size, trade_size, max_inventory - inventory), quote_bid
    if can_sell and trade_price >= quote_ask:
        return "SELL", min(quote_size, trade_size, inventory + max_inventory), quote_ask
    return None, 0.0, 0.0


def run_backtest(args: argparse.Namespace) -> dict[str, Any]:
    if args.quote_size <= 0:
        raise SystemExit("--quote-size must be > 0")
    if args.max_inventory <= 0:
        raise SystemExit("--max-inventory must be > 0")
    if args.signal_threshold < 0:
        raise SystemExit("--signal-threshold must be >= 0")

    event_dir = args.curated_root / args.event_slug
    if not event_dir.exists():
        raise SystemExit(f"event dir not found: {event_dir}")
    selection = load_selection(event_dir, args.market_label, args.token_side)
    parquet_path = event_dir / "orderbook.parquet"

    columns = [
        "timestamp_received",
        "timestamp",
        "market",
        "event_type",
        "asset_id",
        "bids",
        "asks",
        "price",
        "size",
        "side",
        "best_bid",
        "best_ask",
        "transaction_hash",
        "fee_rate_bps",
        "old_tick_size",
        "new_tick_size",
    ]
    table = pq.read_table(
        parquet_path,
        columns=columns,
        filters=[("asset_id", "=", selection.token_id)],
    )
    df = table.to_pandas()
    if df.empty:
        raise SystemExit(f"no parquet rows found for token_id={selection.token_id}")
    df["_row"] = range(len(df))
    if args.replay_order == "source_time":
        # `timestamp` is the exchange/event time and drove the original
        # Shanghai replay. `timestamp_received` remains a stable tie-breaker
        # and part of the PMXT price_change batch key.
        sort_columns = ["timestamp", "timestamp_received", "_row"]
    else:
        # For samples where PMXT `timestamp_received` has no per-asset receive
        # inversion, this keeps replay aligned to collector delivery order
        # instead of reordering rows by exchange/source timestamp.
        sort_columns = ["timestamp_received", "timestamp", "_row"]
    df = df.sort_values(sort_columns, kind="mergesort")

    bids: dict[float, float] = {}
    asks: dict[float, float] = {}
    initialized = False
    quote_bid: float | None = None
    quote_ask: float | None = None
    final_bid: float | None = None
    final_ask: float | None = None
    current_tick_size = Decimal("0.01")
    last_decision_ts: pd.Timestamp | None = None
    last_decision_mid: float | None = None
    decision_frequency = pd.Timedelta(args.decision_frequency)

    cash = 0.0
    inventory = 0.0
    fills: list[dict[str, Any]] = []
    bbo_samples: list[dict[str, Any]] = []
    replay_events = 0
    skipped_before_book = 0
    book_events = 0
    price_change_events = 0
    trade_events = 0
    tick_events = 0
    tick_size_changes_applied = 0
    tick_size_old_mismatches = 0
    tick_size_old_mismatch_examples: list[dict[str, Any]] = []
    fill_tick_price_checks = 0
    fill_tick_price_violations = 0
    fill_tick_price_violation_examples: list[dict[str, Any]] = []
    snapshot_pairs = 0
    snapshot_bbo_mismatches = 0
    snapshot_full_book_mismatches = 0
    crossed_or_locked_books = 0
    negative_spread_samples = 0
    trades_checked = 0
    trades_without_book = 0
    trades_inside_or_at_book = 0
    trades_side_touch = 0
    trades_off_book = 0

    price_change_batch_compared = 0
    price_change_batch_mismatches = 0
    rows = list(df.itertuples(index=False))

    def price_change_batch_key(row: Any) -> tuple[Any, Any, Any, Any, Any]:
        return (row.timestamp_received, row.timestamp, row.market, row.asset_id, row.event_type)

    def message_key(row: Any) -> tuple[Any, Any, Any, Any]:
        return (row.timestamp_received, row.timestamp, row.market, row.asset_id)

    price_change_message_keys = {
        message_key(row) for row in rows if row.event_type == "price_change"
    }
    completed_price_change_batch_keys: set[tuple[Any, Any, Any, Any, Any]] = set()
    split_price_change_batch_keys: set[tuple[Any, Any, Any, Any, Any]] = set()
    last_price_change_key: tuple[Any, Any, Any, Any, Any] | None = None
    in_price_change_run = False
    for row in rows:
        if row.event_type == "price_change":
            key = price_change_batch_key(row)
            if not in_price_change_run or key != last_price_change_key:
                if in_price_change_run and last_price_change_key is not None:
                    completed_price_change_batch_keys.add(last_price_change_key)
                if key in completed_price_change_batch_keys:
                    split_price_change_batch_keys.add(key)
                last_price_change_key = key
            in_price_change_run = True
        else:
            if in_price_change_run and last_price_change_key is not None:
                completed_price_change_batch_keys.add(last_price_change_key)
            in_price_change_run = False
            last_price_change_key = None
    if in_price_change_run and last_price_change_key is not None:
        completed_price_change_batch_keys.add(last_price_change_key)
    split_price_change_batch_key_examples = [
        {
            "timestamp_received": as_utc_iso(key[0]),
            "timestamp": as_utc_iso(key[1]),
            "market": str(key[2]),
            "asset_id": str(key[3]),
            "event_type": str(key[4]),
        }
        for key in list(split_price_change_batch_keys)[:5]
    ]
    same_message_snapshot_pairs = 0
    raw_snapshot_pairs = 0
    raw_snapshot_bbo_mismatches = 0
    raw_snapshot_full_book_mismatches = 0

    def update_replayed_bbo() -> tuple[float | None, float | None]:
        nonlocal final_bid, final_ask, quote_bid, quote_ask
        best_bid, best_ask = best_bid_ask(bids, asks)
        final_bid, final_ask = best_bid, best_ask
        if best_bid is not None and best_ask is not None and best_bid < best_ask:
            quote_bid, quote_ask = best_bid, best_ask
        else:
            quote_bid, quote_ask = None, None
        return best_bid, best_ask

    def record_fill_tick_price(price: float, ts: Any, context: str) -> None:
        nonlocal fill_tick_price_checks, fill_tick_price_violations
        fill_tick_price_checks += 1
        if price_on_tick(price, current_tick_size):
            return
        fill_tick_price_violations += 1
        if len(fill_tick_price_violation_examples) < 5:
            fill_tick_price_violation_examples.append(
                {
                    "timestamp": as_utc_iso(ts),
                    "context": context,
                    "price": price,
                    "tick_size": decimal_to_json(current_tick_size),
                }
            )

    def record_book_sanity(best_bid: float | None, best_ask: float | None) -> None:
        nonlocal crossed_or_locked_books, negative_spread_samples
        if best_bid is None or best_ask is None:
            return
        if best_bid >= best_ask:
            crossed_or_locked_books += 1
        if best_ask - best_bid < -PRICE_EPSILON:
            negative_spread_samples += 1

    def maybe_run_book_strategy(ts: Any, best_bid: float | None, best_ask: float | None) -> None:
        nonlocal inventory, cash, last_decision_ts, last_decision_mid
        mid = None if best_bid is None or best_ask is None else (best_bid + best_ask) / 2
        mark = mark_price_for_inventory(best_bid, best_ask, inventory)
        if (
            args.strategy == "buy_hold_first_ask"
            and mid is not None
            and best_ask is not None
            and not fills
            and inventory < float(args.max_inventory)
        ):
            fill_qty = min(float(args.quote_size), float(args.max_inventory) - inventory)
            inventory += fill_qty
            cash -= fill_qty * best_ask
            record_fill_tick_price(best_ask, ts, "buy_hold_first_ask")
            append_fill(
                fills,
                timestamp=ts,
                strategy=args.strategy,
                reason="first_valid_ask",
                fill_side="BUY",
                quantity=fill_qty,
                price=best_ask,
                inventory_after=inventory,
                cash_after=cash,
            )
        elif args.strategy in {"momentum_taker", "contrarian_taker"} and mid is not None:
            ts_value = pd.Timestamp(ts)
            should_decide = last_decision_ts is None or ts_value - last_decision_ts >= decision_frequency
            if should_decide:
                if last_decision_mid is not None:
                    delta = mid - last_decision_mid
                    signal: str | None = None
                    if delta >= float(args.signal_threshold):
                        signal = "BUY" if args.strategy == "momentum_taker" else "SELL"
                    elif delta <= -float(args.signal_threshold):
                        signal = "SELL" if args.strategy == "momentum_taker" else "BUY"
                    if signal == "BUY" and best_ask is not None and inventory < float(args.max_inventory):
                        fill_qty = min(float(args.quote_size), float(args.max_inventory) - inventory)
                        inventory += fill_qty
                        cash -= fill_qty * best_ask
                        record_fill_tick_price(best_ask, ts, args.strategy)
                        append_fill(
                            fills,
                            timestamp=ts,
                            strategy=args.strategy,
                            reason=f"mid_delta={delta:.6f}",
                            fill_side="BUY",
                            quantity=fill_qty,
                            price=best_ask,
                            inventory_after=inventory,
                            cash_after=cash,
                        )
                    elif signal == "SELL" and best_bid is not None and inventory > -float(args.max_inventory):
                        fill_qty = min(float(args.quote_size), inventory + float(args.max_inventory))
                        inventory -= fill_qty
                        cash += fill_qty * best_bid
                        record_fill_tick_price(best_bid, ts, args.strategy)
                        append_fill(
                            fills,
                            timestamp=ts,
                            strategy=args.strategy,
                            reason=f"mid_delta={delta:.6f}",
                            fill_side="SELL",
                            quantity=fill_qty,
                            price=best_bid,
                            inventory_after=inventory,
                            cash_after=cash,
                        )
                last_decision_ts = ts_value
                last_decision_mid = mid
        bbo_samples.append(
            {
                "timestamp": pd.Timestamp(ts),
                "best_bid": best_bid,
                "best_ask": best_ask,
                "mid": mid,
                "mark_price": mark,
                "tick_size": decimal_to_json(current_tick_size),
                "spread": None if best_bid is None or best_ask is None else best_ask - best_bid,
                "inventory": inventory,
                "cash": cash,
                "mtm_equity": cash + inventory * (mark or 0.0),
            }
        )

    i = 0
    while i < len(rows):
        row = rows[i]
        event_type = row.event_type
        ts = row.timestamp_received

        if event_type == "price_change":
            key = price_change_batch_key(row)
            j = i + 1
            while j < len(rows) and rows[j].event_type == "price_change" and price_change_batch_key(rows[j]) == key:
                j += 1
            batch = rows[i:j]
            price_change_events += len(batch)
            if not initialized:
                skipped_before_book += len(batch)
                i = j
                continue
            for change in batch:
                price = as_float(change.price)
                size = as_float(change.size)
                if price is None or size is None:
                    continue
                update_book_from_price_change(
                    bids,
                    asks,
                    price=price,
                    size=size,
                    side=str(change.side).upper(),
                )
            replay_events += len(batch)
            best_bid, best_ask = update_replayed_bbo()
            pmxt_bid = as_float(getattr(batch[-1], "best_bid", None))
            pmxt_ask = as_float(getattr(batch[-1], "best_ask", None))
            if pmxt_bid is not None or pmxt_ask is not None:
                price_change_batch_compared += 1
                if not diagnostic_bbo_equal((best_bid, best_ask), (pmxt_bid, pmxt_ask)):
                    price_change_batch_mismatches += 1
            record_book_sanity(best_bid, best_ask)
            maybe_run_book_strategy(ts, best_bid, best_ask)
            i = j
            continue

        if event_type == "book":
            next_bids = parse_levels(row.bids)
            next_asks = parse_levels(row.asks)
            if initialized:
                raw_snapshot_pairs += 1
                local_bbo = best_bid_ask(bids, asks)
                snapshot_bbo = best_bid_ask(next_bids, next_asks)
                raw_bbo_mismatch = not bbo_equal(local_bbo, snapshot_bbo)
                raw_full_book_mismatch = not books_equal(bids, next_bids) or not books_equal(asks, next_asks)
                if raw_bbo_mismatch:
                    raw_snapshot_bbo_mismatches += 1
                if raw_full_book_mismatch:
                    raw_snapshot_full_book_mismatches += 1
                if message_key(row) in price_change_message_keys:
                    same_message_snapshot_pairs += 1
                else:
                    snapshot_pairs += 1
                    if raw_bbo_mismatch:
                        snapshot_bbo_mismatches += 1
                    if raw_full_book_mismatch:
                        snapshot_full_book_mismatches += 1
            bids = next_bids
            asks = next_asks
            initialized = True
            book_events += 1
            replay_events += 1
            best_bid, best_ask = update_replayed_bbo()
            record_book_sanity(best_bid, best_ask)
            maybe_run_book_strategy(ts, best_bid, best_ask)
        elif event_type == "last_trade_price":
            trade_events += 1
            if not initialized:
                trades_without_book += 1
                skipped_before_book += 1
                i += 1
                continue
            trade_price = as_float(row.price)
            trade_size = as_float(row.size)
            if trade_price is None or trade_size is None:
                i += 1
                continue
            current_bid, current_ask = best_bid_ask(bids, asks)
            if current_bid is None or current_ask is None:
                trades_without_book += 1
            else:
                trades_checked += 1
                if current_bid - PRICE_EPSILON <= trade_price <= current_ask + PRICE_EPSILON:
                    trades_inside_or_at_book += 1
                else:
                    trades_off_book += 1
                trade_side = str(row.side).upper()
                if trade_side == "BUY" and abs(trade_price - current_ask) <= PRICE_EPSILON:
                    trades_side_touch += 1
                elif trade_side == "SELL" and abs(trade_price - current_bid) <= PRICE_EPSILON:
                    trades_side_touch += 1
            if args.strategy == "maker_bbo":
                fill_side, fill_qty, fill_price = fill_decision(
                    trade_side=row.side,
                    trade_price=trade_price,
                    trade_size=trade_size,
                    quote_bid=quote_bid,
                    quote_ask=quote_ask,
                    quote_size=float(args.quote_size),
                    inventory=inventory,
                    max_inventory=float(args.max_inventory),
                    fill_model=args.fill_model,
                )
                if fill_side == "BUY" and fill_qty > 0:
                    inventory += fill_qty
                    cash -= fill_qty * fill_price
                    record_fill_tick_price(fill_price, ts, "maker_bbo")
                    append_fill(
                        fills,
                        timestamp=ts,
                        strategy=args.strategy,
                        reason="maker_trade_print",
                        fill_side=fill_side,
                        quantity=fill_qty,
                        price=fill_price,
                        trade_price=trade_price,
                        trade_size=trade_size,
                        trade_side=row.side,
                        inventory_after=inventory,
                        cash_after=cash,
                        transaction_hash=row.transaction_hash,
                    )
                elif fill_side == "SELL" and fill_qty > 0:
                    inventory -= fill_qty
                    cash += fill_qty * fill_price
                    record_fill_tick_price(fill_price, ts, "maker_bbo")
                    append_fill(
                        fills,
                        timestamp=ts,
                        strategy=args.strategy,
                        reason="maker_trade_print",
                        fill_side=fill_side,
                        quantity=fill_qty,
                        price=fill_price,
                        trade_price=trade_price,
                        trade_size=trade_size,
                        trade_side=row.side,
                        inventory_after=inventory,
                        cash_after=cash,
                        transaction_hash=row.transaction_hash,
                    )
                if fill_side is not None and fill_qty > 0:
                    best_bid, best_ask = best_bid_ask(bids, asks)
                    mid = None if best_bid is None or best_ask is None else (best_bid + best_ask) / 2
                    mark = mark_price_for_inventory(best_bid, best_ask, inventory)
                    bbo_samples.append(
                        {
                            "timestamp": pd.Timestamp(ts),
                            "best_bid": best_bid,
                            "best_ask": best_ask,
                            "mid": mid,
                            "mark_price": mark,
                            "tick_size": decimal_to_json(current_tick_size),
                            "spread": None if best_bid is None or best_ask is None else best_ask - best_bid,
                            "inventory": inventory,
                            "cash": cash,
                            "mtm_equity": cash + inventory * (mark or 0.0),
                        }
                    )
            replay_events += 1
        elif event_type == "tick_size_change":
            tick_events += 1
            old_tick_size = as_decimal(row.old_tick_size)
            new_tick_size = as_decimal(row.new_tick_size)
            if old_tick_size is not None and old_tick_size != current_tick_size:
                tick_size_old_mismatches += 1
                if len(tick_size_old_mismatch_examples) < 5:
                    tick_size_old_mismatch_examples.append(
                        {
                            "timestamp_received": as_utc_iso(row.timestamp_received),
                            "timestamp": as_utc_iso(row.timestamp),
                            "expected_current_tick_size": decimal_to_json(current_tick_size),
                            "event_old_tick_size": decimal_to_json(old_tick_size),
                            "event_new_tick_size": decimal_to_json(new_tick_size),
                        }
                    )
            if new_tick_size is not None:
                current_tick_size = new_tick_size
                tick_size_changes_applied += 1
            if not initialized:
                skipped_before_book += 1
                i += 1
                continue
            replay_events += 1
            best_bid, best_ask = update_replayed_bbo()
            maybe_run_book_strategy(ts, best_bid, best_ask)
        i += 1

    final_mid = None
    if final_bid is not None and final_ask is not None:
        final_mid = (final_bid + final_ask) / 2
    final_mark_price = mark_price_for_inventory(final_bid, final_ask, inventory)
    mtm_pnl = cash + inventory * (final_mark_price or 0.0)
    settlement_pnl = None
    if selection.settlement_value is not None:
        settlement_pnl = cash + inventory * selection.settlement_value

    out_dir = args.out_dir
    out_dir.mkdir(parents=True, exist_ok=True)
    safe_label = slugify(selection.market_label.replace("\u00b0", "deg"))
    safe_frequency = slugify(args.timeseries_frequency)
    stem = "__".join(
        [
            slugify(selection.event_slug),
            safe_label,
            slugify(selection.token_side.lower()),
            slugify(args.strategy),
        ]
    )
    run_id = "__".join(
        [
            stem,
            f"fill{slugify(args.fill_model)}",
            f"q{slugify_number(float(args.quote_size))}",
            f"max{slugify_number(float(args.max_inventory))}",
            f"freq{safe_frequency}",
            f"thr{slugify_number(float(args.signal_threshold))}",
        ]
    )
    run_dir = child_path(out_dir, run_id)
    run_dir.mkdir(parents=True, exist_ok=True)
    fills_path = child_path(run_dir, "fills.csv")
    bbo_path = child_path(run_dir, f"bbo_{safe_frequency}.csv")
    summary_path = child_path(run_dir, "summary.json")

    with fills_path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(
            f,
            fieldnames=[
                "timestamp",
                "strategy",
                "reason",
                "fill_side",
                "quantity",
                "price",
                "trade_price",
                "trade_size",
                "trade_side",
                "inventory_after",
                "cash_after",
                "transaction_hash",
            ],
        )
        writer.writeheader()
        writer.writerows(fills)

    if bbo_samples:
        bbo_df = pd.DataFrame(bbo_samples).set_index("timestamp").sort_index()
        bbo_out = bbo_df.resample(args.timeseries_frequency).last().dropna(how="all")
        bbo_out.to_csv(bbo_path, encoding="utf-8")
    else:
        bbo_path.write_text("", encoding="utf-8")

    buy_qty = sum(f["quantity"] for f in fills if f["fill_side"] == "BUY")
    sell_qty = sum(f["quantity"] for f in fills if f["fill_side"] == "SELL")
    gross_notional = sum(f["quantity"] * f["price"] for f in fills)
    snapshot_bbo_mismatch_rate = None
    if snapshot_pairs:
        snapshot_bbo_mismatch_rate = snapshot_bbo_mismatches / snapshot_pairs
    snapshot_full_book_mismatch_rate = None
    if snapshot_pairs:
        snapshot_full_book_mismatch_rate = snapshot_full_book_mismatches / snapshot_pairs
    raw_snapshot_bbo_mismatch_rate = None
    if raw_snapshot_pairs:
        raw_snapshot_bbo_mismatch_rate = raw_snapshot_bbo_mismatches / raw_snapshot_pairs
    raw_snapshot_full_book_mismatch_rate = None
    if raw_snapshot_pairs:
        raw_snapshot_full_book_mismatch_rate = raw_snapshot_full_book_mismatches / raw_snapshot_pairs
    trade_off_book_rate = None
    if trades_checked:
        trade_off_book_rate = trades_off_book / trades_checked
    trade_side_touch_rate = None
    if trades_checked:
        trade_side_touch_rate = trades_side_touch / trades_checked
    price_change_batch_mismatch_rate = None
    if price_change_batch_compared:
        price_change_batch_mismatch_rate = price_change_batch_mismatches / price_change_batch_compared
    results_validated = (
        snapshot_bbo_mismatch_rate is not None
        and snapshot_bbo_mismatch_rate <= SNAPSHOT_BBO_MISMATCH_WARN_THRESHOLD
        and price_change_batch_mismatch_rate is not None
        and price_change_batch_mismatch_rate <= BBO_MISMATCH_WARN_THRESHOLD
        and trade_off_book_rate is not None
        and trade_off_book_rate <= TRADE_OFF_BOOK_WARN_THRESHOLD
    )
    summary = {
        "selection": {
            "event_slug": selection.event_slug,
            "event_title": selection.event_title,
            "market_label": selection.market_label,
            "market_id": selection.market_id,
            "condition_id": selection.condition_id,
            "question": selection.question,
            "token_side": selection.token_side,
            "token_id": selection.token_id,
            "settlement_value": selection.settlement_value,
        },
        "inputs": {
            "event_dir": display_input_path(event_dir, args.curated_root),
            "parquet_path": display_input_path(parquet_path, args.curated_root),
            "curated_root_mode": "repo_local"
            if args.curated_root.resolve().is_relative_to(ROOT.resolve())
            else "external_curated_root",
            "rows_for_token": int(len(df)),
            "quote_size": float(args.quote_size),
            "max_inventory": float(args.max_inventory),
            "strategy": args.strategy,
            "run_id": run_id,
            "fill_model": args.fill_model,
            "decision_frequency": args.decision_frequency,
            "signal_threshold": float(args.signal_threshold),
            "replay_order": args.replay_order,
        },
        "replay_quality": {
            "book_events": book_events,
            "price_change_events": price_change_events,
            "trade_events": trade_events,
            "tick_size_change_events": tick_events,
            "replay_events_after_initial_book": replay_events,
            "skipped_before_initial_book": skipped_before_book,
            "pmxt_derived_bbo_diagnostic": {
                "note": "PMXT best_bid/best_ask is treated as price_change batch-level BBO, not per-row ground truth. Boundary best_ask>=1.0 and best_bid<=0.0 are normalized as missing quotes for diagnostics only.",
                "price_change_batch_compared": price_change_batch_compared,
                "price_change_batch_mismatches": price_change_batch_mismatches,
                "price_change_batch_mismatch_rate": price_change_batch_mismatch_rate,
                "split_price_change_batch_key_count": len(split_price_change_batch_keys),
                "split_price_change_batch_key_examples": split_price_change_batch_key_examples,
                "bbo_mismatch_warn_threshold": BBO_MISMATCH_WARN_THRESHOLD,
            },
            "tick_size": {
                "initial_tick_size": "0.01",
                "final_tick_size": decimal_to_json(current_tick_size),
                "tick_size_changes_applied": tick_size_changes_applied,
                "old_tick_size_mismatches": tick_size_old_mismatches,
                "old_tick_size_mismatch_examples": tick_size_old_mismatch_examples,
                "fill_tick_price_checks": fill_tick_price_checks,
                "fill_tick_price_violations": fill_tick_price_violations,
                "fill_tick_price_violation_examples": fill_tick_price_violation_examples,
            },
            "snapshot_alignment": {
                "note": "Primary snapshot mismatch rates exclude book snapshots that share the same timestamp_received/timestamp/market/asset_id key with price_change rows, because those are likely same-message checkpoints rather than independent next snapshots.",
                "raw_snapshot_pairs": raw_snapshot_pairs,
                "raw_snapshot_bbo_mismatches": raw_snapshot_bbo_mismatches,
                "raw_snapshot_bbo_mismatch_rate": raw_snapshot_bbo_mismatch_rate,
                "raw_snapshot_full_book_mismatches": raw_snapshot_full_book_mismatches,
                "raw_snapshot_full_book_mismatch_rate": raw_snapshot_full_book_mismatch_rate,
                "same_message_snapshot_pairs": same_message_snapshot_pairs,
                "snapshot_pairs": snapshot_pairs,
                "snapshot_bbo_mismatches": snapshot_bbo_mismatches,
                "snapshot_bbo_mismatch_rate": snapshot_bbo_mismatch_rate,
                "snapshot_full_book_mismatches": snapshot_full_book_mismatches,
                "snapshot_full_book_mismatch_rate": snapshot_full_book_mismatch_rate,
                "snapshot_bbo_mismatch_warn_threshold": SNAPSHOT_BBO_MISMATCH_WARN_THRESHOLD,
            },
            "book_sanity": {
                "crossed_or_locked_books": crossed_or_locked_books,
                "negative_spread_samples": negative_spread_samples,
            },
            "trade_sanity": {
                "trades_checked": trades_checked,
                "trades_without_book": trades_without_book,
                "trades_inside_or_at_book": trades_inside_or_at_book,
                "trades_side_touch": trades_side_touch,
                "trades_off_book": trades_off_book,
                "trade_off_book_rate": trade_off_book_rate,
                "trade_side_touch_rate": trade_side_touch_rate,
                "trade_off_book_warn_threshold": TRADE_OFF_BOOK_WARN_THRESHOLD,
            },
            "results_validated": results_validated,
            "result_label": "validated_baseline" if results_validated else "smoke_test_unvalidated",
        },
        "backtest": {
            "fills": len(fills),
            "buy_fills": sum(1 for f in fills if f["fill_side"] == "BUY"),
            "sell_fills": sum(1 for f in fills if f["fill_side"] == "SELL"),
            "buy_qty": buy_qty,
            "sell_qty": sell_qty,
            "gross_notional": gross_notional,
            "ending_inventory": inventory,
            "ending_cash": cash,
            "final_best_bid": final_bid,
            "final_best_ask": final_ask,
            "final_mid": final_mid,
            "final_mark_price": final_mark_price,
            "mtm_pnl": mtm_pnl,
            "settlement_pnl": settlement_pnl,
            "return_on_gross_notional": None
            if gross_notional == 0 or settlement_pnl is None
            else settlement_pnl / gross_notional,
        },
        "outputs": {
            "fills_csv": repo_path(fills_path),
            "bbo_csv": repo_path(bbo_path),
            "summary_json": repo_path(summary_path),
        },
        "assumptions": [
            "Single-token research harness; not a full event-level negRisk/combo model.",
            "maker_bbo uses PMXT last_trade_price events and fills only at the current valid two-sided uncrossed BBO when trade side is compatible; quotes are cleared when the replayed book becomes one-sided or crossed.",
            "Taker strategies assume immediate top-of-book execution at sampled BBO and do not model taker delay or slippage beyond top level.",
            "tick_size_change is tracked as replay state starting at 0.01 and updating from each event new_tick_size; fill prices are checked against the active tick grid.",
            "No latency, queue priority, fees, rebates, rewards, or partial queue-ahead model yet.",
            "L2 book is reconstructed from PMXT book snapshots plus price_change rows for the selected asset_id.",
            f"Replay row order is {args.replay_order}.",
        ],
    }
    summary_path.write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    return summary


def main() -> None:
    args = parse_args()
    summary = run_backtest(args)
    print(json.dumps(summary, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
