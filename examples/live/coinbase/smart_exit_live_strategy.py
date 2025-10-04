#!/usr/bin/env python3
"""
Smart Exit Live Trading Strategy for Coinbase

This strategy monitors your existing crypto holdings and automatically:
1. Sells 50% when you hit +50% profit
2. Sets a 20% trailing stop on the remaining 50%
3. Tracks tax obligations (37% of all profits)

Uses your current Coinbase holdings as seed money.
"""

from decimal import Decimal

from nautilus_trader.adapters.coinbase.config import CoinbaseDataClientConfig
from nautilus_trader.adapters.coinbase.config import CoinbaseExecClientConfig
from nautilus_trader.adapters.coinbase.factories import CoinbaseLiveDataClientFactory
from nautilus_trader.adapters.coinbase.factories import CoinbaseLiveExecClientFactory
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.data import Data
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.strategy import Strategy


class SmartExitStrategyConfig(StrategyConfig, frozen=True):
    """Configuration for Smart Exit Strategy."""
    
    instrument_id: str
    
    # Entry tracking (set these to your actual purchase prices)
    entry_price: Decimal  # Your original purchase price
    
    # Exit parameters
    profit_target_pct: Decimal = Decimal("0.50")  # 50% profit target
    first_exit_pct: Decimal = Decimal("0.50")  # Sell 50% at profit target
    trailing_stop_pct: Decimal = Decimal("0.20")  # 20% trailing stop on remaining
    
    # Tax tracking
    tax_rate: Decimal = Decimal("0.37")  # 37% tax on profits
    
    # Monitoring
    check_interval_secs: int = 60  # Check prices every 60 seconds


