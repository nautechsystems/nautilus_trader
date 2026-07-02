"""Polymarket v1 research backtest entry point."""

from __future__ import annotations

import argparse
import csv
import hashlib
import importlib.util
import json
import shutil
import sys
import uuid
from dataclasses import dataclass, field
from datetime import UTC, datetime
from decimal import Decimal
from pathlib import Path
from types import ModuleType
from typing import Any, Mapping

import yaml

from polymarket.adapters.live_event_bundle_v1 import LiveEventBundleV1Adapter
from polymarket.adapters.live_ws_v1 import LiveWsV1Adapter
from polymarket.adapters.pmxt_event_v1 import PMXTEventV1Adapter
from polymarket.adapters.pmxt_parquet_v1 import PMXTParquetV1Adapter
from polymarket.adapters.utils import repo_relative_or_absolute
from polymarket.models import L2ReplayStepV1, L2UpdateV1, PolymarketL2DatasetV1
from polymarket.strategies.base import BasePolymarketStrategyV1


REPO_ROOT = Path(__file__).resolve().parents[1]
ADAPTERS = {
    PMXTParquetV1Adapter.adapter_name: PMXTParquetV1Adapter,
    PMXTEventV1Adapter.adapter_name: PMXTEventV1Adapter,
    LiveWsV1Adapter.adapter_name: LiveWsV1Adapter,
    LiveEventBundleV1Adapter.adapter_name: LiveEventBundleV1Adapter,
}


def as_decimal(value: Any) -> Decimal:
    if isinstance(value, Decimal):
        return value
    return Decimal(str(value))


def decimal_json(value: Decimal | None) -> str | None:
    if value is None:
        return None
    return format(value.normalize(), "f")


def now_run_id() -> str:
    return datetime.now(tz=UTC).strftime("%Y%m%dT%H%M%SZ")


def ensure_child(parent: Path, child: Path) -> Path:
    parent_resolved = parent.resolve()
    child_resolved = child.resolve()
    if parent_resolved != child_resolved and parent_resolved not in child_resolved.parents:
        raise ValueError(f"refusing to write outside {parent}: {child}")
    return child_resolved


def is_child(parent: Path, child: Path) -> bool:
    parent_resolved = parent.resolve()
    child_resolved = child.resolve()
    return parent_resolved == child_resolved or parent_resolved in child_resolved.parents


@dataclass(slots=True)
class BookStateV1:
    bids: dict[Decimal, Decimal] = field(default_factory=dict)
    asks: dict[Decimal, Decimal] = field(default_factory=dict)

    def apply(self, update: L2UpdateV1) -> None:
        if update.event_type == "book":
            self.bids = {level.price: level.size for level in update.bids if level.size > 0}
            self.asks = {level.price: level.size for level in update.asks if level.size > 0}
            return
        if update.event_type != "price_change" or update.price is None or update.size is None:
            return
        levels = self.bids if update.side == "BUY" else self.asks if update.side == "SELL" else None
        if levels is None:
            return
        if update.size <= 0:
            levels.pop(update.price, None)
        else:
            levels[update.price] = update.size

    @property
    def best_bid(self) -> Decimal | None:
        return max(self.bids) if self.bids else None

    @property
    def best_ask(self) -> Decimal | None:
        return min(self.asks) if self.asks else None

    @property
    def bid_size(self) -> Decimal:
        bid = self.best_bid
        return self.bids.get(bid, Decimal("0")) if bid is not None else Decimal("0")

    @property
    def ask_size(self) -> Decimal:
        ask = self.best_ask
        return self.asks.get(ask, Decimal("0")) if ask is not None else Decimal("0")


@dataclass(frozen=True, slots=True)
class L2BookViewV1:
    asset_id: str | None
    best_bid: Decimal | None
    best_ask: Decimal | None
    bid_size: Decimal
    ask_size: Decimal
    tick_size: Decimal


@dataclass(slots=True)
class RestingOrderV1:
    order_id: str
    side: str
    price: Decimal
    quantity: Decimal
    remaining: Decimal
    label: str
    created_sequence: int
    created_timestamp: str


@dataclass(slots=True)
class FillV1:
    timestamp: str
    sequence: int
    side: str
    price: Decimal
    quantity: Decimal
    order_id: str
    label: str
    liquidity: str


