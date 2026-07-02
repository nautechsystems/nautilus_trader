from __future__ import annotations

from decimal import Decimal

from polymarket.strategies.base import BasePolymarketStrategyV1


class BuyHoldFirstAskStrategy(BasePolymarketStrategyV1):
    def __init__(self, quantity="10"):
        super().__init__(quantity=quantity)
        self.quantity = Decimal(str(quantity))
        self.done = False

    def on_replay_step(self, step, book, context):
        if self.done or book.best_ask is None:
            return
        context.market_order("BUY", self.quantity, label="buy_hold_first_ask")
        self.done = True


class MakerBboStrategy(BasePolymarketStrategyV1):
    """One-level BBO quoting smoke strategy.

    The v1 engine fills resting orders from subsequent trade events.  This
    intentionally mirrors the old research smoke only at the plumbing level: it
    is not a queue model and does not claim maker priority.
    """

    def __init__(self, quantity="10"):
        super().__init__(quantity=quantity)
        self.quantity = Decimal(str(quantity))

    def on_replay_step(self, step, book, context):
        context.cancel_all()
        if book.best_bid is not None:
            context.limit_order("BUY", book.best_bid, self.quantity, label="maker_bbo")
        if book.best_ask is not None:
            context.limit_order("SELL", book.best_ask, self.quantity, label="maker_bbo")


class MidDeltaTakerStrategy(BasePolymarketStrategyV1):
    def __init__(self, quantity="10", threshold="0.03", mode="momentum"):
        super().__init__(quantity=quantity, threshold=threshold, mode=mode)
        self.quantity = Decimal(str(quantity))
        self.threshold = Decimal(str(threshold))
        self.mode = str(mode)
        self.last_mid = None

    def on_replay_step(self, step, book, context):
        if book.best_bid is None or book.best_ask is None:
            return
        mid = (book.best_bid + book.best_ask) / Decimal("2")
        if self.last_mid is None:
            self.last_mid = mid
            return
        delta = mid - self.last_mid
        self.last_mid = mid
        if abs(delta) < self.threshold:
            return
        if self.mode == "momentum":
            side = "BUY" if delta > 0 else "SELL"
        elif self.mode == "contrarian":
            side = "SELL" if delta > 0 else "BUY"
        else:
            raise ValueError(f"unsupported mode: {self.mode}")
        context.market_order(side, self.quantity, label=f"{self.mode}_taker")


class MomentumTakerStrategy(MidDeltaTakerStrategy):
    def __init__(self, quantity="10", threshold="0.03"):
        super().__init__(quantity=quantity, threshold=threshold, mode="momentum")


class ContrarianTakerStrategy(MidDeltaTakerStrategy):
    def __init__(self, quantity="10", threshold="0.03"):
        super().__init__(quantity=quantity, threshold=threshold, mode="contrarian")
