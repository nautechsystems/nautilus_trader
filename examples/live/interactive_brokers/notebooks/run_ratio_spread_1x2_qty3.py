# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.17.3
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %%
# Note: Use the jupytext python extension to be able to open this python file in jupyter as a notebook

# %%
"""
Test 1x2 Ratio Spread with Quantity 3.

This test places a market order for 3 units of a 1x2 ratio spread:
- Long 1 E4DN5 P6350 per spread unit
- Long 2 E4DN5 P6355 per spread unit
- Total order: 3 spread units = Long 3 P6350, Long 6 P6355

This will test the new spread execution functionality and verify:
1. Proper spread instrument creation
2. Order submission and acceptance
3. Fill event handling for ratio spreads
4. Portfolio position tracking

"""

# %%

import os
import threading
import time

from ibapi.common import MarketDataTypeEnum as IBMarketDataTypeEnum

from nautilus_trader.adapters.interactive_brokers.common import IB
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.config import SymbologyMethod
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveDataClientFactory
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveExecClientFactory
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RoutingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.config import StrategyConfig
from nautilus_trader.trading.strategy import Strategy


# %%
class RatioSpreadTestConfig(StrategyConfig, frozen=True):
    """
    Configuration for 1x2 ratio spread test.
    """

    spread_instrument_id: InstrumentId


