from polymarket.strategies.base import BasePolymarketStrategyV1


class BuyHoldFirstAskStrategy(BasePolymarketStrategyV1):
    def __init__(self, quantity="10"):
        super().__init__(quantity=quantity)
        self.quantity = quantity
        self.done = False

    def on_replay_step(self, step, book, context):
        if self.done or book.best_ask is None:
            return
        context.market_order("BUY", self.quantity, label="buy_hold_first_ask")
        self.done = True
