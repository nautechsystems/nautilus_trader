# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------


from nautilus_trader.common.enums import LogColor
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.trading.config import StrategyConfig
from nautilus_trader.trading.strategy import Strategy


class DemoStrategyConfig(StrategyConfig, frozen=True):
    bar_type: BarType
    instrument: Instrument


class DemoStrategy(Strategy):

    def __init__(self, config: DemoStrategyConfig):
        super().__init__(config=config)

        # Track if we've already placed an order
        self.order_placed = False

        # Track total bars seen
        self.count_of_bars: int = 0
        self.show_portfolio_at_bar: int | None = 0

    def on_start(self):
        """
        Handle strategy start event.
        """
        # Subscribe to market data
        self.subscribe_bars(self.config.bar_type)

        # Show initial portfolio state
        self.show_portfolio_info("Portfolio state (Before trade)")

    def on_bar(self, bar: Bar):
        """
        Handle new bar event.
        """
        # Increment total bars seen
        self.count_of_bars += 1

        # Show portfolio state if we reached target bar
        if self.show_portfolio_at_bar == self.count_of_bars:
            self.show_portfolio_info("Portfolio state (2 minutes after position opened)")

        # Only place one order for demonstration
        if not self.order_placed:
            # Prepare values for order
            last_price = bar.close
            tick_size = self.config.instrument.price_increment
            profit_price = self.config.instrument.make_price(last_price + (10 * tick_size))
            stoploss_price = self.config.instrument.make_price(last_price - (10 * tick_size))

            # Create BUY MARKET order with PT and SL (both 10 ticks)
            bracket_order_list = self.order_factory.bracket(
                instrument_id=self.config.instrument.id,
                order_side=OrderSide.BUY,
                quantity=self.config.instrument.make_qty(1),  # Trade size: 1 contract
                time_in_force=TimeInForce.GTC,
                tp_price=profit_price,
                sl_trigger_price=stoploss_price,
            )

            # Submit order and remember it
            self.submit_order_list(bracket_order_list)
            self.order_placed = True
            self.log.info(f"Submitted bracket order: {bracket_order_list}", color=LogColor.GREEN)

    def on_position_opened(self, event: PositionOpened):
        """
        Handle position opened event.
        """
        # Log position details
        self.log.info(f"Position opened: {event}", color=LogColor.GREEN)

        # Show portfolio state when position is opened
        self.show_portfolio_info("Portfolio state (In position):")

        # Set target bar number for next portfolio display
        self.show_portfolio_at_bar = self.count_of_bars + 2  # Show after 2 bars

    def on_stop(self):
        """
        Handle strategy stop event.
        """
        # Show final portfolio state
        self.show_portfolio_info("Portfolio state (After trade)")

    def show_portfolio_info(self, intro_message: str = ""):
        """
        Display current portfolio information.
        """
        if intro_message:
            self.log.info(f"====== {intro_message} ======")

        # POSITION information
        self.log.info("Portfolio -> Position information:", color=LogColor.BLUE)
        is_flat = self.portfolio.is_flat(self.config.instrument.id)
        self.log.info(f"Is flat: {is_flat}", color=LogColor.BLUE)

        net_position = self.portfolio.net_position(self.config.instrument.id)
        self.log.info(f"Net position: {net_position} contract(s)", color=LogColor.BLUE)

        net_exposure = self.portfolio.net_exposure(self.config.instrument.id)
        self.log.info(f"Net exposure: {net_exposure}", color=LogColor.BLUE)

        # -----------------------------------------------------

        # P&L information
        self.log.info("Portfolio -> P&L information:", color=LogColor.YELLOW)

        realized_pnl = self.portfolio.realized_pnl(self.config.instrument.id)
        self.log.info(f"Realized P&L: {realized_pnl}", color=LogColor.YELLOW)

        unrealized_pnl = self.portfolio.unrealized_pnl(self.config.instrument.id)
        self.log.info(f"Unrealized P&L: {unrealized_pnl}", color=LogColor.YELLOW)

        # -----------------------------------------------------

        self.log.info("Portfolio -> Account information:", color=LogColor.CYAN)
        margins_init = self.portfolio.margins_init(self.config.instrument.venue)
        self.log.info(f"Initial margin: {margins_init}", color=LogColor.CYAN)

        margins_maint = self.portfolio.margins_maint(self.config.instrument.venue)
        self.log.info(f"Maintenance margin: {margins_maint}", color=LogColor.CYAN)

        balances_locked = self.portfolio.balances_locked(self.config.instrument.venue)
        self.log.info(f"Locked balance: {balances_locked}", color=LogColor.CYAN)
