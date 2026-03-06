import datetime as dt

from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.strategy import Strategy


class DemoStrategy(Strategy):
    def __init__(self, input_bartype: BarType):
        super().__init__()

        # Input data
        self.input_bartype = input_bartype
        self.instrument_id = input_bartype.instrument_id
        self.bars_processed = 0

        # Order placed
        self.order_placed = False

        # Start/End time of strategy
        self.start_time = None
        self.end_time = None

    def on_start(self):
        # Remember and log start time of strategy
        self.start_time = dt.datetime.now()
        self.log.info(f"Strategy started at: {self.start_time}")

        # Subscribe to primary data
        self.subscribe_bars(self.input_bartype)

    def on_bar(self, bar: Bar):
        self.bars_processed += 1
        self.log.info(
            f"Bar #{self.bars_processed} | Time: {unix_nanos_to_dt(bar.ts_event):%Y-%m-%d %H:%M:%S} | Bar: {bar}",
            color=LogColor.BLUE,
        )

        # Enter: SELL MARKET order (at 3rd bar)
        if not self.order_placed and self.bars_processed == 3:
            order = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.SELL,
                quantity=Quantity.from_int(1),  # 1 contract
                time_in_force=TimeInForce.GTC,
            )
            self.submit_order(order)
            self.order_placed = True
            self.log.info(f"Market order placed at {bar.close}", color=LogColor.GREEN)

        # Exit: BUY MARKET order (at 6th bar)
        if self.order_placed and self.bars_processed == 6:
            order = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(1),  # 1 contract
                time_in_force=TimeInForce.GTC,
            )
            self.submit_order(order)
            self.order_placed = True
            self.log.info(f"Market order placed at {bar.close}", color=LogColor.RED)

    def on_stop(self):
        # Remember and log end time of strategy
        self.end_time = dt.datetime.now()
        self.log.info(f"Strategy finished at: {self.end_time}")

        # Log count of processed bars
        self.log.info(f"Total bars processed: {self.bars_processed}")