# %%
class RatioSpreadTestStrategy(Strategy):
    """
    Strategy to test 1x2 ratio spread execution with quantity 3.
    """

    def __init__(self, config: RatioSpreadTestConfig):
        super().__init__(config=config)
        self.order_placed = False
        self.execution_events: list[OrderFilled] = []
        self.order_events: list[OrderAccepted | OrderSubmitted | OrderRejected] = []
        self.quote_tick_count = 0
        self.instrument_loaded = False

    def on_start(self):
        """
        Handle strategy start event.
        """
        self.log.info("=" * 80, color=LogColor.BLUE)
        self.log.info(
            "1x2 RATIO SPREAD TEST (Quantity 3) - DYNAMIC LOADING",
            color=LogColor.BLUE,
        )
        self.log.info("=" * 80, color=LogColor.BLUE)

        self.log.info(f"Testing spread: {self.config.spread_instrument_id}")
        self.log.info("Expected execution:")
        self.log.info("  - 3 spread units")
        self.log.info("  - Long 3 contracts of E1AQ5 C6400")
        self.log.info("  - Long 6 contracts of E1AQ5 P6440")

        # Request the spread instrument dynamically (not pre-loaded)
        self.log.info("Requesting spread instrument dynamically...")
        self.request_instrument(self.config.spread_instrument_id)

    def on_instrument(self, instrument):
        """
        Handle instrument response and place order.
        """
        self.log.info(f"Received instrument: {instrument.id}")
        self.log.info(f"Instrument type: {type(instrument)}")

        # Mark instrument as loaded and subscribe to quote ticks
        self.instrument_loaded = True
        self.log.info("Subscribing to quote ticks for spread instrument...")
        self.subscribe_quote_ticks(instrument.id)

        # Place order immediately after getting instrument
        if not self.order_placed:
            self._place_ratio_spread_order(instrument)

    def _place_ratio_spread_order(self, instrument):
        """
        Place a market order for the 1x2 ratio spread.
        """
        self.log.info("=" * 60, color=LogColor.GREEN)
        self.log.info("PLACING 1x2 RATIO SPREAD MARKET ORDER (DAY)", color=LogColor.GREEN)
        self.log.info("=" * 60, color=LogColor.GREEN)

        # Create market order for 3 spread units (DAY required for combo orders)
        order = self.order_factory.market(
            instrument_id=self.config.spread_instrument_id,
            order_side=OrderSide.BUY,  # Buy the spread
            quantity=instrument.make_qty(3),  # 3 spread units
            time_in_force=TimeInForce.DAY,  # DAY required for combo orders by IB
        )

        self.log.info("Order details:")
        self.log.info(f"   Client Order ID: {order.client_order_id}")
        self.log.info(f"   Instrument: {order.instrument_id}")
        self.log.info(f"   Side: {order.side}")
        self.log.info(f"   Quantity: {order.quantity}")
        self.log.info(f"   Order Type: {order.order_type}")

        # Submit the order
        self.submit_order(order)
        self.order_placed = True

        self.log.info(
            "Market order submitted for 1x2 ratio spread",
            color=LogColor.GREEN,
        )

    def on_quote_tick(self, tick):
        """
        Handle quote tick events for the spread instrument.
        """
        self.quote_tick_count += 1

        # Log first few quote ticks to verify subscription is working
        if self.quote_tick_count <= 5:
            self.log.info("=" * 60, color=LogColor.CYAN)
            self.log.info(f"QUOTE TICK #{self.quote_tick_count} RECEIVED", color=LogColor.CYAN)
            self.log.info("=" * 60, color=LogColor.CYAN)
            self.log.info(f"   Instrument: {tick.instrument_id}")
            self.log.info(f"   Bid: {tick.bid_price} @ {tick.bid_size}")
            self.log.info(f"   Ask: {tick.ask_price} @ {tick.ask_size}")
            self.log.info(f"   Spread: {float(tick.ask_price) - float(tick.bid_price):.4f}")
            self.log.info(f"   Event Time: {tick.ts_event}")
        elif self.quote_tick_count == 6:
            self.log.info(
                f"Quote tick subscription working! Received {self.quote_tick_count} ticks so far...",
                color=LogColor.GREEN,
            )
        elif self.quote_tick_count % 10 == 0:
            # Log every 10th tick after the first 5
            self.log.info(
                f"Quote tick #{self.quote_tick_count}: Bid={tick.bid_price}, Ask={tick.ask_price}",
                color=LogColor.CYAN,
            )

    def on_order_submitted(self, event: OrderSubmitted):
        """
        Handle order submitted events.
        """
        self.order_events.append(("SUBMITTED", event))
        self.log.info(
            f"ORDER SUBMITTED: {event.client_order_id} | Account: {event.account_id}",
            color=LogColor.BLUE,
        )

    def on_order_accepted(self, event: OrderAccepted):
        """
        Handle order accepted events.
        """
        self.order_events.append(("ACCEPTED", event))
        self.log.info(
            f"ORDER ACCEPTED: {event.client_order_id} | Venue Order ID: {event.venue_order_id}",
            color=LogColor.GREEN,
        )

    def on_order_rejected(self, event: OrderRejected):
        """
        Handle order rejected events.
        """
        self.order_events.append(("REJECTED", event))
        self.log.error(f"ORDER REJECTED: {event.client_order_id} | Reason: {event.reason}")

    def on_order_filled(self, event: OrderFilled):
        """Handle order filled events - KEY for understanding ratio spread execution."""
        self.execution_events.append(event)

        self.log.info("=" * 80, color=LogColor.MAGENTA)
        self.log.info(f"FILL #{len(self.execution_events)} RECEIVED", color=LogColor.MAGENTA)
        self.log.info("=" * 80, color=LogColor.MAGENTA)

        self.log.info(f"   Client Order ID: {event.client_order_id}")
        self.log.info(f"   Venue Order ID: {event.venue_order_id}")
        self.log.info(f"   Instrument: {event.instrument_id}")
        self.log.info(f"   Order Side: {event.order_side}")
        self.log.info(f"   Fill Quantity: {event.last_qty}")
        self.log.info(f"   Fill Price: {event.last_px}")
        self.log.info(f"   Commission: {event.commission}")
        self.log.info(f"   Trade ID: {event.trade_id}")

        # Analyze fill quantity interpretation
        self._analyze_fill(event)

        # Check portfolio state
        self._check_portfolio_state()

    def _analyze_fill(self, event: OrderFilled):
        """
        Analyze what the fill represents.
        """
        self.log.info("FILL ANALYSIS:", color=LogColor.YELLOW)

        fill_qty = int(event.last_qty.as_double())

        if event.order_side == OrderSide.BUY:
            self.log.info(f"   LONG leg fill: {fill_qty} contracts", color=LogColor.CYAN)
            self.log.info("   Expected: 1 contract per spread unit", color=LogColor.CYAN)
        elif event.order_side == OrderSide.SELL:
            self.log.info(f"   SHORT leg fill: {fill_qty} contracts", color=LogColor.CYAN)
            self.log.info("   Expected: 2 contracts per spread unit", color=LogColor.CYAN)

        # Check if this is spread-level or leg-level fill
        if str(event.instrument_id) == str(self.config.spread_instrument_id):
            self.log.info("   SPREAD-LEVEL FILL", color=LogColor.GREEN)
        else:
            self.log.info(f"   LEG-LEVEL FILL: {event.instrument_id}", color=LogColor.YELLOW)

    def _check_portfolio_state(self):
        """
        Check current portfolio positions.
        """
        self.log.info("PORTFOLIO STATE:", color=LogColor.CYAN)

        cache = self.cache
        all_positions = list(cache.positions_open()) + list(cache.positions_closed())

        if not all_positions:
            self.log.info("   No positions in portfolio")
            return

        for position in all_positions:
            self.log.info(f"   {position.instrument_id}: {position.side} {position.quantity}")

    def on_stop(self):
        """
        Handle strategy stop and provide final analysis.
        """
        self.log.info("\n" + "=" * 80, color=LogColor.BLUE)
        self.log.info("FINAL TEST ANALYSIS", color=LogColor.BLUE)
        self.log.info("=" * 80, color=LogColor.BLUE)

        # Dynamic loading analysis
        self.log.info(
            f"Instrument loaded dynamically: {'YES' if self.instrument_loaded else 'NO'}",
        )
        self.log.info(f"Quote ticks received: {self.quote_tick_count}")

        # Order and execution analysis
        self.log.info(f"Total fills received: {len(self.execution_events)}")
        self.log.info(f"Total order events: {len(self.order_events)}")

        if self.execution_events:
            buy_fills = [e for e in self.execution_events if e.order_side == OrderSide.BUY]
            sell_fills = [e for e in self.execution_events if e.order_side == OrderSide.SELL]

            buy_qty = sum(int(f.last_qty.as_double()) for f in buy_fills)
            sell_qty = sum(int(f.last_qty.as_double()) for f in sell_fills)

            self.log.info(f"BUY fills: {len(buy_fills)} (total qty: {buy_qty})")
            self.log.info(f"SELL fills: {len(sell_fills)} (total qty: {sell_qty})")

            # Expected: 3 long, 6 short for 3 spread units
            self.log.info("Expected for 3 spread units: 3 long, 6 short")

            if buy_qty == 3 and sell_qty == 6:
                self.log.info("EXECUTION MATCHES EXPECTED RATIOS", color=LogColor.GREEN)
            else:
                self.log.info("EXECUTION PATTERN UNCLEAR", color=LogColor.YELLOW)
        else:
            self.log.info("No fills received")