class SmartExitStrategy(Strategy):
    """
    Smart Exit Strategy - Automatically manage your crypto exits!
    
    This strategy:
    1. Monitors your existing position
    2. Sells 50% when you hit +50% profit
    3. Sets 20% trailing stop on remaining 50%
    4. Tracks tax obligations
    """
    
    def __init__(self, config: SmartExitStrategyConfig):
        super().__init__(config)
        
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.entry_price = float(config.entry_price)
        self.profit_target_pct = float(config.profit_target_pct)
        self.first_exit_pct = float(config.first_exit_pct)
        self.trailing_stop_pct = float(config.trailing_stop_pct)
        self.tax_rate = float(config.tax_rate)
        
        # State tracking
        self.highest_price = self.entry_price
        self.first_exit_done = False
        self.trailing_stop_active = False
        self.total_tax_owed = 0.0
        self.total_profits = 0.0
        
        # Position tracking
        self.initial_position_size = None
        self.current_position_size = None
    
    def on_start(self):
        """Actions to be performed on strategy start."""
        self.log.info("=" * 80)
        self.log.info("SMART EXIT STRATEGY STARTED")
        self.log.info("=" * 80)
        self.log.info(f"Instrument: {self.instrument_id}")
        self.log.info(f"Entry Price: ${self.entry_price:.2f}")
        self.log.info(f"Profit Target: {self.profit_target_pct*100:.0f}%")
        self.log.info(f"First Exit: {self.first_exit_pct*100:.0f}% of position")
        self.log.info(f"Trailing Stop: {self.trailing_stop_pct*100:.0f}%")
        self.log.info(f"Tax Rate: {self.tax_rate*100:.0f}%")
        self.log.info("=" * 80)
        
        # Subscribe to quotes for price updates
        self.subscribe_quote_ticks(self.instrument_id)
        
        # Get current position
        self._check_existing_position()
    
    def _check_existing_position(self):
        """Check if we have an existing position."""
        positions = list(self.cache.positions_open(instrument_id=self.instrument_id))
        
        if positions:
            position = positions[0]
            self.initial_position_size = float(position.quantity)
            self.current_position_size = float(position.quantity)
            
            self.log.info(f"📊 Found existing position: {self.current_position_size} {self.instrument_id.symbol}")
            self.log.info(f"   Entry Price: ${self.entry_price:.2f}")
            self.log.info(f"   Current Size: {self.current_position_size}")
        else:
            self.log.warning("⚠️  No existing position found!")
            self.log.warning("   This strategy is designed to manage existing holdings.")
            self.log.warning("   Please buy the asset first, then restart the strategy.")
    
    def on_quote_tick(self, tick: QuoteTick):
        """Handle quote tick updates."""
        if not self.initial_position_size:
            return  # No position to manage
        
        current_price = float(tick.ask_price)  # Use ask price for selling
        
        # Update highest price
        if current_price > self.highest_price:
            self.highest_price = current_price
        
        # Calculate current profit
        profit_pct = (current_price - self.entry_price) / self.entry_price
        
        # Log status periodically (every 100 ticks to avoid spam)
        if tick.ts_event % 100 == 0:
            self.log.info(f"💰 Current Price: ${current_price:.2f} | "
                         f"Profit: {profit_pct*100:+.2f}% | "
                         f"Highest: ${self.highest_price:.2f}")
        
        # FIRST EXIT: Sell 50% at +50% profit
        if not self.first_exit_done and profit_pct >= self.profit_target_pct:
            self._execute_first_exit(current_price)
        
        # TRAILING STOP: Monitor for 20% drop from highest
        elif self.first_exit_done and not self.trailing_stop_active:
            drawdown = (self.highest_price - current_price) / self.highest_price
            if drawdown >= self.trailing_stop_pct:
                self._execute_trailing_stop_exit(current_price)
    
    def _execute_first_exit(self, current_price: float):
        """Execute the first exit (sell 50% at +50% profit)."""
        if not self.instrument:
            self.log.error("No instrument loaded")
            return
        
        # Calculate quantity to sell (50% of initial position)
        sell_quantity = self.initial_position_size * self.first_exit_pct
        
        # Create market sell order
        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(sell_quantity),
            time_in_force=TimeInForce.GTC,
            reduce_only=True,
        )
        
        # Calculate profit and tax
        profit = (current_price - self.entry_price) * sell_quantity
        tax_owed = profit * self.tax_rate
        net_profit = profit - tax_owed
        
        self.total_profits += profit
        self.total_tax_owed += tax_owed
        
        self.submit_order(order)
        self.first_exit_done = True
        
        self.log.info("=" * 80)
        self.log.info("🎯 FIRST EXIT TRIGGERED!")
        self.log.info("=" * 80)
        self.log.info(f"Price: ${current_price:.2f} ({(current_price/self.entry_price-1)*100:+.2f}%)")
        self.log.info(f"Selling: {sell_quantity:.8f} ({self.first_exit_pct*100:.0f}% of position)")
        self.log.info(f"Gross Profit: ${profit:.2f}")
        self.log.info(f"Tax Owed (37%): ${tax_owed:.2f}")
        self.log.info(f"Net Profit: ${net_profit:.2f}")
        self.log.info(f"Remaining Position: {self.initial_position_size * (1-self.first_exit_pct):.8f}")
        self.log.info("=" * 80)
        self.log.info("🔒 Trailing stop now active on remaining 50%")
        self.log.info(f"   Will sell if price drops {self.trailing_stop_pct*100:.0f}% from highest point")
        self.log.info("=" * 80)
    
    def _execute_trailing_stop_exit(self, current_price: float):
        """Execute trailing stop exit (sell remaining 50%)."""
        if not self.instrument:
            self.log.error("No instrument loaded")
            return
        
        # Calculate remaining quantity
        remaining_quantity = self.initial_position_size * (1 - self.first_exit_pct)
        
        # Create market sell order
        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(remaining_quantity),
            time_in_force=TimeInForce.GTC,
            reduce_only=True,
        )
        
        # Calculate profit and tax
        profit = (current_price - self.entry_price) * remaining_quantity
        tax_owed = profit * self.tax_rate
        net_profit = profit - tax_owed
        
        self.total_profits += profit
        self.total_tax_owed += tax_owed
        
        self.submit_order(order)
        self.trailing_stop_active = True
        
        self.log.info("=" * 80)
        self.log.info("🛑 TRAILING STOP TRIGGERED!")
        self.log.info("=" * 80)
        self.log.info(f"Highest Price: ${self.highest_price:.2f}")
        self.log.info(f"Current Price: ${current_price:.2f} (down {(1-current_price/self.highest_price)*100:.2f}%)")
        self.log.info(f"Selling: {remaining_quantity:.8f} (remaining 50%)")
        self.log.info(f"Gross Profit: ${profit:.2f}")
        self.log.info(f"Tax Owed (37%): ${tax_owed:.2f}")
        self.log.info(f"Net Profit: ${net_profit:.2f}")
        self.log.info("=" * 80)
        self.log.info("📊 TOTAL RESULTS:")
        self.log.info(f"   Total Gross Profit: ${self.total_profits:.2f}")
        self.log.info(f"   Total Tax Owed: ${self.total_tax_owed:.2f}")
        self.log.info(f"   Total Net Profit: ${self.total_profits - self.total_tax_owed:.2f}")
        self.log.info("=" * 80)
        self.log.info("✅ All positions closed. Strategy complete!")
        self.log.info("=" * 80)
    
    def on_data(self, data: Data):
        """Handle generic data updates."""
        pass
    
    def on_stop(self):
        """Actions to be performed on strategy stop."""
        self.log.info("=" * 80)
        self.log.info("SMART EXIT STRATEGY STOPPED")
        self.log.info("=" * 80)
        if self.total_profits > 0:
            self.log.info(f"Total Profits: ${self.total_profits:.2f}")
            self.log.info(f"Total Tax Owed: ${self.total_tax_owed:.2f}")
            self.log.info(f"Net Profit: ${self.total_profits - self.total_tax_owed:.2f}")
        self.log.info("=" * 80)


# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("SMART-EXIT-001"),
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        "COINBASE": CoinbaseDataClientConfig(
            api_key=None,  # Will use COINBASE_API_KEY env var
            api_secret=None,  # Will use COINBASE_API_SECRET env var
        ),
    },
    exec_clients={
        "COINBASE": CoinbaseExecClientConfig(
            api_key=None,  # Will use COINBASE_API_KEY env var
            api_secret=None,  # Will use COINBASE_API_SECRET env var
        ),
    },
)

# Instantiate the node
node = TradingNode(config=config_node)

# Configure strategies for each coin you own
# TODO: Update these with YOUR actual entry prices!
strategies = [
    SmartExitStrategyConfig(
        instrument_id="BTC-USD.COINBASE",
        entry_price=Decimal("108160.61"),  # UPDATE THIS with your actual BTC buy price
    ),
    SmartExitStrategyConfig(
        instrument_id="ETH-USD.COINBASE",
        entry_price=Decimal("2524.91"),  # UPDATE THIS with your actual ETH buy price
    ),
    SmartExitStrategyConfig(
        instrument_id="SOL-USD.COINBASE",
        entry_price=Decimal("147.90"),  # UPDATE THIS with your actual SOL buy price
    ),
    SmartExitStrategyConfig(
        instrument_id="ADA-USD.COINBASE",
        entry_price=Decimal("0.58"),  # UPDATE THIS with your actual ADA buy price
    ),
]

# Add strategies to the node
for config in strategies:
    strategy = SmartExitStrategy(config=config)
    node.trader.add_strategy(strategy)

# Register client factories
node.add_data_client_factory("COINBASE", CoinbaseLiveDataClientFactory)
node.add_exec_client_factory("COINBASE", CoinbaseLiveExecClientFactory)
node.build()

# Run the node
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()

