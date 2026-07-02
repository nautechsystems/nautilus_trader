"""Golden correctness tests for the Polymarket v1 replay engine."""

from __future__ import annotations

from datetime import UTC, datetime, timedelta
from decimal import Decimal

from polymarket.backtest_v1 import BacktestEngineV1
from polymarket.models import (
    DatasetMetadataV1,
    L2ReplayStepV1,
    L2UpdateV1,
    LevelV1,
    PolymarketL2DatasetV1,
)
from polymarket.strategies.base import BasePolymarketStrategyV1


BASE = datetime(2026, 1, 1, tzinfo=UTC)


def ts(seconds: int) -> datetime:
    return BASE + timedelta(seconds=seconds)


def dataset(steps: list[L2ReplayStepV1]) -> PolymarketL2DatasetV1:
    return PolymarketL2DatasetV1(
        metadata=DatasetMetadataV1(
            dataset_id="synthetic",
            adapter_name="synthetic",
            adapter_version="v1",
            source_type="test",
        ),
        steps=tuple(steps),
    )


def step(sequence: int, updates: list[L2UpdateV1]) -> L2ReplayStepV1:
    return L2ReplayStepV1(sequence=sequence, timestamp_received=ts(sequence), timestamp=ts(sequence), updates=tuple(updates))


def book(asset_id: str = "yes", *, bid: str = "0.50", ask: str = "0.60") -> L2UpdateV1:
    return L2UpdateV1(
        event_type="book",
        market="m1",
        asset_id=asset_id,
        bids=(LevelV1(Decimal(bid), Decimal("100")),),
        asks=(LevelV1(Decimal(ask), Decimal("100")),),
    )


class BuyWhenAskAtMost(BasePolymarketStrategyV1):
    def __init__(self, threshold: str, quantity: str = "1") -> None:
        super().__init__(threshold=threshold, quantity=quantity)
        self.threshold = Decimal(threshold)
        self.quantity = Decimal(quantity)
        self.done = False

    def on_replay_step(self, step, book, context) -> None:
        if not self.done and book.best_ask is not None and book.best_ask <= self.threshold:
            context.market_order("BUY", self.quantity, label="threshold_buy")
            self.done = True


def test_strategy_cannot_trade_before_future_book_update() -> None:
    d = dataset(
        [
            step(1, [book(ask="0.80")]),
            step(2, [L2UpdateV1(event_type="price_change", market="m1", asset_id="yes", side="SELL", price=Decimal("0.58"), size=Decimal("100"))]),
        ],
    )
    engine = BacktestEngineV1(dataset=d, strategy=BuyWhenAskAtMost("0.60"), selected_asset_id="yes")
    metrics = engine.run()

    assert metrics["fills"] == 1
    assert engine.fills[0].sequence == 2
    assert engine.fills[0].price == Decimal("0.58")


def test_price_change_step_is_atomic_for_strategy_decisions() -> None:
    d = dataset(
        [
            step(1, [book(bid="0.50", ask="0.90")]),
            step(
                2,
                [
                    L2UpdateV1(event_type="price_change", market="m1", asset_id="yes", side="BUY", price=Decimal("0.56"), size=Decimal("100")),
                    L2UpdateV1(event_type="price_change", market="m1", asset_id="yes", side="SELL", price=Decimal("0.58"), size=Decimal("100")),
                ],
            ),
        ],
    )
    engine = BacktestEngineV1(dataset=d, strategy=BuyWhenAskAtMost("0.60", quantity="2"), selected_asset_id="yes")
    metrics = engine.run()

    assert metrics["fills"] == 1
    assert engine.fills[0].sequence == 2
    assert engine.fills[0].price == Decimal("0.58")
    assert metrics["position"] == "2"


class RecordTickSizes(BasePolymarketStrategyV1):
    def __init__(self) -> None:
        super().__init__()
        self.seen: list[Decimal] = []

    def on_replay_step(self, step, book, context) -> None:
        self.seen.append(book.tick_size)


def test_tick_size_change_is_causal_state() -> None:
    strategy = RecordTickSizes()
    d = dataset(
        [
            step(1, [book()]),
            step(2, [L2UpdateV1(event_type="tick_size_change", market="m1", asset_id="yes", old_tick_size=Decimal("0.01"), new_tick_size=Decimal("0.001"))]),
        ],
    )
    metrics = BacktestEngineV1(dataset=d, strategy=strategy, selected_asset_id="yes").run()

    assert strategy.seen == [Decimal("0.01"), Decimal("0.001")]
    assert metrics["tick_size_changes_applied"] == 1
    assert metrics["final_tick_size"] == "0.001"


class QuoteThenWait(BasePolymarketStrategyV1):
    def __init__(self) -> None:
        super().__init__()
        self.quoted = False

    def on_replay_step(self, step, book, context) -> None:
        if not self.quoted and book.best_ask is not None:
            context.limit_order("SELL", book.best_ask, "10", label="maker_sell")
            self.quoted = True


def test_maker_fill_respects_partial_trade_size_and_mtm() -> None:
    d = dataset(
        [
            step(1, [book(ask="0.60")]),
            step(2, [L2UpdateV1(event_type="trade", market="m1", asset_id="yes", side="BUY", price=Decimal("0.60"), size=Decimal("3"))]),
        ],
    )
    engine = BacktestEngineV1(dataset=d, strategy=QuoteThenWait(), selected_asset_id="yes")
    metrics = engine.run()

    assert metrics["fills"] == 1
    assert engine.fills[0].quantity == Decimal("3")
    assert metrics["cash"] == "1.8"
    assert metrics["position"] == "-3"
    assert metrics["final_equity"] == "0.15"

