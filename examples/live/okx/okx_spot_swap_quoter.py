#!/usr/bin/env python3
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

from decimal import Decimal

from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXExecClientConfig
from nautilus_trader.adapters.okx import OKXLiveDataClientFactory
from nautilus_trader.adapters.okx import OKXLiveExecClientFactory
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.core.nautilus_pyo3 import OKXMarginMode
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# Configuration
token = "ETH"
symbol_spot = f"{token}-USDT"
symbol_swap = f"{token}-USDT-SWAP"
use_spot_margin = True  # True for MARGIN, False for SPOT (cash)

# Order quantities
order_qty_spot = Decimal("2.00")  # In quote currency (USDT)
order_qty_swap = Decimal("0.01")  # In base currency (ETH contracts)

# Trading parameters
tob_offset_ticks = 100


class SpotSwapQuoterConfig(StrategyConfig, frozen=True, kw_only=True):
    """
    Configuration for the spot and swap quoter strategy.
    """

    spot_instrument_id: InstrumentId
    swap_instrument_id: InstrumentId
    spot_order_qty: Decimal
    swap_order_qty: Decimal
    tob_offset_ticks: int = 100
    log_data: bool = False
    close_positions_on_stop: bool = True


class SpotSwapQuoter(Strategy):
    """
    A quoter that places orders on both spot and swap instruments.

    Opens a position on start and maintains quotes on both instruments.

    """

    def __init__(self, config: SpotSwapQuoterConfig) -> None:
        super().__init__(config)
        self.spot_instrument: Instrument | None = None
        self.swap_instrument: Instrument | None = None

        # Spot state
        self._spot_tick_size = Decimal(0)
        self._spot_price_offset = Decimal(0)
        self._spot_order_qty = None
        self._spot_bid_order: LimitOrder | None = None
        self._spot_ask_order: LimitOrder | None = None

        # Swap state
        self._swap_tick_size = Decimal(0)
        self._swap_price_offset = Decimal(0)
        self._swap_order_qty = None
        self._swap_bid_order: LimitOrder | None = None
        self._swap_ask_order: LimitOrder | None = None

        self._opened_position = False

    def on_start(self) -> None:
        self.spot_instrument = self.cache.instrument(self.config.spot_instrument_id)
        if self.spot_instrument is None:
            self.log.error(
                f"Could not find spot instrument for {self.config.spot_instrument_id}",
            )
            self.stop()
            return

        self.swap_instrument = self.cache.instrument(self.config.swap_instrument_id)
        if self.swap_instrument is None:
            self.log.error(f"Could not find swap instrument for {self.config.swap_instrument_id}")
            self.stop()
            return

        # Initialize spot parameters
        self._spot_tick_size = self.spot_instrument.price_increment.as_decimal()
        offset_ticks = max(self.config.tob_offset_ticks, 0)
        self._spot_price_offset = self._spot_tick_size * offset_ticks
        self._spot_order_qty = self.spot_instrument.make_qty(self.config.spot_order_qty)

        # Initialize swap parameters
        self._swap_tick_size = self.swap_instrument.price_increment.as_decimal()
        self._swap_price_offset = self._swap_tick_size * offset_ticks
        self._swap_order_qty = self.swap_instrument.make_qty(self.config.swap_order_qty)

        # Subscribe to quote ticks
        self.subscribe_quote_ticks(self.config.spot_instrument_id)
        self.subscribe_quote_ticks(self.config.swap_instrument_id)

        # Open initial position on spot
        self.open_position_on_start()

    def open_position_on_start(self) -> None:
        """
        Open a position on the spot instrument.
        """
        if self.spot_instrument is None:
            return

        order = self.order_factory.market(
            instrument_id=self.config.spot_instrument_id,
            order_side=OrderSide.BUY,
            quantity=self._spot_order_qty,
            time_in_force=TimeInForce.GTC,
            quote_quantity=True,  # Market BUY orders use quote quantity (USDT)
        )

        self.submit_order(order)
        self._opened_position = True
        self.log.info(
            f"Opened position on {self.config.spot_instrument_id} with order {order.client_order_id}",
            LogColor.BLUE,
        )

    def on_quote_tick(self, quote: QuoteTick) -> None:
        if self.config.log_data:
            self.log.info(repr(quote), LogColor.CYAN)

        # Handle spot quotes
        if quote.instrument_id == self.config.spot_instrument_id:
            self._maintain_spot_orders(quote)

        # Handle swap quotes
        elif quote.instrument_id == self.config.swap_instrument_id:
            self._maintain_swap_orders(quote)

    def _maintain_spot_orders(self, quote: QuoteTick) -> None:
        if self.spot_instrument is None:
            return

        if not self.is_running:
            # Don't create new orders if stopping
            return

        assert self._spot_order_qty is not None  # type checking

        # Clear order references on any terminal status
        if self._spot_bid_order and self._spot_bid_order.is_closed:
            self._spot_bid_order = None
        if self._spot_ask_order and self._spot_ask_order.is_closed:
            self._spot_ask_order = None

        # Calculate desired prices with bounds checking
        desired_bid = quote.bid_price.as_decimal() - self._spot_price_offset
        desired_ask = quote.ask_price.as_decimal() + self._spot_price_offset

        # Guard against non-positive prices
        min_price = self.spot_instrument.price_increment
        if desired_bid <= 0:
            self.log.warning(
                f"Calculated bid price {desired_bid} <= 0, using min price {min_price}",
            )
            desired_bid = min_price
        if desired_ask <= desired_bid:
            self.log.warning(f"Calculated ask price {desired_ask} <= bid {desired_bid}, skipping")
            return

        # Place BID order if none exists
        if self._spot_bid_order is None:
            price = self.spot_instrument.make_price(desired_bid)
            base_qty = self._spot_order_qty.as_decimal() / desired_bid
            quantity = self.spot_instrument.make_qty(base_qty)
            order = self.order_factory.limit(
                instrument_id=self.config.spot_instrument_id,
                order_side=OrderSide.BUY,
                price=price,
                quantity=quantity,
                post_only=True,
                quote_quantity=False,
            )
            self._spot_bid_order = order
            self.submit_order(order)

        # Place ASK order if none exists
        if self._spot_ask_order is None:
            price = self.spot_instrument.make_price(desired_ask)
            base_qty = self._spot_order_qty.as_decimal() / desired_ask
            quantity = self.spot_instrument.make_qty(base_qty)
            order = self.order_factory.limit(
                instrument_id=self.config.spot_instrument_id,
                order_side=OrderSide.SELL,
                price=price,
                quantity=quantity,
                post_only=True,
                quote_quantity=False,
            )
            self._spot_ask_order = order
            self.submit_order(order)

    def _maintain_swap_orders(self, quote: QuoteTick) -> None:
        if self.swap_instrument is None:
            return

        if not self.is_running:
            # Don't create new orders if stopping
            return

        # Clear order references on any terminal status
        if self._swap_bid_order and self._swap_bid_order.is_closed:
            self._swap_bid_order = None
        if self._swap_ask_order and self._swap_ask_order.is_closed:
            self._swap_ask_order = None

        # Calculate desired prices with bounds checking
        desired_bid = quote.bid_price.as_decimal() - self._swap_price_offset
        desired_ask = quote.ask_price.as_decimal() + self._swap_price_offset

        # Guard against non-positive prices
        min_price = self.swap_instrument.price_increment
        if desired_bid <= 0:
            self.log.warning(
                f"Calculated swap bid price {desired_bid} <= 0, using min price {min_price}",
            )
            desired_bid = min_price
        if desired_ask <= desired_bid:
            self.log.warning(
                f"Calculated swap ask price {desired_ask} <= bid {desired_bid}, skipping",
            )
            return

        # Place BID order if none exists
        if self._swap_bid_order is None:
            price = self.swap_instrument.make_price(desired_bid)
            order = self.order_factory.limit(
                instrument_id=self.config.swap_instrument_id,
                order_side=OrderSide.BUY,
                price=price,
                quantity=self._swap_order_qty,
                post_only=True,
                quote_quantity=False,
            )
            self._swap_bid_order = order
            self.submit_order(order)

        # Place ASK order if none exists
        if self._swap_ask_order is None:
            price = self.swap_instrument.make_price(desired_ask)
            order = self.order_factory.limit(
                instrument_id=self.config.swap_instrument_id,
                order_side=OrderSide.SELL,
                price=price,
                quantity=self._swap_order_qty,
                post_only=True,
                quote_quantity=False,
            )
            self._swap_ask_order = order
            self.submit_order(order)

    def on_event(self, event) -> None:
        # Handle fills and reset state
        if isinstance(event, OrderFilled):
            # Spot fills
            if (
                self._spot_bid_order
                and event.client_order_id == self._spot_bid_order.client_order_id
            ):
                self._spot_bid_order = None
            elif (
                self._spot_ask_order
                and event.client_order_id == self._spot_ask_order.client_order_id
            ):
                self._spot_ask_order = None

            # Swap fills
            if (
                self._swap_bid_order
                and event.client_order_id == self._swap_bid_order.client_order_id
            ):
                self._swap_bid_order = None
            elif (
                self._swap_ask_order
                and event.client_order_id == self._swap_ask_order.client_order_id
            ):
                self._swap_ask_order = None

    def on_stop(self) -> None:
        # Cancel all orders
        self.cancel_all_orders(self.config.spot_instrument_id)
        self.cancel_all_orders(self.config.swap_instrument_id)

        assert self.spot_instrument is not None  # type checking
        assert self.swap_instrument is not None  # type checking

        if self.config.close_positions_on_stop:
            # Close spot positions created by this strategy only
            spot_positions = self.cache.positions_open(
                instrument_id=self.config.spot_instrument_id,
                strategy_id=self.id,
            )
            for position in spot_positions:
                if position.side == PositionSide.SHORT:
                    # SHORT positions require BUY to close
                    # OKX spot margin requires quote_quantity=True for MARKET BUY
                    # Convert position quantity (ETH) to quote amount (USDT)
                    quote_tick = self.cache.quote_tick(self.config.spot_instrument_id)
                    if quote_tick is None:
                        self.log.warning(
                            f"No quote tick available for {self.config.spot_instrument_id}, cannot close SHORT position",
                        )
                        continue

                    # Use mid price to estimate quote amount needed
                    mid_price = (quote_tick.bid_price + quote_tick.ask_price) / 2
                    quote_amount = position.quantity.as_decimal() * mid_price

                    # Create quantity in quote currency (USDT) using price precision
                    quote_qty = Quantity(
                        quote_amount,
                        precision=self.spot_instrument.price_precision,
                    )

                    self.log.info(
                        f"Closing SHORT position {position.id} with quote quantity {quote_amount:.2f} USDT",
                        LogColor.YELLOW,
                    )

                    # Submit market order with quote quantity
                    order = self.order_factory.market(
                        instrument_id=position.instrument_id,
                        order_side=OrderSide.BUY,
                        quantity=quote_qty,
                        time_in_force=TimeInForce.GTC,
                        reduce_only=True,
                        quote_quantity=True,
                    )
                    self.submit_order(order, position_id=position.id)
                else:
                    # LONG positions can use normal close_position
                    self.close_position(position)

            # Close swap positions
            self.close_all_positions(self.config.swap_instrument_id)

        # Reset state
        self._spot_bid_order = None
        self._spot_ask_order = None
        self._swap_bid_order = None
        self._swap_ask_order = None