class BacktestContextV1:
    def __init__(self, engine: BacktestEngineV1) -> None:
        self._engine = engine

    @property
    def position(self) -> Decimal:
        return self._engine.position

    @property
    def cash(self) -> Decimal:
        return self._engine.cash

    def market_order(self, side: str, quantity: Decimal | str | int | float, *, label: str = "") -> None:
        self._engine.submit_market_order(side=side, quantity=as_decimal(quantity), label=label)

    def limit_order(
        self,
        side: str,
        price: Decimal | str | int | float,
        quantity: Decimal | str | int | float,
        *,
        label: str = "",
    ) -> str:
        return self._engine.submit_limit_order(
            side=side,
            price=as_decimal(price),
            quantity=as_decimal(quantity),
            label=label,
        )

    def cancel_all(self) -> None:
        self._engine.resting_orders.clear()


class BacktestEngineV1:
    def __init__(
        self,
        *,
        dataset: PolymarketL2DatasetV1,
        strategy: BasePolymarketStrategyV1,
        selected_asset_id: str | None = None,
        initial_cash: Decimal = Decimal("0"),
        initial_tick_size: Decimal = Decimal("0.01"),
    ) -> None:
        self.dataset = dataset
        self.strategy = strategy
        self.selected_asset_id = selected_asset_id
        self.books: dict[str, BookStateV1] = {}
        self.current_asset_id: str | None = selected_asset_id
        self.current_step: L2ReplayStepV1 | None = None
        self.tick_size = initial_tick_size
        self.position = Decimal("0")
        self.cash = initial_cash
        self.fills: list[FillV1] = []
        self.resting_orders: list[RestingOrderV1] = []
        self.tick_size_changes_applied = 0

    def run(self) -> dict[str, Any]:
        context = BacktestContextV1(self)
        self.strategy.on_start(context)
        for step in self.dataset.steps:
            self.current_step = step
            self._apply_step(step)
            self._fill_from_trades(step)
            book = self.book_view()
            self.strategy.on_replay_step(step, book, context)
        self.strategy.on_finish(context)
        final_mark = self.mark_price()
        final_equity = self.cash + self.position * (final_mark or Decimal("0"))
        return {
            "steps": len(self.dataset.steps),
            "fills": len(self.fills),
            "cash": decimal_json(self.cash),
            "position": decimal_json(self.position),
            "final_mark_price": decimal_json(final_mark),
            "final_equity": decimal_json(final_equity),
            "tick_size_changes_applied": self.tick_size_changes_applied,
            "final_tick_size": decimal_json(self.tick_size),
        }

    def _apply_step(self, step: L2ReplayStepV1) -> None:
        for update in step.updates:
            if self.selected_asset_id is not None and update.asset_id != self.selected_asset_id:
                continue
            self.current_asset_id = update.asset_id
            if update.event_type in {"book", "price_change"}:
                self.books.setdefault(update.asset_id, BookStateV1()).apply(update)
            elif update.event_type == "tick_size_change" and update.new_tick_size is not None:
                self.tick_size = update.new_tick_size
                self.tick_size_changes_applied += 1

    def _fill_from_trades(self, step: L2ReplayStepV1) -> None:
        for update in step.updates:
            if update.event_type != "trade" or update.price is None or update.size is None:
                continue
            if self.selected_asset_id is not None and update.asset_id != self.selected_asset_id:
                continue
            remaining_trade = update.size
            for order in list(self.resting_orders):
                if remaining_trade <= 0:
                    break
                if not self._trade_hits_order(update, order):
                    continue
                qty = min(order.remaining, remaining_trade)
                self._record_fill(order.side, update.price, qty, order.order_id, order.label, "maker")
                order.remaining -= qty
                remaining_trade -= qty
                if order.remaining <= 0:
                    self.resting_orders.remove(order)

    @staticmethod
    def _trade_hits_order(update: L2UpdateV1, order: RestingOrderV1) -> bool:
        if update.price is None:
            return False
        if order.side == "BUY":
            compatible_side = update.side in {None, "SELL"}
            return compatible_side and update.price <= order.price
        compatible_side = update.side in {None, "BUY"}
        return compatible_side and update.price >= order.price

    def book_view(self) -> L2BookViewV1:
        asset_id = self.current_asset_id
        book = self.books.get(asset_id or "")
        return L2BookViewV1(
            asset_id=asset_id,
            best_bid=book.best_bid if book else None,
            best_ask=book.best_ask if book else None,
            bid_size=book.bid_size if book else Decimal("0"),
            ask_size=book.ask_size if book else Decimal("0"),
            tick_size=self.tick_size,
        )

    def mark_price(self) -> Decimal | None:
        view = self.book_view()
        if view.best_bid is not None and view.best_ask is not None:
            return (view.best_bid + view.best_ask) / Decimal("2")
        return view.best_bid if view.best_bid is not None else view.best_ask

    def submit_market_order(self, *, side: str, quantity: Decimal, label: str = "") -> None:
        view = self.book_view()
        if side.upper() == "BUY":
            price = view.best_ask
            available = view.ask_size
        elif side.upper() == "SELL":
            price = view.best_bid
            available = view.bid_size
        else:
            raise ValueError(f"unsupported side: {side}")
        if price is None or available <= 0:
            return
        qty = min(quantity, available)
        if qty <= 0:
            return
        self._record_fill(side.upper(), price, qty, "market", label, "taker")

    def submit_limit_order(self, *, side: str, price: Decimal, quantity: Decimal, label: str = "") -> str:
        step = self.current_step
        if step is None:
            raise RuntimeError("cannot place order before replay starts")
        order_id = f"L{len(self.resting_orders) + len(self.fills) + 1}"
        self.resting_orders.append(
            RestingOrderV1(
                order_id=order_id,
                side=side.upper(),
                price=price,
                quantity=quantity,
                remaining=quantity,
                label=label,
                created_sequence=step.sequence,
                created_timestamp=step.timestamp_received.isoformat().replace("+00:00", "Z"),
            ),
        )
        return order_id

    def _record_fill(
        self,
        side: str,
        price: Decimal,
        quantity: Decimal,
        order_id: str,
        label: str,
        liquidity: str,
    ) -> None:
        if self.current_step is None:
            raise RuntimeError("no current replay step")
        if side == "BUY":
            self.position += quantity
            self.cash -= price * quantity
        elif side == "SELL":
            self.position -= quantity
            self.cash += price * quantity
        else:
            raise ValueError(f"unsupported fill side: {side}")
        self.fills.append(
            FillV1(
                timestamp=self.current_step.timestamp_received.isoformat().replace("+00:00", "Z"),
                sequence=self.current_step.sequence,
                side=side,
                price=price,
                quantity=quantity,
                order_id=order_id,
                label=label,
                liquidity=liquidity,
            ),
        )


