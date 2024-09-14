# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import datetime
from datetime import timedelta
from decimal import Decimal

import pandas as pd

from nautilus_trader.common.component import TimeEvent
from nautilus_trader.common.config import ActorConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.examples.strategies.simple_binance_symbols_filter import extract_symbol_info
from nautilus_trader.examples.strategies.simple_binance_symbols_filter import filter_with_onboard_date
from nautilus_trader.examples.strategies.simple_binance_symbols_filter import select_with_min_notional
from nautilus_trader.examples.strategies.simple_binance_symbols_filter import select_with_quoteAsset
from nautilus_trader.examples.strategies.simple_cross_sectional_metrics import generate_metrics
from nautilus_trader.examples.strategies.simple_cross_sectional_metrics import get_binance_historical_bars
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.trading.controller import Controller
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.trader import Trader


class BinanceFutureInstrumentSelectorControllerConfig(ActorConfig, frozen=True):
    interval_secs: int = 3600
    min_notional_threshold: float = 6
    quote_asset: str = "USDT"
    # Filter the symbol df based on the onboard date
    onboard_date_filter_type: str = "range"
    onboard_date_reference_date: datetime = datetime(2023, 1, 1)
    onboard_date_end_date: datetime = datetime(2024, 6, 1)


class BinanceFutureInstrumentSelectorController(Controller):
    """
    A controller for managing strategies dynamically based on top instruments by score.

    It filters symbols based on various criteria, computes scores, and manages
    strategies accordingly.

    """

    def __init__(
        self,
        trader: Trader,
        config: BinanceFutureInstrumentSelectorControllerConfig | None = None,
    ) -> None:
        if config is None:
            config = BinanceFutureInstrumentSelectorControllerConfig()
        PyCondition.type(config, BinanceFutureInstrumentSelectorControllerConfig, "config")
        super().__init__(config=config, trader=trader)

        self.interval_secs: int = config.interval_secs
        self.min_notional_threshold: float = config.min_notional_threshold
        self.quote_asset: str = config.quote_asset
        self.onboard_date_filter_type: str = config.onboard_date_filter_type
        self.onboard_date_reference_date: datetime = config.onboard_date_reference_date
        self.onboard_date_end_date: datetime = config.onboard_date_end_date
        self._trader: Trader = trader
        self.filtered_instrument_id_values: list[str] = []
        self.filtered_symbols: pd.DataFrame = pd.DataFrame()
        self.active_strategies: dict[str, Strategy] = {}

    def on_start(self) -> None:
        """
        Initialize and filter symbols on start.
        """
        # Get symbol info
        symbol_df, error_message = extract_symbol_info()
        if error_message:
            self.log.error(f"Error extracting symbol info: {error_message}")
            return

        # Filter the symbol df based on the quote asset
        filtered_by_asset = select_with_quoteAsset(symbol_df, self.quote_asset)

        # Filter the symbol df based on the min notional threshold
        filtered_by_min_notional = select_with_min_notional(
            filtered_by_asset, self.min_notional_threshold
        )

        # Filter the symbol df based on the onboard date
        self.filtered_symbols = filter_with_onboard_date(
            filtered_by_min_notional,
            self.onboard_date_filter_type,
            self.onboard_date_reference_date,
            self.onboard_date_end_date,
        )

        self.filtered_instrument_id_values = [
            f"{item}-PERP.BINANCE" for item in self.filtered_symbols.symbol.to_numpy()
        ]
        self.log.info(
            f"the length of filtered_instrument_id_values is {len(self.filtered_instrument_id_values)}"
        )
        # Set timer for periodic strategy management
        self.clock.set_timer(
            name="instrument_selector",
            interval=timedelta(seconds=self.interval_secs),
            callback=self.strategy_dynamic_management,
        )

    def strategy_dynamic_management(self, event: TimeEvent) -> None:
        """
        Manage strategies dynamically based on top instruments by score.

        Args:
            event (TimeEvent): The time event triggering this method.

        """
        # Get the top 2 instruments by volume
        top_2_instrument_id_values = self.get_top_2_instruments_by_score()

        # Remove strategies that are not in the top 2 and have no open positions
        for instrument_id_value in list(self.active_strategies.keys()):
            instrument_id = InstrumentId.from_str(instrument_id_value)
            if instrument_id_value not in top_2_instrument_id_values and self.active_strategies[
                instrument_id_value
            ].portfolio.is_flat(instrument_id):
                self.log.info(f"stopping and removing strategy for {instrument_id_value}")
                self.stop_strategy(self.active_strategies[instrument_id_value])
                self.remove_strategy(self.active_strategies[instrument_id_value])
                del self.active_strategies[instrument_id_value]

        # Add strategies for top 2 instruments not in active strategies
        for instrument_id_value in top_2_instrument_id_values:
            if instrument_id_value not in self.active_strategies:
                instrument = self.cache.instrument(
                    instrument_id=InstrumentId.from_str(instrument_id_value)
                )
                if instrument:
                    self.log.info(f"creating strategy for {instrument_id_value}")
                    strategy = self.create_strategy_instance(instrument)
                    self.create_strategy(strategy, start=True)
                    self.active_strategies[instrument_id_value] = strategy
                else:
                    self.log.warning(f"Instrument not found in cache: {instrument_id_value}")

    def get_top_2_instruments_by_score(self) -> list[str]:
        """
        Get the top 2 instruments by score.

        Returns:
            List[str]: List of top 2 instrument IDs.

        """
        end_time = datetime.now()
        start_time = end_time - timedelta(days=7)

        # Fetch the 15-minute interval Kline data for the past 7 days for each symbol
        df_dict = {}
        for symbol in self.filtered_symbols.symbol.to_numpy():
            try:
                df_dict[symbol] = get_binance_historical_bars(
                    symbol=symbol,
                    start=start_time,
                    end=end_time,
                    interval="15m",
                )
            except Exception as e:
                self.log.error(f"Error fetching data for {symbol}: {e}")

        # Compute metrics for each symbol
        final_scores_df = generate_metrics(df_dict)

        # Sort the DataFrame by average score and filter the top 2 symbols
        top_2_symbols_df = final_scores_df.nlargest(2, "average_score")

        # Get the top 2 symbols with the highest average score
        top_2_symbols = top_2_symbols_df["symbol"].tolist()
        self.log.info(f"the top 2 symbols are {top_2_symbols}")
        return [f"{symbol}-PERP.BINANCE" for symbol in top_2_symbols]

    def create_strategy_instance(self, instrument: Instrument) -> Strategy:
        """
        Create a strategy instance for the given instrument.

        Args:
            instrument (Instrument): The instrument to create a strategy for.

        Returns:
            Strategy: An instance of EMACross strategy.

        """
        strategy_config = EMACrossConfig(
            instrument_id=instrument.id,
            external_order_claims=[instrument.id],
            bar_type=BarType.from_str(f"{instrument.id.value}-1-MINUTE-LAST-EXTERNAL"),
            fast_ema_period=10,
            slow_ema_period=20,
            trade_size=Decimal("100.0") * instrument.size_increment.as_decimal(),
            order_id_tag=str(UUID4()),
            oms_type="HEDGING",
        )
        return EMACross(config=strategy_config)