# Setup instruments
spot_instrument_id = InstrumentId.from_str(f"{symbol_spot}.{OKX}")
swap_instrument_id = InstrumentId.from_str(f"{symbol_swap}.{OKX}")

reconciliation_instrument_ids = [spot_instrument_id, swap_instrument_id]
load_ids = frozenset([spot_instrument_id, swap_instrument_id])
external_order_claims = [spot_instrument_id, swap_instrument_id]

# Determine instrument types based on use_spot_margin flag
spot_instrument_type = OKXInstrumentType.MARGIN if use_spot_margin else OKXInstrumentType.SPOT
instrument_types = (
    spot_instrument_type,
    OKXInstrumentType.SWAP,
)

contract_types = (OKXContractType.LINEAR,)

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        log_level_file="DEBUG",
        use_pyo3=True,
    ),
    exec_engine=LiveExecEngineConfig(
        convert_quote_qty_to_base=False,
        reconciliation=True,
        reconciliation_instrument_ids=reconciliation_instrument_ids,
        open_check_interval_secs=5.0,
        open_check_open_only=False,
        position_check_interval_secs=60,
        purge_closed_orders_interval_mins=15,
        purge_closed_orders_buffer_mins=60,
        purge_closed_positions_interval_mins=15,
        purge_closed_positions_buffer_mins=60,
        purge_account_events_interval_mins=15,
        purge_account_events_lookback_mins=60,
        graceful_shutdown_on_exception=True,
    ),
    risk_engine=LiveRiskEngineConfig(bypass=True),  # Must bypass for spot for now
    data_clients={
        OKX: OKXDataClientConfig(
            api_key=None,  # 'OKX_API_KEY' env var
            api_secret=None,  # 'OKX_API_SECRET' env var
            api_passphrase=None,  # 'OKX_API_PASSPHRASE' env var
            base_url_http=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(
                load_all=False,
                load_ids=load_ids,
            ),
            instrument_types=instrument_types,
            contract_types=contract_types,
            is_demo=False,  # If client uses the demo API
            http_timeout_secs=10,
        ),
    },
    exec_clients={
        OKX: OKXExecClientConfig(
            api_key=None,  # 'OKX_API_KEY' env var
            api_secret=None,  # 'OKX_API_SECRET' env var
            api_passphrase=None,  # 'OKX_API_PASSPHRASE' env var
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(
                load_all=False,
                load_ids=load_ids,
            ),
            instrument_types=instrument_types,
            contract_types=contract_types,
            margin_mode=OKXMarginMode.CROSS,
            use_spot_margin=use_spot_margin,
            is_demo=False,  # If client uses the demo API
            use_fills_channel=False,  # Set to True if VIP5+ to get separate fill reports
            http_timeout_secs=10,
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure strategy
config_strategy = SpotSwapQuoterConfig(
    spot_instrument_id=spot_instrument_id,
    swap_instrument_id=swap_instrument_id,
    spot_order_qty=order_qty_spot,
    swap_order_qty=order_qty_swap,
    tob_offset_ticks=tob_offset_ticks,
    log_data=False,
    use_hyphens_in_client_order_ids=False,  # OKX doesn't allow hyphens
)

# Instantiate strategy
strategy = SpotSwapQuoter(config=config_strategy)

# Add strategy
node.trader.add_strategy(strategy)

# Register client factories with the node
node.add_data_client_factory(OKX, OKXLiveDataClientFactory)
node.add_exec_client_factory(OKX, OKXLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