def load_yaml(path: Path) -> dict[str, Any]:
    return yaml.safe_load(path.read_text(encoding="utf-8")) or {}


def load_adapter(config: Mapping[str, Any]) -> PolymarketL2DatasetV1:
    adapter_config = config.get("adapter") or {}
    name = str(adapter_config.get("name"))
    if name not in ADAPTERS:
        raise ValueError(f"unknown adapter {name!r}; expected one of {sorted(ADAPTERS)}")
    return ADAPTERS[name](repo_root=REPO_ROOT).load(adapter_config)


def resolve_strategy_path(config_path: Path, strategy_config: Mapping[str, Any]) -> tuple[Path, str, list[str]]:
    original = str(strategy_config.get("path"))
    if original in {"", "None"}:
        raise ValueError("strategy.path is required")
    path = Path(original)
    if not path.is_absolute():
        path = config_path.parent / path
    resolved = path.resolve()
    warnings: list[str] = []
    if not is_child(REPO_ROOT, resolved):
        warnings.append("strategy.path resolves outside the repository; audit before sharing this run.")
    return resolved, original, warnings


def load_strategy(config_path: Path, strategy_config: Mapping[str, Any]) -> tuple[BasePolymarketStrategyV1, dict[str, Any]]:
    source_path, original, warnings = resolve_strategy_path(config_path, strategy_config)
    class_name = str(strategy_config.get("class"))
    params = dict(strategy_config.get("params") or {})
    module_name = f"_polymarket_strategy_{uuid.uuid4().hex}"
    spec = importlib.util.spec_from_file_location(module_name, source_path)
    if spec is None or spec.loader is None:
        raise ImportError(f"cannot load strategy file: {source_path}")
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    strategy_class = getattr(module, class_name)
    strategy = strategy_class(**params)
    if not isinstance(strategy, BasePolymarketStrategyV1):
        raise TypeError(f"{class_name} must subclass BasePolymarketStrategyV1")
    provenance = {
        "loader_mode": "path",
        "source_path_original": original,
        "source_path_resolved": repo_relative_or_absolute(source_path, repo_root=REPO_ROOT),
        "class": class_name,
        "params_resolved": params,
        "source_sha256": hashlib.sha256(source_path.read_bytes()).hexdigest(),
        "warnings": warnings,
    }
    return strategy, provenance


