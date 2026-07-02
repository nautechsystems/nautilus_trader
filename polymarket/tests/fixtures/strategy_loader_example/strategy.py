from polymarket.strategies.base import BasePolymarketStrategyV1


class LoaderExampleStrategy(BasePolymarketStrategyV1):
    def __init__(self, quantity="5"):
        super().__init__(quantity=quantity)
        self.quantity = quantity
        self.done = False

    def on_replay_step(self, step, book, context):
        if not self.done and book.best_ask is not None:
            context.market_order("BUY", self.quantity, label="loader_example")
            self.done = True
