#!/usr/bin/env python3
"""Run a minimal PMXT event replay and simple strategy backtest.

The first target is the curated Shanghai temperature event:

    highest-temperature-in-shanghai-on-june-9-2026 / 25C / YES

This is intentionally a small research harness, not a production engine:
- reads the event_index + gamma raw metadata;
- reads the curated PMXT orderbook parquet;
- rebuilds one selected token's L2 book from book + price_change events;
- validates replayed BBO against PMXT best_bid/best_ask fields;
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


def count_grouped_bbo_mismatches(df: pd.DataFrame) -> tuple[int, int]:
    """Compare PMXT BBO after applying all rows with the same receive timestamp.

    PMXT price_change rows often arrive as small batches sharing the same
    timestamp_received. A row-by-row comparison can over-count mismatches when
    best_bid/best_ask describe the post-batch book. This grouped check is the
    replay-quality metric used for the report.
    """

    bids: dict[float, float] = {}
    asks: dict[float, float] = {}
    initialized = False
    current_ts: Any = None
    expected_bid: float | None = None
    expected_ask: float | None = None
    compared = 0
    mismatches = 0

    def close_group() -> None:
        nonlocal compared, mismatches
        if not initialized or expected_bid is None or expected_ask is None:
            return
        best_bid, best_ask = best_bid_ask(bids, asks)
        compared += 1
        if best_bid is None or best_ask is None:
            mismatches += 1
            return
        if abs(best_bid - expected_bid) > 1e-9 or abs(best_ask - expected_ask) > 1e-9:
            mismatches += 1

    for row in df.itertuples(index=False):
        ts = row.timestamp_received
        if current_ts is None:
            current_ts = ts
        elif ts != current_ts:
            close_group()
            current_ts = ts
            expected_bid = None
            expected_ask = None

        event_type = row.event_type
        if event_type == "book":
            bids = parse_levels(row.bids)
            asks = parse_levels(row.asks)
            initialized = True
        elif event_type == "price_change" and initialized:
            price = as_float(row.price)
            size = as_float(row.size)
            if price is None or size is None:
                continue
            side = str(row.side).upper()
            if side == "BUY":
                update_level(bids, price, size)
            elif side == "SELL":
                update_level(asks, price, size)
            pmxt_bid = as_float(getattr(row, "best_bid", None))
            pmxt_ask = as_float(getattr(row, "best_ask", None))
            if pmxt_bid is not None and pmxt_ask is not None:
                expected_bid = pmxt_bid
                expected_ask = pmxt_ask

    close_group()
    return compared, mismatches


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
    df = df.sort_values(["timestamp_received", "_row"], kind="mergesort")

    bids: dict[float, float] = {}
    asks: dict[float, float] = {}
    initialized = False
    quote_bid: float | None = None
    quote_ask: float | None = None
    final_bid: float | None = None
    final_ask: float | None = None
    last_decision_ts: pd.Timestamp | None = None
    last_decision_mid: float | None = None
    decision_frequency = pd.Timedelta(args.decision_frequency)

    cash = 0.0
    inventory = 0.0
    fills: list[dict[str, Any]] = []
    bbo_samples: list[dict[str, Any]] = []
    replay_events = 0
    skipped_before_book = 0
    bbo_row_mismatch = 0
    book_events = 0
    price_change_events = 0
    trade_events = 0
    tick_events = 0

    for row in df.itertuples(index=False):
        event_type = row.event_type
        ts = row.timestamp_received

        if event_type == "book":
            bids = parse_levels(row.bids)
            asks = parse_levels(row.asks)
            initialized = True
            book_events += 1
        elif event_type == "price_change":
            price_change_events += 1
            if not initialized:
                skipped_before_book += 1
                continue
            price = as_float(row.price)
            size = as_float(row.size)
            side = str(row.side).upper()
            if price is None or size is None:
                continue
            if side == "BUY":
                update_level(bids, price, size)
            elif side == "SELL":
                update_level(asks, price, size)
        elif event_type == "last_trade_price":
            trade_events += 1
            if not initialized:
                skipped_before_book += 1
                continue
            trade_price = as_float(row.price)
            trade_size = as_float(row.size)
            if trade_price is None or trade_size is None:
                continue
            if args.strategy != "maker_bbo":
                continue
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
                mark = 0.0 if quote_bid is None or quote_ask is None else (quote_bid + quote_ask) / 2
                bbo_samples.append(
                    {
                        "timestamp": pd.Timestamp(ts),
                        "best_bid": quote_bid,
                        "best_ask": quote_ask,
                        "mid": None if quote_bid is None or quote_ask is None else mark,
                        "spread": None if quote_bid is None or quote_ask is None else quote_ask - quote_bid,
                        "inventory": inventory,
                        "cash": cash,
                        "mtm_equity": cash + inventory * mark,
                    }
                )
            elif fill_side == "SELL" and fill_qty > 0:
                inventory -= fill_qty
                cash += fill_qty * fill_price
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
                mark = 0.0 if quote_bid is None or quote_ask is None else (quote_bid + quote_ask) / 2
                bbo_samples.append(
                    {
                        "timestamp": pd.Timestamp(ts),
                        "best_bid": quote_bid,
                        "best_ask": quote_ask,
                        "mid": None if quote_bid is None or quote_ask is None else mark,
                        "spread": None if quote_bid is None or quote_ask is None else quote_ask - quote_bid,
                        "inventory": inventory,
                        "cash": cash,
                        "mtm_equity": cash + inventory * mark,
                    }
                )
        elif event_type == "tick_size_change":
            tick_events += 1
            if not initialized:
                skipped_before_book += 1
                continue
        else:
            continue

        if not initialized:
            continue

        replay_events += 1
        best_bid, best_ask = best_bid_ask(bids, asks)
        final_bid, final_ask = best_bid, best_ask
        if best_bid is not None and best_ask is not None and best_bid < best_ask:
            quote_bid, quote_ask = best_bid, best_ask

        pmxt_bid = as_float(getattr(row, "best_bid", None))
        pmxt_ask = as_float(getattr(row, "best_ask", None))
        if event_type == "price_change" and pmxt_bid is not None and pmxt_ask is not None:
            if best_bid is None or best_ask is None:
                bbo_row_mismatch += 1
            elif abs(best_bid - pmxt_bid) > 1e-9 or abs(best_ask - pmxt_ask) > 1e-9:
                bbo_row_mismatch += 1

        if event_type in {"book", "price_change", "tick_size_change"}:
            mid = None if best_bid is None or best_ask is None else (best_bid + best_ask) / 2
            mark = mid if mid is not None else 0.0
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
                mark = mid
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
                    "spread": None if best_bid is None or best_ask is None else best_ask - best_bid,
                    "inventory": inventory,
                    "cash": cash,
                    "mtm_equity": cash + inventory * mark,
                }
            )

    final_mid = None
    if final_bid is not None and final_ask is not None:
        final_mid = (final_bid + final_ask) / 2
    mtm_pnl = cash + inventory * (final_mid or 0.0)
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
    bbo_group_compared, bbo_group_mismatch = count_grouped_bbo_mismatches(df)
    bbo_group_mismatch_rate = None
    if bbo_group_compared:
        bbo_group_mismatch_rate = bbo_group_mismatch / bbo_group_compared
    results_validated = (
        bbo_group_mismatch_rate is not None
        and bbo_group_mismatch_rate <= BBO_MISMATCH_WARN_THRESHOLD
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
        },
        "replay_quality": {
            "book_events": book_events,
            "price_change_events": price_change_events,
            "trade_events": trade_events,
            "tick_size_change_events": tick_events,
            "replay_events_after_initial_book": replay_events,
            "skipped_before_initial_book": skipped_before_book,
            "bbo_row_mismatch_count": bbo_row_mismatch,
            "bbo_group_compared": bbo_group_compared,
            "bbo_group_mismatch_count": bbo_group_mismatch,
            "bbo_group_mismatch_rate": bbo_group_mismatch_rate,
            "bbo_mismatch_warn_threshold": BBO_MISMATCH_WARN_THRESHOLD,
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
            "maker_bbo uses PMXT last_trade_price events and fills only at our standing best bid/ask when trade side is compatible.",
            "Taker strategies assume immediate top-of-book execution at sampled BBO and do not model taker delay or slippage beyond top level.",
            "No latency, queue priority, fees, rebates, rewards, or partial queue-ahead model yet.",
            "L2 book is reconstructed from PMXT book snapshots plus price_change rows for the selected asset_id.",
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