# %%
# Create 1x2 ratio spread
leg1_id = InstrumentId.from_str("E1CU5 P6440.XCME")  # Long put
leg2_id = InstrumentId.from_str("E1CU5 P6450.XCME")  # Short put

spread_id = InstrumentId.new_spread(
    [
        (leg1_id, 1),
        (leg2_id, -1),
    ],
)

print(f"Testing 1x2 ratio spread: {spread_id}")
print("Order: 3 spread units")
print("Expected execution: Long 3 P6350, Long 6 P6355")
print()

# Configure instrument provider (no pre-loaded spread IDs)
instrument_provider = InteractiveBrokersInstrumentProviderConfig(
    symbology_method=SymbologyMethod.IB_SIMPLIFIED,
    convert_exchange_to_mic_venue=True,
    build_futures_chain=False,
    build_options_chain=False,
    min_expiry_days=0,
    max_expiry_days=30,
    # load_ids=frozenset([spread_id]),  # Removed - testing dynamic loading
)

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id="RATIO-SPREAD-TEST",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        IB: InteractiveBrokersDataClientConfig(
            ibg_port=7497,
            instrument_provider=instrument_provider,
            market_data_type=IBMarketDataTypeEnum.DELAYED_FROZEN,
        ),
    },
    exec_clients={
        IB: InteractiveBrokersExecClientConfig(
            ibg_port=7497,
            instrument_provider=instrument_provider,
            routing=RoutingConfig(default=True),
            account_id=os.environ.get("TWS_ACCOUNT"),
        ),
    },
    timeout_connection=90.0,
    timeout_reconciliation=5.0,
    timeout_portfolio=5.0,
    timeout_disconnection=5.0,
    timeout_post_stop=2.0,
)

# Create and configure node
node = TradingNode(config=config_node)
strategy_config = RatioSpreadTestConfig(spread_instrument_id=spread_id)
strategy = RatioSpreadTestStrategy(config=strategy_config)

node.trader.add_strategy(strategy)
node.add_data_client_factory(IB, InteractiveBrokersLiveDataClientFactory)
node.add_exec_client_factory(IB, InteractiveBrokersLiveExecClientFactory)
node.build()

# %%
print("Starting 1x2 Ratio Spread Test (Dynamic Loading + Quote Ticks)...")
print("This will:")
print("1. Connect to Interactive Brokers")
print("2. Dynamically request the 1x2 ratio spread instrument (not pre-loaded)")
print("3. Subscribe to quote ticks for the spread")
print("4. Place a market order for 3 spread units")
print("5. Monitor execution events and quote ticks for 60 seconds")
print("6. Auto-stop and analyze results")
print()
print("IMPORTANT: Make sure TWS/IB Gateway is running!")
print("IMPORTANT: This will place a REAL market order in paper trading!")
print()


# %%
def auto_stop_node(node, delay_seconds=15):
    """
    Automatically stop the node after a delay.
    """

    def stop_after_delay():
        time.sleep(delay_seconds)
        node.stop()

    thread = threading.Thread(target=stop_after_delay)
    thread.daemon = True
    thread.start()


# %%
# Start auto-stop timer
auto_stop_node(node, delay_seconds=60)

try:
    node.run()
except KeyboardInterrupt:
    node.stop()
finally:
    node.dispose()