def build_resolved_config(
    *,
    config: Mapping[str, Any],
    config_path: Path,
    run_dir: Path,
    dataset: PolymarketL2DatasetV1,
    strategy_provenance: Mapping[str, Any],
    run_id: str,
) -> dict[str, Any]:
    return {
        "experiment": dict(config.get("experiment") or {}),
        "adapter": {
            "name": dataset.metadata.adapter_name,
            "adapter_version": dataset.metadata.adapter_version,
            "source_type": dataset.metadata.source_type,
            "source_files_resolved": list(dataset.metadata.source_files),
            "assumptions": list(dataset.metadata.assumptions),
            "warnings": list(dataset.metadata.warnings),
        },
        "strategy": dict(strategy_provenance),
        "runtime": {
            "run_id": run_id,
            "created_at_utc": datetime.now(tz=UTC).isoformat().replace("+00:00", "Z"),
            "config_path": repo_relative_or_absolute(config_path, repo_root=REPO_ROOT),
            "run_dir": repo_relative_or_absolute(run_dir, repo_root=REPO_ROOT),
        },
    }


def write_outputs(run_dir: Path, engine: BacktestEngineV1, metrics: Mapping[str, Any]) -> None:
    fills_path = run_dir / "fills.csv"
    with fills_path.open("w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(
            f,
            fieldnames=["timestamp", "sequence", "side", "price", "quantity", "order_id", "label", "liquidity"],
        )
        writer.writeheader()
        for fill in engine.fills:
            writer.writerow(
                {
                    "timestamp": fill.timestamp,
                    "sequence": fill.sequence,
                    "side": fill.side,
                    "price": decimal_json(fill.price),
                    "quantity": decimal_json(fill.quantity),
                    "order_id": fill.order_id,
                    "label": fill.label,
                    "liquidity": fill.liquidity,
                },
            )
    (run_dir / "metrics.json").write_text(json.dumps(metrics, indent=2), encoding="utf-8")
    (run_dir / "summary.json").write_text(json.dumps({"backtest": metrics}, indent=2), encoding="utf-8")


def resolve_output_dir(config_path: Path, report_config: Mapping[str, Any]) -> Path:
    """Resolve run output root and keep it inside the experiment-local runs tree."""

    runs_root = (config_path.parent / "runs").resolve()
    configured = report_config.get("output_dir", "./runs")
    output_dir = Path(str(configured))
    if not output_dir.is_absolute():
        output_dir = config_path.parent / output_dir
    output_dir = output_dir.resolve()
    if not is_child(runs_root, output_dir):
        raise ValueError(
            "report.output_dir must stay inside the experiment-local runs directory "
            f"{runs_root}; got {output_dir}",
        )
    return output_dir


def run_from_config(config_path: Path) -> dict[str, Any]:
    config_path = config_path.resolve()
    config = load_yaml(config_path)
    run_id = str((config.get("runtime") or {}).get("run_id") or now_run_id())
    report_config = config.get("report") or {}
    output_dir = resolve_output_dir(config_path, report_config)
    run_dir = ensure_child(output_dir, output_dir / run_id)
    run_dir.mkdir(parents=True, exist_ok=True)

    dataset = load_adapter(config)
    strategy, strategy_provenance = load_strategy(config_path, config.get("strategy") or {})
    engine = BacktestEngineV1(
        dataset=dataset,
        strategy=strategy,
        selected_asset_id=(config.get("selection") or {}).get("asset_id"),
        initial_cash=as_decimal((config.get("portfolio") or {}).get("initial_cash", "0")),
        initial_tick_size=as_decimal((config.get("replay") or {}).get("initial_tick_size", "0.01")),
    )
    metrics = engine.run()
    resolved = build_resolved_config(
        config=config,
        config_path=config_path,
        run_dir=run_dir,
        dataset=dataset,
        strategy_provenance=strategy_provenance,
        run_id=run_id,
    )
    shutil.copyfile(config_path, run_dir / "original_config.yml")
    (run_dir / "resolved_config.json").write_text(json.dumps(resolved, indent=2), encoding="utf-8")
    write_outputs(run_dir, engine, metrics)
    summary = {
        "run_dir": repo_relative_or_absolute(run_dir, repo_root=REPO_ROOT),
        "outputs": {
            "original_config": repo_relative_or_absolute(run_dir / "original_config.yml", repo_root=REPO_ROOT),
            "resolved_config": repo_relative_or_absolute(run_dir / "resolved_config.json", repo_root=REPO_ROOT),
            "metrics": repo_relative_or_absolute(run_dir / "metrics.json", repo_root=REPO_ROOT),
            "fills_csv": repo_relative_or_absolute(run_dir / "fills.csv", repo_root=REPO_ROOT),
            "summary_json": repo_relative_or_absolute(run_dir / "summary.json", repo_root=REPO_ROOT),
        },
        "backtest": metrics,
    }
    print(json.dumps(summary, indent=2))
    return summary


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    run_from_config(args.config)


if __name__ == "__main__":
    main()
